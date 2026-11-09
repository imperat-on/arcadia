//! Fontes de download locais — port de `app/electron/sources.js`.
//!
//! O registro (`sources.json`), os catálogos baixados (`sources/{id}.json`), o
//! índice leve (`sources_index.json`) e as ETags de cada fonte são o mesmo
//! estado local do cliente Electron. A persistência é injetada
//! ([`SourceStorage`]); [`FsSourceStorage`] reproduz os caminhos do app e
//! [`MemorySourceStorage`] serve aos testes.
//!
//! O catálogo remoto (`/catalog/v1/sources/{id}/games`) tem prioridade sobre o
//! cache local ao montar o índice e ao abrir um jogo; sem servidor, o cache
//! responde. Nenhuma chamada de IPC é feita aqui: os envelopes devolvidos são
//! os mesmos que o `main.js` entrega ao renderer.
//!
//! O sync de sources (`supabase/sources.js`) é representado pelos helpers
//! puros [`push_payload`], [`next_watermarks`] e [`pull_sources`]: apenas
//! `{source_id, url, name}` (ou `{source_id, removed:true}`) atravessa a rede,
//! nunca `etag`/`lastMod`/`count` nem chaves de API. O RPC em si continua em
//! `arcadia-sync`, que já é dono dos watermarks (`srcPush`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::catalog::CatalogClient;
use crate::http::{js_truthy, Fetcher, Request};

/// Registro de fontes (relativo à raiz de conta quando houver).
pub const REGISTRY_FILE: &str = "sources.json";
/// Índice leve persistido.
pub const INDEX_FILE: &str = "sources_index.json";
/// Pasta dos catálogos baixados.
pub const CACHE_DIR: &str = "sources";
/// Limite padrão de `sources.search`.
pub const DEFAULT_LIMIT: usize = 40;
/// Timeout de download de uma fonte (`AbortSignal.timeout(60000)`).
pub const SOURCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

// ── armazenamento injetado ─────────────────────────────────────────────────

/// Armazenamento de arquivos das fontes. Chaves são os caminhos relativos do
/// app (`sources.json`, `sources_index.json`, `sources/{id}.json`).
pub trait SourceStorage: Send + Sync {
    fn read(&self, key: &str) -> Option<Vec<u8>>;
    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()>;
    fn remove(&self, key: &str);
}

/// Armazenamento em disco com os mesmos caminhos do Electron.
///
/// `data_dir` é a raiz global (`ARCADIA_DATA_DIR`); `registry_dir` é a pasta da
/// conta ativa (onde vive `sources.json`). Para o modo sem conta, os dois são
/// iguais.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsSourceStorage {
    data_dir: PathBuf,
    registry_dir: PathBuf,
}

impl FsSourceStorage {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            registry_dir: data_dir.clone(),
            data_dir,
        }
    }

    /// `sources.json` em `registry_dir`; cache/índice em `data_dir`.
    pub fn with_registry(data_dir: impl Into<PathBuf>, registry_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            registry_dir: registry_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn registry_dir(&self) -> &Path {
        &self.registry_dir
    }

    fn path_for(&self, key: &str) -> PathBuf {
        if key == REGISTRY_FILE {
            self.registry_dir.join(key)
        } else {
            self.data_dir.join(key)
        }
    }
}

impl SourceStorage for FsSourceStorage {
    fn read(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path_for(key)).ok()
    }

    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.path_for(key);
        let Some(parent) = path.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "caminho sem diretório",
            ));
        };
        std::fs::create_dir_all(parent)?;
        let tmp = path.with_extension(format!(
            "{}tmp",
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        std::fs::write(&tmp, data)?;
        std::fs::rename(tmp, path)
    }

    fn remove(&self, key: &str) {
        let _ = std::fs::remove_file(self.path_for(key));
    }
}

/// Armazenamento em memória (testes e chamadores sem disco).
#[derive(Debug, Default)]
pub struct MemorySourceStorage {
    files: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemorySourceStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.lock().contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.lock().get(key).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.lock().keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Injeta conteúdo diretamente (fixtures de teste).
    pub fn put(&self, key: &str, data: impl Into<Vec<u8>>) {
        self.lock().insert(key.to_string(), data.into());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SourceStorage for MemorySourceStorage {
    fn read(&self, key: &str) -> Option<Vec<u8>> {
        self.get(key)
    }

    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        self.lock().insert(key.to_string(), data.to_vec());
        Ok(())
    }

    fn remove(&self, key: &str) {
        self.lock().remove(key);
    }
}

// ── relógio, catálogo remoto e desafio Cloudflare ──────────────────────────

/// Relógio de `Date.now()`; testes usam [`ManualClock`].
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> u64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[derive(Debug, Default)]
pub struct ManualClock {
    millis: AtomicU64,
}

impl ManualClock {
    pub fn new(millis: u64) -> Self {
        Self {
            millis: AtomicU64::new(millis),
        }
    }

