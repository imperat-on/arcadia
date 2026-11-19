//! Composição dos provedores: os handlers `meta:art` e `meta:text`.
//!
//! `app/electron/main.js` junta as fontes numa lista só e converte falha de
//! provedor em item de `erros`, mantendo as demais. Este módulo reproduz essa
//! composição com os clientes de [`crate::providers`], sem IPC nem filesystem:
//!
//! - `meta:art`: Steam → SteamGridDB → IGDB → Xbox → Wallhaven (só hero) →
//!   PS Store;
//! - `meta:text`: Steam (com appid resolvido por título) → Xbox → IGDB.
//!
//! A `MediaConfig` carrega a chave da SteamGridDB (sempre vinda de configuração
//! ou argumento), o idioma, o mercado Xbox e a base opcional do Worker. As
//! bases HTTP podem ser trocadas por [`MediaEndpoints`] para os testes usarem
//! um servidor local.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::art::{
    art_cache_base as build_art_cache_base, download_to, ArtResolver, DownloadedArt, LogoArt,
    SquareArt,
};
use crate::error::{MediaError, Result};
use crate::http::Fetcher;
use crate::providers::igdb::{igdb_art_de, igdb_textos_de, IgdbClient, IgdbSearch, IGDB_PROXY};
use crate::providers::psn::{best_psn_result, PsnClient, PSN_GQL_OP, PSN_STORE};
use crate::providers::steam::{SteamClient, STEAM_CDN, STEAM_SEARCH, STEAM_STORE};
use crate::providers::steamgriddb::{SgdbArtOptions, SteamGridDb, SGDB_BASE};
use crate::providers::wallhaven::{WallhavenClient, WALLHAVEN_BASE, WALLHAVEN_DEFAULT_ATLEAST};
use crate::providers::xbox::{xbox_art_de, xbox_texto_de, XboxClient, XBOX_CATALOG, XBOX_SEARCH};
use crate::providers::{ArtKind, Artwork, TextCandidate};
use crate::text::titles_match;

/// Mapa de idioma do app para o parâmetro `l` da Steam (`main.js`).
pub fn steam_language_for(locale: &str) -> &'static str {
    match locale {
        "pt-BR" => "portuguese",
        "es-ES" => "spanish",
        _ => "english",
    }
}

/// `xboxLocale(cfg)`: o mercado vem da config (BR por padrão) e o idioma é
/// limitado aos três que o app oferece.
pub fn xbox_locale_for(locale: &str, market: Option<&str>) -> (String, String) {
    let language = match locale {
        "pt-BR" => "pt-br",
        "es-ES" => "es-es",
        _ => "en-us",
    };
    let market = market
        .map(str::trim)
        .filter(|market| !market.is_empty())
        .unwrap_or("BR");
    (market.to_string(), language.to_string())
}

/// `^retro:([^:]+)` do `meta:art`: plataforma retrô vira dica de score da IGDB.
pub fn retro_platform(game_id: &str) -> Option<String> {
    let rest = game_id.strip_prefix("retro:")?;
    let platform = rest.split(':').next().unwrap_or("");
    if platform.is_empty() {
        None
    } else {
        Some(platform.to_string())
    }
}

/// Configuração da camada de mídia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaConfig {
    /// Chave da SteamGridDB; ausente/vazia desliga a fonte, como no JS.
    pub steamgriddb_api_key: Option<String>,
    /// Base do Worker `/arte-quadrada` (opcional).
    pub base_worker: Option<String>,
    /// Idioma da UI (`pt-BR`, `en-US`, `es-ES`).
    pub language: String,
    /// Mercado da Xbox (`BR`, `US`…); vazio cai em `BR`.
    pub xbox_market: Option<String>,
    /// Pasta `art/` do diretório de dados (opcional).
    pub art_dir: Option<PathBuf>,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            steamgriddb_api_key: None,
            base_worker: None,
            language: "en-US".to_string(),
            xbox_market: None,
            art_dir: None,
        }
    }
}

impl MediaConfig {
    pub fn steam_language(&self) -> &'static str {
        steam_language_for(&self.language)
    }

    pub fn xbox_locale(&self) -> (String, String) {
        xbox_locale_for(&self.language, self.xbox_market.as_deref())
    }

    /// Chave efetiva: `trim()` e vazio desligam a SGDB (como `String(...).trim()`).
    pub fn sgdb_key(&self) -> Option<&str> {
        self.steamgriddb_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }
}

