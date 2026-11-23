//! Loja Steam: busca, sugestões, "em alta", itens oficiais e appdetails.
//!
//! Porte de `app/electron/steamstore.js` (busca/sugestão/popular/itens) e da
//! parte de `app/electron/main.js` que lê `storesearch`/`appdetails`
//! (`buildSysinfo`). Todos os endpoints, parâmetros de país/idioma, formatos de
//! capa e envelopes IPC são preservados.
//!
//! Diferenças deliberadas, documentadas:
//!
//! - o índice local persistido (`local-search.js`) não faz parte deste crate;
//!   quando um [`CatalogClient`] é injetado, a rota `/catalog/v1/search` é
//!   consultada antes da rede, como no oráculo. Sem ele, a busca começa na
//!   Steam;
//! - `Intl.NumberFormat` depende do locale do sistema e não é reproduzível de
//!   forma determinística; [`format_price`] usa um formato fixo por moeda
//!   (BRL/USD/EUR) e um fallback estável (o valor cru continua disponível em
//!   `price` quando o chamador precisar);
//! - o cache em disco do Electron é substituído por caches em memória com os
//!   mesmos TTLs e limites de entrada. A borda pode persistir
//!   [`StoreCacheSnapshot`] se quiser sobreviver ao restart.
//!
//! Nenhum teste toca a rede: o [`Fetcher`] é injetado.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use arcadia_catalog::catalog::CatalogClient;
use arcadia_catalog::http::{encode_uri_component, FetchError, Fetcher, Request, Response};
use arcadia_catalog::sources::{Clock, SystemClock};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// `https://store.steampowered.com/api/storesearch/` (`storesearch`).
pub const STEAM_SEARCH: &str = "https://store.steampowered.com/api/storesearch/";
/// `https://store.steampowered.com/api/appdetails`.
pub const STEAM_APPDETAILS: &str = "https://store.steampowered.com/api/appdetails";
/// `IStoreBrowseService/GetItems` — tipo e assets oficiais.
pub const STEAM_ITEMS: &str = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";
/// Base dos assets devolvidos por `GetItems`.
pub const STEAM_ITEMS_ASSETS: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";
/// Catálogo Hubcap (ex-Morrenus).
pub const HUBCAP_BASE: &str = "https://hubcapmanifest.com/api/v1";
/// Uso da chave (`/api-keys/stats` no site). O luatools-moon chama este
/// endpoint em `GetHubcapStats`.
pub const HUBCAP_STATS: &str = "https://hubcapmanifest.com/api/v1/user/stats";
/// Dump por gênero/lista usado pelo "Em alta".
pub const STEAMSPY_API: &str = "https://steamspy.com/api.php";
/// Provedores de manifesto (as URLs são as mesmas da busca e do download).
pub const RYUU_BASE: &str = "http://167.235.229.108";
pub const SUSHI_BASE: &str =
    "https://raw.githubusercontent.com/sushi-dev55-alt/sushitools-games-repo-alt/refs/heads/main";
pub const SUSHI_TREE: &str =
    "https://api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main";
pub const TWENTYTWO_BASE: &str = "https://api.twentytwocloud.com/download";
/// CDN antigo, usado como fallback de capa.
pub const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
/// CDN de headers do "Em alta" (mesmo do oráculo).
pub const STEAM_CDN_AKAMAI: &str = "https://cdn.akamai.steamstatic.com/steam/apps";

/// `User-Agent` aplicado por `gh()` a todas as requisições.
pub const USER_AGENT: &str = "arcadia";

/// Resultados do dropdown de sugestões.
pub const SUGGEST_LIMIT: usize = 8;
/// Teto da busca completa (`BUSCA_MAX`).
pub const SEARCH_MAX: usize = 24;
/// TTL do resultado completo da busca (5 min).
pub const SEARCH_TTL_MS: u64 = 5 * 60 * 1000;
/// TTL dos metadados `GetItems` (7 dias).
pub const ITEMS_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// TTL do "Em alta" (6 h).
pub const POPULAR_TTL_MS: u64 = 6 * 60 * 60 * 1000;
/// TTL dos dumps por gênero (12 h).
pub const GENRE_TTL_MS: u64 = 12 * 60 * 60 * 1000;
/// TTL do índice Sushi (6 h).
pub const SUSHI_TTL_MS: u64 = 6 * 60 * 60 * 1000;
/// TTL da página pronta de `recent` (2 min): Home/Loja voltam instantâneas.
pub const RECENT_TTL_MS: u64 = 2 * 60 * 1000;
/// TTL das sondagens de manifesto (7 dias).
pub const MANIFEST_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Teto de entradas do cache de itens.
pub const ITEMS_MAX: usize = 4000;
/// Teto de appids por chamada de `GetItems`.
pub const ITEMS_BATCH: usize = 100;
/// Enumeração da Steam: só `0` é jogo instalável (4 = DLC, 1 = demo…).
pub const GAME_TYPE: u32 = 0;

/// `STEAM_LANG_MAP`: idioma da UI -> idioma aceito pela Steam.
pub const STEAM_LANG_MAP: &[(&str, &str)] = &[
    ("pt-BR", "portuguese"),
    ("en-US", "english"),
    ("es-ES", "spanish"),
];

/// Enumera os tipos de erro de rede/parse da loja sem vazar corpos enormes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct StoreError {
    pub message: String,
}

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// `steamLang()`: mapeia o idioma da config; desconhecido cai em `english`.
pub fn steam_language(ui_language: &str) -> &'static str {
    STEAM_LANG_MAP
        .iter()
        .find(|(key, _)| *key == ui_language)
        .map(|(_, value)| *value)
        .unwrap_or("english")
}

/// Config efetiva da loja (injetada; nada é lido do disco aqui).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// Chave do Hubcap. Só vai no header `Authorization`, nunca na URL.
    pub hubcap_api_key: Option<String>,
    /// Idioma da UI (`pt-BR`, `en-US`, `es-ES`…).
    pub language: String,
    /// País em minúsculas para `storesearch`/`appdetails` (`br` nos dois).
    pub country: String,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            hubcap_api_key: None,
            language: "en-US".to_string(),
            country: "br".to_string(),
        }
    }
}

impl StoreConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn with_country(mut self, country: impl Into<String>) -> Self {
        self.country = country.into().to_lowercase();
        self
    }

    pub fn with_hubcap_key(mut self, key: Option<impl Into<String>>) -> Self {
        self.hubcap_api_key = key.map(Into::into).filter(|key| !key.trim().is_empty());
        self
    }

    pub fn steam_language(&self) -> &'static str {
        steam_language(&self.language)
    }

    /// `country_code` de `GetItems` é maiúsculo (`BR`).
    pub fn country_upper(&self) -> String {
        self.country.to_uppercase()
    }
}

/// Bases configuráveis (testes usam servidor local).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBases {
    pub search: String,
    pub appdetails: String,
    pub items: String,
    pub items_assets: String,
    pub hubcap: String,
    pub steamspy: String,
    pub ryuu: String,
    pub sushi: String,
    pub sushi_tree: String,
    pub twenty_two: String,
    pub cdn: String,
    pub cdn_akamai: String,
}

impl Default for StoreBases {
    fn default() -> Self {
        Self {
            search: STEAM_SEARCH.to_string(),
            appdetails: STEAM_APPDETAILS.to_string(),
            items: STEAM_ITEMS.to_string(),
            items_assets: STEAM_ITEMS_ASSETS.to_string(),
            hubcap: HUBCAP_BASE.to_string(),
            steamspy: STEAMSPY_API.to_string(),
            ryuu: RYUU_BASE.to_string(),
            sushi: SUSHI_BASE.to_string(),
            sushi_tree: SUSHI_TREE.to_string(),
            twenty_two: TWENTYTWO_BASE.to_string(),
            cdn: STEAM_CDN.to_string(),
            cdn_akamai: STEAM_CDN_AKAMAI.to_string(),
        }
    }
}

impl StoreBases {
    fn trim(base: &str) -> &str {
        base.trim_end_matches('/')
    }

    pub fn search_url(&self, query: &str, country: &str, language: &str) -> String {
        format!(
            "{}?term={}&cc={}&l={}",
            self.search.trim_end_matches('?'),
            encode_uri_component(query),
            country,
            language
        )
    }

    pub fn appdetails_url(&self, appid: &str, country: &str, language: &str) -> String {
        format!(
            "{}?appids={}&cc={}&l={}",
            self.appdetails.trim_end_matches('?'),
            appid,
            country,
            language
        )
    }

