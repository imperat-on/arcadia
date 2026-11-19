//! Repositório local retrô — port 1:1 de `app/electron/retro-repository.js`,
//! mais as operações de biblioteca de `retro:libraryAdd`/`retro:libraryRemove`
//! do `main.js`.
//!
//! Os arquivos ficam em `{dataDir}/retro/` com os mesmos nomes/versões do
//! Electron. Campos desconhecidos sobrevivem a leitura/gravação (os jogos,
//! ofertas e matches são mantidos como `serde_json::Value`), a escrita é
//! atômica e nenhum teste toca a rede.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex, MutexGuard};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::util::{js_truthy, now_ms, safe_read_json, safe_read_typed, safe_write_json};

/// Versão dos arquivos de catálogo/índice/ofertas/matches.
pub const CACHE_VERSION: u64 = 2;
/// Limite de leitura de um arquivo do repositório.
pub const MAX_CACHE_SIZE_MB: u64 = 500;
/// Limite de ofertas aceitas por `setOffers`.
pub const MAX_OFFERS: usize = 100_000;

const MAX_CACHE_BYTES: u64 = MAX_CACHE_SIZE_MB * 1024 * 1024;

/// Erros do repositório.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    #[error("Offers exceed maximum ({0})")]
    OffersExceedMaximum(usize),
}

