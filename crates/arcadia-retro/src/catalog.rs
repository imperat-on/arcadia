//! Catálogo retrô — ports 1:1 de `retro-catalog.js` (V1 + cache),
//! `retro-catalog-builder.js` (catálogo canônico), `retro-catalog-v2.js`
//! (store V2) e `retro-server-catalog.js` (envelopes/fallback local).
//!
//! A rede é sempre injetada via [`Fetcher`]; o disco usa `dataDir` (testes com
//! diretórios temporários). Os envelopes devolvidos são exatamente os que o
//! `main.js` entrega em `retro:list`, `retro:game`, `retro:offer`,
//! `retro:stats`, `retro:audit` e `retro:migrate`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcadia_catalog::http::{encode_uri_component, form_urlencode, Fetcher, Request};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::Url;

use crate::matcher::{
    js_locale_compare, match_batch_with, Hashes, MatchOptions, MatchQuality, RetroGame, RetroOffer,
};
use crate::offers::{process_source_downloads, Offer, OfferSource};
use crate::repository::{RepositoryError, RetroRepository};
use crate::title::{clean_text, js_trim, normalize_retro_title};
use crate::util::{
    field_or_string, js_or_string, js_safe_integer, js_truthy, now_ms, safe_read_json,
    safe_write_json, sha256_hex,
};

pub const HYDRA_API: &str = "https://api.hydralibrary.com/sources?status=Classics&limit=100";
pub const CACHE_FILE_NAME: &str = "retro-catalog.json";
pub const CACHE_VERSION: u64 = 1;
pub const CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const MAX_SOURCES: usize = 100;
pub const MAX_GAMES: usize = 50_000;
pub const MAX_URIS: usize = 16;
pub const MAX_TITLE: usize = 300;
pub const MAX_RAW_TITLE: usize = 2048;
pub const MAX_DESCRIPTION: usize = 2400;
pub const MAX_LIMIT: usize = 48;
pub const MAX_QUERY: usize = 120;
pub const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
const API_LISTING_TIMEOUT_MS: u64 = 20_000;
const SOURCE_TIMEOUT_MS: u64 = 60_000;

/// Falha de catálogo, com as fontes parciais quando existirem.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct CatalogError {
    pub message: String,
    pub sources: Vec<CatalogSource>,
}

impl CatalogError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sources: Vec::new(),
        }
    }

    pub fn with_sources(message: impl Into<String>, sources: Vec<CatalogSource>) -> Self {
        Self {
            message: message.into(),
            sources,
        }
    }
}

impl From<arcadia_catalog::http::FetchError> for CatalogError {
    fn from(error: arcadia_catalog::http::FetchError) -> Self {
        Self::new(error.message)
    }
}

// ── normalização V1 ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogSource {
    pub id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub games_count: i64,
    pub status: Vec<String>,
    pub registry_url: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl CatalogSource {
    pub fn to_offer_source(&self) -> OfferSource {
        OfferSource::new(self.id.clone(), self.title.clone())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogGame {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    pub source_id: String,
    pub source_title: String,
    pub platform: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capa: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_cover: Option<String>,
    pub file_size: String,
    pub upload_date: String,
    pub uris: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Catalog {
    pub version: u64,
    pub updated_at: i64,
    pub sources: Vec<CatalogSource>,
    pub games: Vec<CatalogGame>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            updated_at: 0,
            sources: Vec::new(),
            games: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// `sourceId`: IDs alfanuméricos de até 64 caracteres (ou numéricos).
pub fn source_id(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let id = js_trim(&js_or_string(Some(value))).to_string();
    if id.is_empty() {
        return String::new();
    }
    let valid = id
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        });
    if valid {
        id
    } else {
        String::new()
    }
}

/// `normalizeSource`: aceita apenas fontes com status/tag `Classics`.
pub fn normalize_source(value: &Value) -> Option<CatalogSource> {
    let object = value.as_object()?;
    let status_values = ["status", "tags", "statuses", "tag"]
        .iter()
        .flat_map(|key| match object.get(*key) {
            Some(Value::Array(items)) => items.clone(),
            Some(item) => vec![item.clone()],
            None => Vec::new(),
        })
        .collect::<Vec<_>>();
    let status: Vec<String> = status_values
        .iter()
        .filter_map(Value::as_str)
        .flat_map(|text| text.split([';', ',', '|']))
        .map(|text| js_trim(text).to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !status
        .iter()
        .any(|value| value.eq_ignore_ascii_case("classics"))
    {
        return None;
    }

    let id_value = object
        .get("id")
        .filter(|value| !value.is_null())
        .or_else(|| object.get("sourceId"));
    let id = source_id(id_value);
    let url_value = object
        .get("url")
        .filter(|value| js_truthy(Some(value)))
        .or_else(|| object.get("link").filter(|value| js_truthy(Some(value))));
    let url = url_value
        .and_then(|value| safe_public_url_value(value, 2048))
        .unwrap_or_default();
    let title_value = object
        .get("title")
        .filter(|value| js_truthy(Some(value)))
        .or_else(|| object.get("name").filter(|value| js_truthy(Some(value))));
    let title = clean_text(&js_or_string(title_value), 160);
    if id.is_empty() || url.is_empty() || title.is_empty() {
        return None;
    }

    let games_count = js_safe_integer(object.get("gamesCount"))
        .map(|value| value.max(0))
        .unwrap_or(0);
    Some(CatalogSource {
        registry_url: format!(
            "https://library.hydra.wiki/sources/{}",
            encode_uri_component(&id)
        ),
        id,
        title,
        description: clean_text(&field_or_string(value, "description"), 500),
        url,
        games_count,
        status,
        extra: Map::new(),
    })
}

/// `safePublicUrl` para um `Value` (só aceita strings, como o JS).
pub fn safe_public_url_value(value: &Value, max_length: usize) -> Option<String> {
    let Value::String(text) = value else {
        return None;
    };
    safe_public_url_str(text, max_length)
}

/// `safePublicUrl`: HTTPS público, sem hash/userinfo/porta não padrão.
pub fn safe_public_url_str(value: &str, max_length: usize) -> Option<String> {
    if value.encode_utf16().count() > max_length {
        return None;
    }
    let trimmed = js_trim(value);
    if trimmed.is_empty() {
        return None;
    }
    let mut url = Url::parse(trimmed).ok()?;
    if url.fragment().is_some_and(|fragment| !fragment.is_empty()) {
        return None;
    }
    if url.scheme() != "https" {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some_and(|password| !password.is_empty()) {
        return None;
    }
    match url.port() {
        None => {}
        Some(443) => {
            let _ = url.set_port(None);
        }
        Some(_) => return None,
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return None;
    }
    if host.starts_with('[') || host.contains(':') {
        return None;
    }
    if let Some(octets) = parse_ipv4(&host) {
        if octets[0] == 10
            || octets[0] == 127
            || (octets[0] == 192 && octets[1] == 168)
            || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        {
            return None;
        }
    }
    Some(url.to_string())
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (index, part) in parts.iter().enumerate() {
        octets[index] = part.parse().ok()?;
    }
    Some(octets)
}

static RE_BTIH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)^urn:btih:[A-Za-z0-9]+$").expect("regex btih")
});

/// `normalizeUri`: magnets válidos ou URLs HTTPS públicas.
pub fn normalize_uri(value: &Value) -> Option<String> {
    let Value::String(text) = value else {
        return None;
    };
    let uri = js_trim(text);
    if uri.is_empty()
        || uri.encode_utf16().count() > 8192
        || uri
            .chars()
            .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'))
    {
        return None;
    }
    if uri.to_ascii_lowercase().starts_with("magnet:") {
        let parsed = Url::parse(uri).ok()?;
        if !parsed.username().is_empty()
            || parsed
                .password()
                .is_some_and(|password| !password.is_empty())
        {
            return None;
        }
        let has_btih = parsed
            .query_pairs()
            .filter(|(key, _)| key == "xt")
            .any(|(_, value)| RE_BTIH.is_match(&value));
        if !has_btih {
            return None;
        }
        let suffix = uri.find(':').map(|index| &uri[index + 1..])?;
        return Some(format!("magnet:{suffix}"));
    }
    safe_public_url_str(uri, 8192)
}

/// Nome legado de `cleanRetroTitle`.
pub fn clean_retro_title(value: &str) -> String {
    normalize_retro_title(value)
}

fn first_truthy<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .find(|field| js_truthy(Some(field)))
}

fn public_image_url(value: &Value, keys: &[&str]) -> Option<String> {
    let field = first_truthy(value, keys)?;
    safe_public_url_value(field, 2048)
}

/// `normalizeGame`: um download Hydra vira um jogo da listagem V1.
pub fn normalize_game(
    download: &Value,
    source: &CatalogSource,
    index: usize,
) -> Option<CatalogGame> {
    download.as_object()?;
    let original_title = clean_text(&js_or_string(download.get("title")), MAX_RAW_TITLE);
    let title = clean_retro_title(&original_title);
    let raw_uris: Vec<Value> = match download.get("uris") {
        Some(Value::Array(items)) => items.clone(),
        _ => match download.get("uri") {
            Some(uri) if js_truthy(Some(uri)) => vec![uri.clone()],
            _ => Vec::new(),
        },
    };
    let mut uris = Vec::new();
    for raw in &raw_uris {
        let Some(uri) = normalize_uri(raw) else {
            continue;
        };
        if uris.iter().any(|existing| existing == &uri) {
            continue;
        }
        if uris.len() < MAX_URIS {
            uris.push(uri);
        }
    }
    if title.is_empty() || uris.is_empty() {
        return None;
    }

    let description_value = download
        .get("descriptionHtml")
        .filter(|value| js_truthy(Some(value)))
        .or_else(|| download.get("description"));
    let description = clean_text(&js_or_string(description_value), MAX_DESCRIPTION);
    let cover = public_image_url(
        download,
        &["cover", "image", "thumbnail", "artwork", "header_image"],
    );
    let capa = public_image_url(download, &["capa", "portrait", "coverPortrait"]);
    let fallback_cover = public_image_url(download, &["fallbackCover", "fallback_cover"]);

    Some(CatalogGame {
        id: format!("{}:{}", source.id, index),
        title: title.clone(),
        original_title: if !original_title.is_empty() && original_title != title {
            Some(original_title)
        } else {
            None
        },
        source_id: source.id.clone(),
        source_title: source.title.clone(),
        platform: clean_text(&js_or_string(download.get("platform")), 60),
        description,
        cover,
        capa,
        fallback_cover,
        file_size: clean_text(&js_or_string(download.get("fileSize")), 80),
        upload_date: clean_text(&js_or_string(download.get("uploadDate")), 80),
        uris,
        extra: Map::new(),
    })
}

/// `normalizePayload`: valida o envelope da fonte e normaliza os downloads.
pub fn normalize_payload(data: &Value, source: &CatalogSource) -> Vec<CatalogGame> {
    let Some(object) = data.as_object() else {
        return Vec::new();
    };
    match object.get("name") {
        Some(Value::String(name)) if !js_trim(name).is_empty() => {}
        _ => return Vec::new(),
    }
    let Some(downloads) = object.get("downloads").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut games = Vec::new();
    for (index, download) in downloads.iter().enumerate() {
        if games.len() >= MAX_GAMES {
            break;
        }
        if let Some(game) = normalize_game(download, source, index) {
            games.push(game);
        }
    }
    games
}

// ── cache V1 ───────────────────────────────────────────────────────────────