    pub fn hubcap_library_url(&self, query: &str) -> String {
        format!(
            "{}/library?search={}",
            Self::trim(&self.hubcap),
            encode_uri_component(query)
        )
    }

    pub fn hubcap_manifest_url(&self, appid: &str) -> String {
        format!("{}/manifest/{}", Self::trim(&self.hubcap), appid)
    }

    pub fn ryuu_url(&self, appid: &str) -> String {
        format!("{}/{}", Self::trim(&self.ryuu), appid)
    }

    pub fn sushi_url(&self, appid: &str) -> String {
        format!("{}/{}.zip", Self::trim(&self.sushi), appid)
    }

    pub fn twenty_two_url(&self, appid: &str) -> String {
        format!("{}?appid={}", Self::trim(&self.twenty_two), appid)
    }

    pub fn steamspy_url(&self, lista: &str) -> String {
        format!(
            "{}?request={}",
            Self::trim(&self.steamspy),
            encode_uri_component(lista)
        )
    }

    fn asset_url(&self, assets: &Value, filename: &str) -> String {
        let Some(format_string) = assets.get("asset_url_format").and_then(Value::as_str) else {
            return String::new();
        };
        if format_string.is_empty() || filename.is_empty() {
            return String::new();
        }
        format!(
            "{}{}",
            self.items_assets,
            format_string.replace("${FILENAME}", filename)
        )
    }
}

/// Jogo no formato consumido pela UI da loja (`jogos[]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreGame {
    pub appid: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capa: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heroi: Option<String>,
    #[serde(default)]
    pub manifest: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fontes: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preco: String,
}

impl StoreGame {
    pub fn new(appid: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            appid: appid.into(),
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn with_cover(mut self, cover: impl Into<String>) -> Self {
        self.cover = cover.into();
        self
    }
}

/// Item de `GetItems` (tipo + artes oficiais).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreItem {
    pub tipo: u32,
    #[serde(default)]
    pub capa: String,
    #[serde(default)]
    pub heroi: String,
    #[serde(default)]
    pub icon: String,
}

/// Resultado de `itensDaLoja`: `respondidos` são os appids com veredito
/// confiável (inclusive "não existe na loja"); os demais ficaram sem resposta
/// de rede e não podem ser confundidos com remoção.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreItemsResult {
    pub mapa: HashMap<String, StoreItem>,
    pub respondidos: HashSet<String>,
}

/// `store:suggest` — `{ ok, jogos, cache? }` / `{ ok:false, error }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SuggestResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jogos: Option<Vec<StoreGame>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SuggestResponse {
    pub fn ok(jogos: Vec<StoreGame>) -> Self {
        Self {
            ok: true,
            jogos: Some(jogos),
            cache: None,
            error: None,
        }
    }

    pub fn cached(jogos: Vec<StoreGame>) -> Self {
        Self {
            cache: Some(true),
            ..Self::ok(jogos)
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            jogos: None,
            cache: None,
            error: Some(message.into()),
        }
    }
}

/// `store:search` — `{ ok, jogos, fonte, avisos, cache? }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jogos: Option<Vec<StoreGame>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fonte: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avisos: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SearchResponse {
    pub fn success(jogos: Vec<StoreGame>, fonte: impl Into<String>, avisos: Vec<String>) -> Self {
        Self {
            ok: true,
            jogos: Some(jogos),
            fonte: Some(fonte.into()),
            avisos: Some(avisos),
            cache: None,
            error: None,
        }
    }

    pub fn cached(mut self) -> Self {
        self.cache = Some(true);
        self
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            jogos: None,
            fonte: None,
            avisos: None,
            cache: None,
            error: Some(message.into()),
        }
    }
}

/// `store:recent`/`popular` — página + metadados.
///
/// `Deserialize` existe para o cache em disco: o snapshot grava a resposta e
/// `restore_caches` a reconstrói no boot seguinte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jogos: Option<Vec<StoreGame>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    /// `Some(Value::Null)` serializa `null` (última página); `None` omite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revalidando: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velho: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RecentResponse {
    fn page(mut self, jogos: Vec<StoreGame>, offset: usize, limit: usize, total: usize) -> Self {
        let games = jogos;
        let has_more = offset + games.len() < total;
        self.ok = true;
        self.jogos = Some(games);
        self.offset = Some(offset);
        self.total = Some(total);
        self.limit = Some(limit);
        self.has_more = Some(has_more);
        self.next_offset = Some(if has_more {
            json!(offset + self.jogos.as_ref().map(Vec::len).unwrap_or(0))
        } else {
            Value::Null
        });
        self
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            jogos: None,
            offset: None,
            total: None,
            limit: None,
            has_more: None,
            next_offset: None,
            cache: None,
            revalidando: None,
            velho: None,
            error: Some(message.into()),
        }
    }
}

/// Screenshot de `appdetails` no formato de `game:sysinfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppScreenshot {
    pub thumb: String,
    pub full: String,
}

/// Trailer de `appdetails` no formato de `game:sysinfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppMovie {
    pub id: String,
    pub name: String,
    pub thumb: String,
    pub mp4: String,
    pub webm: String,
    pub hls: String,
}

/// Campos ricos de `appdetails` que `buildSysinfo` entrega à página do jogo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDetails {
    pub appid: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub short_description: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub controller_support: String,
    #[serde(default)]
    pub languages: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub req_min: String,
    #[serde(default)]
    pub req_rec: String,
    #[serde(default)]
    pub screenshots: Vec<AppScreenshot>,
    #[serde(default)]
    pub movies: Vec<AppMovie>,
}

/// Envelope `game:sysinfo`/`appdetails`: `{ ok, info }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppDetailsResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<AppDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AppDetailsResponse {
    pub fn success(info: AppDetails) -> Self {
        Self {
            ok: true,
            info: Some(info),
            error: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            info: None,
            error: Some(message.into()),
        }
    }
}

/// Uso diário da chave Hubcap (`GET /api/v1/user/stats`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HubcapStats {
    pub ok: bool,
    pub username: Option<String>,
    pub daily_usage: Option<u64>,
    pub daily_limit: Option<u64>,
    /// `rejected` quando o Hubcap recusou a chave; `unreachable` no resto.
    pub error: Option<String>,
}

/// Validação barata da chave do Hubcap (`store:validarChaveHubcap`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HubcapKeyValidation {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erro: Option<String>,
}

/// Opções de paginação de `search`.
///
/// O IPC congelado chama `store:search` só com `query`; `offset`/`limit` são
/// úteis para chamadas diretas. Filtros de catálogo (`launcher`, `genre`,
/// `tags`…) pertencem ao índice local, que não faz parte deste crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub offset: usize,
    pub limit: Option<usize>,
}

impl SearchOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct ItemCache {
    at: u64,
    tipo: Option<u32>,
    capa: String,
    heroi: String,
    icon: String,
}

impl ItemCache {
    fn item(&self) -> Option<StoreItem> {
        self.tipo.map(|tipo| StoreItem {
            tipo,
            capa: self.capa.clone(),
            heroi: self.heroi.clone(),
            icon: self.icon.clone(),
        })
    }
}

#[derive(Debug, Default)]
struct Caches {
    suggest: HashMap<String, Vec<StoreGame>>,
    search: HashMap<String, (u64, SearchResponse)>,
    items: HashMap<String, ItemCache>,
    popular: HashMap<String, (u64, Vec<StoreGame>)>,
    genre: HashMap<String, (u64, Vec<StoreGame>)>,
    recent: HashMap<String, (u64, RecentResponse)>,
    sushi: Option<(u64, HashSet<String>)>,
    manifests: HashMap<String, (u64, bool)>,
}

impl Caches {
    fn len(&self) -> usize {
        self.items.len()
    }
}

/// Snapshot exportável dos caches em memória (persistência opcional na borda).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoreCacheSnapshot {
    #[serde(default)]
    pub search: HashMap<String, Value>,
    #[serde(default)]
    pub items: HashMap<String, Value>,
    #[serde(default)]
    pub popular: HashMap<String, Value>,
    #[serde(default)]
    pub genre: HashMap<String, Value>,
    /// Páginas de `recent` (é o que abre Início e Loja). Sem persistir isto,
    /// todo boot paga a latência de rede inteira na primeira carga: medido em
    /// 3.603 ms antes do warm.
    #[serde(default)]
    pub recent: HashMap<String, Value>,
}

/// Cliente da loja. `Fetcher` é injetado; `CatalogClient` é opcional.
#[derive(Clone)]
pub struct StoreClient {
    fetcher: Arc<dyn Fetcher>,
    catalog: Option<CatalogClient>,
    config: StoreConfig,
    bases: StoreBases,
    clock: Arc<dyn Clock>,
    caches: Arc<Mutex<Caches>>,
    availability_probe: bool,
}

