//! Cliente do catálogo servido pelo backend — port 1:1 de
//! `app/electron/catalog.js`.
//!
//! A resposta mais recente fica em um espelho local e é usada quando o
//! servidor está indisponível. O espelho é revalidado com `ETag` /
//! `If-None-Match`: um `304` devolve o JSON já gravado sem baixar de novo.
//! **Não existe TTL** no cliente congelado — a validade é decidida pelo
//! servidor por ETag; o que muda com o espelho presente é o timeout (5 s em
//! vez de 30 s) e o fallback offline.
//!
//! Diferenças deliberadas (documentadas):
//!
//! - o armazenamento do espelho é injetado ([`MirrorStore`]); a produção usa
//!   [`FsMirrorStore`] no mesmo `catalog_espelho/` do Electron;
//! - os cabeçalhos de autenticação vêm de [`AuthProvider`] em vez de
//!   `supabase/client._authHeaders()`;
//! - o resultado é um [`CatalogResponse`] tipado, serializável com os mesmos
//!   nomes de campo (`data`, `error`, `fallback`, `notModified`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use url::Url;

use crate::http::{Fetcher, HttpClientFetcher, Request};

/// Prefixo aceito por `catalogGet`; qualquer outro caminho é recusado.
pub const CATALOG_PREFIX: &str = "/catalog/v1/";
/// Pasta do espelho dentro do diretório de dados.
pub const ESPELHO_DIR_NAME: &str = "catalog_espelho";
/// Timeout quando já existe espelho em disco.
pub const MIRROR_TIMEOUT: Duration = Duration::from_secs(5);
/// Timeout sem espelho (primeira instalação / `noMirror`).
pub const FULL_TIMEOUT: Duration = Duration::from_secs(30);

/// `{ message }` — o envelope de erro do cliente congelado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogError {
    pub message: String,
}

impl CatalogError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Resultado de uma chamada ao catálogo.
///
/// `fallback` e `notModified` são omitidos quando o JS também os omitiria
/// (`Option::None`); `data` e `error` aparecem sempre, inclusive como `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogResponse {
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<CatalogError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "notModified"
    )]
    pub not_modified: Option<bool>,
}

impl CatalogResponse {
    pub fn ok(data: Value) -> Self {
        Self {
            data: Some(data),
            error: None,
            fallback: Some(false),
            not_modified: None,
        }
    }

    pub fn fallback(data: Value) -> Self {
        Self {
            data: Some(data),
            error: None,
            fallback: Some(true),
            not_modified: None,
        }
    }

    pub fn fallback_not_modified(data: Value) -> Self {
        Self {
            data: Some(data),
            error: None,
            fallback: Some(true),
            not_modified: Some(true),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            data: None,
            error: Some(CatalogError::new(message)),
            fallback: None,
            not_modified: None,
        }
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error.as_ref().map(|error| error.message.as_str())
    }
}

/// Opções de `catalogGet(pathname, opts)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatalogOptions {
    /// `noMirror`: não lê nem grava espelho/ETag (payload sensível).
    pub no_mirror: bool,
    /// Override de timeout; ausente usa 5 s/30 s conforme o espelho existir.
    pub timeout: Option<Duration>,
}

impl CatalogOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_no_mirror(mut self, no_mirror: bool) -> Self {
        self.no_mirror = no_mirror;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout = Some(Duration::from_millis(timeout_ms));
        self
    }
}

/// Armazenamento do espelho (JSON + ETag por nome de arquivo).
///
/// Falhas de escrita/leitura são silenciosas, como no JS: o espelho é
/// otimização e nunca pode derrubar o catálogo online.
pub trait MirrorStore: Send + Sync {
    fn read_json(&self, name: &str) -> Option<Value>;
    fn write_json(&self, name: &str, value: &Value);
    fn read_etag(&self, name: &str) -> Option<String>;
    fn write_etag(&self, name: &str, etag: &str);
    fn exists(&self, name: &str) -> bool;
}

/// Espelho em disco: `{dir}/{name}.json` e `{dir}/{name}.json.etag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMirrorStore {
    dir: PathBuf,
}

impl FsMirrorStore {
    /// `new(data_dir)` usa `data_dir/catalog_espelho`, como `ESPELHO_DIR`.
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            dir: data_dir.as_ref().join(ESPELHO_DIR_NAME),
        }
    }

    /// Usa exatamente `dir` como pasta do espelho.
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn etag_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.etag"))
    }
}