    pub fn set(&self, millis: u64) {
        self.millis.store(millis, Ordering::SeqCst);
    }

    pub fn advance(&self, millis: u64) {
        self.millis.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.millis.load(Ordering::SeqCst)
    }
}

/// Catálogo remoto de uma fonte (equivalente ao `catalogGet` de `carregarFonte`).
#[async_trait]
pub trait SourceCatalog: Send + Sync {
    /// Devolve o objeto `data` (`{ name, downloads: [...] }`) ou `None`.
    async fn source_games(&self, source_id: &str) -> Option<Value>;
}

#[async_trait]
impl SourceCatalog for CatalogClient {
    async fn source_games(&self, source_id: &str) -> Option<Value> {
        let response = self
            .get(&format!("/catalog/v1/sources/{source_id}/games"))
            .await;
        let payload = response.data?;
        let data = payload.get("data")?;
        if data.get("downloads").map(Value::is_array).unwrap_or(false) {
            Some(data.clone())
        } else {
            None
        }
    }
}

/// Resolve o desafio Cloudflare via janela/navegador real. O crate não abre
/// janelas; em produção quem tiver essa capacidade injeta a implementação.
#[async_trait]
pub trait ChallengeSolver: Send + Sync {
    async fn resolve(&self, url: &str) -> Option<Value>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoChallenge;

#[async_trait]
impl ChallengeSolver for NoChallenge {
    async fn resolve(&self, _url: &str) -> Option<Value> {
        None
    }
}

// ── tipos públicos ─────────────────────────────────────────────────────────

/// Entrada do registro local — mesmos campos do Electron, inclusive os
/// desconhecidos que já estavam no arquivo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub etag: String,
    #[serde(default, rename = "lastMod")]
    pub last_mod: String,
    #[serde(default, rename = "addedAt")]
    pub added_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SourceInfo {
    pub fn new(id: impl Into<String>, url: impl Into<String>, added_at: u64) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            name: String::new(),
            etag: String::new(),
            last_mod: String::new(),
            added_at,
            count: None,
            extra: Map::new(),
        }
    }
}

/// Item do índice leve. `fold` é interno e não sai na busca.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: String,
    /// Título já dobrado (interno).
    #[serde(default)]
    pub fold: String,
    /// Campo de índices antigos; lido se `fold` faltar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower: Option<String>,
    #[serde(default, rename = "fileSize")]
    pub file_size: String,
    #[serde(default, rename = "uploadDate")]
    pub upload_date: String,
    #[serde(default)]
    pub src: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl IndexEntry {
    pub fn view(&self) -> SearchResult {
        SearchResult {
            reference: self.reference.clone(),
            title: self.title.clone(),
            file_size: self.file_size.clone(),
            upload_date: self.upload_date.clone(),
            src: self.src.clone(),
            extra: self.extra.clone(),
        }
    }
}

/// Resultado de busca (sem `fold`/`lower`, como o `...leve` do JS).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: String,
    #[serde(default, rename = "fileSize")]
    pub file_size: String,
    #[serde(default, rename = "uploadDate")]
    pub upload_date: String,
    #[serde(default)]
    pub src: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Envelope de `sources:add`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddSourceResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AddSourceResult {
    pub fn ok(source: SourceInfo) -> Self {
        Self {
            ok: true,
            source: Some(source),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            source: None,
            error: Some(message.into()),
        }
    }
}

/// Envelope de `sources:remove`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoveSourceResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RemoveSourceResult {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
        }
    }
}

/// Um item de `sources:sync`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSyncItem {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mudou: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Envelope de `sources:sync`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSyncResult {
    pub ok: bool,
    pub results: Vec<SourceSyncItem>,
}

/// Envelope de `sources:game`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceGameResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SourceGameResult {
    pub fn ok(game: Value, source: String) -> Self {
        Self {
            ok: true,
            game: Some(game),
            source: Some(source),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            game: None,
            source: None,
            error: Some(message.into()),
        }
    }
}

/// Resultado de baixar/validar uma fonte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadOutcome {
    pub mudou: bool,
    pub count: u64,
}

/// Resultado de aplicar um `pull_sources`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullOutcome {
    /// `data.length` do JS (inclui linhas já existentes).
    pub pulled: usize,
    pub changed: bool,
    pub added: usize,
}

// ── funções puras: id, URL, validação ──────────────────────────────────────

/// `srcId(url)`: SHA-256 do texto exato da URL, 12 primeiros hex.
pub fn source_id(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    hex::encode(digest)[..12].to_string()
}

/// `^https?://` do JS (case-sensitive, só o prefixo).
pub fn valid_source_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// `true` para ids no formato gerado ([0-9a-f]{12}) ou slug seguro.
pub fn valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

fn cache_key(id: &str) -> String {
    format!("{CACHE_DIR}/{id}.json")
}