pub fn read_cache(path: &Path) -> Option<Catalog> {
    let value = safe_read_json(path, MAX_JSON_BYTES)?;
    let object = value.as_object()?;
    if object.get("version").and_then(Value::as_u64) != Some(CACHE_VERSION) {
        return None;
    }
    if !object.get("games")?.is_array() || !object.get("sources")?.is_array() {
        return None;
    }
    let mut catalog: Catalog = serde_json::from_value(value).ok()?;
    for game in &mut catalog.games {
        let original_title = game
            .original_title
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| (!game.title.is_empty()).then_some(game.title.as_str()))
            .unwrap_or("")
            .trim()
            .to_string();
        let title = clean_retro_title(&original_title);
        let title = if title.is_empty() {
            original_title.clone()
        } else {
            title
        };
        game.title = title.clone();
        if !original_title.is_empty() && original_title != title {
            game.original_title = Some(original_title);
        }
    }
    Some(catalog)
}

pub fn write_cache(path: &Path, catalog: &Catalog) {
    if let Ok(value) = serde_json::to_value(catalog) {
        let _ = safe_write_json(path, &value, false);
    }
}

pub fn source_cache_path(data_dir: &Path, url: &str) -> PathBuf {
    let id: String = sha256_hex(url).chars().take(12).collect();
    data_dir.join("sources").join(format!("{id}.json"))
}

/// `createRetroCatalog` (atalho de construção).
pub fn create_retro_catalog(
    data_dir: impl Into<PathBuf>,
    fetcher: Arc<dyn Fetcher>,
) -> RetroCatalog {
    RetroCatalog::new(data_dir, fetcher)
}

pub fn read_local_source_payload(data_dir: &Path, url: &str) -> Option<Value> {
    safe_read_json(&source_cache_path(data_dir, url), MAX_JSON_BYTES)
}

// ── catálogo V1 ────────────────────────────────────────────────────────────

/// Exportação sem paginação usada pela migração V2.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExport {
    pub sources: Vec<CatalogSource>,
    pub games: Vec<CatalogGame>,
    pub updated_at: i64,
}

impl CatalogExport {
    fn from_catalog(catalog: &Catalog) -> Self {
        Self {
            sources: catalog.sources.clone(),
            games: catalog.games.clone(),
            updated_at: catalog.updated_at,
        }
    }
}