impl MirrorStore for FsMirrorStore {
    fn read_json(&self, name: &str) -> Option<Value> {
        let text = std::fs::read_to_string(self.path(name)).ok()?;
        serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|value| !value.is_null())
    }

    fn write_json(&self, name: &str, value: &Value) {
        let Ok(text) = serde_json::to_string(value) else {
            return;
        };
        let Ok(()) = std::fs::create_dir_all(&self.dir) else {
            return;
        };
        let destination = self.path(name);
        let tmp = self.dir.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(tmp, destination);
        }
    }

    fn read_etag(&self, name: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.etag_path(name)).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn write_etag(&self, name: &str, etag: &str) {
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = std::fs::write(self.etag_path(name), etag);
        }
    }

    fn exists(&self, name: &str) -> bool {
        self.path(name).exists()
    }
}

/// Espelho em memória para testes e para chamadores sem disco.
#[derive(Debug, Default)]
pub struct MemoryMirrorStore {
    objects: Mutex<HashMap<String, Value>>,
    etags: Mutex<HashMap<String, String>>,
}

impl MemoryMirrorStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.lock_objects().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.lock_objects().clear();
        self.lock_etags().clear();
    }

    fn lock_objects(&self) -> std::sync::MutexGuard<'_, HashMap<String, Value>> {
        self.objects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_etags(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.etags
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl MirrorStore for MemoryMirrorStore {
    fn read_json(&self, name: &str) -> Option<Value> {
        self.lock_objects()
            .get(name)
            .filter(|value| !value.is_null())
            .cloned()
    }

    fn write_json(&self, name: &str, value: &Value) {
        self.lock_objects().insert(name.to_string(), value.clone());
    }

    fn read_etag(&self, name: &str) -> Option<String> {
        self.lock_etags().get(name).cloned()
    }

    fn write_etag(&self, name: &str, etag: &str) {
        self.lock_etags().insert(name.to_string(), etag.to_string());
    }

    fn exists(&self, name: &str) -> bool {
        self.lock_objects().contains_key(name)
    }
}

/// Cabeçalhos de autenticação equivalentes a `supabase/client._authHeaders()`.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    fn headers(&self) -> Vec<(String, String)>;

    /// Chamado no primeiro `401`; o cliente repete a requisição uma única vez
    /// mesmo que a renovação falhe, exatamente como o JS.
    async fn refresh(&self) -> bool {
        false
    }
}

/// Sem autenticação (catálogo público/testes).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoAuth;

#[async_trait]
impl AuthProvider for NoAuth {
    fn headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Cabeçalhos fixos; bom para testes e para integrações que já têm sessão.
#[derive(Debug, Clone, Default)]
pub struct StaticAuth {
    headers: Vec<(String, String)>,
}

impl StaticAuth {
    pub fn new(headers: Vec<(String, String)>) -> Self {
        Self { headers }
    }

    /// `apikey: <anon>` + `authorization: Bearer <token>`; token vazio omite o
    /// bearer, como o cliente.
    pub fn with_anon_key(anon_key: impl Into<String>, token: Option<String>) -> Self {
        let mut headers = vec![("apikey".to_string(), anon_key.into())];
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        Self { headers }
    }
}

#[async_trait]
impl AuthProvider for StaticAuth {
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }
}

/// Nome do arquivo de espelho para um caminho de catálogo.
///
/// Port literal de `espelhoPath`: remove `catalog/v1` do início, troca cada
/// code unit fora de `[A-Za-z0-9_-]` por `_`, ordena os pares da query e
/// acrescenta os 12 primeiros hex do SHA-256 da query.
pub fn mirror_file_name(pathname: &str) -> String {
    let base = Url::parse("http://catalog.local/").expect("base fixa válida");
    let url = Url::options()
        .base_url(Some(&base))
        .parse(pathname)
        .unwrap_or_else(|_| base.clone());

    let mut parts: Vec<String> = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if parts.first().map(String::as_str) == Some("catalog")
        && parts.get(1).map(String::as_str) == Some("v1")
    {
        parts.drain(0..2);
    }
    let name = parts
        .iter()
        .map(|part| sanitize_mirror_part(part))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let name = if name.is_empty() {
        "catalog".to_string()
    } else {
        name
    };

    let mut query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    query.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let query = query
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let suffix = if query.is_empty() {
        String::new()
    } else {
        let digest = Sha256::digest(query.as_bytes());
        let hex = hex::encode(digest);
        format!("_{}", &hex[..12])
    };
    format!("{name}{suffix}.json")
}

fn sanitize_mirror_part(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for unit in part.encode_utf16() {
        let character = char::from_u32(unit as u32).unwrap_or('\u{fffd}');
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            out.push(character);
        } else {
            out.push('_');
        }
    }
    out
}

/// Caminho completo do espelho (equivalente a `espelhoPath`).
pub fn catalog_mirror_path(data_dir: impl AsRef<Path>, pathname: &str) -> PathBuf {
    data_dir
        .as_ref()
        .join(ESPELHO_DIR_NAME)
        .join(mirror_file_name(pathname))
}

type InFlightEntry = Arc<watch::Sender<Option<CatalogResponse>>>;
type InFlightMap = HashMap<String, InFlightEntry>;