impl StoreClient {
    pub fn new(fetcher: Arc<dyn Fetcher>, config: StoreConfig) -> Self {
        Self {
            fetcher,
            catalog: None,
            config,
            bases: StoreBases::default(),
            clock: Arc::new(SystemClock),
            caches: Arc::new(Mutex::new(Caches::default())),
            availability_probe: true,
        }
    }

    /// Produção/teste local sobre `arcadia-api`.
    pub fn from_base_url(
        api_base: &str,
        config: StoreConfig,
    ) -> Result<Self, arcadia_catalog::http::FetchError> {
        let fetcher = arcadia_catalog::http::HttpClientFetcher::from_base_url(api_base)?;
        Ok(Self::new(Arc::new(fetcher), config))
    }

    pub fn with_bases(mut self, bases: StoreBases) -> Self {
        self.bases = bases;
        self
    }

    pub fn with_catalog(mut self, catalog: CatalogClient) -> Self {
        self.catalog = Some(catalog);
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Liga/desliga a sonda `HEAD` direta nos provedores. Ligada por padrão,
    /// como no oráculo; desligar deixa `fontes`/`manifest` só com o que o
    /// catálogo/Hubcap já respondeu (útil em testes ou em modo econômico).
    pub fn with_availability_probe(mut self, enabled: bool) -> Self {
        self.availability_probe = enabled;
        self
    }

    pub fn config(&self) -> &StoreConfig {
        &self.config
    }

    pub fn bases(&self) -> &StoreBases {
        &self.bases
    }

    pub fn catalog(&self) -> Option<&CatalogClient> {
        self.catalog.as_ref()
    }

    pub fn fetcher(&self) -> &Arc<dyn Fetcher> {
        &self.fetcher
    }

    pub fn clear_caches(&self) {
        *self.lock_caches() = Caches::default();
    }

    pub fn cache_snapshot(&self) -> StoreCacheSnapshot {
        let caches = self.lock_caches();
        StoreCacheSnapshot {
            search: caches
                .search
                .iter()
                .filter_map(|(key, (at, response))| {
                    serde_json::to_value(response)
                        .ok()
                        .map(|value| (key.clone(), json!({"at": at, "res": value})))
                })
                .collect(),
            items: caches
                .items
                .iter()
                .filter_map(|(key, item)| {
                    serde_json::to_value(item)
                        .ok()
                        .map(|value| (key.clone(), value))
                })
                .collect(),
            popular: caches
                .popular
                .iter()
                .filter_map(|(key, (at, games))| {
                    serde_json::to_value(games)
                        .ok()
                        .map(|value| (key.clone(), json!({"at": at, "jogos": value})))
                })
                .collect(),
            genre: caches
                .genre
                .iter()
                .filter_map(|(key, (at, games))| {
                    serde_json::to_value(games)
                        .ok()
                        .map(|value| (key.clone(), json!({"at": at, "jogos": value})))
                })
                .collect(),
            recent: caches
                .recent
                .iter()
                .filter_map(|(key, (at, response))| {
                    serde_json::to_value(response)
                        .ok()
                        .map(|value| (key.clone(), json!({"at": at, "res": value})))
                })
                .collect(),
        }
    }

    /// Repovoa os caches a partir de um snapshot em disco.
    ///
    /// Só aceita entradas que o TTL ainda considera válidas — um cache velho
    /// mostraria preços/lista de outra semana. O `recent` é o que importa
    /// aqui: é a primeira coisa que a Home e a Loja pedem.
    pub fn restore_caches(&self, snapshot: StoreCacheSnapshot) {
        let now = self.now();
        let mut caches = self.lock_caches();

        for (key, value) in snapshot.recent {
            let Some(at) = value.get("at").and_then(Value::as_u64) else {
                continue;
            };
            if now.saturating_sub(at) >= RECENT_TTL_MS {
                continue;
            }
            let Some(res) = value.get("res") else { continue };
            if let Ok(response) = serde_json::from_value::<RecentResponse>(res.clone()) {
                caches.recent.insert(key, (at, response));
            }
        }

        for (key, value) in snapshot.popular {
            let Some(at) = value.get("at").and_then(Value::as_u64) else {
                continue;
            };
            if now.saturating_sub(at) >= POPULAR_TTL_MS {
                continue;
            }
            if let Some(games) = value.get("jogos") {
                if let Ok(games) = serde_json::from_value::<Vec<StoreGame>>(games.clone()) {
                    caches.popular.insert(key, (at, games));
                }
            }
        }

        for (key, value) in snapshot.genre {
            let Some(at) = value.get("at").and_then(Value::as_u64) else {
                continue;
            };
            if now.saturating_sub(at) >= GENRE_TTL_MS {
                continue;
            }
            if let Some(games) = value.get("jogos") {
                if let Ok(games) = serde_json::from_value::<Vec<StoreGame>>(games.clone()) {
                    caches.genre.insert(key, (at, games));
                }
            }
        }
    }

    fn lock_caches(&self) -> MutexGuard<'_, Caches> {
        self.caches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn now(&self) -> u64 {
        self.clock.now_millis()
    }

    async fn fetch_json(
        &self,
        url: &str,
        timeout: Duration,
        headers: &[(&str, &str)],
    ) -> Result<Value, StoreError> {
        let mut request = Request::get(url)
            .timeout(timeout)
            .header("User-Agent", USER_AGENT);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        match self.fetcher.fetch(request).await {
            Ok(response) if response.is_success() => response
                .json_value()
                .map_err(|error| StoreError::new(error.message)),
            Ok(response) => Err(StoreError::new(format!("HTTP {}", response.status))),
            Err(FetchError { message }) => Err(StoreError::new(message)),
        }
    }

    async fn fetch_response(&self, request: Request) -> Result<Response, StoreError> {
        match self.fetcher.fetch(request).await {
            Ok(response) => Ok(response),
            Err(FetchError { message }) => Err(StoreError::new(message)),
        }
    }

    // ── store:suggest ─────────────────────────────────────────────────────
    /// `suggest(query)`: < 2 caracteres devolve vazio sem tocar na rede.
    pub async fn suggest(&self, query: &str) -> SuggestResponse {
        let trimmed = query.trim();
        if trimmed.chars().count() < 2 {
            return SuggestResponse::ok(Vec::new());
        }
        let key = trimmed.to_lowercase();
        {
            let caches = self.lock_caches();
            if let Some(games) = caches.suggest.get(&key) {
                return SuggestResponse::cached(games.clone());
            }
        }

        let url =
            self.bases
                .search_url(trimmed, &self.config.country, self.config.steam_language());
        let known_types = {
            let caches = self.lock_caches();
            caches
                .items
                .iter()
                .filter_map(|(appid, entry)| entry.item().map(|item| (appid.clone(), item.tipo)))
                .collect::<HashMap<_, _>>()
        };
        let mut games = match self.fetch_json(&url, Duration::from_secs(6), &[]).await {
            Ok(data) => search_items(&data, &self.bases.cdn)
                .into_iter()
                .filter(|game| {
                    known_types
                        .get(&game.appid)
                        .map(|tipo| *tipo == GAME_TYPE)
                        .unwrap_or(true)
                })
                .take(SUGGEST_LIMIT)
                .collect(),
            Err(_) => Vec::new(),
        };

        if games.is_empty() {
            games = self
                .catalog_search_games(trimmed, SUGGEST_LIMIT)
                .await
                .unwrap_or_default();
        }

        let mut caches = self.lock_caches();
        if caches.suggest.len() > 100 {
            caches.suggest.clear();
        }
        caches.suggest.insert(key, games.clone());
        SuggestResponse::ok(games)
    }

    async fn catalog_search_games(&self, query: &str, limit: usize) -> Option<Vec<StoreGame>> {
        let catalog = self.catalog.as_ref()?;
        let path = format!("/catalog/v1/search?q={}", encode_uri_component(query));
        let response = catalog.get(&path).await;
        let items = response
            .data?
            .get("itens")
            .and_then(Value::as_array)
            .cloned()?;
        let games = items
            .iter()
            .filter_map(game_from_catalog_item)
            .take(limit)
            .collect::<Vec<_>>();
        if games.is_empty() {
            None
        } else {
            Some(games)
        }
    }

    // ── store:search ──────────────────────────────────────────────────────
    pub async fn search(&self, query: &str) -> SearchResponse {
        self.search_with(query, &SearchOptions::default()).await
    }

    /// Busca completa: catálogo (opcional) -> Hubcap + Steam -> itens oficiais.
    pub async fn search_with(&self, query: &str, options: &SearchOptions) -> SearchResponse {
        let key_base = query.trim().to_lowercase();
        let key = format!(
            "{}|{}|{}",
            key_base,
            options.offset,
            options
                .limit
                .map_or_else(|| "-".to_string(), |limit| limit.to_string())
        );
        if key_base.chars().count() >= 2 {
            let now = self.now();
            let hit = {
                let caches = self.lock_caches();
                caches
                    .search
                    .get(&key)
                    .filter(|(at, _)| now.saturating_sub(*at) < SEARCH_TTL_MS)
                    .map(|(_, response)| response.clone())
            };
            if let Some(response) = hit {
                return response.cached();
            }
        }

        let response = self.search_real(query, options).await;
        if key_base.chars().count() >= 2 && response.ok {
            let mut caches = self.lock_caches();
            if caches.search.len() > 50 {
                caches.search.clear();
            }
            caches.search.insert(key, (self.now(), response.clone()));
        }
        response
    }

    async fn search_real(&self, query: &str, options: &SearchOptions) -> SearchResponse {
        let offset = options.offset;
        let limit = options
            .limit
            .filter(|limit| *limit > 0)
            .unwrap_or(SEARCH_MAX);
        let catalog_limit = options.limit.filter(|limit| *limit > 0).unwrap_or(40);

        // 1) Catálogo remoto (o índice local persistido fica na borda).
        if self.catalog.is_some() && !query.trim().is_empty() {
            if let Some(games) = self.catalog_search_games(query, 4000).await {
                let selected = games
                    .into_iter()
                    .skip(offset)
                    .take(catalog_limit)
                    .collect::<Vec<_>>();
                let prepared = self.prepare(selected, &HashSet::new()).await;
                let mut sorted = prepared;
                sort_games(&mut sorted, query);
                return SearchResponse::success(sorted, "catalogo", Vec::new());
            }
        }

        let mut by_id: Vec<StoreGame> = Vec::new();
        let mut seen = HashSet::new();
        let mut errors = Vec::new();
        let mut from_hubcap = HashSet::new();

        // 2) Hubcap (opcional: só com chave; chave somente no header).
        if let Some(key) = self.config.hubcap_api_key.as_deref() {
            let url = self.bases.hubcap_library_url(query);
            let auth = format!("Bearer {key}");
            match self
                .fetch_json(
                    &url,
                    Duration::from_secs(12),
                    &[("Authorization", auth.as_str())],
                )
                .await
            {
                Ok(data) => {
                    for game in hubcap_games(&data) {
                        if game.manifest {
                            from_hubcap.insert(game.appid.clone());
                        }
                        upsert_game(&mut by_id, &mut seen, game);
                    }
                }
                Err(error) => errors.push(format!("Hubcap: {}", error.message)),
            }
        }

        // 3) Steam storesearch — catálogo completo, sem chave.
        let url = self
            .bases
            .search_url(query, &self.config.country, self.config.steam_language());
        match self.fetch_json(&url, Duration::from_secs(12), &[]).await {
            Ok(data) => {
                for game in search_items(&data, &self.bases.cdn) {
                    if seen.contains(&game.appid) {
                        continue;
                    }
                    let appid = game.appid.clone();
                    seen.insert(appid.clone());
                    by_id.push(game);
                }
            }
            Err(error) => errors.push(format!("Steam: {}", error.message)),
        }

        if by_id.is_empty() {
            return SearchResponse::failure(if errors.is_empty() {
                "nenhum resultado".to_string()
            } else {
                errors.join(" · ")
            });
        }

        sort_games(&mut by_id, query);
        let selected = by_id
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        let mut prepared = self.prepare(selected, &from_hubcap).await;
        sort_games(&mut prepared, query);
        SearchResponse::success(prepared, "multi", errors)
    }

    /// Completa tipo/capa e marca disponibilidade, como `preparar()`.
    async fn prepare(
        &self,
        games: Vec<StoreGame>,
        from_hubcap: &HashSet<String>,
    ) -> Vec<StoreGame> {
        self.prepare_with(games, from_hubcap, true).await
    }

    /// Variante sem HEAD por fonte — usada em listas para o primeiro paint não
    /// pagar 24 jogos × 2 provedores sequenciais.
    async fn prepare_fast(
        &self,
        games: Vec<StoreGame>,
        from_hubcap: &HashSet<String>,
    ) -> Vec<StoreGame> {
        self.prepare_with(games, from_hubcap, false).await
    }

    async fn prepare_with(
        &self,
        games: Vec<StoreGame>,
        from_hubcap: &HashSet<String>,
        probe_sources: bool,
    ) -> Vec<StoreGame> {
        let appids = games
            .iter()
            .map(|game| game.appid.clone())
            .collect::<Vec<_>>();
        let items = self.items_for(&appids).await;
        let availability = self.load_availability(&appids).await;

        let mut filtered = Vec::new();
        for mut game in games {
            let id = game.appid.clone();
            if let Some(item) = items.mapa.get(&id) {
                if item.tipo != GAME_TYPE {
                    continue;
                }
                if !item.capa.is_empty() {
                    game.capa = Some(item.capa.clone());
                }
                if !item.heroi.is_empty() {
                    game.heroi = Some(item.heroi.clone());
                }
            } else if items.respondidos.contains(&id) {
                // A Steam respondeu e não conhece: removido da loja.
                continue;
            }
            filtered.push(game);
        }
        self.mark_availability(&mut filtered, from_hubcap, availability, probe_sources)
            .await;
        filtered
    }

    // ── GetItems (tipo/capa oficiais) ─────────────────────────────────────
    /// `itensDaLoja(appids)`: cache de 7 dias, lotes de 100 e cache negativo.
    pub async fn items_for(&self, appids: &[String]) -> StoreItemsResult {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        for appid in appids {
            let appid = appid.trim();
            if !appid.is_empty() && seen.insert(appid.to_string()) {
                ids.push(appid.to_string());
            }
        }
        let mut result = StoreItemsResult::default();
        if ids.is_empty() {
            return result;
        }

        let now = self.now();

        // 1) Catálogo remoto (quando injetado).
        if let Some(catalog) = self.catalog.as_ref() {
            let path = format!(
                "/catalog/v1/items?appids={}",
                encode_uri_component(&ids.join(","))
            );
            let response = catalog.get(&path).await;
            if let Some(data) = response.data.and_then(|value| value.get("data").cloned()) {
                if let Some(object) = data.as_object() {
                    for (id, item) in object {
                        if !ids.contains(id) {
                            continue;
                        }
                        let Some(tipo) = item.get("tipo").and_then(Value::as_u64) else {
                            continue;
                        };
                        let item = StoreItem {
                            tipo: tipo as u32,
                            capa: string_field(item, "capa"),
                            heroi: string_field(item, "heroi"),
                            icon: string_field(item, "icon"),
                        };
                        result.mapa.insert(id.clone(), item.clone());
                        result.respondidos.insert(id.clone());
                        self.lock_caches().items.insert(
                            id.clone(),
                            ItemCache {
                                at: now,
                                tipo: Some(item.tipo),
                                capa: item.capa,
                                heroi: item.heroi,
                                icon: item.icon,
                            },
                        );
                    }
                }
            }
        }

        let mut missing = Vec::new();
        {
            let caches = self.lock_caches();
            for id in &ids {
                if result.respondidos.contains(id) {
                    continue;
                }
                if let Some(entry) = caches.items.get(id) {
                    if now.saturating_sub(entry.at) < ITEMS_TTL_MS {
                        result.respondidos.insert(id.clone());
                        if let Some(item) = entry.item() {
                            result.mapa.insert(id.clone(), item);
                        }
                        continue;
                    }
                }
                missing.push(id.clone());
            }
            // Não segura o lock durante a rede.
            drop(caches);
        }

        for batch in missing.chunks(ITEMS_BATCH) {
            let input = json!({
                "ids": batch.iter().map(|id| json!({ "appid": id.parse::<u64>().unwrap_or(0) })).collect::<Vec<_>>(),
                "context": {
                    "language": self.config.steam_language(),
                    "country_code": self.config.country_upper(),
                    "steam_realm": 1
                },
                "data_request": { "include_assets": true }
            });
            let url = format!(
                "{}?input_json={}",
                self.bases.items.trim_end_matches('?'),
                encode_uri_component(&input.to_string())
            );
            let Ok(data) = self.fetch_json(&url, Duration::from_secs(20), &[]).await else {
                // Sem veredito: os appids passam pelo filtro.
                continue;
            };
            for id in batch {
                result.respondidos.insert(id.clone());
                self.lock_caches().items.insert(
                    id.clone(),
                    ItemCache {
                        at: now,
                        ..ItemCache::default()
                    },
                );
            }
            let store_items = data
                .pointer("/response/store_items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for item in store_items {
                let id = value_string_truthy(item.get("appid"));
                let Some(tipo) = item.get("type").and_then(Value::as_u64) else {
                    continue;
                };
                if id.is_empty() {
                    continue;
                }
                let assets = item.get("assets").cloned().unwrap_or(Value::Null);
                let stored = StoreItem {
                    tipo: tipo as u32,
                    capa: self.assets_url(&assets, assets.get("library_capsule")),
                    heroi: self.assets_url(
                        &assets,
                        assets
                            .get("library_hero_2x")
                            .or_else(|| assets.get("library_hero")),
                    ),
                    icon: self.assets_url(
                        &assets,
                        assets.get("icon").or_else(|| assets.get("community_icon")),
                    ),
                };
                result.mapa.insert(id.clone(), stored.clone());
                self.lock_caches().items.insert(
                    id,
                    ItemCache {
                        at: now,
                        tipo: Some(stored.tipo),
                        capa: stored.capa,
                        heroi: stored.heroi,
                        icon: stored.icon,
                    },
                );
            }
        }

        self.trim_items_cache();
        result
    }

    fn assets_url(&self, assets: &Value, filename: Option<&Value>) -> String {
        match filename.and_then(Value::as_str) {
            Some(filename) => self.bases.asset_url(assets, filename),
            None => String::new(),
        }
    }

    fn trim_items_cache(&self) {
        let mut caches = self.lock_caches();
        if caches.len() <= ITEMS_MAX {
            return;
        }
        let mut keys = caches
            .items
            .iter()
            .map(|(key, entry)| (key.clone(), entry.at))
            .collect::<Vec<_>>();
        keys.sort_by_key(|(_, at)| *at);
        let excess = caches.items.len() - ITEMS_MAX;
        for (key, _) in keys.into_iter().take(excess) {
            caches.items.remove(&key);
        }
    }

    // ── Disponibilidade entre provedores ──────────────────────────────────
    async fn load_availability(&self, appids: &[String]) -> Availability {
        let mut availability = Availability::default();
        if appids.is_empty() {
            return availability;
        }
        if let Some(catalog) = self.catalog.as_ref() {
            availability.sushi = self.sushi_ids().await;
            let path = format!(
                "/catalog/v1/manifests?appids={}",
                encode_uri_component(&appids.join(","))
            );
            let response = catalog.get(&path).await;
            if let Some(data) = response.data.and_then(|value| value.get("data").cloned()) {
                if let Some(object) = data.as_object() {
                    for (id, value) in object {
                        availability.geral.insert(id.clone(), value.clone());
                    }
                }
            }
        } else if self.availability_probe {
            availability.sushi = self.sushi_ids().await;
        }
        availability
    }

    async fn mark_availability(
        &self,
        games: &mut [StoreGame],
        from_hubcap: &HashSet<String>,
        availability: Availability,
        probe_sources: bool,
    ) {
        for game in games.iter_mut() {
            let mut sources = Vec::new();
            if from_hubcap.contains(&game.appid) {
                // O nome histórico do oráculo é "Morrenus" (hoje Hubcap).
                sources.push("Morrenus".to_string());
            }
            if let Some(entry) = availability
                .geral
                .get(&game.appid)
                .and_then(Value::as_object)
            {
                if url_ok(entry, &self.bases.sushi_url(&game.appid)) {
                    sources.push("Sushi".to_string());
                }
                if url_ok(entry, &self.bases.ryuu_url(&game.appid)) {
                    sources.push("Ryuu".to_string());
                }
                if url_ok(entry, &self.bases.twenty_two_url(&game.appid)) {
                    sources.push("TwentyTwo Cloud".to_string());
                }
            } else {
                if let Some(sushi) = availability.sushi.as_ref() {
                    if sushi.contains(&game.appid) {
                        sources.push("Sushi".to_string());
                    }
                } else if probe_sources
                    && (self.catalog.is_some() || self.availability_probe)
                    && self.existe(&self.bases.sushi_url(&game.appid), 6000).await
                {
                    sources.push("Sushi".to_string());
                }
                if probe_sources
                    && (self.catalog.is_some() || self.availability_probe)
                    && self.existe(&self.bases.ryuu_url(&game.appid), 6000).await
                {
                    sources.push("Ryuu".to_string());
                }
            }
            game.fontes = sources;
            game.manifest = !game.fontes.is_empty();
        }
    }

    /// `sushiIds()`: catálogo remoto -> árvore do GitHub, cache de 6 h.
    async fn sushi_ids(&self) -> Option<HashSet<String>> {
        let now = self.now();
        if let Some((at, ids)) = self.lock_caches().sushi.clone() {
            if now.saturating_sub(at) < SUSHI_TTL_MS {
                return Some(ids);
            }
        }
        if let Some(catalog) = self.catalog.as_ref() {
            let response = catalog.get("/catalog/v1/sushi").await;
            if let Some(ids) = response
                .data
                .and_then(|value| value.pointer("/data/ids").cloned())
                .and_then(|value| value.as_array().cloned())
            {
                let set = ids
                    .iter()
                    .map(|value| value_string_truthy(Some(value)))
                    .filter(|value| !value.is_empty())
                    .collect::<HashSet<_>>();
                self.lock_caches().sushi = Some((now, set.clone()));
                return Some(set);
            }
        }
        let Ok(data) = self
            .fetch_json(&self.bases.sushi_tree, Duration::from_secs(30), &[])
            .await
        else {
            return None;
        };
        if data.get("truncated").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let tree = data.get("tree").and_then(Value::as_array)?;
        let pattern = Regex::new(r"^(\d+)\.zip$").expect("regex de árvore Sushi");
        let ids = tree
            .iter()
            .filter_map(|node| node.get("path").and_then(Value::as_str))
            .filter_map(|path| pattern.captures(path))
            .map(|capture| capture[1].to_string())
            .collect::<HashSet<_>>();
        self.lock_caches().sushi = Some((now, ids.clone()));
        Some(ids)
    }

    /// `existe(url, timeoutMs)`: HEAD cacheado por 7 dias; erro não é cacheado.
    async fn existe(&self, url: &str, timeout_ms: u64) -> bool {
        let now = self.now();
        if let Some((at, ok)) = self.lock_caches().manifests.get(url).cloned() {
            if now.saturating_sub(at) < MANIFEST_TTL_MS {
                return ok;
            }
        }
        let request = Request::new(arcadia_catalog::http::Method::Head, url)
            .timeout(Duration::from_millis(timeout_ms))
            .header("User-Agent", USER_AGENT);
        match self.fetch_response(request).await {
            Ok(response) => {
                let ok = response.is_success();
                self.lock_caches()
                    .manifests
                    .insert(url.to_string(), (now, ok));
                ok
            }
            Err(_) => false,
        }
    }

    // ── store:recent / popular ────────────────────────────────────────────
    pub async fn recent(
        &self,
        lista: Option<&str>,
        limite: usize,
        offset: usize,
    ) -> RecentResponse {
        let lista = lista.unwrap_or("top100in2weeks");
        let limit = limite.max(1);
        let now = self.now();
        let cache_key = format!("{lista}:{offset}:{limit}");
        if let Some((at, cached)) = self.lock_caches().recent.get(&cache_key).cloned() {
            if now.saturating_sub(at) < RECENT_TTL_MS {
                let mut cached = cached;
                cached.cache = Some(true);
                return cached;
            }
        }

        if lista == "all" {
            let Some(catalog) = self.catalog.as_ref() else {
                return RecentResponse::failure("catálogo indisponível para a lista 'all'");
            };
            let path = format!("/catalog/v1/catalog?offset={offset}&limite={limit}");
            let response = catalog.get(&path).await;
            let Some(data) = response.data else {
                return RecentResponse::failure(
                    response
                        .error
                        .map(|error| error.message)
                        .unwrap_or_else(|| "catálogo vazio".to_string()),
                );
            };
            let items = data
                .get("itens")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let games = items
                .iter()
                .filter_map(game_from_catalog_item)
                .collect::<Vec<_>>();
            let total = data
                .get("total")
                .and_then(Value::as_u64)
                .map(|total| total as usize)
                .unwrap_or(games.len());
            let mut recent = RecentResponse {
                ok: true,
                jogos: None,
                offset: None,
                total: None,
                limit: None,
                has_more: None,
                next_offset: None,
                cache: None,
                revalidando: None,
                velho: None,
                error: None,
            };
            // A rota "all" já vem paginada: não fatia de novo.
            let has_more = offset + games.len() < total;
            recent.jogos = Some(games);
            recent.offset = Some(offset);
            recent.total = Some(total);
            recent.limit = Some(limit);
            recent.has_more = Some(has_more);
            recent.next_offset = Some(if has_more {
                json!(offset + recent.jogos.as_ref().map(Vec::len).unwrap_or(0))
            } else {
                Value::Null
            });
            recent.cache = response.fallback;
            self.lock_caches()
                .recent
                .insert(cache_key, (now, recent.clone()));
            return recent;
        }

        if lista != "top100in2weeks" {
            if let Some((complete, cached)) = self.genre_list(lista).await {
                let total = complete.len();
                let slice = complete
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                let prepared = self.prepare_fast(slice, &HashSet::new()).await;
                let mut recent = RecentResponse::failure("");
                recent.ok = true;
                recent.error = None;
                let mut recent = recent.page(prepared, offset, limit, total);
                if cached {
                    recent.cache = Some(true);
                }
                self.lock_caches()
                    .recent
                    .insert(cache_key, (now, recent.clone()));
                return recent;
            }
            return RecentResponse::failure("lista de gênero indisponível");
        }

        if let Some((complete, cached)) = self.popular_list().await {
            let total = complete.len();
            let slice = complete
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            let prepared = self.prepare_fast(slice, &HashSet::new()).await;
            let mut recent = RecentResponse::failure("");
            recent.ok = true;
            recent.error = None;
            let mut recent = recent.page(prepared, offset, limit, total);
            if cached {
                recent.cache = Some(true);
            }
            self.lock_caches()
                .recent
                .insert(cache_key, (now, recent.clone()));
            return recent;
        }
        RecentResponse::failure("SteamSpy indisponível")
    }

    async fn genre_list(&self, lista: &str) -> Option<(Vec<StoreGame>, bool)> {
        let now = self.now();
        if let Some((at, games)) = self.lock_caches().genre.get(lista).cloned() {
            if now.saturating_sub(at) < GENRE_TTL_MS {
                return Some((games, true));
            }
        }
        if let Some(catalog) = self.catalog.as_ref() {
            let path = format!("/catalog/v1/genre?lista={}", encode_uri_component(lista));
            let response = catalog.get(&path).await;
            if let Some(data) = response.data.and_then(|value| value.get("data").cloned()) {
                let list = data
                    .as_array()
                    .cloned()
                    .or_else(|| {
                        data.get("completa")
                            .or_else(|| data.get("jogos"))
                            .and_then(Value::as_array)
                            .cloned()
                    })
                    .unwrap_or_default();
                let games = list
                    .iter()
                    .filter_map(game_from_catalog_item)
                    .collect::<Vec<_>>();
                if !games.is_empty() {
                    self.lock_caches()
                        .genre
                        .insert(lista.to_string(), (now, games.clone()));
                    return Some((games, false));
                }
            }
        }
        let data = self
            .fetch_json(
                &self.bases.steamspy_url(lista),
                Duration::from_secs(25),
                &[],
            )
            .await
            .ok()?;
        let games = steamspy_games(&data, &self.bases.cdn_akamai);
        if games.is_empty() {
            return None;
        }
        self.lock_caches()
            .genre
            .insert(lista.to_string(), (now, games.clone()));
        Some((games, false))
    }

    async fn popular_list(&self) -> Option<(Vec<StoreGame>, bool)> {
        let now = self.now();
        if let Some((at, games)) = self.lock_caches().popular.get("top100in2weeks").cloned() {
            if now.saturating_sub(at) < POPULAR_TTL_MS {
                return Some((games, true));
            }
        }
        if let Some(catalog) = self.catalog.as_ref() {
            let response = catalog
                .get("/catalog/v1/popular?limite=1000&offset=0")
                .await;
            if let Some(items) = response
                .data
                .as_ref()
                .and_then(|value| value.get("itens"))
                .and_then(Value::as_array)
            {
                let games = items
                    .iter()
                    .filter_map(game_from_catalog_item)
                    .collect::<Vec<_>>();
                if !games.is_empty() {
                    self.lock_caches()
                        .popular
                        .insert("top100in2weeks".to_string(), (now, games.clone()));
                    return Some((games, false));
                }
            }
        }
        let data = self
            .fetch_json(
                &self.bases.steamspy_url("top100in2weeks"),
                Duration::from_secs(15),
                &[],
            )
            .await
            .ok()?;
        let games = steamspy_games(&data, &self.bases.cdn_akamai);
        if games.is_empty() {
            return None;
        }
        self.lock_caches()
            .popular
            .insert("top100in2weeks".to_string(), (now, games.clone()));
        Some((games, false))
    }

    // ── appdetails ────────────────────────────────────────────────────────
    /// `appdetails` cru; `None` quando a Steam responde `success:false`.
    pub async fn app_details(&self, appid: &str) -> Result<Option<AppDetails>, StoreError> {
        let appid = normalize_appid(appid);
        if !valid_appid(&appid) {
            return Ok(None);
        }
        let url =
            self.bases
                .appdetails_url(&appid, &self.config.country, self.config.steam_language());
        let data = self.fetch_json(&url, Duration::from_secs(20), &[]).await?;
        let entry = data.get(&appid);
        if entry.and_then(|entry| entry.get("success")).map(truthy) != Some(true) {
            return Ok(None);
        }
        let Some(info) = entry.and_then(|entry| entry.get("data")) else {
            return Ok(None);
        };
        Ok(Some(parse_app_details(&appid, info)))
    }

    /// Envelope de `game:sysinfo`: `{ ok:true, info }` / `{ ok:false, error }`.
    pub async fn app_details_response(&self, appid: &str) -> AppDetailsResponse {
        match self.app_details(appid).await {
            Ok(Some(info)) => AppDetailsResponse::success(info),
            Ok(None) => AppDetailsResponse::failure("appdetails sem dados"),
            Err(error) => AppDetailsResponse::failure(error.message),
        }
    }

    // ── store:warm / validação da chave ───────────────────────────────────
    /// `aquecer()`: aquece o DNS/TLS da busca com um termo curto. O oráculo
    /// também dispara o índice Sushi e HEADs em background; aqui só a busca é
    /// síncrona (as demais continuam opcionais via `availability_probe`).
    pub async fn warm(&self) -> Result<(), StoreError> {
        // Aquece exatamente as rotas que abrem Início e Loja. A busca antiga
        // (`?term=a`) era lenta e não alimentava nenhuma tela.
        //
        // Os limites têm que casar com o que a UI pede, senão o cache é
        // populado numa chave que ninguém consulta. O nativo abre a Home com
        // 24 (`load_home_recent(24)`); aquecer 40 gravava
        // `top100in2weeks:0:40` e a Home continuava pagando rede por
        // `top100in2weeks:0:24` — medido: 3.810 ms no boot frio e 2.582 ms no
        // quente, com o cache no disco sem ser lido.
        let _ = self.popular_list().await;
        let _ = self.recent(Some("top100in2weeks"), 24, 0).await;
        if let Some(catalog) = self.catalog.as_ref() {
            let _ = catalog.get("/catalog/v1/catalog?offset=0&limite=24").await;
        }
        // Página pronta da Loja (StoreView pede 24).
        let _ = self.recent(Some("all"), 24, 0).await;
        Ok(())
    }
    /// Uso da chave: usuário, pedidos de hoje e limite diário.
    ///
    /// A chave vai no header, nunca na URL. 401/403 viram `rejected`; falha
    /// de rede e 5xx viram `unreachable`, como o `GetHubcapStats` do LuaTools.
    pub async fn hubcap_stats(&self, key: &str) -> HubcapStats {
        let key = key.trim();
        if key.is_empty() {
            return HubcapStats {
                error: Some("sem_chave".to_string()),
                ..HubcapStats::default()
            };
        }
        let auth = format!("Bearer {key}");
        match self
            .fetch_response(
                Request::get(HUBCAP_STATS)
                    .timeout(Duration::from_secs(10))
                    .header("User-Agent", USER_AGENT)
                    .header("Authorization", auth),
            )
            .await
        {
            Ok(response) if response.status == 200 => parse_hubcap_stats(&response.text_body()),
            Ok(response) if matches!(response.status, 401 | 403) => HubcapStats {
                error: Some("rejected".to_string()),
                ..HubcapStats::default()
            },
            Ok(_) => HubcapStats {
                error: Some("unreachable".to_string()),
                ..HubcapStats::default()
            },
            Err(_) => HubcapStats {
                error: Some("unreachable".to_string()),
                ..HubcapStats::default()
            },
        }
    }

    /// Baixa o zip de manifesto do Hubcap e devolve o `.lua` do appid.
    ///
    /// É o arquivo que o luatools-moon publica em `config/stplug-in`: começa
    /// com `addappid(appid)` e traz as chaves de depot e os `setManifestid`.
    /// Sem chave configurada devolve `None` em vez de erro.
    pub async fn hubcap_manifest_lua(&self, appid: &str) -> Result<Option<String>, StoreError> {
        let Some(bytes) = self.hubcap_manifest_zip(appid).await? else { return Ok(None) };
        lua_from_zip(&bytes, appid).map(Some)
    }

    pub async fn hubcap_install_manifest(&self, appid: &str) -> Result<Option<crate::depot::ProviderArchive>, StoreError> {
        let Some(bytes) = self.hubcap_manifest_zip(appid).await? else { return Ok(None) };
        crate::depot::parse_provider_archive(&bytes, appid).map(Some).map_err(StoreError::new)
    }

    async fn hubcap_manifest_zip(&self, appid: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let appid = appid.trim();
        if !valid_appid(appid) {
            return Err(StoreError::new("appid inválido"));
        }
        let Some(key) = self.config.hubcap_api_key.as_deref() else {
            return Ok(None);
        };
        let url = self.bases.hubcap_manifest_url(appid);
        let auth = format!("Bearer {key}");
        let response = self
            .fetch_response(
                Request::get(url)
                    .timeout(Duration::from_secs(30))
                    .header("User-Agent", USER_AGENT)
                    .header("Authorization", auth),
            )
            .await?;
        if matches!(response.status, 401 | 403) {
            return Err(StoreError::new("chave_invalida"));
        }
        if !response.is_success() {
            return Err(StoreError::new(format!("HTTP {}", response.status)));
        }
        Ok(Some(response.body))
    }

    /// `validarChaveHubcap`: decide pelo status, sem baixar o zip.
    pub async fn validate_hubcap_key(&self, key: &str) -> HubcapKeyValidation {
        let key = key.trim();
        if key.is_empty() {
            return HubcapKeyValidation {
                ok: false,
                motivo: Some("sem_chave".to_string()),
                ..HubcapKeyValidation::default()
            };
        }
        let url = self.bases.hubcap_manifest_url("440");
        let auth = format!("Bearer {key}");
        match self
            .fetch_response(
                Request::get(url)
                    .timeout(Duration::from_secs(15))
                    .header("User-Agent", USER_AGENT)
                    .header("Authorization", auth),
            )
            .await
        {
            Ok(response) if response.status == 200 => HubcapKeyValidation {
                ok: true,
                status: Some(200),
                ..HubcapKeyValidation::default()
            },
            Ok(response) if matches!(response.status, 401 | 403) => HubcapKeyValidation {
                ok: false,
                status: Some(response.status),
                motivo: Some("chave_invalida".to_string()),
                erro: None,
            },
            Ok(response) => HubcapKeyValidation {
                ok: false,
                status: Some(response.status),
                motivo: Some(format!("http_{}", response.status)),
                erro: None,
            },
            Err(error) => HubcapKeyValidation {
                ok: false,
                motivo: Some("rede".to_string()),
                erro: Some(error.message),
                ..HubcapKeyValidation::default()
            },
        }
    }

    /// `itensDaLoja` conveniente para a borda (resolve capa/hero de um jogo).
    pub async fn item_for(&self, appid: &str) -> Option<StoreItem> {
        let ids = vec![normalize_appid(appid)];
        self.items_for(&ids).await.mapa.remove(&ids[0])
    }

    /// Alias com o nome do oráculo (`itensDaLoja`).
    pub async fn itens_da_loja(&self, appids: &[String]) -> StoreItemsResult {
        self.items_for(appids).await
    }

    /// Alias com o nome do oráculo (`popular`).
    pub async fn popular(
        &self,
        lista: Option<&str>,
        limite: usize,
        offset: usize,
    ) -> RecentResponse {
        self.recent(lista, limite, offset).await
    }

    /// Alias de `appdetails` no formato do envelope de `game:sysinfo`.
    pub async fn appdetails(&self, appid: &str) -> AppDetailsResponse {
        self.app_details_response(appid).await
    }
}

#[derive(Debug, Default)]
struct Availability {
    sushi: Option<HashSet<String>>,
    geral: HashMap<String, Value>,
}

// ── Parsers puros ──────────────────────────────────────────────────────────

/// Extrai appid de `steam:730`, `730`, número JSON ou objeto `{appid}`.
pub fn normalize_appid(value: &str) -> String {
    let trimmed = value.trim();
    let stripped = trimmed.strip_prefix("steam:").unwrap_or(trimmed);
    stripped.trim().to_string()
}

/// Appid Steam válido: não vazio e só dígitos. Evita injeção em URLs, nomes de
/// ACF e caminhos de `stplug-in`.
pub fn valid_appid(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

fn parse_hubcap_stats(body: &str) -> HubcapStats {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return HubcapStats {
            error: Some("unreachable".to_string()),
            ..HubcapStats::default()
        };
    };
    let number = |key: &str| {
        value.get(key).and_then(|item| {
            item.as_u64()
                .or_else(|| item.as_i64().map(|value| value.max(0) as u64))
                .or_else(|| item.as_str().and_then(|text| text.parse().ok()))
        })
    };
    HubcapStats {
        ok: true,
        username: value
            .get("username")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        daily_usage: number("daily_usage"),
        daily_limit: number("daily_limit"),
        error: None,
    }
}

/// Lê o `.lua` do appid dentro do zip do Hubcap.
///
/// O zip pode trazer `{appid}.lua` ou o script dentro de uma pasta. Se não
/// houver um arquivo com o nome do jogo, usa o primeiro `.lua`.
fn lua_from_zip(bytes: &[u8], appid: &str) -> Result<String, StoreError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|error| StoreError::new(format!("zip do Hubcap ilegível: {error}")))?;
    let wanted = format!("{appid}.lua");
    let mut fallback = None;
    let mut chosen = None;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| StoreError::new(format!("zip do Hubcap ilegível: {error}")))?;
        if !file.name().ends_with(".lua") {
            continue;
        }
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text)
            .map_err(|error| StoreError::new(format!("lua do Hubcap ilegível: {error}")))?;
        if file.name().ends_with(&wanted) {
            chosen = Some(text);
            break;
        }
        if fallback.is_none() {
            fallback = Some(text);
        }
    }
    chosen
        .or(fallback)
        .filter(|text| text.contains("addappid"))
        .ok_or_else(|| StoreError::new("o zip do Hubcap não trouxe um .lua"))
}