/// Bases HTTP dos provedores. Os defaults são os endpoints congelados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEndpoints {
    pub sgdb: String,
    pub steam_cdn: String,
    pub steam_store: String,
    pub steam_search: String,
    pub igdb_proxy: String,
    pub xbox_search: String,
    pub xbox_catalog: String,
    pub wallhaven: String,
    pub psn_store: String,
    pub psn_graphql: String,
}

impl Default for MediaEndpoints {
    fn default() -> Self {
        Self {
            sgdb: SGDB_BASE.to_string(),
            steam_cdn: STEAM_CDN.to_string(),
            steam_store: STEAM_STORE.to_string(),
            steam_search: STEAM_SEARCH.to_string(),
            igdb_proxy: IGDB_PROXY.to_string(),
            xbox_search: XBOX_SEARCH.to_string(),
            xbox_catalog: XBOX_CATALOG.to_string(),
            wallhaven: WALLHAVEN_BASE.to_string(),
            psn_store: PSN_STORE.to_string(),
            psn_graphql: PSN_GQL_OP.to_string(),
        }
    }
}

/// Pedido de `meta:art`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtRequest {
    pub game_id: String,
    pub title: String,
    pub kind: ArtKind,
    /// Id da SGDB já resolvido (evita a busca por título).
    pub sgdb_id: Option<u64>,
    /// Filtro de dimensões pedido pelo usuário.
    pub dimensions: Vec<String>,
}

impl ArtRequest {
    pub fn new(game_id: impl Into<String>, title: impl Into<String>, kind: ArtKind) -> Self {
        Self {
            game_id: game_id.into(),
            title: title.into(),
            kind,
            sgdb_id: None,
            dimensions: Vec::new(),
        }
    }

    pub fn with_sgdb_id(mut self, sgdb_id: u64) -> Self {
        self.sgdb_id = Some(sgdb_id);
        self
    }

    pub fn with_dimensions<I, S>(mut self, dimensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.dimensions = dimensions.into_iter().map(Into::into).collect();
        self
    }
}

/// Resultado de `meta:art` (o `ok` fica implícito no `Result`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtSearch {
    pub candidatos: Vec<Artwork>,
    pub jogos: Vec<crate::providers::steamgriddb::SgdbGame>,
    pub erros: Vec<String>,
}

/// Resultado de `meta:text`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSearch {
    pub textos: Vec<TextCandidate>,
    pub erros: Vec<String>,
}

/// Fachada que liga todos os provedores na ordem do cliente congelado.
pub struct MediaService {
    fetcher: Arc<dyn Fetcher>,
    config: MediaConfig,
    endpoints: MediaEndpoints,
    steam: SteamClient,
    igdb: IgdbClient,
    xbox: XboxClient,
    psn: PsnClient,
    wallhaven: WallhavenClient,
    sgdb: Option<SteamGridDb>,
}

impl MediaService {
    pub fn new(fetcher: Arc<dyn Fetcher>, config: MediaConfig) -> Self {
        Self::with_endpoints(fetcher, config, MediaEndpoints::default())
    }

    pub fn with_endpoints(
        fetcher: Arc<dyn Fetcher>,
        config: MediaConfig,
        endpoints: MediaEndpoints,
    ) -> Self {
        let steam = SteamClient::new(Arc::clone(&fetcher))
            .with_cdn_base(endpoints.steam_cdn.clone())
            .with_store_base(endpoints.steam_store.clone())
            .with_search_base(endpoints.steam_search.clone());
        let igdb = IgdbClient::new(Arc::clone(&fetcher)).with_base(endpoints.igdb_proxy.clone());
        let xbox = XboxClient::new(Arc::clone(&fetcher))
            .with_search_base(endpoints.xbox_search.clone())
            .with_catalog_base(endpoints.xbox_catalog.clone());
        let psn = PsnClient::new(Arc::clone(&fetcher))
            .with_store_base(endpoints.psn_store.clone())
            .with_graphql_base(endpoints.psn_graphql.clone());
        let wallhaven =
            WallhavenClient::new(Arc::clone(&fetcher)).with_base(endpoints.wallhaven.clone());
        let sgdb = config.sgdb_key().map(|key| {
            SteamGridDb::new(Arc::clone(&fetcher), key.to_string())
                .with_base(endpoints.sgdb.clone())
        });
        Self {
            fetcher,
            config,
            endpoints,
            steam,
            igdb,
            xbox,
            psn,
            wallhaven,
            sgdb,
        }
    }