// ── formatos persistidos ───────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepositoryManifest {
    pub version: u64,
    pub generated_at: Option<i64>,
    pub catalog_version: Option<Value>,
    pub systems: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for RepositoryManifest {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            generated_at: None,
            catalog_version: None,
            systems: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogIndex {
    pub version: u64,
    pub updated_at: Option<i64>,
    pub games: Vec<Value>,
    pub systems_map: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for CatalogIndex {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            updated_at: None,
            games: Vec::new(),
            systems_map: Map::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OffersFile {
    pub version: u64,
    pub updated_at: Option<i64>,
    pub offers: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for OffersFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            updated_at: None,
            offers: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MatchesFile {
    pub version: u64,
    pub updated_at: Option<i64>,
    pub matches: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for MatchesFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            updated_at: None,
            matches: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtworkFile {
    pub version: u64,
    pub cache: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ArtworkFile {
    fn default() -> Self {
        Self {
            version: 1,
            cache: Map::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UnmatchedFile {
    pub version: u64,
    pub offers: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for UnmatchedFile {
    fn default() -> Self {
        Self {
            version: 1,
            offers: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverridesFile {
    pub version: u64,
    pub overrides: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for OverridesFile {
    fn default() -> Self {
        Self {
            version: 1,
            overrides: Map::new(),
            extra: Map::new(),
        }
    }
}

/// Estatísticas de `getStats`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStats {
    pub games: usize,
    pub offers: usize,
    pub matched: usize,
    pub unmatched: usize,
    pub catalog_version: Option<Value>,
    pub last_updated: Option<i64>,
}

#[derive(Default)]
struct RepositoryState {
    manifest: Option<RepositoryManifest>,
    catalog_index: Option<CatalogIndex>,
    offers: Option<OffersFile>,
    matches: Option<MatchesFile>,
    artwork: Option<ArtworkFile>,
    unmatched: Option<UnmatchedFile>,
    overrides: Option<OverridesFile>,
}

/// Repositório em memória com persistência em `{dataDir}/retro/`.
pub struct RetroRepository {
    retro_dir: PathBuf,
    manifest_path: PathBuf,
    index_path: PathBuf,
    offers_path: PathBuf,
    matches_path: PathBuf,
    artwork_path: PathBuf,
    unmatched_path: PathBuf,
    overrides_path: PathBuf,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    state: Mutex<RepositoryState>,
}

impl RetroRepository {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self::with_now(data_dir, Arc::new(now_ms))
    }

    pub fn with_now(data_dir: impl Into<PathBuf>, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        let retro_dir = data_dir.into().join("retro");
        Self {
            manifest_path: retro_dir.join("catalog-manifest.json"),
            index_path: retro_dir.join("catalog-index-v2.json"),
            offers_path: retro_dir.join("offers-v2.json"),
            matches_path: retro_dir.join("matches-v2.json"),
            artwork_path: retro_dir.join("artwork-v1.json"),
            unmatched_path: retro_dir.join("unmatched-v1.json"),
            overrides_path: retro_dir.join("overrides-v1.json"),
            retro_dir,
            now,
            state: Mutex::new(RepositoryState::default()),
        }
    }

    pub fn retro_dir(&self) -> &Path {
        &self.retro_dir
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    pub fn offers_path(&self) -> &Path {
        &self.offers_path
    }

    pub fn matches_path(&self) -> &Path {
        &self.matches_path
    }

    pub fn artwork_path(&self) -> &Path {
        &self.artwork_path
    }

    pub fn unmatched_path(&self) -> &Path {
        &self.unmatched_path
    }

    pub fn overrides_path(&self) -> &Path {
        &self.overrides_path
    }

    pub fn now(&self) -> i64 {
        (self.now)()
    }

    fn lock(&self) -> MutexGuard<'_, RepositoryState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // ── loaders (lazy, iguais ao JS) ──────────────────────────────────────

    fn load_manifest_state<'a>(
        &self,
        state: &'a mut RepositoryState,
    ) -> &'a mut RepositoryManifest {
        if state.manifest.is_none() {
            let loaded =
                safe_read_typed::<RepositoryManifest>(&self.manifest_path, MAX_CACHE_BYTES)
                    .filter(|manifest| manifest.version == CACHE_VERSION)
                    .unwrap_or_default();
            state.manifest = Some(loaded);
        }
        state.manifest.as_mut().expect("manifest carregado")
    }

    fn load_index_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut CatalogIndex {
        if state.catalog_index.is_none() {
            let loaded = safe_read_typed::<CatalogIndex>(&self.index_path, MAX_CACHE_BYTES)
                .filter(|index| index.version == CACHE_VERSION)
                .unwrap_or_default();
            state.catalog_index = Some(loaded);
        }
        state.catalog_index.as_mut().expect("índice carregado")
    }

    fn load_offers_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut OffersFile {
        if state.offers.is_none() {
            let loaded = safe_read_typed::<OffersFile>(&self.offers_path, MAX_CACHE_BYTES)
                .filter(|offers| offers.version == CACHE_VERSION)
                .unwrap_or_default();
            state.offers = Some(loaded);
        }
        state.offers.as_mut().expect("ofertas carregadas")
    }

    fn load_matches_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut MatchesFile {
        if state.matches.is_none() {
            let loaded = safe_read_typed::<MatchesFile>(&self.matches_path, MAX_CACHE_BYTES)
                .filter(|matches| matches.version == CACHE_VERSION)
                .unwrap_or_default();
            state.matches = Some(loaded);
        }
        state.matches.as_mut().expect("matches carregados")
    }

    fn load_artwork_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut ArtworkFile {
        if state.artwork.is_none() {
            let loaded = safe_read_typed::<ArtworkFile>(&self.artwork_path, MAX_CACHE_BYTES)
                .filter(|artwork| artwork.version == 1)
                .unwrap_or_default();
            state.artwork = Some(loaded);
        }
        state.artwork.as_mut().expect("artwork carregado")
    }

    fn load_unmatched_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut UnmatchedFile {
        if state.unmatched.is_none() {
            let loaded = safe_read_typed::<UnmatchedFile>(&self.unmatched_path, MAX_CACHE_BYTES)
                .filter(|unmatched| unmatched.version == 1)
                .unwrap_or_default();
            state.unmatched = Some(loaded);
        }
        state.unmatched.as_mut().expect("unmatched carregado")
    }

    fn load_overrides_state<'a>(&self, state: &'a mut RepositoryState) -> &'a mut OverridesFile {
        if state.overrides.is_none() {
            let loaded = safe_read_typed::<OverridesFile>(&self.overrides_path, MAX_CACHE_BYTES)
                .filter(|overrides| overrides.version == 1)
                .unwrap_or_default();
            state.overrides = Some(loaded);
        }
        state.overrides.as_mut().expect("overrides carregados")
    }

    // ── savers ────────────────────────────────────────────────────────────

    fn save_manifest_state(&self, state: &RepositoryState) {
        if let Some(manifest) = &state.manifest {
            if let Ok(value) = serde_json::to_value(manifest) {
                let _ = safe_write_json(&self.manifest_path, &value, true);
            }
        }
    }

    fn save_index_state(&self, state: &RepositoryState) {
        if let Some(index) = &state.catalog_index {
            if let Ok(value) = serde_json::to_value(index) {
                let _ = safe_write_json(&self.index_path, &value, true);
            }
        }
    }

    fn save_offers_state(&self, state: &RepositoryState) {
        if let Some(offers) = &state.offers {
            if let Ok(value) = serde_json::to_value(offers) {
                let _ = safe_write_json(&self.offers_path, &value, true);
            }
        }
    }

    fn save_matches_state(&self, state: &RepositoryState) {
        if let Some(matches) = &state.matches {
            if let Ok(value) = serde_json::to_value(matches) {
                let _ = safe_write_json(&self.matches_path, &value, true);
            }
        }
    }

    fn save_artwork_state(&self, state: &RepositoryState) {
        if let Some(artwork) = &state.artwork {
            if let Ok(value) = serde_json::to_value(artwork) {
                let _ = safe_write_json(&self.artwork_path, &value, true);
            }
        }
    }

    fn save_unmatched_state(&self, state: &RepositoryState) {
        if let Some(unmatched) = &state.unmatched {
            if let Ok(value) = serde_json::to_value(unmatched) {
                let _ = safe_write_json(&self.unmatched_path, &value, true);
            }
        }
    }

    fn save_overrides_state(&self, state: &RepositoryState) {
        if let Some(overrides) = &state.overrides {
            if let Ok(value) = serde_json::to_value(overrides) {
                let _ = safe_write_json(&self.overrides_path, &value, true);
            }
        }
    }

    // ── manifest ──────────────────────────────────────────────────────────

    pub fn load_manifest(&self) -> RepositoryManifest {
        let mut state = self.lock();
        self.load_manifest_state(&mut state).clone()
    }

    /// Substitui o manifest em memória e grava.
    pub fn set_manifest(&self, manifest: RepositoryManifest) {
        let mut state = self.lock();
        state.manifest = Some(manifest);
        self.save_manifest_state(&state);
    }

    pub fn save_manifest(&self) {
        let state = self.lock();
        self.save_manifest_state(&state);
    }

    // ── jogos ─────────────────────────────────────────────────────────────

    pub fn get_games(&self) -> Vec<Value> {
        let mut state = self.lock();
        self.load_index_state(&mut state).games.clone()
    }

    pub fn get_game(&self, game_id: &str) -> Option<Value> {
        let mut state = self.lock();
        self.load_index_state(&mut state)
            .games
            .iter()
            .find(|game| game.get("id").and_then(Value::as_str) == Some(game_id))
            .cloned()
    }

    pub fn set_catalog_index(&self, games: Vec<Value>, systems_map: Map<String, Value>) {
        let mut state = self.lock();
        let index = self.load_index_state(&mut state);
        index.games = games;
        index.systems_map = systems_map;
        index.updated_at = Some(self.now());
        self.save_index_state(&state);
    }

    /// Variante de `setCatalogIndex(games)` com `systemsMap` vazio.
    pub fn set_games(&self, games: Vec<Value>) {
        self.set_catalog_index(games, Map::new());
    }

    pub fn get_systems_map(&self) -> Map<String, Value> {
        let mut state = self.lock();
        self.load_index_state(&mut state).systems_map.clone()
    }

    // ── ofertas ───────────────────────────────────────────────────────────

    pub fn get_offers(&self) -> Vec<Value> {
        let mut state = self.lock();
        self.load_offers_state(&mut state).offers.clone()
    }

    pub fn get_offers_for_game(&self, game_id: &str) -> Vec<Value> {
        let mut state = self.lock();
        let offer_ids: Vec<String> = self
            .load_matches_state(&mut state)
            .matches
            .iter()
            .filter(|entry| entry.get("gameId").and_then(Value::as_str) == Some(game_id))
            .filter_map(|entry| entry.get("offerId").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();
        self.load_offers_state(&mut state)
            .offers
            .iter()
            .filter(|offer| {
                offer
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| offer_ids.iter().any(|wanted| wanted == id))
            })
            .cloned()
            .collect()
    }

    pub fn get_offer(&self, offer_id: &str) -> Option<Value> {
        let mut state = self.lock();
        self.load_offers_state(&mut state)
            .offers
            .iter()
            .find(|offer| offer.get("id").and_then(Value::as_str) == Some(offer_id))
            .cloned()
    }

    pub fn set_offers(&self, new_offers: Vec<Value>) -> Result<(), RepositoryError> {
        if new_offers.len() > MAX_OFFERS {
            return Err(RepositoryError::OffersExceedMaximum(MAX_OFFERS));
        }
        let mut state = self.lock();
        let offers = self.load_offers_state(&mut state);
        offers.offers = new_offers;
        offers.updated_at = Some(self.now());
        self.save_offers_state(&state);
        Ok(())
    }

    // ── matches ───────────────────────────────────────────────────────────

    pub fn get_matches(&self) -> Vec<Value> {
        let mut state = self.lock();
        self.load_matches_state(&mut state).matches.clone()
    }

    pub fn set_matches(&self, new_matches: Vec<Value>) {
        let mut state = self.lock();
        let matches = self.load_matches_state(&mut state);
        matches.matches = new_matches;
        matches.updated_at = Some(self.now());
        self.save_matches_state(&state);
    }

    // ── artwork ───────────────────────────────────────────────────────────

    pub fn get_artwork(&self, game_id: &str) -> Option<Value> {
        let mut state = self.lock();
        self.load_artwork_state(&mut state)
            .cache
            .get(game_id)
            .cloned()
    }

    pub fn set_artwork(&self, game_id: &str, artwork: &Value) {
        let mut state = self.lock();
        let cache = &mut self.load_artwork_state(&mut state).cache;
        let mut entry = artwork.as_object().cloned().unwrap_or_default();
        entry.insert("updatedAt".to_string(), json!(self.now()));
        cache.insert(game_id.to_string(), Value::Object(entry));
        self.save_artwork_state(&state);
    }

    // ── overrides ─────────────────────────────────────────────────────────

    pub fn get_override(&self, fingerprint: &str) -> Option<Value> {
        let mut state = self.lock();
        self.load_overrides_state(&mut state)
            .overrides
            .get(fingerprint)
            .cloned()
    }

    pub fn set_override(&self, fingerprint: &str, override_data: &Value) {
        let mut state = self.lock();
        let overrides = &mut self.load_overrides_state(&mut state).overrides;
        let mut entry = override_data.as_object().cloned().unwrap_or_default();
        entry.insert("updatedAt".to_string(), json!(self.now()));
        overrides.insert(fingerprint.to_string(), Value::Object(entry));
        self.save_overrides_state(&state);
    }

    pub fn remove_override(&self, fingerprint: &str) {
        let mut state = self.lock();
        self.load_overrides_state(&mut state)
            .overrides
            .remove(fingerprint);
        self.save_overrides_state(&state);
    }

    pub fn overrides_map(&self) -> Map<String, Value> {
        let mut state = self.lock();
        self.load_overrides_state(&mut state).overrides.clone()
    }

    // ── unmatched ─────────────────────────────────────────────────────────

    pub fn get_unmatched(&self) -> Vec<Value> {
        let mut state = self.lock();
        self.load_unmatched_state(&mut state).offers.clone()
    }

    pub fn set_unmatched(&self, offers: Vec<Value>) {
        let mut state = self.lock();
        self.load_unmatched_state(&mut state).offers = offers;
        self.save_unmatched_state(&state);
    }

    // ── estatísticas ──────────────────────────────────────────────────────

    pub fn get_stats(&self) -> RepositoryStats {
        let mut state = self.lock();
        let games = self.load_index_state(&mut state).games.len();
        let offers_file = self.load_offers_state(&mut state);
        let offers = offers_file.offers.len();
        let last_updated = offers_file.updated_at.filter(|value| *value != 0);
        let matched = self.load_matches_state(&mut state).matches.len();
        let unmatched = self.load_unmatched_state(&mut state).offers.len();
        // O JS lê `manifest?.catalogVersion`; o manifest só participa se já foi
        // carregado explicitamente, como no Electron.
        let catalog_version = state
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.catalog_version.clone())
            .filter(|value| js_truthy(Some(value)));
        RepositoryStats {
            games,
            offers,
            matched,
            unmatched,
            catalog_version,
            last_updated,
        }
    }

    /// Auditoria local de cobertura (arte/textos) e ofertas sem match.
    pub fn audit(&self, system: Option<&str>, samples_limit: usize) -> Value {
        let all_games = self.get_games();
        let games: Vec<&Value> = all_games
            .iter()
            .filter(|game| match system {
                Some(system) => game.get("systemId").and_then(Value::as_str) == Some(system),
                None => true,
            })
            .collect();

        let mut covers = 0usize;
        let mut descriptions = 0usize;
        let mut screenshots = 0usize;
        let mut heroes = 0usize;
        let mut logos = 0usize;
        let mut missing_samples: Vec<Value> = Vec::new();
        let mut by_system: std::collections::BTreeMap<String, [usize; 6]> =
            std::collections::BTreeMap::new();

        for game in &games {
            let system_id = game
                .get("systemId")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let counts = by_system.entry(system_id.clone()).or_insert([0; 6]);
            counts[0] += 1;

            let has_cover =
                artwork_value(game, &["cover"]).is_some() || field_truthy(game, "cover");
            let has_description =
                field_truthy(game, "summary") || field_truthy(game, "description");
            let has_screenshots = array_len(artwork_value(game, &["screenshots"])) > 0
                || array_len(game.get("screenshots")) > 0;
            let has_hero = artwork_value(game, &["hero", "titleScreen", "background"]).is_some()
                || field_truthy(game, "hero")
                || field_truthy(game, "background");
            let has_logo = artwork_value(game, &["logo"]).is_some() || field_truthy(game, "logo");

            if has_cover {
                covers += 1;
                counts[1] += 1;
            }
            if has_description {
                descriptions += 1;
                counts[2] += 1;
            }
            if has_screenshots {
                screenshots += 1;
                counts[3] += 1;
            }
            if has_hero {
                heroes += 1;
                counts[4] += 1;
            }
            if has_logo {
                logos += 1;
                counts[5] += 1;
            }

            if missing_samples.len() < samples_limit {
                let missing = if !has_cover {
                    Some("cover")
                } else if !has_description {
                    Some("description")
                } else if !has_screenshots {
                    Some("screenshots")
                } else if !has_hero {
                    Some("hero")
                } else if !has_logo {
                    Some("logo")
                } else {
                    None
                };
                if let Some(missing) = missing {
                    missing_samples.push(json!({
                        "id": game.get("id").cloned().unwrap_or(Value::Null),
                        "title": game.get("title").cloned().unwrap_or(Value::Null),
                        "systemId": game.get("systemId").cloned().unwrap_or(Value::Null),
                        "offer_count": game.get("offerCount").cloned().unwrap_or(json!(0)),
                        "missingField": missing,
                    }));
                }
            }
        }

        let systems: Vec<Value> = by_system
            .into_iter()
            .map(|(system_id, counts)| {
                json!({
                    "systemId": system_id,
                    "games": counts[0],
                    "covers": counts[1],
                    "descriptions": counts[2],
                    "screenshots": counts[3],
                    "heroes": counts[4],
                    "logos": counts[5],
                })
            })
            .collect();

        let unmatched_all = self.get_unmatched();
        let unmatched_filtered: Vec<&Value> = unmatched_all
            .iter()
            .filter(|entry| match system {
                Some(system) => entry.get("systemId").and_then(Value::as_str) == Some(system),
                None => true,
            })
            .collect();
        let mut by_reason: Map<String, Value> = Map::new();
        let mut unmatched_by_system: Map<String, Value> = Map::new();
        for entry in &unmatched_filtered {
            let reason = entry
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            bump(&mut by_reason, reason);
            let system_id = entry
                .get("systemId")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            bump(&mut unmatched_by_system, system_id);
        }
        let unmatched_samples: Vec<Value> = unmatched_filtered
            .iter()
            .take(samples_limit)
            .map(|entry| {
                json!({
                    "title": entry.get("title").cloned().unwrap_or(Value::Null),
                    "platform": entry
                        .get("platform")
                        .or_else(|| entry.get("platformRaw"))
                        .cloned()
                        .unwrap_or(Value::Null),
                    "systemId": entry.get("systemId").cloned().unwrap_or(Value::Null),
                    "reason": entry.get("reason").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();

        json!({
            "ok": true,
            "version": "v2",
            "totals": {
                "games": games.len(),
                "covers": covers,
                "descriptions": descriptions,
                "screenshots": screenshots,
                "heroes": heroes,
                "logos": logos,
            },
            "systems": systems,
            "missing": {
                "cover": games.len().saturating_sub(covers),
                "description": games.len().saturating_sub(descriptions),
                "screenshots": games.len().saturating_sub(screenshots),
                "hero": games.len().saturating_sub(heroes),
                "logo": games.len().saturating_sub(logos),
            },
            "unmatched": {
                "total": unmatched_filtered.len(),
                "byReason": by_reason,
                "bySystem": unmatched_by_system,
                "samples": unmatched_samples,
            },
            "samples": missing_samples,
        })
    }
}

/// `createRetroRepository` (atalho de construção).
pub fn create_retro_repository(data_dir: impl Into<PathBuf>) -> RetroRepository {
    RetroRepository::new(data_dir)
}

fn field_truthy(value: &Value, key: &str) -> bool {
    js_truthy(value.get(key))
}

fn artwork_value<'a>(game: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let artwork = game.get("artwork")?.as_object()?;
    keys.iter()
        .filter_map(|key| artwork.get(*key))
        .find(|value| js_truthy(Some(value)))
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map(Vec::len).unwrap_or(0)
}

fn bump(map: &mut Map<String, Value>, key: &str) {
    let entry = map.get(key).and_then(Value::as_u64).unwrap_or(0);
    map.insert(key.to_string(), json!(entry + 1));
}

// ── biblioteca retrô (retro:libraryAdd / retro:libraryRemove) ──────────────

static RETRO_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^retro:[a-z0-9][a-z0-9._:-]{1,500}$").expect("regex id retrô")
});

/// Envelope de `retro:libraryAdd`/`retro:libraryRemove`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub games: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl LibraryResult {
    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            added: None,
            games: None,
            error: Some(message.into()),
        }
    }
}

/// Persistência de `custom_games.json` para entradas retrô.
///
/// Reproduz o handler `retro:libraryAdd` do `main.js`: valida o namespace
/// `retro:`, não duplica IDs, grava somente os campos conhecidos e mantém
/// qualquer campo extra de entradas antigas.
#[derive(Clone, Debug)]
pub struct RetroLibrary {
    path: PathBuf,
}

impl RetroLibrary {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self::new(data_dir.into().join("custom_games.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Vec<Value> {
        safe_read_json(&self.path, MAX_CACHE_BYTES)
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
    }

    pub fn add(&self, payload: &Value) -> LibraryResult {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let title = payload
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if !RETRO_ID.is_match(id) || title.is_empty() || title.encode_utf16().count() > 1024 {
            return LibraryResult::error("jogo retrô inválido");
        }

        let mut all = self.list();
        if all
            .iter()
            .any(|game| game.get("id").and_then(Value::as_str) == Some(id))
        {
            return LibraryResult {
                ok: true,
                added: Some(false),
                games: Some(all),
                error: None,
            };
        }

        let mut entry = Map::new();
        entry.insert("id".to_string(), json!(id));
        entry.insert("title".to_string(), json!(title));
        entry.insert("launcher".to_string(), json!("retro"));
        entry.insert(
            "platform".to_string(),
            match payload.get("platform").and_then(Value::as_str) {
                Some(platform) => json!(slice_chars(platform, 80)),
                None => json!("retro"),
            },
        );
        if let Some(system_id) = payload.get("systemId").and_then(Value::as_str) {
            entry.insert("systemId".to_string(), json!(slice_chars(system_id, 120)));
        }
        if let Some(cover) = payload.get("cover").and_then(Value::as_str) {
            entry.insert("cover".to_string(), json!(slice_chars(cover, 2000)));
        }
        if let Some(hero) = payload.get("hero").and_then(Value::as_str) {
            entry.insert("hero".to_string(), json!(slice_chars(hero, 2000)));
        }
        if let Some(description) = payload.get("description").and_then(Value::as_str) {
            entry.insert(
                "description".to_string(),
                json!(slice_chars(description, 10_000)),
            );
        }
        if let Some(genres) = payload.get("genres").and_then(Value::as_array) {
            let cleaned: Vec<Value> = genres
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(32)
                .map(|value| json!(value))
                .collect();
            entry.insert("genres".to_string(), Value::Array(cleaned));
        }
        if let Some(release_year) = payload
            .get("releaseYear")
            .and_then(Value::as_f64)
            .filter(|value| value.fract() == 0.0)
        {
            entry.insert("releaseYear".to_string(), json!(release_year as i64));
        }
        entry.insert("installed".to_string(), json!(false));
        entry.insert("retro".to_string(), json!(true));
        all.push(Value::Object(entry));

        if safe_write_json(&self.path, &Value::Array(all.clone()), true).is_err() {
            return LibraryResult::error("falha ao gravar biblioteca retrô");
        }
        LibraryResult {
            ok: true,
            added: Some(true),
            games: Some(self.list()),
            error: None,
        }
    }

    pub fn remove(&self, id: &str) -> LibraryResult {
        let value = id.trim();
        if !RETRO_ID.is_match(value) {
            return LibraryResult::error("jogo retrô inválido");
        }
        let all = self.list();
        let rest: Vec<Value> = all
            .iter()
            .filter(|game| game.get("id").and_then(Value::as_str) != Some(value))
            .cloned()
            .collect();
        if rest.len() == all.len() {
            return LibraryResult {
                ok: true,
                added: None,
                games: Some(all),
                error: None,
            };
        }
        if safe_write_json(&self.path, &Value::Array(rest), true).is_err() {
            return LibraryResult::error("falha ao gravar biblioteca retrô");
        }
        LibraryResult {
            ok: true,
            added: None,
            games: Some(self.list()),
            error: None,
        }
    }
}

fn slice_chars(value: &str, max: usize) -> String {
    crate::title::js_slice_utf16(value, max).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn game(id: &str, system: &str, title: &str) -> Value {
        json!({"id": id, "systemId": system, "title": title, "sortTitle": title.to_lowercase()})
    }

    #[test]
    fn estado_inicial_e_vazio() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        assert!(repository.get_games().is_empty());
        assert!(repository.get_offers().is_empty());
        assert_eq!(repository.get_stats().games, 0);
    }

    #[test]
    fn armazena_e_recupera_jogos_ofertas_e_matches() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        repository.set_catalog_index(
            vec![game("g1", "sony-playstation", "Final Fantasy VII")],
            Map::new(),
        );
        repository
            .set_offers(vec![
                json!({"id": "o1", "normalizedTitle": "Game 1", "uris": ["magnet:1"]}),
                json!({"id": "o2", "normalizedTitle": "Game 1", "uris": ["magnet:2"]}),
                json!({"id": "o3", "normalizedTitle": "Game 2", "uris": ["magnet:3"]}),
            ])
            .unwrap();
        repository.set_matches(vec![
            json!({"offerId": "o1", "gameId": "g1"}),
            json!({"offerId": "o2", "gameId": "g1"}),
            json!({"offerId": "o3", "gameId": "g2"}),
        ]);
        assert_eq!(repository.get_games().len(), 1);
        assert_eq!(repository.get_offers_for_game("g1").len(), 2);
        assert_eq!(repository.get_offer("o2").unwrap()["id"], json!("o2"));
        assert!(repository.get_offer("missing").is_none());
    }

    #[test]
    fn artwork_e_overrides_recebem_updated_at() {
        let dir = temp_dir();
        let repository = RetroRepository::with_now(dir.path(), Arc::new(|| 42));
        repository.set_artwork(
            "g1",
            &json!({"cover": "https://example.test/c.jpg", "provider": "libretro"}),
        );
        let artwork = repository.get_artwork("g1").unwrap();
        assert_eq!(artwork["cover"], json!("https://example.test/c.jpg"));
        assert_eq!(artwork["updatedAt"], json!(42));

        repository.set_override("offer", &json!({"gameId": "g1"}));
        assert_eq!(
            repository.get_override("offer").unwrap()["gameId"],
            json!("g1")
        );
        assert_eq!(
            repository.get_override("offer").unwrap()["updatedAt"],
            json!(42)
        );
        repository.remove_override("offer");
        assert!(repository.get_override("offer").is_none());
    }

    #[test]
    fn unmatched_e_stats() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        repository.set_catalog_index(
            vec![game("g1", "nes", "A"), game("g2", "snes", "B")],
            Map::new(),
        );
        repository
            .set_offers(vec![
                json!({"id": "o1"}),
                json!({"id": "o2"}),
                json!({"id": "o3"}),
            ])
            .unwrap();
        repository.set_matches(vec![
            json!({"offerId": "o1", "gameId": "g1"}),
            json!({"offerId": "o2", "gameId": "g1"}),
            json!({"offerId": "o3", "gameId": "g2"}),
        ]);
        repository.set_unmatched(vec![json!({"offerId": "o4", "reason": "no match"})]);
        let stats = repository.get_stats();
        assert_eq!(stats.games, 2);
        assert_eq!(stats.offers, 3);
        assert_eq!(stats.matched, 3);
        assert_eq!(stats.unmatched, 1);
    }

    #[test]
    fn recusa_mais_de_max_offers() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        let offers = vec![json!({"id": "o"}); MAX_OFFERS + 1];
        let error = repository.set_offers(offers).unwrap_err();
        assert_eq!(error, RepositoryError::OffersExceedMaximum(MAX_OFFERS));
        assert_eq!(error.to_string(), "Offers exceed maximum (100000)");
    }

    #[test]
    fn persiste_e_preserva_campos_desconhecidos() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        let mut custom_game = game("g1", "nes", "A");
        custom_game["custom"] = json!({"nested": true});
        repository.set_catalog_index(vec![custom_game], Map::new());
        repository
            .set_offers(vec![json!({"id": "o1", "extra": "kept"})])
            .unwrap();

        let reloaded = RetroRepository::new(dir.path());
        assert_eq!(reloaded.get_games()[0]["custom"], json!({"nested": true}));
        assert_eq!(reloaded.get_offers()[0]["extra"], json!("kept"));
        assert_eq!(reloaded.get_stats().offers, 1);
    }

    #[test]
    fn campos_desconhecidos_no_topo_do_arquivo_sobrevivem_ao_set() {
        let dir = temp_dir();
        let retro = dir.path().join("retro");
        std::fs::create_dir_all(&retro).unwrap();
        std::fs::write(
            retro.join("offers-v2.json"),
            serde_json::to_string(&json!({
                "version": 2,
                "updatedAt": 1,
                "offers": [],
                "unknownTop": {"kept": true}
            }))
            .unwrap(),
        )
        .unwrap();
        let repository = RetroRepository::new(dir.path());
        repository.set_offers(vec![json!({"id": "o1"})]).unwrap();
        let stored = safe_read_json(&retro.join("offers-v2.json"), MAX_CACHE_BYTES).unwrap();
        assert_eq!(stored["unknownTop"], json!({"kept": true}));
        assert_eq!(stored["offers"][0]["id"], json!("o1"));
    }

    #[test]
    fn arquivo_com_versao_errada_volta_ao_padrao() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.path().join("retro")).unwrap();
        std::fs::write(
            dir.path().join("retro").join("catalog-index-v2.json"),
            serde_json::to_string(&json!({"version": 1, "games": [{"id": "old"}]})).unwrap(),
        )
        .unwrap();
        let repository = RetroRepository::new(dir.path());
        assert!(repository.get_games().is_empty());
    }

    #[test]
    fn escrita_atomica_nao_deixa_temporarios() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        repository.set_catalog_index(vec![game("g1", "nes", "A")], Map::new());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("retro"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn auditoria_conta_cobertura_e_samples() {
        let dir = temp_dir();
        let repository = RetroRepository::new(dir.path());
        repository.set_catalog_index(
            vec![
                json!({
                    "id": "g1",
                    "systemId": "nes",
                    "title": "Com capa",
                    "summary": "texto",
                    "offerCount": 2,
                    "artwork": {"cover": "https://x/c.jpg", "screenshots": ["https://x/s.jpg"]}
                }),
                json!({"id": "g2", "systemId": "nes", "title": "Sem nada"}),
            ],
            Map::new(),
        );
        repository.set_unmatched(vec![
            json!({"offerId": "o1", "reason": "no system identified", "title": "X", "platformRaw": "ps9"}),
            json!({"offerId": "o2", "reason": "no system identified", "title": "Y"}),
            json!({"offerId": "o3", "reason": "no match", "title": "Z", "systemId": "nes"}),
        ]);
        let audit = repository.audit(None, 10);
        assert_eq!(audit["ok"], json!(true));
        assert_eq!(audit["totals"]["games"], json!(2));
        assert_eq!(audit["totals"]["covers"], json!(1));
        assert_eq!(audit["totals"]["descriptions"], json!(1));
        assert_eq!(audit["missing"]["cover"], json!(1));
        assert_eq!(audit["unmatched"]["total"], json!(3));
        assert_eq!(
            audit["unmatched"]["byReason"]["no system identified"],
            json!(2)
        );
        assert_eq!(audit["unmatched"]["bySystem"]["unknown"], json!(2));
        assert_eq!(audit["unmatched"]["samples"].as_array().unwrap().len(), 3);

        let filtered = repository.audit(Some("nes"), 5);
        assert_eq!(filtered["unmatched"]["total"], json!(1));
    }

    #[test]
    fn biblioteca_adiciona_sem_duplicar_e_remove_legado() {
        let dir = temp_dir();
        let library = RetroLibrary::with_data_dir(dir.path());
        let payload = json!({
            "id": "retro:nes:slug:abc",
            "title": "Game",
            "platform": "nes",
            "systemId": "nintendo-nes",
            "cover": "https://x/cover.jpg",
            "genres": ["Action", " ", 1, "RPG"],
            "releaseYear": 1990,
            "custom": "ignored"
        });
        let added = library.add(&payload);
        assert!(added.ok);
        assert_eq!(added.added, Some(true));
        let games = added.games.unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0]["launcher"], json!("retro"));
        assert_eq!(games[0]["retro"], json!(true));
        assert_eq!(games[0]["genres"], json!(["Action", "RPG"]));
        assert_eq!(games[0]["releaseYear"], json!(1990));
        assert!(games[0].get("custom").is_none());

        let duplicate = library.add(&payload);
        assert_eq!(duplicate.added, Some(false));
        assert_eq!(duplicate.games.unwrap().len(), 1);

        // Entrada legada com launcher "custom" e campo extra.
        let path = library.path();
        let mut existing = Value::Array(library.list());
        existing[0]["launcher"] = json!("custom");
        existing[0]["legacyField"] = json!(true);
        safe_write_json(path, &existing, true).unwrap();

        let removed = library.remove("retro:nes:slug:abc");
        assert!(removed.ok);
        assert!(removed.games.unwrap().is_empty());
    }

    #[test]
    fn biblioteca_rejeita_id_invalido() {
        let dir = temp_dir();
        let library = RetroLibrary::with_data_dir(dir.path());
        assert_eq!(
            library
                .add(&json!({"id": "steam:1", "title": "x"}))
                .error
                .as_deref(),
            Some("jogo retrô inválido")
        );
        assert_eq!(
            library.remove("nope").error.as_deref(),
            Some("jogo retrô inválido")
        );
    }

    #[test]
    fn stats_ultimo_update_zero_vira_null_e_manifest_so_quando_carregado() {
        let dir = temp_dir();
        let repository = RetroRepository::with_now(dir.path(), Arc::new(|| 0));
        repository.set_offers(vec![]).unwrap();
        assert_eq!(repository.get_stats().last_updated, None);
        let mut manifest = repository.load_manifest();
        manifest.catalog_version = Some(json!("v2"));
        repository.set_manifest(manifest);
        assert_eq!(repository.get_stats().catalog_version, Some(json!("v2")));
    }

    #[test]
    fn numbers_helper_nao_entra_em_panico() {
        let dir = temp_dir();
        let library = RetroLibrary::with_data_dir(dir.path());
        assert_eq!(
            library.add(&json!({"id": "retro:xx", "title": "t"})).error,
            None
        );
        let raw = safe_read_json(library.path(), MAX_CACHE_BYTES).unwrap();
        assert_eq!(raw.as_array().unwrap().len(), 1);
    }
}