/// Extrai appid de um valor JSON: número, `"730"`, `"steam:730"`.
pub fn extract_appid(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    let appid = normalize_appid(&raw);
    if appid.is_empty() {
        None
    } else {
        Some(appid)
    }
}

/// `String(valor || "")` para ids/nomes de JSON.
fn value_string_truthy(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(true)) => "true".to_string(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => String::new(),
    }
}

/// `Boolean(valor)` do JS.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn game_from_catalog_item(item: &Value) -> Option<StoreGame> {
    let appid = value_string_truthy(item.get("appid"));
    let title = value_string_truthy(item.get("title"));
    if appid.is_empty() || title.is_empty() {
        return None;
    }
    Some(StoreGame {
        cover: string_field(item, "cover"),
        capa: item.get("capa").and_then(Value::as_str).map(str::to_string),
        heroi: item
            .get("heroi")
            .and_then(Value::as_str)
            .map(str::to_string),
        manifest: item.get("manifest").map(truthy).unwrap_or(false),
        ..StoreGame::new(appid, title)
    })
}

fn search_items(data: &Value, cdn: &str) -> Vec<StoreGame> {
    data.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let appid = value_string_truthy(item.get("id"));
                    let title = value_string_truthy(item.get("name"));
                    if appid.is_empty() || title.is_empty() {
                        return None;
                    }
                    let tiny = value_string_truthy(item.get("tiny_image"));
                    Some(StoreGame {
                        cover: if tiny.is_empty() {
                            format!("{}/{}/header.jpg", cdn.trim_end_matches('/'), appid)
                        } else {
                            tiny
                        },
                        preco: search_price(item.get("price")),
                        ..StoreGame::new(appid, title)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn hubcap_games(data: &Value) -> Vec<StoreGame> {
    data.get("games")
        .and_then(Value::as_array)
        .map(|games| {
            games
                .iter()
                .filter_map(|game| {
                    let appid = value_string_truthy(game.get("game_id"));
                    let title = value_string_truthy(game.get("game_name"));
                    if appid.is_empty() || title.is_empty() {
                        return None;
                    }
                    Some(StoreGame {
                        cover: value_string_truthy(game.get("header_image")),
                        manifest: game.get("manifest_available").map(truthy).unwrap_or(false),
                        ..StoreGame::new(appid, title)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn upsert_game(games: &mut Vec<StoreGame>, seen: &mut HashSet<String>, game: StoreGame) {
    if let Some(index) = games.iter().position(|current| current.appid == game.appid) {
        games[index] = game;
    } else {
        seen.insert(game.appid.clone());
        games.push(game);
    }
}

/// `relevancia(titulo, q)`: exato 0, prefixo 1, palavra 2, contém 3, resto 4.
pub fn relevance(title: &str, query: &str) -> u8 {
    let title = title.to_lowercase();
    let title = title.trim();
    let query = query.to_lowercase();
    let query = query.trim();
    if title == query {
        return 0;
    }
    if title.starts_with(query) {
        return 1;
    }
    let pattern = format!(r"\b{}", regex::escape(query));
    if Regex::new(&pattern)
        .map(|regex| regex.is_match(title))
        .unwrap_or(false)
    {
        return 2;
    }
    if title.contains(query) {
        return 3;
    }
    4
}

/// Conteúdo acessório (OST, demo, artbook…): vai para o fim, nunca é escondido.
pub fn is_accessory(title: &str) -> bool {
    static ACCESSORY: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    ACCESSORY
        .get_or_init(|| {
            Regex::new(
                r"(?i)\b(ost|soundtrack|artbook|art book|digital (comic|artbook|deluxe)|wallpaper|demo|playtest|beta|trilha sonora|skin|costume|avatar|emote|upgrade pack)\b",
            )
            .expect("regex de acessório")
        })
        .is_match(title)
}

/// `ordenar(jogos, q)`: acessório, relevância do título, manifesto.
pub fn sort_games(games: &mut [StoreGame], query: &str) {
    games.sort_by(|a, b| {
        let accessory_a = is_accessory(&a.title);
        let accessory_b = is_accessory(&b.title);
        if accessory_a != accessory_b {
            return accessory_a.cmp(&accessory_b);
        }
        relevance(&a.title, query)
            .cmp(&relevance(&b.title, query))
            .then_with(|| b.manifest.cmp(&a.manifest))
    });
}

/// Preço do `storesearch` (`{ currency, initial, final }`) formatado.
pub fn search_price(price: Option<&Value>) -> String {
    let Some(price) = price else {
        return String::new();
    };
    let cents = price.get("final").and_then(Value::as_f64);
    let currency = price.get("currency").and_then(Value::as_str);
    match (cents, currency) {
        (Some(cents), Some(currency)) => format_price(cents, currency),
        _ => String::new(),
    }
}

/// `precoDestaque`: `0` vira "Gratuito"; sem moeda/valor, vazio.
///
/// O `Intl.NumberFormat(undefined, …)` do Electron depende do locale do
/// sistema. Aqui BRL/USD/EUR têm formato fixo e as demais moedas caem num
/// fallback estável.
pub fn format_price(cents: f64, currency: &str) -> String {
    if currency.is_empty() || !cents.is_finite() {
        return String::new();
    }
    if cents == 0.0 {
        return "Gratuito".to_string();
    }
    let negative = cents < 0.0;
    let rounded_cents = cents.abs().round() as u64;
    let major = rounded_cents / 100;
    let minor = (rounded_cents % 100) as u32;
    let grouped = group_thousands(major as i64, '.');
    let sign = if negative { "-" } else { "" };
    match currency.to_uppercase().as_str() {
        "BRL" => format!("{sign}R$ {grouped},{minor:02}"),
        "USD" => format!("{sign}${}.{minor:02}", group_thousands(major as i64, ',')),
        "EUR" => format!("{sign}€ {grouped},{minor:02}"),
        other => format!("{sign}{grouped}.{minor:02} {other}"),
    }
}

fn group_thousands(value: i64, separator: char) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(character);
    }
    out
}

fn url_ok(entry: &Map<String, Value>, url: &str) -> bool {
    entry
        .get(url)
        .and_then(|value| value.get("ok"))
        .map(truthy)
        .unwrap_or(false)
}

fn steamspy_games(data: &Value, cdn: &str) -> Vec<StoreGame> {
    let mut values = data
        .as_object()
        .map(|object| object.values().collect::<Vec<_>>())
        .unwrap_or_default();
    values.sort_by(|a, b| {
        let a = a.get("ccu").and_then(Value::as_f64).unwrap_or(0.0);
        let b = b.get("ccu").and_then(Value::as_f64).unwrap_or(0.0);
        b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
    });
    values
        .into_iter()
        .filter_map(|game| {
            let appid = value_string_truthy(game.get("appid"));
            let title = value_string_truthy(game.get("name"));
            if appid.is_empty() || title.is_empty() {
                return None;
            }
            Some(StoreGame {
                cover: format!("{}/{}/header.jpg", cdn.trim_end_matches('/'), appid),
                manifest: false,
                ..StoreGame::new(appid, title)
            })
        })
        .collect()
}

/// `semHTML` de `metadata.js`: tags, entidades e colapso de quebras.
pub fn strip_html(html: &str) -> String {
    static BR: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static P: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static TAGS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static NEWLINES: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let br = BR.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").expect("regex br"));
    let p = P.get_or_init(|| Regex::new(r"(?i)</p>").expect("regex p"));
    let tags = TAGS.get_or_init(|| Regex::new(r"<[^>]+>").expect("regex tags"));
    let newlines = NEWLINES.get_or_init(|| Regex::new(r"\n{3,}").expect("regex newlines"));
    let text = br.replace_all(html, "\n");
    let text = p.replace_all(&text, "\n");
    let text = tags.replace_all(&text, "");
    let text = text
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    newlines.replace_all(&text, "\n\n").trim().to_string()
}

fn image_count(html: &str) -> usize {
    html.match_indices("<img").count()
}

fn parse_app_details(appid: &str, info: &Value) -> AppDetails {
    let about_the_game = string_field(info, "about_the_game");
    let detailed = string_field(info, "detailed_description");
    let about = if image_count(&detailed) > image_count(&about_the_game) {
        detailed
    } else if !about_the_game.is_empty() {
        about_the_game
    } else {
        detailed
    };
    let reqs = info.get("pc_requirements");
    let (req_min, req_rec) = match reqs {
        Some(Value::Object(object)) => (
            string_field(&Value::Object(object.clone()), "minimum"),
            string_field(&Value::Object(object.clone()), "recommended"),
        ),
        _ => (String::new(), String::new()),
    };
    let screenshots = info
        .get("screenshots")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .take(12)
                .map(|shot| AppScreenshot {
                    thumb: string_field(shot, "path_thumbnail"),
                    full: string_field(shot, "path_full"),
                })
                .collect()
        })
        .unwrap_or_default();
    let movies = info
        .get("movies")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .take(6)
                .map(|movie| {
                    let mp4 = movie.get("mp4");
                    let webm = movie.get("webm");
                    let pick = |value: Option<&Value>| {
                        value
                            .and_then(|value| value.get("max").or_else(|| value.get("480")))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string()
                    };
                    AppMovie {
                        id: value_string_truthy(movie.get("id")),
                        name: string_field(movie, "name"),
                        thumb: string_field(movie, "thumbnail"),
                        mp4: pick(mp4),
                        webm: pick(webm),
                        hls: string_field(movie, "hls_h264"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    AppDetails {
        appid: appid.to_string(),
        genres: info.get("genres").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|genre| genre.get("description").and_then(Value::as_str).map(str::to_string)).collect(),
        short_description: string_field(info, "short_description"),
        about,
        publishers: string_array(info.get("publishers")),
        developers: string_array(info.get("developers")),
        release_date: info
            .get("release_date")
            .and_then(|value| value.get("date"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        controller_support: string_field(info, "controller_support"),
        languages: string_field(info, "supported_languages"),
        header: string_field(info, "header_image"),
        background: {
            let raw = string_field(info, "background_raw");
            if raw.is_empty() {
                string_field(info, "background")
            } else {
                raw
            }
        },
        req_min,
        req_rec,
        screenshots,
        movies,
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