#[derive(Clone)]
pub struct CatalogClient {
    fetcher: Arc<dyn Fetcher>,
    store: Arc<dyn MirrorStore>,
    auth: Arc<dyn AuthProvider>,
    base_url: String,
    in_flight: Arc<Mutex<InFlightMap>>,
}

impl CatalogClient {
    pub fn new(
        fetcher: Arc<dyn Fetcher>,
        store: Arc<dyn MirrorStore>,
        auth: Arc<dyn AuthProvider>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            fetcher,
            store,
            auth,
            base_url: base_url.into(),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Cliente pronto para produção/testes locais sobre o `arcadia-api`.
    pub fn from_base_url(
        base_url: &str,
        store: Arc<dyn MirrorStore>,
        auth: Arc<dyn AuthProvider>,
    ) -> Result<Self, crate::http::FetchError> {
        let fetcher = HttpClientFetcher::from_base_url(base_url)?;
        Ok(Self::new(Arc::new(fetcher), store, auth, base_url))
    }

    /// Usa um [`arcadia_api::HttpClient`] já configurado (inclui a lista de
    /// endereços primário/reserva). A base das URLs é `config().url`.
    pub fn from_http_client(
        client: arcadia_api::HttpClient,
        store: Arc<dyn MirrorStore>,
        auth: Arc<dyn AuthProvider>,
    ) -> Self {
        let base_url = client.config().url.clone();
        Self::new(
            Arc::new(HttpClientFetcher::new(client)),
            store,
            auth,
            base_url,
        )
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn store(&self) -> &Arc<dyn MirrorStore> {
        &self.store
    }

    pub fn fetcher(&self) -> &Arc<dyn Fetcher> {
        &self.fetcher
    }

    pub fn auth(&self) -> &Arc<dyn AuthProvider> {
        &self.auth
    }

    /// `catalogGetEspelho(pathname)`.
    pub fn read_mirror(&self, pathname: &str) -> Option<Value> {
        self.store.read_json(&mirror_file_name(pathname))
    }

    /// `catalogGet(pathname)` com as opções padrão.
    pub async fn get(&self, pathname: &str) -> CatalogResponse {
        self.get_with(pathname, CatalogOptions::default()).await
    }

    /// `catalogGet(pathname, opts)`.
    pub async fn get_with(&self, pathname: &str, opts: CatalogOptions) -> CatalogResponse {
        if !pathname.starts_with(CATALOG_PREFIX) {
            return CatalogResponse::failure("caminho de catalogo invalido");
        }

        // Dedupe em voo: chamadas simultâneas para o mesmo caminho dividem a
        // mesma promessa, como o `Map` `emVoo` do JS.
        let existing = {
            let map = self.lock_in_flight();
            map.get(pathname).cloned()
        };
        if let Some(sender) = existing {
            let mut receiver = sender.subscribe();
            loop {
                if let Some(response) = receiver.borrow().clone() {
                    return response;
                }
                if receiver.changed().await.is_err() {
                    return CatalogResponse::failure("requisicao cancelada");
                }
            }
        }

        let (sender, _) = watch::channel(None);
        let sender = Arc::new(sender);
        {
            let mut map = self.lock_in_flight();
            map.insert(pathname.to_string(), Arc::clone(&sender));
        }
        let mut guard = LeaderGuard {
            client: self.clone(),
            pathname: pathname.to_string(),
            sender: Arc::clone(&sender),
            armed: true,
        };

        let response = self.lead(pathname, opts).await;

        let _ = sender.send(Some(response.clone()));
        self.lock_in_flight().remove(pathname);
        guard.armed = false;
        response
    }

    fn lock_in_flight(&self) -> std::sync::MutexGuard<'_, InFlightMap> {
        self.in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn lead(&self, pathname: &str, opts: CatalogOptions) -> CatalogResponse {
        match self.fetch_once(pathname, opts).await {
            Attempt::Done(response) => response,
            Attempt::Renew => {
                // Como o JS: renova e tenta UMA vez, independentemente de a
                // renovação ter tido sucesso; se continuar 401, cai no espelho.
                let _ = self.auth.refresh().await;
                match self.fetch_once(pathname, opts).await {
                    Attempt::Done(response) => response,
                    Attempt::Renew => self.fallback_or(pathname, opts, "HTTP 401"),
                }
            }
        }
    }

    async fn fetch_once(&self, pathname: &str, opts: CatalogOptions) -> Attempt {
        let name = mirror_file_name(pathname);
        let base = self.base_url.strip_suffix('/').unwrap_or(&self.base_url);
        let url = format!("{base}{pathname}");

        let mut headers = self.auth.headers();
        headers.push(("accept".into(), "application/json".into()));
        headers.push(("accept-encoding".into(), "gzip".into()));
        if !opts.no_mirror {
            if let Some(etag) = self.store.read_etag(&name) {
                if !etag.is_empty() {
                    headers.push(("if-none-match".into(), etag));
                }
            }
        }

        let has_mirror = !opts.no_mirror && self.store.exists(&name);
        let timeout = opts.timeout.unwrap_or(if has_mirror {
            MIRROR_TIMEOUT
        } else {
            FULL_TIMEOUT
        });
        let request = Request::get(url).headers(headers).timeout(timeout);

        let response = match self.fetcher.fetch(request).await {
            Ok(response) => response,
            Err(error) => return Attempt::Done(self.fallback_or(pathname, opts, error.message)),
        };

        if response.status == 304 {
            return match self.mirror_data(pathname, opts) {
                Some(data) => Attempt::Done(CatalogResponse::fallback_not_modified(data)),
                None => Attempt::Done(CatalogResponse::failure("HTTP 304 sem espelho")),
            };
        }

        if !response.is_success() {
            if response.status == 401 {
                return Attempt::Renew;
            }
            return Attempt::Done(self.fallback_or(
                pathname,
                opts,
                format!("HTTP {}", response.status),
            ));
        }

        if !opts.no_mirror {
            if let Some(etag) = response.find_header("etag") {
                if !etag.is_empty() {
                    self.store.write_etag(&name, etag);
                }
            }
        }

        match response.json_value() {
            Ok(data) => {
                if !opts.no_mirror {
                    self.store.write_json(&name, &data);
                }
                Attempt::Done(CatalogResponse::ok(data))
            }
            Err(error) => Attempt::Done(self.fallback_or(pathname, opts, error.message)),
        }
    }

    fn mirror_data(&self, pathname: &str, opts: CatalogOptions) -> Option<Value> {
        if opts.no_mirror {
            None
        } else {
            self.store.read_json(&mirror_file_name(pathname))
        }
    }

    fn fallback_or(
        &self,
        pathname: &str,
        opts: CatalogOptions,
        message: impl Into<String>,
    ) -> CatalogResponse {
        match self.mirror_data(pathname, opts) {
            Some(data) => CatalogResponse::fallback(data),
            None => CatalogResponse::failure(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Attempt {
    Done(CatalogResponse),
    Renew,
}

struct LeaderGuard {
    client: CatalogClient,
    pathname: String,
    sender: Arc<watch::Sender<Option<CatalogResponse>>>,
    armed: bool,
}

impl Drop for LeaderGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Chamador cancelado: libera os waiters com erro e remove a entrada.
        let _ = self
            .sender
            .send(Some(CatalogResponse::failure("requisicao cancelada")));
        self.client.lock_in_flight().remove(&self.pathname);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_name_matches_frozen_layout() {
        assert_eq!(mirror_file_name("/catalog/v1/popular"), "popular.json");
        assert_eq!(
            mirror_file_name("/catalog/v1/sources/abc/games"),
            "sources_abc_games.json"
        );
        assert_eq!(mirror_file_name(""), "catalog.json");
        assert_eq!(mirror_file_name("/"), "catalog.json");
    }

    #[test]
    fn mirror_name_separates_and_order_normalizes_queries() {
        let a = mirror_file_name("/catalog/v1/items?appids=1");
        let b = mirror_file_name("/catalog/v1/items?appids=2");
        assert_ne!(a, b);
        assert_eq!(
            mirror_file_name("/catalog/v1/items?a=1&b=2"),
            mirror_file_name("/catalog/v1/items?b=2&a=1")
        );
        assert!(a.starts_with("items_"));
        assert!(a.ends_with(".json"));
    }

    #[test]
    fn mirror_name_never_escapes_the_directory() {
        let name = mirror_file_name("/catalog/v1/sources/../../passwd/games?q=x");
        assert!(!name.contains(".."));
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        let path = catalog_mirror_path("/tmp/data", "/catalog/v1/sources/../../passwd/games?q=x");
        assert!(path.starts_with(std::path::Path::new("/tmp/data").join(ESPELHO_DIR_NAME)));
    }

    #[test]
    fn response_serializes_exact_field_names_and_omits_optional_flags() {
        let ok = CatalogResponse::ok(serde_json::json!({ "ok": true }));
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            serde_json::json!({
                "data": { "ok": true },
                "error": null,
                "fallback": false
            })
        );
        let invalid = CatalogResponse::failure("caminho de catalogo invalido");
        assert_eq!(
            serde_json::to_value(&invalid).unwrap(),
            serde_json::json!({
                "data": null,
                "error": { "message": "caminho de catalogo invalido" }
            })
        );
        let modified = CatalogResponse::fallback_not_modified(serde_json::json!(1));
        assert_eq!(modified.not_modified, Some(true));
        assert_eq!(modified.fallback, Some(true));
    }
}
