//! Metadados retrô — port 1:1 de `app/electron/retro-metadata-igdb.js` e
//! `app/electron/retro-metadata-enricher.js`.
//!
//! O mapeamento IGDB (textos/artes), as queries Apicalypse, o cache com TTL e
//! as regras de merge/enriquecimento são puros. A rede é sempre injetada por
//! [`Fetcher`]; os testes usam duplos, nunca HTTP real.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcadia_catalog::http::{form_urlencode, Fetcher, Request};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::util::{js_truthy, now_ms, safe_read_json, safe_write_json, sha256_hex};

pub const METADATA_CACHE_VERSION: u64 = 1;
pub const METADATA_TTL_DAYS: i64 = 30;
pub const ARTWORK_TTL_DAYS: i64 = 7;
pub const MAX_CACHE_SIZE_MB: u64 = 200;
pub const RATE_LIMIT_DELAY_MS: u64 = 250;
pub const METADATA_TTL_MS: i64 = METADATA_TTL_DAYS * 24 * 60 * 60 * 1000;

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const IGDB_URL: &str = "https://api.igdb.com/v4/games";
const SUPPORTED_SYSTEMS: &[&str] = &[
    "sony-playstation-2",
    "sony-playstation-3",
    "sony-playstation",
    "sony-psp",
];

/// Mapeamento `systemId` → platform ID da IGDB.
pub const PLATFORM_MAP: &[(&str, u64)] = &[
    ("sony-playstation", 7),
    ("sony-playstation-2", 8),
    ("sony-playstation-3", 9),
    ("sony-psp", 38),
];

pub fn igdb_platform_id(system_id: &str) -> Option<u64> {
    PLATFORM_MAP
        .iter()
        .find(|(id, _)| *id == system_id)
        .map(|(_, platform)| *platform)
}

/// Credenciais Twitch/IGDB. O `Debug` nunca revela o secret.
#[derive(Clone, PartialEq, Eq)]
pub struct IgdbCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for IgdbCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IgdbCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"***")
            .finish()
    }
}

impl IgdbCredentials {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }
}

/// Hash estável das credenciais (16 hex), sem gravar os segredos.
pub fn get_credentials_hash(credentials: &IgdbCredentials) -> String {
    let data = format!("{}:{}", credentials.client_id, credentials.client_secret);
    sha256_hex(&data).chars().take(16).collect()
}

/// Arte da IGDB já com URLs prontas.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct IgdbArtwork {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_hd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artworks: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<Vec<String>>,
}

/// Metadados normalizados de um jogo IGDB.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IgdbMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub summary: Option<String>,
    pub release_date: Option<String>,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub game_modes: Vec<String>,
    pub perspectives: Vec<String>,
    pub rating: Option<f64>,
    pub rating_count: i64,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub artwork: IgdbArtwork,
}

fn names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn image_url(image_id: &str, size: &str) -> String {
    format!("https://images.igdb.com/igdb/image/upload/{size}/{image_id}.jpg")
}

/// Normaliza a resposta crua da IGDB para o formato do Arcadia.
pub fn normalize_igdb_game(game: &Value) -> IgdbMetadata {
    let title = game
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let summary = game
        .get("summary")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            game.get("storyline")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .map(ToString::to_string);
    let release_date = game
        .get("first_release_date")
        .and_then(Value::as_f64)
        .filter(|seconds| *seconds != 0.0)
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds as i64, 0))
        .map(|date| date.format("%Y-%m-%d").to_string());

    let aggregated_rating = game
        .get("aggregated_rating")
        .and_then(Value::as_f64)
        .filter(|value| *value != 0.0);
    let rating = aggregated_rating.or_else(|| {
        game.get("rating")
            .and_then(Value::as_f64)
            .filter(|value| *value != 0.0)
    });

    let aggregated_count = game
        .get("aggregated_rating_count")
        .and_then(Value::as_f64)
        .filter(|value| *value != 0.0);
    let rating_count = aggregated_count
        .or_else(|| {
            game.get("rating_count")
                .and_then(Value::as_f64)
                .filter(|value| *value != 0.0)
        })
        .unwrap_or(0.0) as i64;

    let mut developer = None;
    let mut publisher = None;
    if let Some(companies) = game.get("involved_companies").and_then(Value::as_array) {
        for involved in companies {
            let company_name = involved
                .get("company")
                .and_then(|company| company.get("name"))
                .and_then(Value::as_str);
            let Some(company_name) = company_name else {
                continue;
            };
            if js_truthy(involved.get("developer")) {
                developer = Some(company_name.to_string());
            }
            if js_truthy(involved.get("publisher")) {
                publisher = Some(company_name.to_string());
            }
        }
    }

    let cover_id = game
        .get("cover")
        .and_then(|cover| cover.get("image_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let artworks = game
        .get("artworks")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("image_id").and_then(Value::as_str))
                .map(|id| image_url(id, "t_1080p"))
                .collect::<Vec<_>>()
        });
    let screenshots = game
        .get("screenshots")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("image_id").and_then(Value::as_str))
                .map(|id| image_url(id, "t_screenshot_big"))
                .collect::<Vec<_>>()
        });

    IgdbMetadata {
        title,
        summary,
        release_date,
        genres: names(game.get("genres")),
        themes: names(game.get("themes")),
        game_modes: names(game.get("game_modes")),
        perspectives: names(game.get("player_perspectives")),
        rating,
        rating_count,
        developer,
        publisher,
        artwork: IgdbArtwork {
            cover: cover_id.map(|id| image_url(id, "t_cover_big")),
            cover_hd: cover_id.map(|id| image_url(id, "t_1080p")),
            artworks,
            screenshots,
        },
    }
}