    pub fn config(&self) -> &MediaConfig {
        &self.config
    }

    pub fn endpoints(&self) -> &MediaEndpoints {
        &self.endpoints
    }

    pub fn fetcher(&self) -> &Arc<dyn Fetcher> {
        &self.fetcher
    }

    pub fn steam(&self) -> &SteamClient {
        &self.steam
    }

    pub fn igdb(&self) -> &IgdbClient {
        &self.igdb
    }

    pub fn xbox(&self) -> &XboxClient {
        &self.xbox
    }

    pub fn psn(&self) -> &PsnClient {
        &self.psn
    }

    pub fn wallhaven(&self) -> &WallhavenClient {
        &self.wallhaven
    }

    pub fn sgdb(&self) -> Option<&SteamGridDb> {
        self.sgdb.as_ref()
    }

    /// Junta todos os candidatos de arte, exatamente como `meta:art`.
    ///
    /// Falha de uma fonte vira uma linha em `erros`; as demais continuam.
    /// `pedido inválido` (sem `game_id`) é o mesmo curto-circuito do handler.
    pub async fn art(&self, request: &ArtRequest) -> Result<ArtSearch> {
        if request.game_id.is_empty() {
            return Err(MediaError::message("pedido inválido"));
        }
        let mut result = ArtSearch::default();

        // Steam: arte oficial, sem chave. Só existe para jogos da Steam.
        match self.steam.art(&request.game_id, request.kind).await {
            Ok(artworks) => result.candidatos.extend(artworks),
            Err(error) => result.erros.push(format!("Steam: {error}")),
        }

        // SteamGridDB: arte da comunidade, qualquer loja, inclui animados.
        if let Some(sgdb) = self.sgdb.as_ref() {
            let id = match request.sgdb_id {
                Some(id) => Some(id),
                None => match sgdb.search(&request.title).await {
                    Ok(games) => {
                        let id = games.first().map(|game| game.id);
                        result.jogos = games;
                        id
                    }
                    Err(error) => {
                        result.erros.push(format!("SteamGridDB: {error}"));
                        None
                    }
                },
            };
            if let Some(id) = id {
                let options = SgdbArtOptions::default().with_dimensions(request.dimensions.clone());
                match sgdb.art(id, request.kind, &options).await {
                    Ok(artworks) => result.candidatos.extend(artworks),
                    Err(error) => result.erros.push(format!("SteamGridDB: {error}")),
                }
            }
        } else {
            result
                .erros
                .push("SteamGridDB: sem chave de API (defina nas Configurações)".to_string());
        }

        // IGDB: arte de qualquer plataforma. A plataforma retrô melhora o score.
        let mut igdb_search = IgdbSearch::new(request.title.clone());
        if let Some(platform) = retro_platform(&request.game_id) {
            igdb_search = igdb_search.with_platform(platform);
        }
        match self.igdb.search(&igdb_search).await {
            Ok(games) => result.candidatos.extend(igdb_art_de(&games, request.kind)),
            Err(error) => result.erros.push(format!("IGDB: {error}")),
        }

        // Xbox: catálogo público, sem chave.
        let (market, locale) = self.config.xbox_locale();
        match self.xbox.search(&request.title, &market, &locale).await {
            Ok(games) => {
                if let Some(first) = games.first() {
                    match self.xbox.product(&first.id, &market, &locale).await {
                        Ok(Some(product)) => result
                            .candidatos
                            .extend(xbox_art_de(&product, request.kind)),
                        Ok(None) => {}
                        Err(error) => result.erros.push(format!("Xbox: {error}")),
                    }
                }
            }
            Err(error) => result.erros.push(format!("Xbox: {error}")),
        }

        // Wallhaven: só para o fundo (hero).
        if request.kind == ArtKind::Hero {
            match self
                .wallhaven
                .search(&request.title, WALLHAVEN_DEFAULT_ATLEAST)
                .await
            {
                Ok(artworks) => result.candidatos.extend(artworks),
                Err(error) => result.erros.push(format!("Wallhaven: {error}")),
            }
        }

        // PS Store: arte oficial da PlayStation (pública, por scraping).
        match self.psn.store_search(&request.title).await {
            Ok(tiles) => {
                if let Some(best) = best_psn_result(&tiles, &request.title) {
                    match self.psn.store_art(&best.id, &best.tipo, request.kind).await {
                        Ok(artworks) => result.candidatos.extend(artworks),
                        Err(error) => result.erros.push(format!("PS Store: {error}")),
                    }
                }
            }
            Err(error) => result.erros.push(format!("PS Store: {error}")),
        }

        Ok(result)
    }