fn safe_cache_key(id: &str) -> Option<String> {
    valid_source_id(id).then(|| cache_key(id))
}

/// `validar(data)` do JS: aceita `{ downloads: [...] }` ou array puro e mantém
/// só os itens com `title` e alguma URI não vazia.
pub fn validar_downloads(data: &Value) -> Option<Vec<Value>> {
    let downloads = match data {
        Value::Array(items) => items,
        Value::Object(_) => data.get("downloads")?.as_array()?,
        _ => return None,
    };
    if downloads.is_empty() {
        return None;
    }
    let filtered: Vec<Value> = downloads
        .iter()
        .filter(|download| js_truthy(download.get("title")) && download_has_uri(download))
        .cloned()
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

fn download_has_uri(download: &Value) -> bool {
    if let Some(Value::Array(uris)) = download.get("uris") {
        return uris.iter().any(|uri| {
            truthy_text(Some(uri))
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false)
        });
    }
    if let Some(Value::String(uris)) = download.get("uris") {
        return !uris.trim().is_empty();
    }
    matches!(download.get("uri"), Some(Value::String(uri)) if !uri.trim().is_empty())
}

fn js_display_or_empty(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Array(items) => items
            .iter()
            .map(js_display_or_empty)
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
        Value::Null => String::new(),
    }
}

fn truthy_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if !js_truthy(Some(value)) {
        return None;
    }
    Some(js_display_or_empty(value))
}

// ── título: fold, canonicalização e match (1:1 com sources.js) ─────────────

const STOPWORDS_TITULO: &[&str] = &[
    "the", "a", "an", "of", "and", "or", "to", "in", "on", "at", "for", "with", "from", "de", "da",
    "do", "das", "dos", "e", "y", "la", "el", "los", "las", "del", "le", "les", "des", "du",
];

const MARCA_VERSAO: &[&str] = &[
    "v", "ver", "versao", "version", "build", "patch", "update", "hotfix", "rev", "revision",
];

const CONTEXTO_NUMERO: &[&str] = &[
    "episode", "ep", "part", "chapter", "act", "book", "vol", "volume", "disc", "disk",
];

const UNIDADE_METADADO: &[&str] = &[
    "gb", "mb", "kb", "tb", "gib", "mib", "kib", "tib", "bit", "bits",
];

/// `foldTitulo`: NFD sem diacríticos, ligaduras dobradas e apóstrofos fora.
pub fn fold_titulo(value: &str) -> String {
    let lowered = value.to_lowercase();
    let decomposed: String = lowered.nfd().collect();
    let mut out = String::with_capacity(decomposed.len());
    for character in decomposed.chars() {
        if ('\u{0300}'..='\u{036f}').contains(&character) {
            continue;
        }
        match character {
            'ø' => out.push('o'),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'ß' => out.push_str("ss"),
            'ł' => out.push('l'),
            'đ' => out.push('d'),
            '\'' | '\u{2019}' | '`' | '\u{00b4}' => {}
            _ => out.push(character),
        }
    }
    out
}

fn split_palavras(fold: &str) -> Vec<String> {
    fold.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn palavras_titulo(value: &str) -> Vec<String> {
    split_palavras(&fold_titulo(value))
}

/// Título canônico: palavras exigidas, sequência numérica e forma compacta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalTitle {
    pub palavras: Vec<String>,
    pub seq: HashSet<u32>,
    pub exigidas: Vec<String>,
    pub compacto: String,
}

/// `canonicoFolded(fold, palavrasPre)` do JS.
pub fn canonico_folded(fold: &str, palavras_pre: Option<&[String]>) -> CanonicalTitle {
    let owned;
    let palavras: &[String] = match palavras_pre {
        Some(words) => words,
        None => {
            owned = split_palavras(fold);
            &owned
        }
    };

    let mut seq = HashSet::new();
    let mut exigidas = Vec::new();
    let mut versao = false;

    for (index, palavra) in palavras.iter().enumerate() {
        let palavra = palavra.as_str();
        let anterior = if index > 0 {
            palavras[index - 1].as_str()
        } else {
            ""
        };
        let proxima = palavras.get(index + 1).map(String::as_str).unwrap_or("");

        if is_marca_versao(palavra) {
            let romano_curto = palavra == "v";
            if ends_with_digit(palavra)
                || (starts_with_digit(proxima) && !(romano_curto && is_year(proxima)))
            {
                versao = true;
                continue;
            }
        }
        if versao && starts_with_digit(palavra) {
            continue;
        }
        versao = false;

        let mut numero = 0_u32;
        if is_all_digits(palavra) {
            // Números vizinhos ("1.112.48699928") são versão, não sequência.
            if starts_with_digit(anterior) || starts_with_digit(proxima) {
                continue;
            }
            // Metadado não é sequência do título.
            if anterior == "from"
                || anterior == "alpha"
                || proxima == "dlc"
                || proxima == "dlcs"
                || UNIDADE_METADADO.contains(&proxima)
            {
                continue;
            }
            let parsed = palavra.parse::<u32>().unwrap_or(0);
            if (1..=99).contains(&parsed) {
                numero = parsed;
            } else {
                exigidas.push(palavra.to_string());
                continue;
            }
        } else if let Some(n) = roman_number(palavra) {
            // "I" solto é pronome; só conta com contexto de parte/episódio.
            // "V"/"X" são sequência sempre.
            if palavra.chars().count() > 1
                || palavra == "v"
                || palavra == "x"
                || CONTEXTO_NUMERO.contains(&anterior)
            {
                numero = n;
            }
        } else if let Some(n) = word_number(palavra) {
            if CONTEXTO_NUMERO.contains(&anterior) {
                numero = n;
            }
        }

        if numero > 0 {
            seq.insert(numero);
            exigidas.push(format!("#{numero}"));
            continue;
        }
        if palavra.chars().count() < 2 || STOPWORDS_TITULO.contains(&palavra) {
            continue;
        }
        exigidas.push(palavra.to_string());
    }

    CanonicalTitle {
        palavras: palavras.to_vec(),
        seq,
        exigidas,
        compacto: palavras.join(""),
    }
}