fn serial_query(serial: &str, platform_id: u64) -> String {
    let normalized = serial.replace(['-', '_'], "");
    format!(
        "fields name, summary, genres.name, involved_companies.company.name,
             involved_companies.developer, involved_companies.publisher,
             first_release_date, cover.url, cover.image_id,
             artworks.url, artworks.image_id, screenshots.url, screenshots.image_id,
             aggregated_rating, aggregated_rating_count, rating, rating_count,
             storyline, game_modes.name, player_perspectives.name, themes.name;
      where platforms = {platform_id} & (version_title ~ \"{normalized}\"*
            | alternative_names.name ~ \"*{normalized}*\");
      limit 5;"
    )
}

fn title_query(title: &str, platform_id: u64) -> String {
    let safe_title = title.replace('"', "\\\"");
    format!(
        "fields name, summary, genres.name, involved_companies.company.name,
             involved_companies.developer, involved_companies.publisher,
             first_release_date, cover.url, cover.image_id,
             artworks.url, artworks.image_id, screenshots.url, screenshots.image_id,
             aggregated_rating, aggregated_rating_count, rating, rating_count,
             storyline, game_modes.name, player_perspectives.name, themes.name;
      search \"{safe_title}\";
      where platforms = {platform_id};
      limit 3;"
    )
}

/// Query Apicalypse usada por `searchBySerial` (serial sem `-`/`_`).
pub fn build_serial_query(serial: &str, system_id: &str) -> Option<String> {
    let platform_id = igdb_platform_id(system_id)?;
    Some(serial_query(serial, platform_id))
}

/// Query Apicalypse usada por `searchByTitle` (aspas escapadas).
pub fn build_title_query(title: &str, system_id: &str) -> Option<String> {
    let platform_id = igdb_platform_id(system_id)?;
    Some(title_query(title, platform_id))
}

/// Provedor de metadados injetável (produção: [`IgdbClient`]; testes: duplo).
#[async_trait]
pub trait IgdbLookup: Send + Sync {
    async fn search_by_serial(&self, serial: &str, system_id: &str) -> Option<IgdbMetadata>;
    async fn search_by_title(&self, title: &str, system_id: &str) -> Option<IgdbMetadata>;
}

#[derive(Clone, Debug)]
struct CachedToken {
    token: String,
    expiry: i64,
}

/// Cliente IGDB sobre um [`Fetcher`] injetado.
pub struct IgdbClient {
    fetcher: Arc<dyn Fetcher>,
    credentials: IgdbCredentials,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    token: Mutex<Option<CachedToken>>,
    rate_limit: tokio::sync::Mutex<()>,
    delay_ms: u64,
}

impl IgdbClient {
    pub fn new(fetcher: Arc<dyn Fetcher>, credentials: IgdbCredentials) -> Self {
        Self {
            fetcher,
            credentials,
            now: Arc::new(now_ms),
            token: Mutex::new(None),
            rate_limit: tokio::sync::Mutex::new(()),
            delay_ms: RATE_LIMIT_DELAY_MS,
        }
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }

    /// Ajusta o atraso de rate limit (0 em testes unitários).
    pub fn with_delay_ms(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    async fn access_token(&self) -> Option<String> {
        let now = self.now();
        if let Ok(guard) = self.token.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.expiry > now + 60_000 {
                    return Some(cached.token.clone());
                }
            }
        }

        let body = form_urlencode(&[
            ("client_id", self.credentials.client_id.as_str()),
            ("client_secret", self.credentials.client_secret.as_str()),
            ("grant_type", "client_credentials"),
        ]);
        let request = Request::post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body);
        let response = self.fetcher.fetch(request).await.ok()?;
        if !response.is_success() {
            return None;
        }
        let value = response.json_value().ok()?;
        let token = value
            .get("access_token")
            .and_then(Value::as_str)?
            .to_string();
        let expires_in = value
            .get("expires_in")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if let Ok(mut guard) = self.token.lock() {
            *guard = Some(CachedToken {
                token: token.clone(),
                expiry: now + (expires_in * 1000.0) as i64,
            });
        }
        Some(token)
    }

    async fn request_games(&self, query: &str) -> Option<Value> {
        let token = self.access_token().await?;
        let request = Request::post(IGDB_URL)
            .header("Client-ID", self.credentials.client_id.clone())
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "text/plain")
            .body(query.to_string());
        if self.delay_ms > 0 {
            let _guard = self.rate_limit.lock().await;
            let response = self.fetcher.fetch(request).await.ok()?;
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            if !response.is_success() {
                return None;
            }
            return response.json_value().ok();
        }
        let response = self.fetcher.fetch(request).await.ok()?;
        if !response.is_success() {
            return None;
        }
        response.json_value().ok()
    }

    async fn search(&self, query: String) -> Option<IgdbMetadata> {
        let results = self.request_games(&query).await?;
        let first = results.as_array()?.first()?;
        Some(normalize_igdb_game(first))
    }
}