    /// Junta as descrições na ordem de qualidade: Steam, Xbox, IGDB.
    pub async fn text(&self, game_id: &str, title: &str) -> TextSearch {
        let mut result = TextSearch::default();

        match self.steam_texts(game_id, title).await {
            Ok(texts) => result.textos.extend(texts),
            Err(error) => result.erros.push(format!("Steam: {error}")),
        }

        let (market, locale) = self.config.xbox_locale();
        match self.xbox.search(title, &market, &locale).await {
            Ok(games) => {
                if let Some(first) = games.first() {
                    match self.xbox.product(&first.id, &market, &locale).await {
                        Ok(Some(product)) => result.textos.extend(xbox_texto_de(&product)),
                        Ok(None) => {}
                        Err(error) => result.erros.push(format!("Xbox: {error}")),
                    }
                }
            }
            Err(error) => result.erros.push(format!("Xbox: {error}")),
        }

        match self.igdb.search(&IgdbSearch::new(title)).await {
            Ok(games) => result.textos.extend(igdb_textos_de(&games)),
            Err(error) => result.erros.push(format!("IGDB: {error}")),
        }

        result
    }

    /// Steam do `meta:text`: para jogo fora da Steam, resolve o appid por
    /// título e passa como fallback (com o mesmo `tituloBate` do JS).
    async fn steam_texts(&self, game_id: &str, title: &str) -> Result<Vec<TextCandidate>> {
        let mut appid = String::new();
        if !game_id.starts_with("steam:") && !title.is_empty() {
            let suggestions = self
                .steam
                .suggest(title, "br", self.config.steam_language())
                .await?;
            appid = suggestions
                .iter()
                .find(|item| titles_match(&item.title, title))
                .map(|item| item.appid.clone())
                .unwrap_or_default();
        }
        self.steam
            .textos(game_id, self.config.steam_language(), &appid)
            .await
    }

    /// `arteQuadrada`: PSN → Worker → Xbox → SGDB.
    pub async fn square_art(&self, title: &str) -> Option<SquareArt> {
        let mut resolver = ArtResolver::new(Arc::clone(&self.fetcher))
            .with_psn(self.psn.clone())
            .with_xbox(self.xbox.clone());
        if let Some(base) = self
            .config
            .base_worker
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
        {
            resolver = resolver.with_worker_base(base);
        }
        if let Some(sgdb) = self.sgdb.clone() {
            resolver = resolver.with_sgdb(sgdb);
        }
        resolver.square_art(title).await
    }

    /// `logoAuto`: PSN → SGDB.
    pub async fn logo(&self, title: &str) -> Option<LogoArt> {
        let mut resolver = ArtResolver::new(Arc::clone(&self.fetcher))
            .with_psn(self.psn.clone())
            .with_xbox(self.xbox.clone());
        if let Some(sgdb) = self.sgdb.clone() {
            resolver = resolver.with_sgdb(sgdb);
        }
        resolver.logo(title).await
    }

    /// Baixa a arte escolhida para `dest_without_extension` (o chamador cria a
    /// pasta `art/` e passa a base `{safeId}-{kind}-{timestamp}`).
    pub async fn download_art(
        &self,
        url: &str,
        dest_without_extension: impl AsRef<Path>,
    ) -> Result<DownloadedArt> {
        download_to(self.fetcher.as_ref(), url, dest_without_extension).await
    }