/// `tituloCanonico(value)`.
pub fn titulo_canonico(value: &str) -> CanonicalTitle {
    canonico_folded(&fold_titulo(value), None)
}

/// Resultado de `matchTitulo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleMatch {
    pub score: u8,
    pub extra: usize,
}

/// `matchTitulo(alvo, candidato)`.
pub fn match_titulo(alvo: &str, candidato: &str) -> Option<TitleMatch> {
    match_titulo_with(alvo, candidato, None, None)
}

/// Versão com cache opcional do alvo/`fold` do candidato.
pub fn match_titulo_with(
    alvo: &str,
    candidato: &str,
    alvo_canon: Option<&CanonicalTitle>,
    cand_fold: Option<&str>,
) -> Option<TitleMatch> {
    let canonical = match alvo_canon {
        Some(canonical) => canonical.clone(),
        None => titulo_canonico(alvo),
    };
    let fold = match cand_fold {
        Some(fold) => fold.to_string(),
        None => fold_titulo(candidato),
    };
    let palavras = split_palavras(&fold);
    if canonical.palavras.is_empty() || palavras.is_empty() {
        return None;
    }

    // Corte barato: palavra real do alvo (#n é sequência) tem de existir como
    // token no candidato.
    for exigida in &canonical.exigidas {
        if !exigida.starts_with('#')
            && exigida.chars().count() >= 3
            && !palavras.iter().any(|palavra| palavra == exigida)
        {
            return None;
        }
    }

    let candidate = canonico_folded(&fold, Some(&palavras));
    if candidate.palavras.is_empty() {
        return None;
    }
    if canonical.seq.len() != candidate.seq.len() {
        return None;
    }
    for number in &canonical.seq {
        if !candidate.seq.contains(number) {
            return None;
        }
    }
    let candidatas: HashSet<&str> = candidate.exigidas.iter().map(String::as_str).collect();
    for exigida in &canonical.exigidas {
        if !candidatas.contains(exigida.as_str()) {
            return None;
        }
    }

    let raw_equal = alvo.trim().to_lowercase() == candidato.trim().to_lowercase();
    let mut score = 1_u8;
    if raw_equal {
        score = 5;
    } else if candidate.compacto == canonical.compacto {
        score = 4;
    } else if !canonical.compacto.is_empty() && candidate.compacto.starts_with(&canonical.compacto)
    {
        score = 3;
    } else if !canonical.compacto.is_empty() && candidate.compacto.contains(&canonical.compacto) {
        score = 2;
    }
    Some(TitleMatch {
        score,
        extra: candidate
            .palavras
            .len()
            .saturating_sub(canonical.palavras.len()),
    })
}

/// Busca/ranqueia no índice antes do corte (a release certa pode estar em
/// qualquer posição). Entradas que não são match entram só como fallback
/// parcial, como na aba Fontes.
pub fn buscar_no_indice(index: &[IndexEntry], query: &str, limit: usize) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let query_fold = fold_titulo(query);
    let query_compact = query_fold
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    let query_canon = canonico_folded(&query_fold, None);
    let tokens: Vec<String> = split_palavras(&query_fold)
        .into_iter()
        .filter(|token| token.chars().count() >= 3)
        .collect();

    struct Hit {
        index: usize,
        score: u8,
        extra: usize,
    }
    let mut hits = Vec::new();
    for (index, game) in index.iter().enumerate() {
        let title_fold = if !game.fold.is_empty() {
            game.fold.clone()
        } else {
            fold_titulo(game.lower.as_deref().unwrap_or(&game.title))
        };
        if let Some(matched) =
            match_titulo_with(query, &game.title, Some(&query_canon), Some(&title_fold))
        {
            hits.push(Hit {
                index,
                score: matched.score,
                extra: matched.extra,
            });
            continue;
        }
        let title_compact = title_fold
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>();
        let coincidentes = tokens
            .iter()
            .filter(|token| title_compact.contains(token.as_str()))
            .count();
        let parcial = title_fold.contains(&query_fold)
            || (!query_compact.is_empty() && title_compact.contains(&query_compact))
            || (tokens.len() >= 2 && coincidentes >= 2.min(tokens.len()));
        if parcial {
            hits.push(Hit {
                index,
                score: 0,
                extra: usize::MAX,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.extra.cmp(&b.extra))
            .then_with(|| a.index.cmp(&b.index))
    });
    hits.into_iter()
        .take(limit)
        .map(|hit| index[hit.index].view())
        .collect()
}