/// Catálogo V1 (Hydra Classics) com cache em disco.
pub struct RetroCatalog {
    data_dir: PathBuf,
    cache_file: PathBuf,
    api_url: String,
    fetcher: Arc<dyn Fetcher>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    source_payloads: HashMap<String, Value>,
    memory: Mutex<Option<Catalog>>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl RetroCatalog {
    pub fn new(data_dir: impl Into<PathBuf>, fetcher: Arc<dyn Fetcher>) -> Self {
        let data_dir = data_dir.into();
        let cache_file = data_dir.join(CACHE_FILE_NAME);
        let memory = read_cache(&cache_file);
        Self {
            data_dir,
            cache_file,
            api_url: HYDRA_API.to_string(),
            fetcher,
            now: Arc::new(now_ms),
            source_payloads: HashMap::new(),
            memory: Mutex::new(memory),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_api_url(mut self, api_url: impl Into<String>) -> Self {
        self.api_url = api_url.into();
        self
    }

    pub fn with_now(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }

    pub fn with_source_payloads(mut self, payloads: HashMap<String, Value>) -> Self {
        self.source_payloads = payloads;
        self
    }

    pub fn cache_file(&self) -> &Path {
        &self.cache_file
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn memory_catalog(&self) -> Option<Catalog> {
        self.memory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn read_json(&self, url: &str, timeout_ms: u64) -> Result<Value, CatalogError> {
        let request = Request::get(url)
            .header("Accept", "application/json")
            .header("User-Agent", "Arcadia-Retro/1")
            .timeout(Duration::from_millis(timeout_ms));
        let response = self.fetcher.fetch(request).await?;
        if !response.is_success() {
            return Err(CatalogError::new(format!("HTTP {}", response.status)));
        }
        let length = response
            .find_header("content-length")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if length > MAX_JSON_BYTES {
            return Err(CatalogError::new("JSON da fonte excede o limite"));
        }
        if response.body.len() as u64 > MAX_JSON_BYTES {
            return Err(CatalogError::new("JSON da fonte excede o limite"));
        }
        response
            .json_value()
            .map_err(|error| CatalogError::new(error.message))
    }

    async fn fetch_source(&self, source: &CatalogSource) -> Result<Vec<CatalogGame>, CatalogError> {
        if let Some(payload) = self.source_payloads.get(&source.id) {
            return Ok(normalize_payload(payload, source));
        }
        match self.read_json(&source.url, SOURCE_TIMEOUT_MS).await {
            Ok(data) => {
                let games = normalize_payload(&data, source);
                if !games.is_empty() {
                    return Ok(games);
                }
                Ok(read_local_source_payload(&self.data_dir, &source.url)
                    .map(|cached| normalize_payload(&cached, source))
                    .unwrap_or(games))
            }
            Err(error) => {
                if let Some(cached) = read_local_source_payload(&self.data_dir, &source.url) {
                    let games = normalize_payload(&cached, source);
                    if !games.is_empty() {
                        return Ok(games);
                    }
                }
                Err(error)
            }
        }
    }

    async fn refresh_inner(&self) -> Result<Catalog, CatalogError> {
        let listing = self
            .read_json(&self.api_url, API_LISTING_TIMEOUT_MS)
            .await?;
        let raw_candidates: Vec<Value> = match &listing {
            Value::Array(items) => items.clone(),
            Value::Object(object) => object
                .get("sources")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut candidates: Vec<CatalogSource> =
            raw_candidates.iter().filter_map(normalize_source).collect();
        candidates.truncate(MAX_SOURCES);
        if candidates.is_empty() {
            return Err(CatalogError::new("Nenhuma fonte Classics encontrada"));
        }

        let mut games: Vec<CatalogGame> = Vec::new();
        for source in &candidates {
            if let Ok(source_games) = self.fetch_source(source).await {
                games.extend(source_games);
            }
            if games.len() >= MAX_GAMES {
                break;
            }
        }
        if games.is_empty() {
            return Err(CatalogError::with_sources(
                "As fontes Classics não retornaram jogos",
                candidates,
            ));
        }
        games.sort_by(|left, right| {
            js_locale_compare(&left.title.to_lowercase(), &right.title.to_lowercase())
                .then_with(|| js_locale_compare(&left.source_id, &right.source_id))
                .then_with(|| js_locale_compare(&left.id, &right.id))
        });
        games.truncate(MAX_GAMES);
        let catalog = Catalog {
            version: CACHE_VERSION,
            updated_at: self.now(),
            sources: candidates,
            games,
            extra: Map::new(),
        };
        {
            let mut memory = self
                .memory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *memory = Some(catalog.clone());
        }
        write_cache(&self.cache_file, &catalog);
        Ok(catalog)
    }

    /// Força a atualização do catálogo (erros propagam).
    pub async fn refresh(&self) -> Result<Catalog, CatalogError> {
        let _guard = self.refresh_lock.lock().await;
        self.refresh_inner().await
    }

    async fn ensure(&self, force: bool) -> Result<Catalog, CatalogError> {
        if !force {
            if let Some(memory) = self.memory_catalog() {
                if self.now() - memory.updated_at < CACHE_TTL_MS {
                    return Ok(memory);
                }
            }
        }
        let had_memory = self.memory_catalog().is_some();
        let _guard = self.refresh_lock.lock().await;
        if !force {
            if let Some(memory) = self.memory_catalog() {
                if self.now() - memory.updated_at < CACHE_TTL_MS {
                    return Ok(memory);
                }
            }
        }
        match self.refresh_inner().await {
            Ok(catalog) => Ok(catalog),
            Err(_error) if !force && had_memory => Ok(self.memory_catalog().unwrap_or_default()),
            Err(error) => Err(error),
        }
    }

    /// `list`: busca/paginação com o mesmo envelope do IPC.
    pub async fn list(&self, params: &Value) -> Value {
        let offset = js_safe_integer(params.get("offset"))
            .map(|value| value.max(0))
            .unwrap_or(0);
        let limit = js_safe_integer(params.get("limit"))
            .map(|value| value.clamp(1, MAX_LIMIT as i64) as usize)
            .unwrap_or(24);
        let force = js_truthy(params.get("refresh"));
        let catalog = match self.ensure(force).await {
            Ok(catalog) => catalog,
            Err(error) => {
                return json!({
                    "ok": false,
                    "games": [],
                    "sources": error.sources,
                    "total": 0,
                    "offset": offset,
                    "limit": limit,
                    "hasMore": false,
                    "error": error.message,
                });
            }
        };

        let query = js_trim(&js_or_string(params.get("query")))
            .to_lowercase()
            .chars()
            .take(MAX_QUERY)
            .collect::<String>();
        let filtered: Vec<&CatalogGame> = if query.is_empty() {
            catalog.games.iter().collect()
        } else {
            catalog
                .games
                .iter()
                .filter(|game| {
                    [
                        &game.title,
                        &game.platform,
                        &game.source_title,
                        &game.description,
                    ]
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .contains(&query)
                })
                .collect()
        };
        let total = filtered.len();
        let games: Vec<Value> = filtered
            .into_iter()
            .skip(offset as usize)
            .take(limit)
            .map(|game| {
                let mut value = serde_json::to_value(game).unwrap_or(Value::Null);
                if let Value::Object(object) = &mut value {
                    object.remove("uris");
                }
                value
            })
            .collect();
        let sources = serde_json::to_value(&catalog.sources).unwrap_or(json!([]));
        json!({
            "ok": true,
            "games": games,
            "sources": sources,
            "total": total,
            "offset": offset,
            "limit": limit,
            "hasMore": offset as usize + games.len() < total,
            "updatedAt": catalog.updated_at,
        })
    }

    /// `getGame`: devolve o jogo completo (com URIs) e as fontes.
    pub async fn get_game(&self, id: &str) -> Value {
        let catalog = match self.ensure(false).await {
            Ok(catalog) => catalog,
            Err(error) => {
                return json!({
                    "ok": false,
                    "error": error.message,
                    "sources": error.sources,
                });
            }
        };
        let key = js_trim(id);
        let sources = serde_json::to_value(&catalog.sources).unwrap_or(json!([]));
        match catalog.games.iter().find(|game| game.id == key) {
            Some(game) => json!({
                "ok": true,
                "game": serde_json::to_value(game).unwrap_or(Value::Null),
                "sources": sources,
            }),
            None => json!({
                "ok": false,
                "error": "jogo retro não encontrado",
                "sources": sources,
            }),
        }
    }

    /// `exportAll`: não paginado, com URIs (uso interno da migração V2).
    pub async fn export_all(&self, refresh: bool) -> Result<CatalogExport, CatalogError> {
        let catalog = self.ensure(refresh).await?;
        Ok(CatalogExport::from_catalog(&catalog))
    }
}

#[async_trait]
impl V1Catalog for RetroCatalog {
    async fn export_all(&self, refresh: bool) -> Result<CatalogExport, CatalogError> {
        RetroCatalog::export_all(self, refresh).await
    }

    async fn refresh(&self) -> Result<CatalogExport, CatalogError> {
        RetroCatalog::refresh(self)
            .await
            .map(|catalog| CatalogExport::from_catalog(&catalog))
    }
}

// ── catálogo canônico (builder) ────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub serial: Option<String>,
    pub sha1: Option<String>,
    pub crc32: Option<String>,
    pub title: Option<String>,
}

/// `generateGameId`: serial → sha1 → crc32 → slug do título.
pub fn generate_game_id(system_id: &str, identity: &Identity) -> String {
    if let Some(serial) = identity.serial.as_deref().filter(|value| !value.is_empty()) {
        return format!("retro:{system_id}:{serial}");
    }
    if let Some(sha1) = identity.sha1.as_deref().filter(|value| !value.is_empty()) {
        let prefix: String = sha1.chars().take(16).collect();
        return format!("retro:{system_id}:sha1:{prefix}");
    }
    if let Some(crc32) = identity.crc32.as_deref().filter(|value| !value.is_empty()) {
        return format!("retro:{system_id}:crc32:{crc32}");
    }
    let title = identity.title.as_deref().unwrap_or("unknown");
    let slug: String = sha256_hex(title).chars().take(12).collect();
    format!("retro:{system_id}:slug:{slug}")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanonicalGame {
    pub id: String,
    pub system_id: String,
    pub title: String,
    pub sort_title: String,
    pub aliases: Vec<String>,
    pub serials: Vec<String>,
    pub hashes: Hashes,
    pub regions: Vec<String>,
    pub languages: Vec<String>,
    pub release_date: Option<Value>,
    pub developer: Option<Value>,
    pub publisher: Option<Value>,
    pub genres: Vec<Value>,
    pub summary: Option<Value>,
    pub artwork: Map<String, Value>,
    pub offer_count: usize,
    pub match_quality: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `createCanonicalGame`.
pub fn create_canonical_game(offer: &Offer, offer_count: usize) -> CanonicalGame {
    let identity = Identity {
        serial: offer
            .serials
            .first()
            .filter(|value| !value.is_empty())
            .cloned(),
        title: Some(offer.normalized_title.clone()),
        ..Identity::default()
    };
    let mut artwork = Map::new();
    let cover = offer
        .cover
        .as_ref()
        .filter(|value| js_truthy(Some(value)))
        .cloned();
    artwork.insert("cover".to_string(), cover.clone().unwrap_or(Value::Null));
    artwork.insert(
        "provider".to_string(),
        if cover.is_some() {
            json!("source")
        } else {
            Value::Null
        },
    );
    CanonicalGame {
        id: generate_game_id(&offer.system_id.clone().unwrap_or_default(), &identity),
        system_id: offer.system_id.clone().unwrap_or_default(),
        title: offer.normalized_title.clone(),
        sort_title: offer.normalized_title.to_lowercase(),
        aliases: Vec::new(),
        serials: offer.serials.clone(),
        hashes: Hashes::default(),
        regions: offer.region.clone().into_iter().collect(),
        languages: offer.languages.clone(),
        release_date: None,
        developer: None,
        publisher: None,
        genres: Vec::new(),
        summary: None,
        artwork,
        offer_count,
        match_quality: offer
            .r#match
            .quality
            .clone()
            .unwrap_or_else(|| "unmatched".to_string()),
        extra: Map::new(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct MergedOffers {
    pub serials: Vec<String>,
    pub regions: Vec<String>,
    pub languages: Vec<String>,
    pub aliases: Vec<String>,
    pub offer_count: usize,
}

/// `mergeOffers`: união estável de seriais/regiões/idiomas/aliases.
pub fn merge_offers(offers: &[Offer]) -> Option<MergedOffers> {
    offers.first()?;
    let mut serials = Vec::new();
    let mut regions = Vec::new();
    let mut languages = Vec::new();
    let mut aliases = Vec::new();
    let mut seen_serials = std::collections::HashSet::new();
    let mut seen_regions = std::collections::HashSet::new();
    let mut seen_languages = std::collections::HashSet::new();
    let mut seen_aliases = std::collections::HashSet::new();
    for offer in offers {
        for serial in &offer.serials {
            if seen_serials.insert(serial.clone()) {
                serials.push(serial.clone());
            }
        }
        if let Some(region) = offer.region.as_ref().filter(|value| !value.is_empty()) {
            if seen_regions.insert(region.clone()) {
                regions.push(region.clone());
            }
        }
        for language in &offer.languages {
            if seen_languages.insert(language.clone()) {
                languages.push(language.clone());
            }
        }
        if !offer.original_title.is_empty()
            && offer.original_title != offer.normalized_title
            && seen_aliases.insert(offer.original_title.clone())
        {
            aliases.push(offer.original_title.clone());
        }
    }
    Some(MergedOffers {
        serials,
        regions,
        languages,
        aliases,
        offer_count: offers.len(),
    })
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogUnmatched {
    pub offer_id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_raw: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchQualityStats {
    pub exact: usize,
    pub strong: usize,
    pub probable: usize,
    pub unmatched: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStats {
    pub total_offers: usize,
    pub game_offers: usize,
    pub special_offers: usize,
    pub games_created: usize,
    pub matched: usize,
    pub unmatched: usize,
    pub by_system: Map<String, Value>,
    pub by_release_kind: Map<String, Value>,
    pub by_match_quality: MatchQualityStats,
}

#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub min_confidence: i32,
    pub allow_probable: bool,
    pub canonical_games: Option<Vec<CanonicalGame>>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            min_confidence: 65,
            allow_probable: true,
            canonical_games: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CatalogBuild {
    pub games: Vec<CanonicalGame>,
    pub matches: Vec<crate::matcher::BatchMatch>,
    pub unmatched: Vec<CatalogUnmatched>,
    pub special_offers: Vec<Offer>,
    pub stats: CatalogStats,
}

fn to_retro_offer(offer: &Offer) -> RetroOffer {
    RetroOffer {
        id: offer.id.clone(),
        normalized_title: Some(offer.normalized_title.clone()),
        title: Some(offer.original_title.clone()),
        system_id: offer.system_id.clone(),
        serials: offer.serials.clone(),
        hashes: Hashes::default(),
        region: offer.region.clone(),
        release_year: None,
        extra: Map::new(),
    }
}

fn to_retro_game(game: &CanonicalGame) -> RetroGame {
    RetroGame {
        id: game.id.clone(),
        system_id: Some(game.system_id.clone()),
        title: game.title.clone(),
        aliases: game.aliases.clone(),
        serials: game.serials.clone(),
        hashes: game.hashes.clone(),
        regions: game.regions.clone(),
        release_date: game
            .release_date
            .as_ref()
            .and_then(Value::as_str)
            .map(ToString::to_string),
        extra: Map::new(),
    }
}

fn bump(map: &mut Map<String, Value>, key: &str) {
    let current = map.get(key).and_then(Value::as_u64).unwrap_or(0);
    map.insert(key.to_string(), json!(current + 1));
}

/// `buildCanonicalCatalog`: agrupa ofertas, roda o matcher real e consolida.
pub fn build_canonical_catalog(offers: &[Offer], options: &BuildOptions) -> CatalogBuild {
    let game_offers: Vec<Offer> = offers
        .iter()
        .filter(|offer| offer.release_kind == "game" || offer.release_kind == "unknown")
        .cloned()
        .collect();
    let special_offers: Vec<Offer> = offers
        .iter()
        .filter(|offer| offer.release_kind != "game" && offer.release_kind != "unknown")
        .cloned()
        .collect();

    let mut seed_order: Vec<(String, Vec<Offer>)> = Vec::new();
    let mut seed_index: HashMap<String, usize> = HashMap::new();
    let mut no_system: Vec<CatalogUnmatched> = Vec::new();
    for offer in &game_offers {
        match offer.system_id.as_deref().filter(|value| !value.is_empty()) {
            Some(system_id) => {
                let key = format!("{system_id}:{}", offer.normalized_title.to_lowercase());
                let position = *seed_index.entry(key.clone()).or_insert_with(|| {
                    seed_order.push((key, Vec::new()));
                    seed_order.len() - 1
                });
                seed_order[position].1.push(offer.clone());
            }
            None => no_system.push(CatalogUnmatched {
                offer_id: offer.id.clone(),
                reason: "no system identified".to_string(),
                title: Some(offer.normalized_title.clone()),
                system_id: None,
                platform_raw: Some(offer.platform_raw.clone()),
                extra: Map::new(),
            }),
        }
    }

    let seeded_games: Vec<CanonicalGame> = match &options.canonical_games {
        Some(games) => games.clone(),
        None => {
            let mut games = Vec::with_capacity(seed_order.len());
            for (_, grouped) in &seed_order {
                let first = &grouped[0];
                let Some(merged) = merge_offers(grouped) else {
                    continue;
                };
                let mut game = create_canonical_game(first, grouped.len());
                game.id = generate_game_id(
                    first.system_id.as_deref().unwrap_or_default(),
                    &Identity {
                        title: Some(first.normalized_title.clone()),
                        ..Identity::default()
                    },
                );
                game.serials = merged.serials;
                game.regions = merged.regions;
                game.languages = merged.languages;
                game.aliases = merged.aliases;
                games.push(game);
            }
            games
        }
    };

    let matchable: Vec<RetroOffer> = game_offers
        .iter()
        .filter(|offer| {
            offer
                .system_id
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        })
        .map(to_retro_offer)
        .collect();
    let retro_games: Vec<RetroGame> = seeded_games.iter().map(to_retro_game).collect();
    let batch = match_batch_with(
        &matchable,
        &retro_games,
        MatchOptions {
            min_confidence: options.min_confidence,
            allow_probable: options.allow_probable,
        },
    );

    let offers_by_id: HashMap<String, Offer> = game_offers
        .iter()
        .map(|offer| (offer.id.clone(), offer.clone()))
        .collect();
    let mut matches_by_game: Vec<(String, Vec<Offer>)> = Vec::new();
    let mut match_index: HashMap<String, usize> = HashMap::new();
    for found in &batch.matches {
        let position = *match_index.entry(found.game_id.clone()).or_insert_with(|| {
            matches_by_game.push((found.game_id.clone(), Vec::new()));
            matches_by_game.len() - 1
        });
        if let Some(offer) = offers_by_id.get(&found.offer_id) {
            matches_by_game[position].1.push(offer.clone());
        }
    }

    let mut games: Vec<CanonicalGame> = Vec::new();
    for seed in &seeded_games {
        let grouped = matches_by_game
            .iter()
            .find(|(game_id, _)| game_id == &seed.id)
            .map(|(_, offers)| offers.clone())
            .unwrap_or_default();
        if grouped.is_empty() {
            continue;
        }
        let Some(merged) = merge_offers(&grouped) else {
            continue;
        };
        let mut game = seed.clone();
        game.serials = merged.serials;
        game.regions = merged.regions;
        game.languages = merged.languages;
        game.aliases = merged.aliases;
        game.offer_count = merged.offer_count;
        games.push(game);
    }
    games.sort_by(|left, right| js_locale_compare(&left.sort_title, &right.sort_title));

    let mut unmatched: Vec<CatalogUnmatched> = no_system;
    for entry in &batch.unmatched {
        unmatched.push(CatalogUnmatched {
            offer_id: entry.offer_id.clone(),
            reason: entry.reason.clone(),
            title: entry.title.clone(),
            system_id: entry.system_id.clone(),
            platform_raw: None,
            extra: Map::new(),
        });
    }

    let mut stats = CatalogStats {
        total_offers: offers.len(),
        game_offers: game_offers.len(),
        special_offers: special_offers.len(),
        games_created: games.len(),
        matched: batch.matches.len(),
        unmatched: unmatched.len(),
        ..CatalogStats::default()
    };
    for game in &games {
        bump(&mut stats.by_system, &game.system_id);
    }
    for offer in offers {
        let kind = if offer.release_kind.is_empty() {
            "undefined"
        } else {
            offer.release_kind.as_str()
        };
        bump(&mut stats.by_release_kind, kind);
    }
    for found in &batch.matches {
        match found.quality {
            MatchQuality::Exact => stats.by_match_quality.exact += 1,
            _ if found.score >= 95 => stats.by_match_quality.exact += 1,
            MatchQuality::Strong => stats.by_match_quality.strong += 1,
            _ if found.score >= 80 => stats.by_match_quality.strong += 1,
            MatchQuality::Probable => stats.by_match_quality.probable += 1,
            _ if found.score >= 65 => stats.by_match_quality.probable += 1,
            _ => {}
        }
    }
    stats.by_match_quality.unmatched = unmatched.len();

    CatalogBuild {
        games,
        matches: batch.matches,
        unmatched,
        special_offers,
        stats,
    }
}

/// `rebuildCatalogFromSources`.
pub fn rebuild_catalog_from_sources(
    sources: &[CatalogSource],
    payloads: &HashMap<String, Value>,
    options: &BuildOptions,
) -> CatalogBuild {
    let mut offers = Vec::new();
    for source in sources {
        let Some(payload) = payloads.get(&source.id) else {
            continue;
        };
        let downloads = payload
            .get("downloads")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        offers.extend(process_source_downloads(
            &downloads,
            &source.to_offer_source(),
        ));
    }
    build_canonical_catalog(&offers, options)
}

/// `applyOverridesToCatalog`: `gameId: null` ignora; string vira match manual.
pub fn apply_overrides_to_catalog(
    catalog: &CatalogBuild,
    overrides: &HashMap<String, Value>,
) -> CatalogBuild {
    let mut updated_matches = Vec::new();
    for found in &catalog.matches {
        if let Some(Value::Object(override_data)) = overrides.get(&found.offer_id) {
            match override_data.get("gameId") {
                Some(Value::Null) => continue,
                Some(Value::String(game_id)) if !game_id.is_empty() => {
                    let mut manual = found.clone();
                    manual.game_id = game_id.clone();
                    manual.method = "manual".to_string();
                    manual.quality = MatchQuality::Exact;
                    manual.score = 100;
                    updated_matches.push(manual);
                    continue;
                }
                _ => {}
            }
        }
        updated_matches.push(found.clone());
    }

    let mut updated_unmatched = Vec::new();
    for item in &catalog.unmatched {
        match overrides.get(&item.offer_id) {
            Some(Value::Object(override_data)) => match override_data.get("gameId") {
                Some(Value::String(game_id)) if !game_id.is_empty() => {
                    updated_matches.push(crate::matcher::BatchMatch {
                        offer_id: item.offer_id.clone(),
                        game_id: game_id.clone(),
                        score: 100,
                        confidence: 100,
                        method: "manual".to_string(),
                        evidence: "manual override".to_string(),
                        quality: MatchQuality::Exact,
                        tied: false,
                    });
                }
                Some(Value::Null) => {}
                _ => updated_unmatched.push(item.clone()),
            },
            _ => updated_unmatched.push(item.clone()),
        }
    }

    CatalogBuild {
        matches: updated_matches,
        unmatched: updated_unmatched,
        ..catalog.clone()
    }
}

// ── store V2 ───────────────────────────────────────────────────────────────

/// Interface do catálogo V1 usada pela migração/refresh do V2.
#[async_trait]
pub trait V1Catalog: Send + Sync {
    async fn export_all(&self, refresh: bool) -> Result<CatalogExport, CatalogError>;
    async fn refresh(&self) -> Result<CatalogExport, CatalogError>;
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub migrated: bool,
    pub offers_count: usize,
    pub games_count: usize,
    pub matched_count: usize,
    pub unmatched_count: usize,
}

#[derive(Clone, Debug)]
pub struct RefreshResult {
    pub games: Vec<CanonicalGame>,
    pub stats: CatalogStats,
    pub updated_at: i64,
}

/// Store V2: repositório + migração V1 + envelopes do IPC.
#[derive(Clone)]
pub struct RetroCatalogV2 {
    repository: Arc<RetroRepository>,
    v1: Arc<dyn V1Catalog>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    refresh_pending: Arc<AtomicBool>,
}

impl RetroCatalogV2 {
    pub fn new(data_dir: impl Into<PathBuf>, v1: Arc<dyn V1Catalog>) -> Self {
        let repository = Arc::new(RetroRepository::new(data_dir));
        Self::with_repository(repository, v1)
    }

    pub fn with_repository(repository: Arc<RetroRepository>, v1: Arc<dyn V1Catalog>) -> Self {
        Self {
            repository,
            v1,
            now: Arc::new(now_ms),
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            refresh_pending: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_now(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }

    pub fn repository(&self) -> &Arc<RetroRepository> {
        &self.repository
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn save_build(&self, offers: &[Offer], catalog: &CatalogBuild) -> Result<(), CatalogError> {
        let offer_values: Vec<Value> = offers.iter().map(Offer::to_value).collect();
        self.repository
            .set_offers(offer_values)
            .map_err(|error: RepositoryError| CatalogError::new(error.to_string()))?;
        let match_values: Vec<Value> = catalog
            .matches
            .iter()
            .map(|found| serde_json::to_value(found).unwrap_or(Value::Null))
            .collect();
        self.repository.set_matches(match_values);
        let game_values: Vec<Value> = catalog
            .games
            .iter()
            .map(|game| serde_json::to_value(game).unwrap_or(Value::Null))
            .collect();
        self.repository.set_catalog_index(game_values, Map::new());
        let unmatched_values: Vec<Value> = catalog
            .unmatched
            .iter()
            .map(|entry| serde_json::to_value(entry).unwrap_or(Value::Null))
            .collect();
        self.repository.set_unmatched(unmatched_values);
        Ok(())
    }

    /// Migra o cache V1 para o formato canônico. Falhas viram `None`.
    pub async fn migrate_from_v1(&self) -> Option<MigrationResult> {
        let v1_data = self.v1.export_all(false).await.ok()?;
        if v1_data.games.is_empty() {
            return None;
        }
        let sources: HashMap<&str, &CatalogSource> = v1_data
            .sources
            .iter()
            .map(|source| (source.id.as_str(), source))
            .collect();
        let mut offers = Vec::new();
        for game in &v1_data.games {
            let Some(source) = sources.get(game.source_id.as_str()) else {
                continue;
            };
            let download = json!({
                "title": game
                    .original_title
                    .clone()
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| game.title.clone()),
                "platform": game.platform,
                "description": game.description,
                "fileSize": game.file_size,
                "uploadDate": game.upload_date,
                "uris": game.uris,
                "cover": game.cover.clone().or_else(|| game.capa.clone()),
            });
            offers.extend(process_source_downloads(
                std::slice::from_ref(&download),
                &source.to_offer_source(),
            ));
        }
        let catalog = build_canonical_catalog(&offers, &BuildOptions::default());
        self.save_build(&offers, &catalog).ok()?;
        Some(MigrationResult {
            migrated: true,
            offers_count: offers.len(),
            games_count: catalog.games.len(),
            matched_count: catalog.matches.len(),
            unmatched_count: catalog.unmatched.len(),
        })
    }

    async fn refresh_inner(&self) -> Result<RefreshResult, CatalogError> {
        let v1_data = self.v1.refresh().await?;
        if v1_data.sources.is_empty() {
            return Err(CatalogError::new("No sources available"));
        }
        let mut payload_values: Map<String, Value> = Map::new();
        for source in &v1_data.sources {
            payload_values.insert(
                source.id.clone(),
                json!({"name": source.title, "downloads": []}),
            );
        }
        for game in &v1_data.games {
            let Some(payload) = payload_values.get_mut(&game.source_id) else {
                continue;
            };
            if let Some(downloads) = payload.get_mut("downloads").and_then(Value::as_array_mut) {
                downloads.push(json!({
                    "title": game
                        .original_title
                        .clone()
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| game.title.clone()),
                    "platform": game.platform,
                    "description": game.description,
                    "fileSize": game.file_size,
                    "uploadDate": game.upload_date,
                    "uris": game.uris,
                    "cover": game.cover.clone().or_else(|| game.capa.clone()),
                }));
            }
        }
        let payloads: HashMap<String, Value> = payload_values.into_iter().collect();
        let catalog =
            rebuild_catalog_from_sources(&v1_data.sources, &payloads, &BuildOptions::default());
        let overrides: HashMap<String, Value> =
            self.repository.overrides_map().into_iter().collect();
        let final_catalog = apply_overrides_to_catalog(&catalog, &overrides);

        let mut all_offers = Vec::new();
        for source in &v1_data.sources {
            let Some(payload) = payloads.get(&source.id) else {
                continue;
            };
            let downloads = payload
                .get("downloads")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            all_offers.extend(process_source_downloads(
                &downloads,
                &source.to_offer_source(),
            ));
        }
        self.save_build(&all_offers, &final_catalog)?;
        Ok(RefreshResult {
            games: final_catalog.games,
            stats: final_catalog.stats,
            updated_at: self.now(),
        })
    }

    /// Atualiza a partir das fontes V1 (erros propagam).
    pub async fn refresh(&self) -> Result<RefreshResult, CatalogError> {
        let _guard = self.refresh_lock.lock().await;
        self.refresh_inner().await
    }

    async fn ensure(&self, force: bool) -> Result<(), CatalogError> {
        let stats = self.repository.get_stats();
        if stats.games == 0 && !force && self.migrate_from_v1().await.is_some() {
            return Ok(());
        }
        let stats = self.repository.get_stats();
        let age = stats
            .last_updated
            .map(|updated| self.now() - updated)
            .unwrap_or(i64::MAX);
        if age > CACHE_TTL_MS || force {
            if force || stats.games == 0 {
                let _guard = self.refresh_lock.lock().await;
                self.refresh_inner().await?;
            } else {
                // Igual ao JS (`inFlight = refresh(); await inFlight` só sem
                // dados): com catálogo em disco a resposta não espera a rede.
                self.spawn_refresh();
            }
        }
        Ok(())
    }

    fn spawn_refresh(&self) {
        if self.refresh_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let task = self.clone();
        tokio::spawn(async move {
            let _guard = task.refresh_lock.lock().await;
            let _ = task.refresh_inner().await;
            task.refresh_pending.store(false, Ordering::Release);
        });
    }

    pub fn public_game(game: &Value) -> Value {
        let mut output = game.clone();
        let Some(object) = output.as_object_mut() else {
            return output;
        };
        object.insert(
            "platform".to_string(),
            game.get("systemId").cloned().unwrap_or(Value::Null),
        );
        if let Some(artwork) = game.get("artwork").and_then(Value::as_object) {
            if let Some(cover) = artwork.get("cover").filter(|value| js_truthy(Some(value))) {
                object.insert("cover".to_string(), cover.clone());
            }
            if let Some(title_screen) = artwork
                .get("titleScreen")
                .filter(|value| js_truthy(Some(value)))
            {
                object.insert("fallbackCover".to_string(), title_screen.clone());
            }
        }
        Value::Object(object.clone())
    }

    /// `list` do catálogo V2 com facets e estatísticas.
    pub async fn list(&self, params: &Value) -> Value {
        let offset = js_safe_integer(params.get("offset"))
            .map(|value| value.max(0))
            .unwrap_or(0) as usize;
        let limit = js_safe_integer(params.get("limit"))
            .map(|value| value.clamp(1, MAX_LIMIT as i64) as usize)
            .unwrap_or(24);
        let systems: Vec<String> = params
            .get("systems")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .take(32)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let query = js_trim(&js_or_string(params.get("query")))
            .chars()
            .take(MAX_QUERY)
            .collect::<String>();
        let force = js_truthy(params.get("refresh"));

        if let Err(error) = self.ensure(force).await {
            return json!({
                "ok": false,
                "games": [],
                "totalGames": 0,
                "totalOffers": 0,
                "unmatchedOffers": 0,
                "facets": {"systems": {}, "releaseKinds": {}},
                "offset": offset,
                "limit": limit,
                "hasMore": false,
                "error": error.message,
            });
        }

        let all_games = self.repository.get_games();
        let mut filtered: Vec<&Value> = all_games.iter().collect();
        if !systems.is_empty() {
            filtered.retain(|game| {
                game.get("systemId")
                    .and_then(Value::as_str)
                    .is_some_and(|system| systems.iter().any(|wanted| wanted == system))
            });
        }
        if !query.is_empty() {
            let lowered = query.to_lowercase();
            filtered.retain(|game| {
                let title_matches = game
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.to_lowercase().contains(&lowered));
                let alias_matches =
                    game.get("aliases")
                        .and_then(Value::as_array)
                        .is_some_and(|aliases| {
                            aliases
                                .iter()
                                .filter_map(Value::as_str)
                                .any(|alias| alias.to_lowercase().contains(&lowered))
                        });
                title_matches || alias_matches
            });
        }
        let total = filtered.len();
        let games: Vec<Value> = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(Self::public_game)
            .collect();

        let mut systems_facet: Map<String, Value> = Map::new();
        for game in &all_games {
            let key = game
                .get("systemId")
                .and_then(Value::as_str)
                .unwrap_or("undefined");
            bump(&mut systems_facet, key);
        }
        let stats = self.repository.get_stats();
        json!({
            "ok": true,
            "games": games,
            "total": total,
            "totalGames": total,
            "totalOffers": stats.offers,
            "unmatchedOffers": stats.unmatched,
            "facets": {"systems": systems_facet, "releaseKinds": {}},
            "offset": offset,
            "limit": limit,
            "hasMore": offset + games.len() < total,
            "updatedAt": stats.last_updated,
        })
    }

    /// `getGame` do V2: jogo público + sumários de ofertas sem URIs.
    pub async fn get_game(&self, game_id: &str) -> Value {
        if let Err(error) = self.ensure(false).await {
            return json!({"ok": false, "error": error.message});
        }
        let Some(game) = self.repository.get_game(game_id) else {
            return json!({"ok": false, "error": "Game not found"});
        };
        let offers: Vec<Value> = self
            .repository
            .get_offers_for_game(game_id)
            .into_iter()
            .map(offer_summary_value)
            .collect();
        json!({
            "ok": true,
            "game": Self::public_game(&game),
            "offers": offers,
        })
    }

    /// `getOffer`: oferta completa, com URIs.
    pub async fn get_offer(&self, offer_id: &str) -> Value {
        if let Err(error) = self.ensure(false).await {
            return json!({"ok": false, "error": error.message});
        }
        match self.repository.get_offer(offer_id) {
            Some(offer) => json!({"ok": true, "offer": offer}),
            None => json!({"ok": false, "error": "Offer not found"}),
        }
    }

    /// `retro:stats` (envelope do IPC).
    pub fn stats_envelope(&self) -> Value {
        let stats = self.repository.get_stats();
        json!({
            "ok": true,
            "stats": serde_json::to_value(stats).unwrap_or(Value::Null),
        })
    }

    /// `retro:migrate` (envelope do IPC).
    pub async fn migrate_envelope(&self) -> Value {
        match self.migrate_from_v1().await {
            Some(result) => json!({"ok": true, "result": result}),
            None => json!({"ok": true, "result": null}),
        }
    }

    /// `retro:audit`: auditoria local (o servidor remoto continua opcional).
    pub fn audit(&self, params: &Value) -> Value {
        let system = params
            .get("system")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let samples = js_safe_integer(params.get("samples"))
            .map(|value| value.clamp(0, 100) as usize)
            .unwrap_or(20);
        self.repository.audit(system, samples)
    }
}

fn offer_summary_value(offer: Value) -> Value {
    let mut summary = offer;
    if let Value::Object(object) = &mut summary {
        let has_uris = object
            .get("uris")
            .and_then(Value::as_array)
            .is_some_and(|uris| !uris.is_empty());
        let uri_count = object
            .get("uris")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        object.remove("uris");
        object.insert("hasUris".to_string(), Value::Bool(has_uris));
        object.insert("uriCount".to_string(), json!(uri_count));
    }
    summary
}

/// `createRetroCatalogV2` (atalho de construção).
pub fn create_retro_catalog_v2(
    data_dir: impl Into<PathBuf>,
    v1: Arc<dyn V1Catalog>,
) -> RetroCatalogV2 {
    RetroCatalogV2::new(data_dir, v1)
}

// ── server catalog (retro-server-catalog.js) ──────────────────────────────

/// Resultado de um `catalogGet` remoto (`/catalog/v1/retro/...`).
#[async_trait]
pub trait RemoteCatalogProvider: Send + Sync {
    /// Devolve o `data` do envelope remoto ou um erro de transporte.
    async fn catalog_get(&self, path: &str, no_mirror: bool) -> Result<Value, String>;
}

/// Candidato de uma fonte pessoal (`sources.search`).
#[derive(Clone, Debug, Default)]
pub struct PersonalCandidate {
    pub reference: String,
    pub title: String,
    pub source: String,
    pub file_size: Value,
    pub upload_date: Value,
}

/// Jogo resolvido de uma fonte pessoal (`sources.getGame`).
#[derive(Clone, Debug, Default)]
pub struct PersonalGame {
    pub game: Value,
    pub source: Option<String>,
}

/// Fontes pessoais usadas para enriquecer `retro:game`/`retro:offer`.
#[async_trait]
pub trait PersonalSources: Send + Sync {
    async fn search(&self, query: &str, limit: usize) -> Vec<PersonalCandidate>;
    async fn get_game(&self, reference: &str) -> Option<PersonalGame>;
}

/// Precedência remota → fallback local, como `retro-server-catalog.js`.
pub struct RetroServerCatalog {
    fallback: Arc<RetroCatalogV2>,
    remote: Option<Arc<dyn RemoteCatalogProvider>>,
    personal_sources: Option<Arc<dyn PersonalSources>>,
    personal_offers: Mutex<HashMap<String, String>>,
    personal_timeout: Duration,
    /// Respostas recentes por caminho de catálogo.
    ///
    /// O catálogo tem 28 mil jogos e cada `retro:list` custa uma ida ao servidor
    /// (medido: 0,44 s de mediana, com picos de 1,5 s). Sem cache, voltar para a
    /// página anterior ou reabrir a aba pagava o tempo inteiro de novo. O TTL
    /// curto mantém o catálogo fresco e mata a espera da navegação repetida.
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

/// Quanto tempo uma resposta de catálogo fica válida em memória.
pub const CATALOG_CACHE_TTL: Duration = Duration::from_secs(300);

impl RetroServerCatalog {
    pub fn new(fallback: Arc<RetroCatalogV2>) -> Self {
        Self {
            fallback,
            remote: None,
            personal_sources: None,
            personal_offers: Mutex::new(HashMap::new()),
            personal_timeout: Duration::from_millis(400),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_remote(mut self, remote: Arc<dyn RemoteCatalogProvider>) -> Self {
        self.remote = Some(remote);
        self
    }

    pub fn with_personal_sources(mut self, sources: Arc<dyn PersonalSources>) -> Self {
        self.personal_sources = Some(sources);
        self
    }

    pub fn with_personal_timeout(mut self, timeout: Duration) -> Self {
        self.personal_timeout = timeout;
        self
    }

    pub fn repository(&self) -> &Arc<RetroRepository> {
        self.fallback.repository()
    }

    async fn with_fallback<F, Fut>(&self, path: &str, no_mirror: bool, local: F) -> Value
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Value>,
    {
        // Cache em memória primeiro: refazer uma página visitada há pouco não
        // pode custar outra ida ao servidor (0,44 s de mediana, picos de 1,5 s).
        // `no_mirror` desliga o cache junto com o espelho — quem pediu sem
        // cache quer dado fresco de verdade.
        if !no_mirror {
            if let Ok(cache) = self.cache.lock() {
                if let Some((quando, dados)) = cache.get(path) {
                    if quando.elapsed() < CATALOG_CACHE_TTL {
                        return dados.clone();
                    }
                }
            }
        }

        if let Some(remote) = &self.remote {
            if let Ok(data) = remote.catalog_get(path, no_mirror).await {
                if js_truthy(data.get("ok")) {
                    if !no_mirror {
                        if let Ok(mut cache) = self.cache.lock() {
                            cache.insert(path.to_string(), (Instant::now(), data.clone()));
                            // Poda simples: o catálogo navega por dezenas de
                            // páginas e o mapa cresceria sem limite.
                            if cache.len() > 200 {
                                let limite = Instant::now()
                                    .checked_sub(CATALOG_CACHE_TTL)
                                    .unwrap_or_else(Instant::now);
                                cache.retain(|_, (quando, _)| *quando > limite);
                            }
                        }
                    }
                    return data;
                }
            }
        }
        local().await
    }

    fn query(params: &Value) -> String {
        let offset = js_safe_integer(params.get("offset")).unwrap_or(0).max(0);
        let limit = js_safe_integer(params.get("limit"))
            .unwrap_or(24)
            .clamp(1, 100);
        let mut pairs: Vec<(&str, String)> =
            vec![("offset", offset.to_string()), ("limit", limit.to_string())];
        if let Some(query) = params.get("query").and_then(Value::as_str) {
            if !query.is_empty() {
                pairs.push(("query", query.chars().take(120).collect()));
            }
        }
        if params.get("variants").and_then(Value::as_str) == Some("all") {
            pairs.push(("variants", "all".to_string()));
        }
        if params.get("mode").and_then(Value::as_str) == Some("all") {
            pairs.push(("mode", "all".to_string()));
        }
        let system = params
            .get("systems")
            .and_then(Value::as_array)
            .and_then(|systems| systems.first())
            .and_then(Value::as_str)
            .or_else(|| params.get("system").and_then(Value::as_str));
        if let Some(system) = system.filter(|value| !value.is_empty()) {
            pairs.push(("system", system.chars().take(80).collect()));
        }
        let references: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        form_urlencode(&references)
    }

    async fn personal_summaries(&self, game: &Value) -> Vec<Value> {
        let Some(sources) = &self.personal_sources else {
            return Vec::new();
        };
        let Some(title) = game.get("title").and_then(Value::as_str) else {
            return Vec::new();
        };
        let canonical = normalize_retro_title(title).to_lowercase();
        let candidates =
            match tokio::time::timeout(self.personal_timeout, sources.search(title, 50)).await {
                Ok(candidates) => candidates,
                Err(_) => return Vec::new(),
            };
        let mut summaries = Vec::new();
        for candidate in candidates {
            if normalize_retro_title(&candidate.title).to_lowercase() != canonical {
                continue;
            }
            let id = format!("personal:{}", candidate.reference);
            if let Ok(mut offers) = self.personal_offers.lock() {
                offers.insert(id.clone(), candidate.reference.clone());
            }
            let source_id = candidate
                .reference
                .split(':')
                .next()
                .unwrap_or_default()
                .to_string();
            summaries.push(json!({
                "id": id,
                "sourceId": source_id,
                "sourceTitle": if candidate.source.is_empty() { "Personal source" } else { candidate.source.as_str() },
                "originalTitle": candidate.title,
                "normalizedTitle": title,
                "systemId": game.get("systemId").cloned().unwrap_or(Value::Null),
                "fileSize": if js_truthy(Some(&candidate.file_size)) { candidate.file_size } else { json!("") },
                "uploadDate": if js_truthy(Some(&candidate.upload_date)) { candidate.upload_date } else { json!("") },
                "hasUris": true,
                "uriCount": 1,
                "personal": true,
            }));
        }
        summaries
    }

    pub fn list_path(params: &Value) -> String {
        format!("/catalog/v1/retro/games?{}", Self::query(params))
    }

    pub fn game_path(game_id: &str) -> String {
        let id = encode_uri_component(&game_id.chars().take(240).collect::<String>());
        format!("/catalog/v1/retro/games/{id}")
    }

    pub async fn list(&self, params: &Value) -> Value {
        let path = Self::list_path(params);
        self.with_fallback(&path, false, || self.fallback.list(params))
            .await
    }

    pub async fn get_game(&self, game_id: &str) -> Value {
        let path = Self::game_path(game_id);
        let mut response = self
            .with_fallback(&path, false, || self.fallback.get_game(game_id))
            .await;
        if !js_truthy(response.get("ok"))
            || response
                .get("game")
                .and_then(|game| game.get("title"))
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .is_none()
        {
            return response;
        }
        let summaries = match response.get("game") {
            Some(game) => self.personal_summaries(game).await,
            None => Vec::new(),
        };
        if summaries.is_empty() {
            return response;
        }
        if let Some(object) = response.as_object_mut() {
            let offers = object
                .entry("offers".to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(offers) = offers.as_array_mut() {
                offers.extend(summaries.iter().cloned());
            }
            if let Some(game) = object.get_mut("game").and_then(Value::as_object_mut) {
                let current = game
                    .get("offerCount")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                game.insert(
                    "offerCount".to_string(),
                    json!(current as i64 + summaries.len() as i64),
                );
            }
        }
        response
    }

    pub async fn get_offer(&self, offer_id: &str) -> Value {
        let personal_reference = self
            .personal_offers
            .lock()
            .ok()
            .and_then(|offers| offers.get(offer_id).cloned());
        if let Some(reference) = personal_reference {
            let Some(sources) = &self.personal_sources else {
                return json!({"ok": false, "error": "personal_offer_unavailable"});
            };
            let Some(resolved) = sources.get_game(&reference).await else {
                return json!({"ok": false, "error": "personal_offer_unavailable"});
            };
            let title = resolved
                .game
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("");
            let uris: Vec<Value> = resolved
                .game
                .get("uris")
                .and_then(Value::as_array)
                .cloned()
                .or_else(|| {
                    resolved
                        .game
                        .get("uri")
                        .filter(|value| js_truthy(Some(value)))
                        .map(|value| vec![value.clone()])
                })
                .unwrap_or_default();
            let source_id = reference.split(':').next().unwrap_or_default().to_string();
            let file_size = resolved
                .game
                .get("fileSize")
                .filter(|value| js_truthy(Some(value)))
                .cloned()
                .unwrap_or_else(|| json!(""));
            let upload_date = resolved
                .game
                .get("uploadDate")
                .filter(|value| js_truthy(Some(value)))
                .cloned()
                .unwrap_or_else(|| json!(""));
            return json!({
                "ok": true,
                "offer": {
                    "id": offer_id,
                    "sourceId": source_id,
                    "sourceTitle": resolved.source.unwrap_or_else(|| "Personal source".to_string()),
                    "originalTitle": title,
                    "normalizedTitle": normalize_retro_title(title),
                    "fileSize": file_size,
                    "uploadDate": upload_date,
                    "hasUris": !uris.is_empty(),
                    "uriCount": uris.len(),
                    "uris": uris,
                    "personal": true,
                }
            });
        }
        let id = encode_uri_component(&offer_id.chars().take(80).collect::<String>());
        let path = format!("/catalog/v1/retro/offers/{id}");
        self.with_fallback(&path, true, || self.fallback.get_offer(offer_id))
            .await
    }

    pub async fn audit(&self, params: &Value) -> Value {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(system) = params.get("system").and_then(Value::as_str) {
            if !system.is_empty() {
                pairs.push(("system", system.chars().take(80).collect()));
            }
        }
        if let Some(samples) = js_safe_integer(params.get("samples")) {
            pairs.push(("samples", samples.clamp(0, 100).to_string()));
        }
        let references: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let path = format!("/catalog/v1/retro/audit?{}", form_urlencode(&references));
        self.with_fallback(&path, false, || async {
            json!({"ok": false, "error": "retro_audit_unavailable"})
        })
        .await
    }

    pub async fn refresh(&self) -> Result<RefreshResult, CatalogError> {
        self.fallback.refresh().await
    }

    pub async fn migrate_from_v1(&self) -> Option<MigrationResult> {
        self.fallback.migrate_from_v1().await
    }
}

pub use crate::offers::Offer as CatalogOffer;

#[cfg(test)]
mod tests {
    use super::*;
    use arcadia_catalog::http::Response;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    struct MockFetcher {
        responses: Mutex<HashMap<String, Response>>,
        calls: AtomicUsize,
    }

    impl MockFetcher {
        fn new(responses: HashMap<String, Response>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl Fetcher for MockFetcher {
        async fn fetch(
            &self,
            request: Request,
        ) -> Result<Response, arcadia_catalog::http::FetchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .lock()
                .unwrap()
                .get(&request.url)
                .cloned()
                .ok_or_else(|| arcadia_catalog::http::FetchError::new("offline"))
        }
    }

    fn source(id: &str) -> Value {
        json!({
            "id": id,
            "title": "Fonte clássica",
            "url": format!("https://source.test/{id}.json"),
            "status": ["Classics"]
        })
    }

    fn fetcher_with(api: &str, responses: HashMap<String, Response>) -> Arc<MockFetcher> {
        let mut all = responses;
        all.insert(
            api.to_string(),
            Response::json(200, &json!({"sources": [source("classic")]})),
        );
        Arc::new(MockFetcher::new(all))
    }

    fn payload() -> Value {
        json!({
            "name": "Feed",
            "downloads": [
                {
                    "title": "Jogo clássico",
                    "uris": ["magnet:?xt=urn:btih:ABC123"],
                    "platform": "ps1",
                    "fileSize": "700 MB",
                    "descriptionHtml": "<b>Descrição</b> com <a href=\"https://example.test\">link</a>"
                },
                {"title": "Sem URI"}
            ]
        })
    }

    #[test]
    fn aceita_somente_fontes_classics_e_normaliza_campos() {
        let value = json!({
            "id": 10,
            "title": "Fonte clássica",
            "description": "PS1",
            "url": "https://source.test/classics.json",
            "gamesCount": 2,
            "status": ["Trusted", "Classics"]
        });
        let normalized = normalize_source(&value).unwrap();
        assert_eq!(normalized.id, "10");
        assert_eq!(normalized.games_count, 2);
        assert_eq!(
            normalized.registry_url,
            "https://library.hydra.wiki/sources/10"
        );
        let ignored = json!({
            "id": 11,
            "title": "Fonte moderna",
            "url": "https://source.test/modern.json",
            "status": ["Trusted"]
        });
        assert!(normalize_source(&ignored).is_none());
        let tagged = json!({
            "id": "tagged",
            "name": "Tagged",
            "link": "https://source.test/feed.json",
            "status": ["Trusted"],
            "tags": ["CLASSICS"]
        });
        assert_eq!(normalize_source(&tagged).unwrap().title, "Tagged");
    }

    #[test]
    fn rejeita_urls_perigosas_e_normaliza_uris() {
        assert!(normalize_source(&json!({
            "id": 1,
            "title": "x",
            "url": "http://example.test/x",
            "status": ["Classics"]
        }))
        .is_none());
        for url in [
            "https://localhost/x",
            "https://127.0.0.1/x",
            "https://10.0.0.1/x",
            "https://192.168.1.1/x",
            "https://172.20.0.1/x",
            "https://user:pass@example.test/x",
            "https://example.test/x#frag",
            "https://example.test:8443/x",
            "javascript:alert(1)",
        ] {
            assert!(safe_public_url_str(url, 2048).is_none(), "{url}");
        }
        assert_eq!(
            safe_public_url_str("https://example.test:443/x", 2048).as_deref(),
            Some("https://example.test/x")
        );
        assert_eq!(
            normalize_uri(&json!("MAGNET:?dn=game&xt=urn:btih:ABC123")).as_deref(),
            Some("magnet:?dn=game&xt=urn:btih:ABC123")
        );
        assert!(normalize_uri(&json!("magnet:?dn=game")).is_none());
        assert!(normalize_uri(&json!("file:///etc/passwd")).is_none());
    }

    #[test]
    fn normaliza_payload_remove_uris_invalidas_e_deduplica() {
        let source = normalize_source(&json!({
            "id": 1,
            "title": "x",
            "url": "https://example.test/x",
            "status": ["classics"]
        }))
        .unwrap();
        assert!(normalize_payload(&json!({"downloads": []}), &source).is_empty());
        let games = normalize_payload(
            &json!({
                "name": "Feed",
                "downloads": [
                    {
                        "title": "ok [ISO]",
                        "uris": ["https://example.test/file.zip", "file:///etc/passwd"]
                    },
                    {"title": "magnet", "uris": ["MAGNET:?dn=game&xt=urn:btih:ABC123"]},
                    {"title": "bad", "uris": ["javascript:alert(1)"]},
                    {"title": "dup", "uris": ["https://example.test/file.zip", "https://example.test/file.zip"]}
                ]
            }),
            &source,
        );
        assert_eq!(games.len(), 3);
        assert_eq!(games[0].title, "ok");
        assert_eq!(games[0].original_title.as_deref(), Some("ok [ISO]"));
        assert_eq!(games[0].uris, vec!["https://example.test/file.zip"]);
        assert_eq!(games[1].uris[0], "magnet:?dn=game&xt=urn:btih:ABC123");
        assert_eq!(games[2].uris.len(), 1);
    }

    #[tokio::test]
    async fn lista_pagina_busca_e_oculta_uris() {
        let dir = temp_dir();
        let api = "https://api.test/sources";
        let fetcher = fetcher_with(
            api,
            HashMap::from([(
                "https://source.test/classic.json".to_string(),
                Response::json(200, &payload()),
            )]),
        );
        let catalog = RetroCatalog::new(dir.path(), fetcher)
            .with_api_url(api)
            .with_now(Arc::new(|| 100));
        let page = catalog
            .list(&json!({"query": "classico", "limit": 20}))
            .await;
        assert_eq!(page["ok"], json!(true));
        assert_eq!(page["total"], json!(1));
        assert_eq!(page["games"][0]["title"], json!("Jogo classico"));
        assert_eq!(page["games"][0]["sourceTitle"], json!("Fonte clássica"));
        assert_eq!(page["games"][0]["description"], json!("Descrição com link"));
        assert!(page["games"][0].get("uris").is_none());
        let full = catalog.get_game("classic:0").await;
        assert_eq!(full["game"]["uris"][0], json!("magnet:?xt=urn:btih:ABC123"));
    }

    #[tokio::test]
    async fn cache_offline_preserva_catalogo_anterior() {
        let dir = temp_dir();
        let api = "https://api.test/sources";
        let fetcher = fetcher_with(
            api,
            HashMap::from([(
                "https://source.test/classic.json".to_string(),
                Response::json(
                    200,
                    &json!({
                        "name": "Feed",
                        "downloads": [
                            {"title": "Alpha Game", "uris": ["magnet:?xt=urn:btih:A"]},
                            {"title": "Beta Game", "uris": ["magnet:?xt=urn:btih:B"]},
                            {"title": "Gamma Game", "uris": ["magnet:?xt=urn:btih:C"]}
                        ]
                    }),
                ),
            )]),
        );
        let first = RetroCatalog::new(dir.path(), fetcher)
            .with_api_url(api)
            .with_now(Arc::new(|| 100));
        let page = first
            .list(&json!({"query": "game", "offset": 1, "limit": 1}))
            .await;
        assert_eq!(page["total"], json!(3));
        assert_eq!(page["games"][0]["title"], json!("Beta Game"));
        assert_eq!(page["hasMore"], json!(true));

        let offline = RetroCatalog::new(dir.path(), Arc::new(MockFetcher::new(HashMap::new())))
            .with_api_url(api)
            .with_now(Arc::new(|| 100 + 2 * CACHE_TTL_MS));
        let cached = offline.list(&json!({"limit": 2})).await;
        assert_eq!(cached["ok"], json!(true));
        assert_eq!(cached["total"], json!(3));
        assert_eq!(cached["games"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn erro_sem_cache_retorna_envelope_paginado() {
        let dir = temp_dir();
        let catalog = RetroCatalog::new(dir.path(), Arc::new(MockFetcher::new(HashMap::new())))
            .with_api_url("https://api.test/sources");
        let result = catalog.list(&json!({"offset": -4, "limit": 999})).await;
        assert_eq!(result["ok"], json!(false));
        assert_eq!(result["games"], json!([]));
        assert_eq!(result["offset"], json!(0));
        assert_eq!(result["limit"], json!(48));
    }

    #[tokio::test]
    async fn usa_cache_local_da_fonte_quando_http_falha() {
        let dir = temp_dir();
        let api = "https://api.test/sources";
        let url = "https://source.test/classic.json";
        let id: String = sha256_hex(url).chars().take(12).collect();
        let file = dir.path().join("sources").join(format!("{id}.json"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            serde_json::to_string(&json!({
                "name": "Cached feed",
                "downloads": [{"title": "Offline classic", "uris": ["magnet:?xt=urn:btih:CACHED"]}]
            }))
            .unwrap(),
        )
        .unwrap();
        let fetcher = fetcher_with(api, HashMap::new());
        let catalog = RetroCatalog::new(dir.path(), fetcher).with_api_url(api);
        let page = catalog.list(&json!({})).await;
        assert_eq!(page["ok"], json!(true));
        assert_eq!(page["games"][0]["title"], json!("Offline classic"));
    }

    #[tokio::test]
    async fn cache_legado_recebe_limpeza_de_titulos() {
        let dir = temp_dir();
        std::fs::write(
            dir.path().join(CACHE_FILE_NAME),
            serde_json::to_string(&json!({
                "version": 1,
                "updatedAt": 100,
                "sources": [],
                "games": [{
                    "id": "legacy:1",
                    "title": "Game [ISO]",
                    "sourceId": "legacy",
                    "sourceTitle": "Legacy",
                    "customField": {"kept": true}
                }],
                "customTop": 7
            }))
            .unwrap(),
        )
        .unwrap();
        let catalog = RetroCatalog::new(dir.path(), Arc::new(MockFetcher::new(HashMap::new())))
            .with_now(Arc::new(|| 100));
        let result = catalog.list(&json!({})).await;
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["games"][0]["title"], json!("Game"));
        assert_eq!(result["games"][0]["customField"], json!({"kept": true}));
        let cached = read_cache(&dir.path().join(CACHE_FILE_NAME)).unwrap();
        assert_eq!(cached.extra.get("customTop"), Some(&json!(7)));
    }

    #[test]
    fn gera_ids_estaveis_como_o_builder() {
        assert_eq!(
            generate_game_id(
                "sony-playstation",
                &Identity {
                    serial: Some("SLUS00312".to_string()),
                    ..Identity::default()
                }
            ),
            "retro:sony-playstation:SLUS00312"
        );
        assert_eq!(
            generate_game_id(
                "nintendo-nes",
                &Identity {
                    sha1: Some("abcdef1234567890abcdef1234567890abcdef12".to_string()),
                    ..Identity::default()
                }
            ),
            "retro:nintendo-nes:sha1:abcdef1234567890"
        );
        assert_eq!(
            generate_game_id(
                "nintendo-nes",
                &Identity {
                    crc32: Some("12345678".to_string()),
                    ..Identity::default()
                }
            ),
            "retro:nintendo-nes:crc32:12345678"
        );
        assert!(generate_game_id(
            "nintendo-nes",
            &Identity {
                title: Some("Super Mario Bros".to_string()),
                ..Identity::default()
            }
        )
        .starts_with("retro:nintendo-nes:slug:"));
    }

    fn offer(id: &str, title: &str, system: &str, kind: &str) -> Offer {
        Offer {
            id: id.to_string(),
            source_id: "source1".to_string(),
            source_title: "Source".to_string(),
            original_title: title.to_string(),
            normalized_title: title.to_string(),
            system_id: Some(system.to_string()),
            platform_raw: system.to_string(),
            release_kind: kind.to_string(),
            uris: vec![format!("magnet:{id}")],
            ..Offer::default()
        }
    }

    #[test]
    fn cria_jogo_canonico_e_mescla_ofertas() {
        let offer = Offer {
            normalized_title: "Final Fantasy VII".to_string(),
            system_id: Some("sony-playstation".to_string()),
            serials: vec!["SLUS00312".to_string()],
            region: Some("USA".to_string()),
            languages: vec!["en".to_string()],
            cover: Some(json!("https://example.com/cover.jpg")),
            ..Offer::default()
        };
        let game = create_canonical_game(&offer, 1);
        assert_eq!(game.title, "Final Fantasy VII");
        assert_eq!(game.serials[0], "SLUS00312");
        assert_eq!(game.regions[0], "USA");
        assert_eq!(game.offer_count, 1);
        assert_eq!(
            game.artwork["cover"],
            json!("https://example.com/cover.jpg")
        );
        assert_eq!(game.artwork["provider"], json!("source"));

        let offers = vec![
            Offer {
                normalized_title: "Final Fantasy VII".to_string(),
                original_title: "Final Fantasy VII [USA]".to_string(),
                serials: vec!["SLUS00312".to_string()],
                region: Some("USA".to_string()),
                languages: vec!["en".to_string()],
                ..Offer::default()
            },
            Offer {
                normalized_title: "Final Fantasy VII".to_string(),
                original_title: "Final Fantasy VII [EUR]".to_string(),
                serials: vec!["SCES00867".to_string()],
                region: Some("EUR".to_string()),
                languages: vec!["en".to_string(), "fr".to_string(), "de".to_string()],
                ..Offer::default()
            },
        ];
        let merged = merge_offers(&offers).unwrap();
        assert_eq!(merged.offer_count, 2);
        assert_eq!(merged.serials, vec!["SLUS00312", "SCES00867"]);
        assert_eq!(merged.regions, vec!["USA", "EUR"]);
        assert!(merged.languages.contains(&"fr".to_string()));
    }

    #[test]
    fn constroi_catalogo_agrupa_e_filtra_nao_jogos() {
        let offers = vec![
            offer("offer1", "Super Mario Bros", "nintendo-nes", "game"),
            offer("offer2", "Super Mario Bros", "nintendo-nes", "game"),
            offer("offer3", "The Legend of Zelda", "nintendo-nes", "game"),
            offer("offer4", "BIOS Files", "nintendo-nes", "bios"),
        ];
        let catalog = build_canonical_catalog(&offers, &BuildOptions::default());
        assert_eq!(catalog.games.len(), 2);
        assert_eq!(catalog.matches.len(), 3);
        assert_eq!(
            catalog
                .games
                .iter()
                .find(|game| game.title == "Super Mario Bros")
                .unwrap()
                .offer_count,
            2
        );
        assert_eq!(catalog.stats.total_offers, 4);
        assert_eq!(catalog.stats.game_offers, 3);
        assert_eq!(catalog.stats.special_offers, 1);
        assert_eq!(catalog.stats.games_created, 2);
        assert_eq!(catalog.stats.by_system["nintendo-nes"], json!(2));
        assert_eq!(catalog.stats.by_release_kind["game"], json!(3));
        assert_eq!(catalog.stats.by_match_quality.strong, 3);

        let unmatched = build_canonical_catalog(
            &[Offer {
                id: "offer1".to_string(),
                normalized_title: "Unknown Game".to_string(),
                system_id: None,
                release_kind: "game".to_string(),
                platform_raw: "ps9".to_string(),
                ..Offer::default()
            }],
            &BuildOptions::default(),
        );
        assert!(unmatched.games.is_empty());
        assert_eq!(unmatched.unmatched.len(), 1);
        assert_eq!(unmatched.unmatched[0].reason, "no system identified");
        assert_eq!(unmatched.unmatched[0].platform_raw.as_deref(), Some("ps9"));
    }

    #[test]
    fn usa_serial_e_fuzzy_contra_catalogo_canonico_sem_cruzar_sistema() {
        let canonical_games = vec![CanonicalGame {
            id: "retro:ps2:SLUS20312".to_string(),
            system_id: "sony-playstation-2".to_string(),
            title: "Example Adventure".to_string(),
            sort_title: "example adventure".to_string(),
            serials: vec!["SLUS20312".to_string()],
            ..CanonicalGame::default()
        }];
        let offers = vec![
            Offer {
                id: "serial-offer".to_string(),
                normalized_title: "Completely Different Release Name".to_string(),
                system_id: Some("sony-playstation-2".to_string()),
                serials: vec!["SLUS-20312".to_string()],
                release_kind: "game".to_string(),
                ..Offer::default()
            },
            Offer {
                id: "wrong-system".to_string(),
                normalized_title: "Example Adventure".to_string(),
                system_id: Some("sony-playstation-3".to_string()),
                release_kind: "game".to_string(),
                ..Offer::default()
            },
        ];
        let catalog = build_canonical_catalog(
            &offers,
            &BuildOptions {
                canonical_games: Some(canonical_games),
                ..BuildOptions::default()
            },
        );
        assert_eq!(catalog.matches.len(), 1);
        assert_eq!(catalog.matches[0].offer_id, "serial-offer");
        assert_eq!(catalog.matches[0].method, "serial");
        assert_eq!(catalog.matches[0].score, 100);
        assert_eq!(catalog.unmatched.len(), 1);
    }

    #[test]
    fn aplica_overrides_manuais_e_ignorados() {
        let offers = vec![
            offer("o1", "Game", "nes", "game"),
            offer("o2", "Other", "nes", "game"),
        ];
        let catalog = build_canonical_catalog(&offers, &BuildOptions::default());
        let ignored_id = "o1".to_string();
        let manual_id = "o2".to_string();
        let overrides = HashMap::from([
            (ignored_id, json!({"gameId": null})),
            (manual_id, json!({"gameId": "retro:nes:manual"})),
        ]);
        let updated = apply_overrides_to_catalog(&catalog, &overrides);
        assert!(updated.matches.iter().all(|found| found.offer_id != "o1"));
        let manual = updated
            .matches
            .iter()
            .find(|found| found.offer_id == "o2")
            .unwrap();
        assert_eq!(manual.method, "manual");
        assert_eq!(manual.quality, MatchQuality::Exact);
        assert_eq!(manual.score, 100);

        let no_system = build_canonical_catalog(
            &[Offer {
                id: "orphan".to_string(),
                normalized_title: "Orphan".to_string(),
                release_kind: "game".to_string(),
                ..Offer::default()
            }],
            &BuildOptions::default(),
        );
        assert_eq!(no_system.unmatched.len(), 1);
        let promoted = apply_overrides_to_catalog(
            &no_system,
            &HashMap::from([("orphan".to_string(), json!({"gameId": "retro:nes:x"}))]),
        );
        assert!(promoted.unmatched.is_empty());
        assert_eq!(promoted.matches.len(), 1);
        assert_eq!(promoted.matches[0].game_id, "retro:nes:x");
    }

    struct FakeV1 {
        export: Mutex<Option<CatalogExport>>,
        refresh: Mutex<Option<CatalogExport>>,
    }

    impl FakeV1 {
        fn new(export: Option<CatalogExport>, refresh: Option<CatalogExport>) -> Self {
            Self {
                export: Mutex::new(export),
                refresh: Mutex::new(refresh),
            }
        }
    }

    #[async_trait]
    impl V1Catalog for FakeV1 {
        async fn export_all(&self, _refresh: bool) -> Result<CatalogExport, CatalogError> {
            self.export
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| CatalogError::new("sem export"))
        }

        async fn refresh(&self) -> Result<CatalogExport, CatalogError> {
            self.refresh
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| CatalogError::new("sem refresh"))
        }
    }

    fn export_with(games: Vec<CatalogGame>) -> CatalogExport {
        CatalogExport {
            sources: vec![CatalogSource {
                id: "classic".to_string(),
                title: "Classics".to_string(),
                ..CatalogSource::default()
            }],
            games,
            updated_at: 1,
        }
    }

    fn v1_game(index: usize) -> CatalogGame {
        CatalogGame {
            id: format!("classic:{index}"),
            title: format!("Game {index}"),
            source_id: "classic".to_string(),
            source_title: "Classics".to_string(),
            platform: "nes".to_string(),
            uris: vec![format!("magnet:?xt=urn:btih:HASH{index}")],
            ..CatalogGame::default()
        }
    }

    #[tokio::test]
    async fn migra_v1_preservando_uris_somente_na_oferta() {
        let dir = temp_dir();
        let games: Vec<CatalogGame> = (0..120).map(v1_game).collect();
        let v1 = Arc::new(FakeV1::new(Some(export_with(games.clone())), None));
        let catalog = RetroCatalogV2::new(dir.path(), v1).with_now(Arc::new(|| 1));
        let result = catalog.migrate_from_v1().await.unwrap();
        assert_eq!(result.offers_count, 120);
        assert_eq!(result.games_count, 120);
        assert_eq!(catalog.repository().get_stats().offers, 120);

        let page = catalog.list(&json!({"limit": 10})).await;
        assert_eq!(page["ok"], json!(true));
        assert_eq!(page["totalGames"], json!(120));
        assert_eq!(page["games"].as_array().unwrap().len(), 10);
        assert!(!page["games"]
            .as_array()
            .unwrap()
            .iter()
            .any(|game| game.get("uris").is_some()));

        let first_id = page["games"][0]["id"].as_str().unwrap();
        let detail = catalog.get_game(first_id).await;
        assert_eq!(detail["ok"], json!(true));
        let offers = detail["offers"].as_array().unwrap();
        assert!(!offers.is_empty());
        assert!(offers.iter().all(|offer| offer.get("uris").is_none()));
        let offer_id = offers[0]["id"].as_str().unwrap();
        let resolved = catalog.get_offer(offer_id).await;
        assert_eq!(resolved["ok"], json!(true));
        assert_eq!(resolved["offer"]["uris"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refresh_reconstroi_catalogo_e_estatisticas() {
        let dir = temp_dir();
        let data = export_with(vec![v1_game(1), v1_game(2)]);
        let v1 = Arc::new(FakeV1::new(None, Some(data)));
        let catalog = RetroCatalogV2::new(dir.path(), v1).with_now(Arc::new(|| 5));
        let result = catalog.refresh().await.unwrap();
        assert_eq!(result.games.len(), 2);
        assert_eq!(result.stats.games_created, 2);
        assert_eq!(result.updated_at, 5);
        assert_eq!(catalog.repository().get_stats().games, 2);
        assert_eq!(catalog.repository().get_stats().offers, 2);
    }

    #[tokio::test]
    async fn stats_e_migrate_usam_envelopes_do_ipc() {
        let dir = temp_dir();
        let v1 = Arc::new(FakeV1::new(Some(export_with(vec![v1_game(1)])), None));
        let catalog = RetroCatalogV2::new(dir.path(), v1).with_now(Arc::new(|| 1));
        let migrated = catalog.migrate_envelope().await;
        assert_eq!(migrated["ok"], json!(true));
        assert_eq!(migrated["result"]["migrated"], json!(true));
        let stats = catalog.stats_envelope();
        assert_eq!(stats["ok"], json!(true));
        assert_eq!(stats["stats"]["games"], json!(1));
        assert_eq!(stats["stats"]["offers"], json!(1));
    }

    #[tokio::test]
    async fn list_v2_filtra_por_sistema_e_query() {
        let dir = temp_dir();
        let mut nes = v1_game(1);
        nes.title = "Alpha".to_string();
        let mut snes = v1_game(2);
        snes.title = "Beta".to_string();
        snes.platform = "snes".to_string();
        let v1 = Arc::new(FakeV1::new(Some(export_with(vec![nes, snes])), None));
        let catalog = RetroCatalogV2::new(dir.path(), v1).with_now(Arc::new(|| 1));
        assert!(catalog.migrate_from_v1().await.is_some());

        let all = catalog.list(&json!({})).await;
        assert_eq!(all["totalGames"], json!(2));
        assert_eq!(all["facets"]["systems"]["nintendo-nes"], json!(1));
        let filtered = catalog.list(&json!({"systems": ["nintendo-nes"]})).await;
        assert_eq!(filtered["totalGames"], json!(1));
        let searched = catalog.list(&json!({"query": "bet"})).await;
        assert_eq!(searched["totalGames"], json!(1));
        assert_eq!(searched["games"][0]["title"], json!("Beta"));
    }

    #[tokio::test]
    async fn audit_devolve_cobertura_e_unmatched() {
        let dir = temp_dir();
        let v1 = Arc::new(FakeV1::new(Some(export_with(vec![v1_game(1)])), None));
        let catalog = RetroCatalogV2::new(dir.path(), v1).with_now(Arc::new(|| 1));
        assert!(catalog.migrate_from_v1().await.is_some());
        let audit = catalog.audit(&json!({"samples": 5}));
        assert_eq!(audit["ok"], json!(true));
        assert_eq!(audit["totals"]["games"], json!(1));
        assert_eq!(audit["missing"]["cover"], json!(1));
    }

    #[test]
    fn dedupe_de_games_nao_depende_de_hashset_instavel() {
        // Sanidade: o catalogo mantém apenas uma cópia por id de seed.
        let offers = vec![
            offer("a", "Game", "nes", "game"),
            offer("b", "Game", "nes", "game"),
        ];
        let catalog = build_canonical_catalog(&offers, &BuildOptions::default());
        let ids: HashSet<&str> = catalog.games.iter().map(|game| game.id.as_str()).collect();
        assert_eq!(ids.len(), catalog.games.len());
    }

    struct FakeRemote {
        reply: Mutex<Result<Value, String>>,
        paths: Mutex<Vec<(String, bool)>>,
    }

    impl FakeRemote {
        fn new(reply: Result<Value, String>) -> Self {
            Self {
                reply: Mutex::new(reply),
                paths: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RemoteCatalogProvider for FakeRemote {
        async fn catalog_get(&self, path: &str, no_mirror: bool) -> Result<Value, String> {
            self.paths
                .lock()
                .unwrap()
                .push((path.to_string(), no_mirror));
            self.reply.lock().unwrap().clone()
        }
    }

    #[derive(Clone, Default)]
    struct FakePersonal {
        candidates: Vec<PersonalCandidate>,
        game: Option<PersonalGame>,
    }

    #[async_trait]
    impl PersonalSources for FakePersonal {
        async fn search(&self, _query: &str, _limit: usize) -> Vec<PersonalCandidate> {
            self.candidates.clone()
        }

        async fn get_game(&self, _reference: &str) -> Option<PersonalGame> {
            self.game.clone()
        }
    }

    fn catalog_v2_with_game(dir: &std::path::Path, game: Value) -> RetroCatalogV2 {
        let v1 = Arc::new(FakeV1::new(None, None));
        let catalog = RetroCatalogV2::new(dir, v1).with_now(Arc::new(|| 1));
        catalog
            .repository()
            .set_catalog_index(vec![game], Map::new());
        catalog
    }

    #[tokio::test]
    async fn servidor_prefere_remoto_e_cai_no_local_quando_falha() {
        let dir = temp_dir();
        let fallback = Arc::new(catalog_v2_with_game(
            dir.path(),
            json!({"id": "g1", "systemId": "nes", "title": "Local"}),
        ));
        let remote = Arc::new(FakeRemote::new(Ok(json!({
            "ok": true,
            "games": [{"id": "remote"}],
            "total": 1
        }))));
        let server = RetroServerCatalog::new(fallback.clone()).with_remote(remote.clone());
        let result = server
            .list(&json!({"query": "mario", "systems": ["nes"], "limit": 500}))
            .await;
        assert_eq!(result["games"][0]["id"], json!("remote"));
        let paths = remote.paths.lock().unwrap().clone();
        assert_eq!(
            paths[0].0,
            "/catalog/v1/retro/games?offset=0&limit=100&query=mario&system=nes"
        );
        assert!(!paths[0].1);

        let failing = Arc::new(FakeRemote::new(Err("offline".to_string())));
        let server = RetroServerCatalog::new(fallback).with_remote(failing);
        let local = server.list(&json!({})).await;
        assert_eq!(local["ok"], json!(true));
        assert_eq!(local["games"][0]["id"], json!("g1"));
    }

    #[tokio::test]
    async fn servidor_inclui_ofertas_pessoais_e_resolve_uri() {
        let dir = temp_dir();
        let fallback = Arc::new(catalog_v2_with_game(
            dir.path(),
            json!({"id": "g1", "systemId": "nes", "title": "Game", "offerCount": 2}),
        ));
        let personal = Arc::new(FakePersonal {
            candidates: vec![
                PersonalCandidate {
                    reference: "personal-src:42".to_string(),
                    title: "Game [USA]".to_string(),
                    source: "My source".to_string(),
                    file_size: json!("1 MB"),
                    upload_date: json!("2024"),
                },
                PersonalCandidate {
                    reference: "personal-src:99".to_string(),
                    title: "Other".to_string(),
                    source: "My source".to_string(),
                    file_size: json!(""),
                    upload_date: json!(""),
                },
            ],
            game: Some(PersonalGame {
                game: json!({"title": "Game [USA]", "uris": ["magnet:?xt=urn:btih:P"], "fileSize": "1 MB", "uploadDate": "2024"}),
                source: Some("My source".to_string()),
            }),
        });
        let server = RetroServerCatalog::new(fallback).with_personal_sources(personal);
        let detail = server.get_game("g1").await;
        assert_eq!(detail["ok"], json!(true));
        let offers = detail["offers"].as_array().unwrap();
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0]["id"], json!("personal:personal-src:42"));
        assert_eq!(offers[0]["personal"], json!(true));
        assert_eq!(detail["game"]["offerCount"], json!(3));

        let resolved = server.get_offer("personal:personal-src:42").await;
        assert_eq!(resolved["ok"], json!(true));
        assert_eq!(resolved["offer"]["uris"][0], json!("magnet:?xt=urn:btih:P"));
        assert_eq!(resolved["offer"]["hasUris"], json!(true));
        assert_eq!(resolved["offer"]["sourceId"], json!("personal-src"));
    }

    #[tokio::test]
    async fn servidor_audit_sem_remoto_devolve_indisponivel() {
        let dir = temp_dir();
        let fallback = Arc::new(catalog_v2_with_game(
            dir.path(),
            json!({"id": "g1", "systemId": "nes", "title": "Local"}),
        ));
        let remote = Arc::new(FakeRemote::new(Err("offline".to_string())));
        let server = RetroServerCatalog::new(fallback).with_remote(remote.clone());
        let result = server
            .audit(&json!({"system": "nes", "samples": 999}))
            .await;
        assert_eq!(result["ok"], json!(false));
        assert_eq!(result["error"], json!("retro_audit_unavailable"));
        let paths = remote.paths.lock().unwrap().clone();
        assert_eq!(paths[0].0, "/catalog/v1/retro/audit?system=nes&samples=100");
    }

    /// O cache em memória evita a segunda ida ao servidor dentro do TTL.
    ///
    /// Sem isto, voltar para a página anterior ou reabrir a aba pagava de novo
    /// os 0,44 s (mediana) de ida ao catálogo.
    #[tokio::test]
    async fn cache_evita_segunda_ida_ao_servidor() {
        use std::sync::atomic::AtomicUsize;

        struct ContadorRemote {
            chamadas: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl RemoteCatalogProvider for ContadorRemote {
            async fn catalog_get(&self, _path: &str, _no_mirror: bool) -> Result<Value, String> {
                self.chamadas.fetch_add(1, Ordering::SeqCst);
                Ok(json!({ "ok": true, "games": [], "total": 0 }))
            }
        }

        let dir = temp_dir();
        let fallback = Arc::new(catalog_v2_with_game(dir.path(), json!({ "id": "g1" })));
        let chamadas = Arc::new(AtomicUsize::new(0));
        let catalog = RetroServerCatalog::new(fallback).with_remote(Arc::new(ContadorRemote {
            chamadas: Arc::clone(&chamadas),
        }));

        let params = json!({ "offset": 0, "limit": 24 });

        // Primeira chamada: vai ao servidor.
        let _ = catalog.list(&params).await;
        assert_eq!(chamadas.load(Ordering::SeqCst), 1, "primeira ida");

        // Segunda: o cache responde e o contador não sobe.
        let _ = catalog.list(&params).await;
        assert_eq!(
            chamadas.load(Ordering::SeqCst),
            1,
            "cache deve evitar a segunda ida"
        );

        // Parâmetros diferentes são chave diferente: vai ao servidor de novo.
        let _ = catalog.list(&json!({ "offset": 24, "limit": 24 })).await;
        assert_eq!(chamadas.load(Ordering::SeqCst), 2, "página nova, nova ida");
    }
}