    /// Caminho de cache da arte quando `art_dir` foi configurado.
    pub fn art_cache_base(&self, id: &str, kind: ArtKind, timestamp_ms: u64) -> Option<PathBuf> {
        self.config
            .art_dir
            .as_ref()
            .map(|dir| build_art_cache_base(dir, id, kind, timestamp_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};
    use serde_json::json;

    fn config_with_key(key: &str) -> MediaConfig {
        MediaConfig {
            steamgriddb_api_key: Some(key.to_string()),
            ..MediaConfig::default()
        }
    }

    #[test]
    fn language_and_market_helpers_match_main_js() {
        assert_eq!(steam_language_for("pt-BR"), "portuguese");
        assert_eq!(steam_language_for("en-US"), "english");
        assert_eq!(steam_language_for("es-ES"), "spanish");
        assert_eq!(steam_language_for("fr-FR"), "english");
        assert_eq!(
            xbox_locale_for("pt-BR", Some("US")),
            ("US".to_string(), "pt-br".to_string())
        );
        assert_eq!(
            xbox_locale_for("fr-FR", None),
            ("BR".to_string(), "en-us".to_string())
        );
        assert_eq!(
            xbox_locale_for("es-ES", Some("   ")),
            ("BR".to_string(), "es-es".to_string())
        );
    }

    #[test]
    fn retro_platform_uses_the_same_capture() {
        assert_eq!(
            retro_platform("retro:sony-psp:1").as_deref(),
            Some("sony-psp")
        );
        assert_eq!(retro_platform("steam:10"), None);
        assert_eq!(retro_platform("retro:"), None);
    }

    #[test]
    fn config_key_is_trimmed_and_empty_disables_sgdb() {
        assert_eq!(config_with_key(" key ").sgdb_key(), Some("key"));
        assert_eq!(config_with_key("   ").sgdb_key(), None);
        assert_eq!(MediaConfig::default().sgdb_key(), None);
    }

    #[tokio::test]
    async fn art_without_sgdb_key_reports_it_and_keeps_other_sources() {
        let fetcher = MockFetcher::new(vec![
            // Steam: library_600x900.jpg e header.jpg não existem.
            Response::new(404, ""),
            Response::new(404, ""),
            // IGDB: busca sem resultados.
            json_response(json!({"data": []})),
            // Xbox: busca sem resultados.
            json_response(json!({"Payload": {"SearchResults": []}})),
            // PS Store: busca sem tiles.
            Response::text(200, "<html>nada</html>"),
        ]);
        let service = MediaService::new(fetcher.clone(), MediaConfig::default());
        let result = service
            .art(&ArtRequest::new("steam:10", "Portal 2", ArtKind::Cover))
            .await
            .unwrap();
        assert!(result.candidatos.is_empty());
        assert!(result.jogos.is_empty());
        assert_eq!(
            result.erros,
            vec!["SteamGridDB: sem chave de API (defina nas Configurações)".to_string()]
        );
        assert_eq!(fetcher.requests().len(), 5);
    }

    #[tokio::test]
    async fn art_with_explicit_sgdb_id_skips_search_and_aggregates() {
        let fetcher = MockFetcher::new(vec![
            Response::new(404, ""),
            Response::new(404, ""),
            json_response(json!({
                "success": true,
                "data": [{
                    "url": "https://cdn2.steamgriddb.com/grid/abc.png",
                    "width": 600,
                    "height": 900,
                    "mime": "image/png"
                }]
            })),
            json_response(json!({"data": []})),
            json_response(
                json!({"Payload": {"SearchResults": [{"ProductId": "9A", "Title": "Portal 2"}]}}),
            ),
            json_response(json!({"Products": [{"LocalizedProperties": [{
                "ProductTitle": "Portal 2",
                "ProductDescription": "",
                "Images": []
            }]}]})),
            Response::text(200, "<html>nada</html>"),
        ]);
        let service = MediaService::new(fetcher.clone(), config_with_key("chave"));
        let result = service
            .art(&ArtRequest::new("steam:10", "Portal 2", ArtKind::Cover).with_sgdb_id(99))
            .await
            .unwrap();
        assert_eq!(result.candidatos.len(), 1);
        assert_eq!(result.candidatos[0].fonte, "SteamGridDB");
        assert!(result.jogos.is_empty(), "id explícito não dispara a busca");
        assert!(result.erros.is_empty());
        let requests = fetcher.requests();
        assert_eq!(requests[2].url, "https://www.steamgriddb.com/api/v2/grids/game/99?types=static%2Canimated&dimensions=600x900%2C660x930%2C342x482&nsfw=false");
    }

    #[tokio::test]
    async fn art_sgdb_search_fills_jogos_and_uses_the_first() {
        let fetcher = MockFetcher::new(vec![
            Response::new(404, ""),
            Response::new(404, ""),
            json_response(json!({
                "success": true,
                "data": [{"id": 7, "name": "Portal 2", "release_date": 1302931200}]
            })),
            json_response(json!({"success": true, "data": []})),
            json_response(json!({"data": []})),
            json_response(json!({"Payload": {"SearchResults": []}})),
            Response::text(200, "<html>nada</html>"),
        ]);
        let service = MediaService::new(fetcher.clone(), config_with_key("chave"));
        let result = service
            .art(&ArtRequest::new("steam:10", "Portal 2", ArtKind::Cover))
            .await
            .unwrap();
        assert_eq!(result.jogos.len(), 1);
        assert_eq!(result.jogos[0].id, 7);
        let requests = fetcher.requests();
        assert!(requests[2].url.contains("/search/autocomplete/Portal%202"));
        assert!(requests[3].url.contains("/grids/game/7?"));
    }

    #[tokio::test]
    async fn art_invalid_request_short_circuits() {
        let fetcher = MockFetcher::new(vec![]);
        let service = MediaService::new(fetcher.clone(), MediaConfig::default());
        let error = service
            .art(&ArtRequest::new("", "Portal 2", ArtKind::Cover))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "pedido inválido");
        assert!(fetcher.requests().is_empty());
    }

    #[tokio::test]
    async fn text_resolves_steam_appid_by_title_and_keeps_quality_order() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({
                "items": [
                    {"id": 10, "name": "Portal 2", "tiny_image": "https://cdn/tiny.jpg"},
                    {"id": 11, "name": "Portal 2: Still Alive"}
                ]
            })),
            json_response(json!({
                "10": {
                    "success": true,
                    "data": {
                        "short_description": "<b>Curta</b>",
                        "about_the_game": "<p>Completa</p>"
                    }
                }
            })),
            json_response(json!({"Payload": {"SearchResults": []}})),
            json_response(json!({"data": []})),
        ]);
        let service = MediaService::new(fetcher.clone(), MediaConfig::default());
        let result = service.text("gog:portal2", "Portal 2").await;
        assert!(result.erros.is_empty());
        assert_eq!(
            result.textos,
            vec![
                TextCandidate::new("Steam (curta)", "Curta"),
                TextCandidate::new("Steam (completa)", "Completa"),
            ]
        );
        let requests = fetcher.requests();
        assert_eq!(
            requests[0].url,
            "https://store.steampowered.com/api/storesearch/?term=Portal+2&cc=br&l=english"
        );
        assert_eq!(
            requests[1].url,
            "https://store.steampowered.com/api/appdetails?appids=10&l=english"
        );
    }

    #[tokio::test]
    async fn text_steam_uses_the_appid_from_a_steam_game_without_storesearch() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({
                "10": {"success": true, "data": {"short_description": "ok"}}
            })),
            json_response(json!({"Payload": {"SearchResults": []}})),
            json_response(json!({"data": []})),
        ]);
        let service = MediaService::new(fetcher.clone(), MediaConfig::default());
        let result = service.text("steam:10", "Portal 2").await;
        assert_eq!(
            result.textos,
            vec![TextCandidate::new("Steam (curta)", "ok")]
        );
        assert_eq!(
            fetcher.requests()[0].url,
            "https://store.steampowered.com/api/appdetails?appids=10&l=english"
        );
    }

    #[tokio::test]
    async fn text_steam_failure_is_reported_and_igdb_still_runs() {
        let fetcher = MockFetcher::new(vec![
            Response::new(500, ""),
            json_response(json!({"Payload": {"SearchResults": []}})),
            json_response(json!({
                "data": [{
                    "id": 1,
                    "name": "Portal 2",
                    "platforms": [],
                    "first_release_date": 1302931200i64
                }]
            })),
            json_response(json!({
                "data": {"name": "Portal 2", "summary": "<b>Resumo</b>", "storyline": ""}
            })),
        ]);
        let service = MediaService::new(fetcher, MediaConfig::default());
        let result = service.text("gog:portal2", "Portal 2").await;
        assert_eq!(result.erros.len(), 1);
        assert!(result.erros[0].starts_with("Steam: HTTP 500 em "));
        assert_eq!(
            result.textos,
            vec![TextCandidate::new("IGDB — Portal 2", "Resumo")]
        );
    }

    #[tokio::test]
    async fn art_cache_base_requires_configured_art_dir() {
        let fetcher = MockFetcher::new(vec![]);
        let service = MediaService::new(fetcher, MediaConfig::default());
        assert_eq!(service.art_cache_base("steam:10", ArtKind::Cover, 5), None);
        let service = MediaService::new(
            Arc::new(crate::test_support::MockFetcher::new(vec![])) as Arc<dyn Fetcher>,
            MediaConfig {
                art_dir: Some(PathBuf::from("data/art")),
                ..MediaConfig::default()
            },
        );
        assert_eq!(
            service.art_cache_base("steam:10", ArtKind::Cover, 5),
            Some(PathBuf::from("data/art").join("steam_10-cover-5"))
        );
    }
}