// ── helpers de título ──────────────────────────────────────────────────────

fn is_marca_versao(word: &str) -> bool {
    MARCA_VERSAO.iter().any(|prefix| {
        word.strip_prefix(prefix)
            .is_some_and(|rest| rest.chars().all(|character| character.is_ascii_digit()))
    })
}

fn is_all_digits(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|character| character.is_ascii_digit())
}

fn starts_with_digit(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
}

fn ends_with_digit(word: &str) -> bool {
    word.chars()
        .last()
        .is_some_and(|character| character.is_ascii_digit())
}

fn is_year(word: &str) -> bool {
    word.len() == 4
        && (word.starts_with("19") || word.starts_with("20"))
        && word.chars().all(|character| character.is_ascii_digit())
}

fn roman_number(word: &str) -> Option<u32> {
    Some(match word {
        "i" => 1,
        "ii" => 2,
        "iii" => 3,
        "iv" => 4,
        "v" => 5,
        "vi" => 6,
        "vii" => 7,
        "viii" => 8,
        "ix" => 9,
        "x" => 10,
        "xi" => 11,
        "xii" => 12,
        "xiii" => 13,
        _ => return None,
    })
}

fn word_number(word: &str) -> Option<u32> {
    Some(match word {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        _ => return None,
    })
}

// ── sync: payloads puros (push_sources/pull_sources) ────────────────────────

/// Monta o `p_sources` do RPC `push_sources`: fontes ainda não enviadas pelo
/// watermark + tombstones das removidas. Só id/url/name atravessam.
pub fn push_payload(registry: &[SourceInfo], watermarks: &Map<String, Value>) -> Vec<Value> {
    let ids: HashSet<&str> = registry.iter().map(|source| source.id.as_str()).collect();
    let mut payload = Vec::new();
    for source in registry {
        if watermarks.contains_key(&source.id) {
            continue;
        }
        let mut item = Map::new();
        item.insert("source_id".into(), Value::String(source.id.clone()));
        item.insert("url".into(), Value::String(source.url.clone()));
        item.insert("name".into(), Value::String(source.name.clone()));
        payload.push(Value::Object(item));
    }
    for id in watermarks.keys() {
        if !ids.contains(id.as_str()) {
            let mut item = Map::new();
            item.insert("source_id".into(), Value::String(id.clone()));
            item.insert("removed".into(), Value::Bool(true));
            payload.push(Value::Object(item));
        }
    }
    payload
}

/// Watermarks novos depois de um push bem-sucedido: `{url,name}` por fonte e
/// nenhuma entrada para fontes removidas.
pub fn next_watermarks(registry: &[SourceInfo]) -> Map<String, Value> {
    let mut out = Map::new();
    for source in registry {
        let mut item = Map::new();
        item.insert("url".into(), Value::String(source.url.clone()));
        item.insert("name".into(), Value::String(source.name.clone()));
        out.insert(source.id.clone(), Value::Object(item));
    }
    out
}

/// Aplica `pull_sources`: só adiciona ids desconhecidos; nunca atualiza nem
/// apaga os locais. `pulled` é o tamanho da resposta, como no JS.
pub fn pull_sources(
    registry: &mut Vec<SourceInfo>,
    rows: &[Value],
    now_millis: u64,
) -> PullOutcome {
    let mut existing: HashSet<String> = registry.iter().map(|source| source.id.clone()).collect();
    let mut added = 0_usize;
    let mut changed = false;
    for row in rows {
        let Some(id) = row.get("source_id").and_then(source_id_value) else {
            continue;
        };
        if !existing.insert(id.clone()) {
            continue;
        }
        let url = row
            .get("url")
            .and_then(|value| truthy_text(Some(value)))
            .unwrap_or_default();
        let name = row
            .get("name")
            .and_then(|value| truthy_text(Some(value)))
            .unwrap_or_default();
        let mut source = SourceInfo::new(id, url, now_millis);
        source.name = name;
        source.etag = String::new();
        source.last_mod = String::new();
        source.count = Some(0);
        registry.push(source);
        added += 1;
        changed = true;
    }
    PullOutcome {
        pulled: rows.len(),
        changed,
        added,
    }
}