#[async_trait]
impl IgdbLookup for IgdbClient {
    async fn search_by_serial(&self, serial: &str, system_id: &str) -> Option<IgdbMetadata> {
        let platform_id = igdb_platform_id(system_id)?;
        // O JS pede o token antes de checar a plataforma; o resultado é o mesmo.
        self.search(serial_query(serial, platform_id)).await
    }

    async fn search_by_title(&self, title: &str, system_id: &str) -> Option<IgdbMetadata> {
        let platform_id = igdb_platform_id(system_id)?;
        self.search(title_query(title, platform_id)).await
    }
}

// ── cache/enriquecimento ───────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManifestEntry {
    pub fetched_at: i64,
    pub size: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ManifestEntry {
    fn default() -> Self {
        Self {
            fetched_at: 0,
            size: 0,
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetadataManifest {
    pub version: u64,
    pub created: i64,
    pub entries: BTreeMap<String, ManifestEntry>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for MetadataManifest {
    fn default() -> Self {
        Self {
            version: METADATA_CACHE_VERSION,
            created: 0,
            entries: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

/// Opções de `enrichCatalog`.
#[derive(Clone, Default)]
pub struct EnrichOptions {
    pub force: bool,
    pub max_games: Option<usize>,
    pub system_filter: Option<Vec<String>>,
    pub on_progress: Option<Arc<dyn Fn(EnrichProgress) + Send + Sync>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichProgress {
    pub processed: usize,
    pub total: usize,
    pub done: bool,
}

/// Estatísticas de `getCacheStats`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub expired_entries: usize,
    pub total_size_mb: String,
    pub cache_dir: String,
}

/// Enricher com cache em `{dataDir}/retro/metadata`.
pub struct MetadataEnricher {
    cache_dir: PathBuf,
    artwork_cache_dir: PathBuf,
    manifest_path: PathBuf,
    lookup: Option<Arc<dyn IgdbLookup>>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    manifest: Mutex<MetadataManifest>,
}

impl MetadataEnricher {
    pub fn new(data_dir: impl Into<PathBuf>, lookup: Option<Arc<dyn IgdbLookup>>) -> Self {
        Self::with_now(data_dir, lookup, Arc::new(now_ms))
    }

    pub fn with_now(
        data_dir: impl Into<PathBuf>,
        lookup: Option<Arc<dyn IgdbLookup>>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        let data_dir = data_dir.into();
        let cache_dir = data_dir.join("retro").join("metadata");
        let artwork_cache_dir = data_dir.join("retro").join("artwork");
        let _ = std::fs::create_dir_all(&cache_dir);
        let _ = std::fs::create_dir_all(&artwork_cache_dir);
        let manifest_path = cache_dir.join("manifest.json");
        let manifest = safe_read_json(&manifest_path, MAX_CACHE_SIZE_MB * 1024 * 1024)
            .and_then(|value| serde_json::from_value::<MetadataManifest>(value).ok())
            .map(|mut manifest| {
                if manifest.version == 0 {
                    manifest.version = METADATA_CACHE_VERSION;
                }
                manifest
            })
            .unwrap_or_else(|| MetadataManifest {
                created: (now)(),
                ..MetadataManifest::default()
            });
        Self {
            cache_dir,
            artwork_cache_dir,
            manifest_path,
            lookup,
            now,
            manifest: Mutex::new(manifest),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn artwork_cache_dir(&self) -> &Path {
        &self.artwork_cache_dir
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn lock_manifest(&self) -> std::sync::MutexGuard<'_, MetadataManifest> {
        self.manifest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn manifest(&self) -> MetadataManifest {
        self.lock_manifest().clone()
    }

    fn save_manifest(&self) {
        let manifest = self.lock_manifest().clone();
        if let Ok(value) = serde_json::to_value(&manifest) {
            let _ = safe_write_json(&self.manifest_path, &value, true);
        }
    }

    /// `getCacheId`: sha256(systemId:identifier) em 24 hex.
    pub fn get_cache_id(system_id: &str, identifier: &str) -> String {
        sha256_hex(&format!("{system_id}:{identifier}"))
            .chars()
            .take(24)
            .collect()
    }

    pub fn is_cache_valid(entry: &ManifestEntry, ttl_ms: i64, now: i64) -> bool {
        entry.fetched_at != 0 && now - entry.fetched_at < ttl_ms
    }

    pub fn load_from_cache(&self, cache_id: &str) -> Option<Value> {
        safe_read_json(
            &self.cache_dir.join(format!("{cache_id}.json")),
            MAX_CACHE_SIZE_MB * 1024 * 1024,
        )
    }

    pub fn save_to_cache(&self, cache_id: &str, metadata: &Value) {
        let envelope = json!({
            "version": METADATA_CACHE_VERSION,
            "cacheId": cache_id,
            "fetchedAt": self.now(),
            "metadata": metadata,
        });
        if safe_write_json(
            &self.cache_dir.join(format!("{cache_id}.json")),
            &envelope,
            true,
        )
        .is_err()
        {
            return;
        }
        let size = serde_json::to_string(&envelope)
            .map(|text| text.len() as u64)
            .unwrap_or(0);
        let fetched_at = envelope
            .get("fetchedAt")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut manifest = self.lock_manifest();
        manifest.entries.insert(
            cache_id.to_string(),
            ManifestEntry {
                fetched_at,
                size,
                extra: Map::new(),
            },
        );
        drop(manifest);
        self.save_manifest();
    }

    /// Busca metadados com cache de 30 dias e fallback serial → título.
    pub async fn fetch_metadata(&self, game: &Value, force: bool) -> Option<Value> {
        let system_id = game.get("systemId").and_then(Value::as_str)?;
        if !SUPPORTED_SYSTEMS.contains(&system_id) {
            return None;
        }
        let serial = game
            .get("serials")
            .and_then(Value::as_array)
            .and_then(|serials| serials.first())
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let identifier = serial.map(ToString::to_string).or_else(|| {
            game.get("sortTitle")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })?;

        let cache_id = Self::get_cache_id(system_id, &identifier);
        if !force {
            let entry = self.lock_manifest().entries.get(&cache_id).cloned();
            if let Some(entry) = entry {
                if Self::is_cache_valid(&entry, METADATA_TTL_MS, self.now()) {
                    if let Some(cached) = self.load_from_cache(&cache_id) {
                        if let Some(metadata) = cached.get("metadata") {
                            if js_truthy(Some(metadata)) {
                                return Some(metadata.clone());
                            }
                        }
                    }
                }
            }
        }

        let lookup = self.lookup.as_ref()?;
        let mut metadata: Option<IgdbMetadata> = None;
        if let Some(serial) = serial {
            metadata = lookup.search_by_serial(serial, system_id).await;
        }
        if metadata.is_none() {
            if let Some(title) = game
                .get("title")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                metadata = lookup.search_by_title(title, system_id).await;
            }
        }
        match metadata {
            Some(metadata) => {
                let value = serde_json::to_value(&metadata).unwrap_or(Value::Null);
                self.save_to_cache(&cache_id, &value);
                Some(value)
            }
            None => {
                self.save_to_cache(&cache_id, &json!({"notFound": true}));
                None
            }
        }
    }

    /// Aplica os metadados ao jogo, preservando campos desconhecidos.
    pub async fn enrich_game(&self, game: &Value, force: bool) -> Value {
        let Some(metadata) = self.fetch_metadata(game, force).await else {
            return game.clone();
        };
        if js_truthy(metadata.get("notFound")) {
            return game.clone();
        }
        let Some(meta) = metadata.as_object() else {
            return game.clone();
        };
        let Some(mut output) = game.as_object().cloned() else {
            return game.clone();
        };

        let game_title = output.get("title").and_then(Value::as_str);
        let meta_title = meta.get("title").and_then(Value::as_str);
        if let Some(title) = meta_title {
            if Some(title) != game_title {
                output.insert("titleIgdb".to_string(), json!(title));
            } else {
                output.remove("titleIgdb");
            }
        } else {
            output.remove("titleIgdb");
        }

        if let Some(summary) = meta.get("summary") {
            output.insert("summary".to_string(), summary.clone());
        } else {
            output.remove("summary");
        }

        if js_truthy(meta.get("releaseDate")) {
            output.insert(
                "releaseDate".to_string(),
                meta.get("releaseDate").cloned().unwrap_or(Value::Null),
            );
        } else if !js_truthy(output.get("releaseDate")) {
            output.remove("releaseDate");
        }

        for key in ["developer", "publisher"] {
            if let Some(value) = meta.get(key) {
                output.insert(key.to_string(), value.clone());
            } else {
                output.remove(key);
            }
        }

        let merged_genres = merge_unique(
            output.get("genres").and_then(Value::as_array),
            meta.get("genres").and_then(Value::as_array),
        );
        output.insert("genres".to_string(), Value::Array(merged_genres));

        for key in [
            "themes",
            "gameModes",
            "perspectives",
            "rating",
            "ratingCount",
        ] {
            if let Some(value) = meta.get(key) {
                output.insert(key.to_string(), value.clone());
            } else {
                output.remove(key);
            }
        }

        let mut artwork = output
            .get("artwork")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let old_cover = artwork
            .get("cover")
            .filter(|value| js_truthy(Some(value)))
            .cloned();
        let meta_artwork = meta.get("artwork").and_then(Value::as_object);
        let meta_cover = meta_artwork
            .and_then(|artwork| artwork.get("cover"))
            .filter(|value| js_truthy(Some(value)))
            .cloned();
        match old_cover.or(meta_cover) {
            Some(cover) => {
                artwork.insert("cover".to_string(), cover);
            }
            None => {
                artwork.remove("cover");
            }
        }
        for (source_key, target_key) in [
            ("coverHD", "coverHD"),
            ("artworks", "artworks"),
            ("screenshots", "screenshots"),
        ] {
            match meta_artwork
                .and_then(|artwork| artwork.get(source_key))
                .filter(|value| js_truthy(Some(value)))
            {
                Some(value) => {
                    artwork.insert(target_key.to_string(), value.clone());
                }
                None => {
                    artwork.remove(target_key);
                }
            }
        }
        let provider = if meta_artwork
            .and_then(|artwork| artwork.get("cover"))
            .is_some_and(|cover| js_truthy(Some(cover)))
        {
            Some(Value::String("igdb".to_string()))
        } else {
            artwork
                .get("provider")
                .filter(|value| js_truthy(Some(value)))
                .cloned()
        };
        match provider {
            Some(provider) => {
                artwork.insert("provider".to_string(), provider);
            }
            None => {
                artwork.remove("provider");
            }
        }
        output.insert("artwork".to_string(), Value::Object(artwork));
        output.insert("metadataSource".to_string(), json!("igdb"));
        output.insert("metadataFetchedAt".to_string(), json!(self.now()));
        Value::Object(output)
    }

    /// Enriquece uma lista, com filtro de sistema, limite e progresso.
    pub async fn enrich_catalog(&self, games: &[Value], options: &EnrichOptions) -> Vec<Value> {
        let filtered: Vec<&Value> = match &options.system_filter {
            Some(systems) => games
                .iter()
                .filter(|game| {
                    game.get("systemId")
                        .and_then(Value::as_str)
                        .is_some_and(|system| systems.iter().any(|wanted| wanted == system))
                })
                .collect(),
            None => games.iter().collect(),
        };
        let to_enrich: Vec<&Value> = match options.max_games {
            Some(max) => filtered.into_iter().take(max).collect(),
            None => filtered,
        };

        let mut enriched = Vec::with_capacity(to_enrich.len());
        let mut processed = 0usize;
        for game in &to_enrich {
            enriched.push(self.enrich_game(game, options.force).await);
            processed += 1;
            if let Some(progress) = &options.on_progress {
                if processed.is_multiple_of(10) {
                    progress(EnrichProgress {
                        processed,
                        total: to_enrich.len(),
                        done: false,
                    });
                }
            }
        }
        if let Some(progress) = &options.on_progress {
            progress(EnrichProgress {
                processed,
                total: to_enrich.len(),
                done: true,
            });
        }
        enriched
    }

    /// Remove entradas mais antigas que `max_age_days` e devolve o total.
    pub fn clean_cache(&self, max_age_days: i64) -> usize {
        let cutoff = self.now() - max_age_days * 24 * 60 * 60 * 1000;
        let mut removed = 0usize;
        let mut manifest = self.lock_manifest();
        let expired: Vec<String> = manifest
            .entries
            .iter()
            .filter(|(_, entry)| entry.fetched_at < cutoff)
            .map(|(cache_id, _)| cache_id.clone())
            .collect();
        for cache_id in expired {
            let _ = std::fs::remove_file(self.cache_dir.join(format!("{cache_id}.json")));
            manifest.entries.remove(&cache_id);
            removed += 1;
        }
        drop(manifest);
        if removed > 0 {
            self.save_manifest();
        }
        removed
    }

    pub fn get_cache_stats(&self) -> CacheStats {
        let manifest = self.lock_manifest();
        let now = self.now();
        let total_entries = manifest.entries.len();
        let valid_entries = manifest
            .entries
            .values()
            .filter(|entry| Self::is_cache_valid(entry, METADATA_TTL_MS, now))
            .count();
        let total_size: u64 = manifest.entries.values().map(|entry| entry.size).sum();
        CacheStats {
            total_entries,
            valid_entries,
            expired_entries: total_entries - valid_entries,
            total_size_mb: format!("{:.2}", total_size as f64 / (1024.0 * 1024.0)),
            cache_dir: self.cache_dir.display().to_string(),
        }
    }
}

/// `createMetadataEnricher` (atalho de construção).
pub fn create_metadata_enricher(
    data_dir: impl Into<PathBuf>,
    lookup: Option<Arc<dyn IgdbLookup>>,
) -> MetadataEnricher {
    MetadataEnricher::new(data_dir, lookup)
}

fn merge_unique(left: Option<&Vec<Value>>, right: Option<&Vec<Value>>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for value in left
        .into_iter()
        .flatten()
        .chain(right.into_iter().flatten())
    {
        let key = serde_json::to_string(value).unwrap_or_default();
        if seen.insert(key) {
            merged.push(value.clone());
        }
    }
    merged
}

/// `mergeUnique` público para consumidores do enriquecimento.
pub fn merge_unique_values(left: &[Value], right: &[Value]) -> Vec<Value> {
    merge_unique(Some(&left.to_vec()), Some(&right.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn igdb_raw() -> Value {
        json!({
            "name": "Example Adventure",
            "summary": "Resumo",
            "storyline": "História",
            "first_release_date": 946684800,
            "genres": [{"name": "Adventure"}, {"name": "RPG"}],
            "themes": [{"name": "Fantasy"}],
            "game_modes": [{"name": "Single player"}],
            "player_perspectives": [{"name": "Third person"}],
            "aggregated_rating": 91.5,
            "aggregated_rating_count": 120,
            "rating": 88.0,
            "rating_count": 200,
            "involved_companies": [
                {"developer": true, "company": {"name": "Dev A"}},
                {"publisher": true, "company": {"name": "Pub B"}},
                {"developer": true, "company": {"name": "Dev C"}}
            ],
            "cover": {"image_id": "cover1"},
            "artworks": [{"image_id": "art1"}],
            "screenshots": [{"image_id": "shot1"}]
        })
    }

    #[test]
    fn normaliza_resposta_igdb_com_textos_e_artes() {
        let metadata = normalize_igdb_game(&igdb_raw());
        assert_eq!(metadata.title.as_deref(), Some("Example Adventure"));
        assert_eq!(metadata.summary.as_deref(), Some("Resumo"));
        assert_eq!(metadata.release_date.as_deref(), Some("2000-01-01"));
        assert_eq!(
            metadata.genres,
            vec!["Adventure".to_string(), "RPG".to_string()]
        );
        assert_eq!(metadata.themes, vec!["Fantasy".to_string()]);
        assert_eq!(metadata.game_modes, vec!["Single player".to_string()]);
        assert_eq!(metadata.perspectives, vec!["Third person".to_string()]);
        assert_eq!(metadata.rating, Some(91.5));
        assert_eq!(metadata.rating_count, 120);
        assert_eq!(metadata.developer.as_deref(), Some("Dev C"));
        assert_eq!(metadata.publisher.as_deref(), Some("Pub B"));
        assert_eq!(
            metadata.artwork.cover.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_cover_big/cover1.jpg")
        );
        assert_eq!(
            metadata.artwork.cover_hd.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_1080p/cover1.jpg")
        );
        assert_eq!(
            metadata.artwork.screenshots.as_ref().unwrap()[0],
            "https://images.igdb.com/igdb/image/upload/t_screenshot_big/shot1.jpg"
        );
    }

    #[test]
    fn normalizacao_usa_fallbacks_e_arrays_vazios() {
        let metadata = normalize_igdb_game(&json!({
            "name": "Only",
            "storyline": "Sem summary",
            "rating": 0,
            "rating_count": 0,
            "aggregated_rating": 0
        }));
        assert_eq!(metadata.summary.as_deref(), Some("Sem summary"));
        assert_eq!(metadata.release_date, None);
        assert_eq!(metadata.rating, None);
        assert_eq!(metadata.rating_count, 0);
        assert!(metadata.genres.is_empty());
        assert!(metadata.artwork.cover.is_none());
        assert!(metadata.artwork.artworks.is_none());
    }

    #[test]
    fn queries_escapam_serial_e_titulo() {
        let serial = build_serial_query("SLUS-20312", "sony-playstation-2").unwrap();
        assert!(serial.contains("platforms = 8"));
        assert!(serial.contains("version_title ~ \"SLUS20312\"*"));
        assert_eq!(build_serial_query("X", "unknown"), None);
        let title = build_title_query("He said \"hi\"", "sony-playstation").unwrap();
        assert!(title.contains("where platforms = 7"));
        assert!(title.contains("search \"He said \\\"hi\\\"\";"));
    }

    #[test]
    fn credenciais_tem_hash_estavel_e_debug_redigido() {
        let credentials = IgdbCredentials::new("client", "topsecret");
        assert_eq!(get_credentials_hash(&credentials).len(), 16);
        let debug = format!("{credentials:?}");
        assert!(debug.contains("***"));
        assert!(!debug.contains("topsecret"));
    }

    struct FakeLookup {
        metadata: IgdbMetadata,
        serial_calls: AtomicUsize,
        title_calls: AtomicUsize,
        fail: bool,
    }

    impl FakeLookup {
        fn new(metadata: IgdbMetadata) -> Self {
            Self {
                metadata,
                serial_calls: AtomicUsize::new(0),
                title_calls: AtomicUsize::new(0),
                fail: false,
            }
        }
    }

    #[async_trait]
    impl IgdbLookup for FakeLookup {
        async fn search_by_serial(&self, _serial: &str, _system_id: &str) -> Option<IgdbMetadata> {
            self.serial_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                None
            } else {
                Some(self.metadata.clone())
            }
        }

        async fn search_by_title(&self, _title: &str, _system_id: &str) -> Option<IgdbMetadata> {
            self.title_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                None
            } else {
                Some(self.metadata.clone())
            }
        }
    }

    fn game() -> Value {
        json!({
            "id": "retro:ps2:SLUS20312",
            "systemId": "sony-playstation-2",
            "title": "Example Adventure",
            "sortTitle": "example adventure",
            "serials": ["SLUS-20312"],
            "genres": ["Adventure", "Action"],
            "releaseDate": "1999-01-01",
            "artwork": {"provider": "source", "titleScreen": "https://x/title.jpg"},
            "customField": {"kept": true}
        })
    }

    #[tokio::test]
    async fn enrich_preserva_campos_e_aplica_merge() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())));
        let enricher =
            MetadataEnricher::with_now(dir.path(), Some(lookup.clone()), Arc::new(|| 1000));
        let enriched = enricher.enrich_game(&game(), false).await;
        assert_eq!(enriched["summary"], json!("Resumo"));
        // O título da IGDB é igual ao do jogo: `titleIgdb` não é criado.
        assert!(enriched.get("titleIgdb").is_none());
        assert_eq!(enriched["genres"], json!(["Adventure", "Action", "RPG"]));
        assert_eq!(enriched["developer"], json!("Dev C"));
        assert_eq!(enriched["publisher"], json!("Pub B"));
        assert_eq!(enriched["themes"], json!(["Fantasy"]));
        assert_eq!(enriched["gameModes"], json!(["Single player"]));
        assert_eq!(enriched["perspectives"], json!(["Third person"]));
        assert_eq!(enriched["metadataSource"], json!("igdb"));
        assert_eq!(enriched["metadataFetchedAt"], json!(1000));
        assert_eq!(
            enriched["artwork"]["cover"],
            json!("https://images.igdb.com/igdb/image/upload/t_cover_big/cover1.jpg")
        );
        assert_eq!(enriched["artwork"]["provider"], json!("igdb"));
        assert_eq!(
            enriched["artwork"]["titleScreen"],
            json!("https://x/title.jpg")
        );
        assert_eq!(enriched["customField"]["kept"], json!(true));
        assert_eq!(enriched["releaseDate"], json!("2000-01-01"));
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_evita_segunda_busca_e_force_refaz() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())));
        let enricher =
            MetadataEnricher::with_now(dir.path(), Some(lookup.clone()), Arc::new(|| 1000));
        let first = enricher.fetch_metadata(&game(), false).await;
        assert!(first.is_some());
        let second = enricher.fetch_metadata(&game(), false).await;
        assert!(second.is_some());
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 1);
        let forced = enricher.fetch_metadata(&game(), true).await;
        assert!(forced.is_some());
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 2);
        let stats = enricher.get_cache_stats();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.valid_entries, 1);
    }

    #[tokio::test]
    async fn cache_negativo_nao_repete_e_enrich_nao_altera() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup {
            metadata: IgdbMetadata::default(),
            serial_calls: AtomicUsize::new(0),
            title_calls: AtomicUsize::new(0),
            fail: true,
        });
        let enricher =
            MetadataEnricher::with_now(dir.path(), Some(lookup.clone()), Arc::new(|| 1000));
        let game = game();
        let enriched = enricher.enrich_game(&game, false).await;
        assert_eq!(enriched, game);
        // serial falhou e o título também foi tentado
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 1);
        assert_eq!(lookup.title_calls.load(Ordering::SeqCst), 1);
        let cached = enricher.fetch_metadata(&game, false).await;
        assert_eq!(cached, Some(json!({"notFound": true})));
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sistemas_nao_suportados_nao_buscam() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())));
        let enricher =
            MetadataEnricher::with_now(dir.path(), Some(lookup.clone()), Arc::new(|| 1000));
        let mut game = game();
        game["systemId"] = json!("nintendo-nes");
        assert_eq!(enricher.fetch_metadata(&game, false).await, None);
        assert_eq!(lookup.serial_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn limpa_cache_antigo_e_recalcula_stats() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())));
        let enricher = MetadataEnricher::with_now(dir.path(), Some(lookup), Arc::new(|| 1000));
        enricher.fetch_metadata(&game(), false).await.unwrap();
        assert_eq!(enricher.clean_cache(60), 0);
        // Avança o relógio em 61 dias.
        let enricher = MetadataEnricher::with_now(
            dir.path(),
            Some(Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())))),
            Arc::new(|| 1000 + 61 * 24 * 60 * 60 * 1000),
        );
        assert_eq!(enricher.clean_cache(60), 1);
        assert_eq!(enricher.get_cache_stats().total_entries, 0);
    }

    #[tokio::test]
    async fn enrich_catalog_filtra_limita_e_reporta_progresso() {
        let dir = temp_dir();
        let lookup = Arc::new(FakeLookup::new(normalize_igdb_game(&igdb_raw())));
        let enricher = MetadataEnricher::with_now(dir.path(), Some(lookup), Arc::new(|| 1000));
        let games: Vec<Value> = (0..12).map(|_| game()).collect();
        let progress = Arc::new(Mutex::new(Vec::new()));
        let reported = progress.clone();
        let options = EnrichOptions {
            max_games: Some(10),
            system_filter: Some(vec!["sony-playstation-2".to_string()]),
            on_progress: Some(Arc::new(move |item: EnrichProgress| {
                reported.lock().unwrap().push(item.processed);
            })),
            force: false,
        };
        let enriched = enricher.enrich_catalog(&games, &options).await;
        assert_eq!(enriched.len(), 10);
        let values = progress.lock().unwrap().clone();
        assert_eq!(values.first(), Some(&10));
        assert_eq!(values.last(), Some(&10));
    }

    #[test]
    fn merge_unique_values_deduplica() {
        let merged = merge_unique_values(&[json!("A"), json!("B")], &[json!("B"), json!("C")]);
        assert_eq!(merged, vec![json!("A"), json!("B"), json!("C")]);
    }

    struct MockFetcher {
        responses: Mutex<std::collections::HashMap<String, arcadia_catalog::http::Response>>,
        requests: Mutex<Vec<Request>>,
    }

    impl MockFetcher {
        fn new(responses: Vec<(&str, arcadia_catalog::http::Response)>) -> Self {
            Self {
                responses: Mutex::new(
                    responses
                        .into_iter()
                        .map(|(url, response)| (url.to_string(), response))
                        .collect(),
                ),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Fetcher for MockFetcher {
        async fn fetch(
            &self,
            request: Request,
        ) -> Result<arcadia_catalog::http::Response, arcadia_catalog::http::FetchError> {
            let url = request.url.clone();
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .get(&url)
                .cloned()
                .ok_or_else(|| arcadia_catalog::http::FetchError::new("offline"))
        }
    }

    #[tokio::test]
    async fn cliente_igdb_reusa_token_e_monta_query_apicalypse() {
        let fetcher = Arc::new(MockFetcher::new(vec![
            (
                TOKEN_URL,
                arcadia_catalog::http::Response::json(
                    200,
                    &json!({"access_token": "tok", "expires_in": 3600}),
                ),
            ),
            (
                IGDB_URL,
                arcadia_catalog::http::Response::json(200, &json!([igdb_raw()])),
            ),
        ]));
        let client = IgdbClient::new(fetcher.clone(), IgdbCredentials::new("client", "secret"))
            .with_clock(Arc::new(|| 1000))
            .with_delay_ms(0);

        let by_serial = client
            .search_by_serial("SLUS-20312", "sony-playstation-2")
            .await
            .expect("metadados por serial");
        assert_eq!(by_serial.title.as_deref(), Some("Example Adventure"));
        let by_title = client
            .search_by_title("Example \"Adventure\"", "sony-playstation-2")
            .await
            .expect("metadados por título");
        assert_eq!(by_title.summary.as_deref(), Some("Resumo"));

        let requests = fetcher.requests.lock().unwrap();
        assert_eq!(requests.len(), 3, "token deve ser reutilizado");
        assert_eq!(requests[0].url, TOKEN_URL);
        assert_eq!(requests[1].url, IGDB_URL);
        assert_eq!(requests[2].url, IGDB_URL);
        let serial_body = String::from_utf8_lossy(requests[1].body.as_deref().unwrap()).to_string();
        assert!(serial_body.contains("version_title ~ \"SLUS20312\"*"));
        assert!(serial_body.contains("platforms = 8"));
        assert!(requests[1]
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer tok"));
        let title_body = String::from_utf8_lossy(requests[2].body.as_deref().unwrap()).to_string();
        assert!(title_body.contains("search \"Example \\\"Adventure\\\"\";"));
    }
}