fn source_id_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

// ── registro em memória (fonte/armazenamento) ──────────────────────────────

#[derive(Default)]
struct LastFile {
    id: String,
    data: Option<Value>,
}

struct SourceRegistryInner {
    storage: Arc<dyn SourceStorage>,
    fetcher: Arc<dyn Fetcher>,
    catalog: RwLock<Option<Arc<dyn SourceCatalog>>>,
    challenge: RwLock<Arc<dyn ChallengeSolver>>,
    clock: RwLock<Arc<dyn Clock>>,
    index: Mutex<Option<Vec<IndexEntry>>>,
    last_file: Mutex<LastFile>,
    refreshing: AtomicBool,
}

/// Fachada do registro de fontes; `clone` compartilha índice e cache de
/// arquivo, como os módulos globais do Electron.
#[derive(Clone)]
pub struct SourceRegistry {
    inner: Arc<SourceRegistryInner>,
}

impl SourceRegistry {
    pub fn new(storage: Arc<dyn SourceStorage>, fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            inner: Arc::new(SourceRegistryInner {
                storage,
                fetcher,
                catalog: RwLock::new(None),
                challenge: RwLock::new(Arc::new(NoChallenge)),
                clock: RwLock::new(Arc::new(SystemClock)),
                index: Mutex::new(None),
                last_file: Mutex::new(LastFile::default()),
                refreshing: AtomicBool::new(false),
            }),
        }
    }

    pub fn with_catalog(self, catalog: Arc<dyn SourceCatalog>) -> Self {
        *self
            .inner
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(catalog);
        self
    }

    pub fn with_challenge(self, challenge: Arc<dyn ChallengeSolver>) -> Self {
        *self
            .inner
            .challenge
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = challenge;
        self
    }

    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Self {
        *self
            .inner
            .clock
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = clock;
        self
    }

    /// Troca o catálogo remoto mesmo depois de a instância ser compartilhada.
    pub fn set_catalog(&self, catalog: Arc<dyn SourceCatalog>) {
        *self
            .inner
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(catalog);
    }

    fn catalog_client(&self) -> Option<Arc<dyn SourceCatalog>> {
        self.inner
            .catalog
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn challenge_solver(&self) -> Arc<dyn ChallengeSolver> {
        self.inner
            .challenge
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn now_millis(&self) -> u64 {
        self.inner
            .clock
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .now_millis()
    }

    pub fn storage(&self) -> &Arc<dyn SourceStorage> {
        &self.inner.storage
    }

    // ── CRUD ───────────────────────────────────────────────────────────────

    /// `list`: registro local, na ordem do arquivo.
    pub fn list(&self) -> Vec<SourceInfo> {
        self.read_registry()
    }

    /// `addSource(url)`: valida, baixa/valida/cacheia e só então grava.
    pub async fn add_source(&self, url: &str) -> AddSourceResult {
        let url = url.trim();
        if !valid_source_url(url) {
            return AddSourceResult::error("URL inválida");
        }
        let mut registry = self.read_registry();
        if registry.iter().any(|source| source.url == url) {
            return AddSourceResult::error("fonte já adicionada");
        }
        let mut source = SourceInfo::new(source_id(url), url.to_string(), self.now_millis());
        if let Err(error) = self.download_source(&mut source).await {
            return AddSourceResult::error(error);
        }
        registry.push(source.clone());
        self.write_registry(&registry);
        self.invalidate_index();
        AddSourceResult::ok(source)
    }

    /// `removeSource(id)`.
    pub fn remove_source(&self, id: &str) -> RemoveSourceResult {
        let registry = self.read_registry();
        let remaining: Vec<SourceInfo> = registry
            .iter()
            .filter(|source| source.id != id)
            .cloned()
            .collect();
        if remaining.len() == registry.len() {
            return RemoveSourceResult::error("fonte não encontrada");
        }
        self.write_registry(&remaining);
        if let Some(key) = safe_cache_key(id) {
            self.inner.storage.remove(&key);
        }
        self.invalidate_index();
        RemoveSourceResult::ok()
    }

    /// `syncSources()`: revalida todas as fontes; grava ETags mesmo em `304`.
    pub async fn sync_sources(&self) -> SourceSyncResult {
        let mut registry = self.read_registry();
        let mut results = Vec::with_capacity(registry.len());
        for source in &mut registry {
            match self.download_source(source).await {
                Ok(outcome) => results.push(SourceSyncItem {
                    id: source.id.clone(),
                    ok: true,
                    mudou: Some(outcome.mudou),
                    count: Some(outcome.count),
                    error: None,
                }),
                Err(error) => results.push(SourceSyncItem {
                    id: source.id.clone(),
                    ok: false,
                    mudou: None,
                    count: None,
                    error: Some(error),
                }),
            }
        }
        self.write_registry(&registry);
        self.invalidate_index();
        SourceSyncResult { ok: true, results }
    }

    // ── busca e jogo completo ──────────────────────────────────────────────

    /// `search(query, limit = 40)`.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let index = self.load_index().await;
        buscar_no_indice(&index, query, limit)
    }

    /// `getGame(ref)`: lê o JSON completo só quando o ref muda de fonte.
    pub async fn get_game(&self, reference: &str) -> SourceGameResult {
        let mut parts = reference.split(':');
        let id = parts.next().unwrap_or("");
        let position = parts.next().unwrap_or("");
        let cached = {
            let last = self.lock_last_file();
            if last.id == id {
                last.data.clone()
            } else {
                None
            }
        };
        let data = match cached {
            Some(data) => data,
            None => {
                let registry = self.read_registry();
                let Some(source) = registry.iter().find(|source| source.id == id) else {
                    return SourceGameResult::error("fonte nao encontrada");
                };
                let Some(data) = self.load_source(source).await else {
                    return SourceGameResult::error("catalogo da fonte indisponivel");
                };
                {
                    let mut last = self.lock_last_file();
                    last.id = id.to_string();
                    last.data = Some(data.clone());
                }
                data
            }
        };
        let Some(game) = js_index(position).and_then(|index| {
            data.get("downloads")
                .and_then(Value::as_array)
                .and_then(|items| items.get(index))
        }) else {
            return SourceGameResult::error("jogo não encontrado");
        };
        let source_name = truthy_text(data.get("name")).unwrap_or_default();
        SourceGameResult::ok(game.clone(), source_name)
    }

    /// Reconstrói o índice em memória/disco (usado após mudanças externas).
    pub async fn refresh_index(&self) -> Vec<IndexEntry> {
        let index = self.build_index().await;
        if !index.is_empty() {
            self.write_index(&index);
        }
        *self.lock_index() = Some(index.clone());
        index
    }

    /// Lê o índice persistido sem reconstruir (equivalente ao disco do JS).
    pub fn read_index_file(&self) -> Option<Vec<IndexEntry>> {
        let bytes = self.inner.storage.read(INDEX_FILE)?;
        let index: Vec<IndexEntry> = serde_json::from_slice(&bytes).ok()?;
        if index.is_empty() {
            None
        } else {
            Some(index)
        }
    }

    // ── sync (pull local) ──────────────────────────────────────────────────

    /// Aplica `pull_sources` ao registro. Não invalida o índice, como o pull
    /// do `supabase/sources.js` (a próxima busca reaproveita o índice em disco).
    pub fn apply_pull(&self, rows: &[Value]) -> PullOutcome {
        let mut registry = self.read_registry();
        let outcome = pull_sources(&mut registry, rows, self.now_millis());
        if outcome.changed {
            self.write_registry(&registry);
        }
        outcome
    }

    /// Payload do `push_sources` para o watermark atual.
    pub fn push_payload(&self, watermarks: &Map<String, Value>) -> Vec<Value> {
        push_payload(&self.read_registry(), watermarks)
    }

    /// Watermarks depois de um push bem-sucedido.
    pub fn next_watermarks(&self) -> Map<String, Value> {
        next_watermarks(&self.read_registry())
    }

    // ── download/validação de fonte ────────────────────────────────────────

    async fn download_source(&self, source: &mut SourceInfo) -> Result<DownloadOutcome, String> {
        if !valid_source_id(&source.id) {
            return Err("id de fonte inválido".to_string());
        }
        let mut headers = vec![("user-agent".to_string(), "arcadia".to_string())];
        if !source.etag.is_empty() {
            headers.push(("if-none-match".to_string(), source.etag.clone()));
        }
        if !source.last_mod.is_empty() {
            headers.push(("if-modified-since".to_string(), source.last_mod.clone()));
        }
        let request = Request::get(source.url.clone())
            .headers(headers)
            .timeout(SOURCE_TIMEOUT);
        let response = self
            .inner
            .fetcher
            .fetch(request)
            .await
            .map_err(|error| error.message)?;

        if response.status == 304 {
            return Ok(DownloadOutcome {
                mudou: false,
                count: source.count.unwrap_or(0),
            });
        }

        let challenge = is_cloudflare_challenge(&response);
        if !response.is_success() && !challenge {
            return Err(format!("HTTP {}", response.status));
        }

        let data = if response.is_success() {
            response.json_value().map_err(|error| error.message)?
        } else {
            self.challenge_solver()
                .resolve(&source.url)
                .await
                .ok_or_else(|| format!("HTTP {}", response.status))?
        };

        let downloads = validar_downloads(&data)
            .ok_or_else(|| "formato inválido (esperado { downloads: [...] })".to_string())?;
        let name = truthy_text(data.get("name"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| source.name.clone());

        let mut payload = Map::new();
        payload.insert("name".into(), Value::String(name.clone()));
        payload.insert("downloads".into(), Value::Array(downloads.clone()));
        let json =
            serde_json::to_vec(&Value::Object(payload)).map_err(|error| error.to_string())?;
        self.inner
            .storage
            .write(&cache_key(&source.id), &json)
            .map_err(|error| error.to_string())?;

        source.etag = response.find_header("etag").unwrap_or_default().to_string();
        source.last_mod = response
            .find_header("last-modified")
            .unwrap_or_default()
            .to_string();
        source.name = name;
        source.count = Some(downloads.len() as u64);
        Ok(DownloadOutcome {
            mudou: true,
            count: downloads.len() as u64,
        })
    }

    async fn load_source(&self, source: &SourceInfo) -> Option<Value> {
        let catalog = self.catalog_client();
        if let Some(catalog) = catalog {
            if let Some(data) = catalog.source_games(&source.id).await {
                if data.get("downloads").map(Value::is_array).unwrap_or(false) {
                    return Some(data);
                }
            }
        }
        let key = safe_cache_key(&source.id)?;
        let bytes = self.inner.storage.read(&key)?;
        serde_json::from_slice(&bytes).ok()
    }

    async fn build_index(&self) -> Vec<IndexEntry> {
        let mut index = Vec::new();
        for source in self.read_registry() {
            let Some(data) = self.load_source(&source).await else {
                continue;
            };
            let Some(downloads) = data.get("downloads").and_then(Value::as_array) else {
                continue;
            };
            for (position, download) in downloads.iter().enumerate() {
                if !js_truthy(download.get("title")) {
                    continue;
                }
                let title = truthy_text(download.get("title")).unwrap_or_default();
                let source_name = if !source.name.is_empty() {
                    source.name.clone()
                } else if let Some(name) =
                    truthy_text(data.get("name")).filter(|name| !name.is_empty())
                {
                    name
                } else {
                    source.id.clone()
                };
                index.push(IndexEntry {
                    reference: format!("{}:{position}", source.id),
                    fold: fold_titulo(&title),
                    title,
                    lower: None,
                    file_size: truthy_text(download.get("fileSize"))
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    upload_date: truthy_text(download.get("uploadDate"))
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    src: source_name,
                    extra: Map::new(),
                });
            }
        }
        index
    }

    async fn load_index(&self) -> Vec<IndexEntry> {
        if let Some(index) = self.lock_index().clone() {
            return index;
        }
        if let Some(index) = self.read_index_file() {
            *self.lock_index() = Some(index.clone());
            self.spawn_background_refresh();
            return index;
        }
        let index = self.build_index().await;
        if !index.is_empty() {
            self.write_index(&index);
        }
        *self.lock_index() = Some(index.clone());
        index
    }

    /// Revalida em background sem travar a busca, como o `loadIndex` do JS.
    fn spawn_background_refresh(&self) {
        if self.inner.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            self.inner.refreshing.store(false, Ordering::SeqCst);
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let index = this.build_index().await;
            if !index.is_empty() {
                *this.lock_index() = Some(index.clone());
                this.write_index(&index);
            }
            this.inner.refreshing.store(false, Ordering::SeqCst);
        });
    }

    fn invalidate_index(&self) {
        *self.lock_index() = None;
        self.inner.storage.remove(INDEX_FILE);
        *self.lock_last_file() = LastFile::default();
    }

    fn write_index(&self, index: &[IndexEntry]) {
        if let Ok(bytes) = serde_json::to_vec(index) {
            let _ = self.inner.storage.write(INDEX_FILE, &bytes);
        }
    }

    // ── registro ───────────────────────────────────────────────────────────

    fn read_registry(&self) -> Vec<SourceInfo> {
        let Some(bytes) = self.inner.storage.read(REGISTRY_FILE) else {
            return Vec::new();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    fn write_registry(&self, registry: &[SourceInfo]) {
        if let Ok(bytes) = serde_json::to_vec_pretty(registry) {
            let _ = self.inner.storage.write(REGISTRY_FILE, &bytes);
        }
    }

    fn lock_index(&self) -> std::sync::MutexGuard<'_, Option<Vec<IndexEntry>>> {
        self.inner
            .index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_last_file(&self) -> std::sync::MutexGuard<'_, LastFile> {
        self.inner
            .last_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn is_cloudflare_challenge(response: &crate::http::Response) -> bool {
    if response.status != 403 {
        return false;
    }
    response
        .find_header("cf-mitigated")
        .is_some_and(|value| !value.is_empty())
        || response.find_header("server") == Some("cloudflare")
}

fn js_index(raw: &str) -> Option<usize> {
    let text = raw.trim();
    if text.is_empty() {
        return Some(0);
    }
    let number = text.parse::<f64>().ok()?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > usize::MAX as f64 {
        return None;
    }
    Some(number as usize)
}
