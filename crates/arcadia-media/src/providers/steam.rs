//! Steam: arte oficial do CDN e descrições da loja.
//!
//! Sem chave. A arte é descoberta por `HEAD` nos nomes de arquivo conhecidos,
//! porque a Steam não tem endpoint para listar artes e a disponibilidade muda
//! de jogo para jogo. É o mesmo conjunto de `STEAM_FILES` do oráculo.

use std::sync::Arc;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::http::{get_json, query_string, value_str, value_truthy, Fetcher, Request, MOZILLA_UA};
use crate::providers::{ArtKind, Artwork, TextCandidate};
use crate::text::strip_html;

pub const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
pub const STEAM_STORE: &str = "https://store.steampowered.com/api/appdetails";
/// Busca da loja usada pelo `suggest` de `steamstore.js`.
pub const STEAM_SEARCH: &str = "https://store.steampowered.com/api/storesearch/";

/// Quantos resultados o dropdown de sugestões devolve (o mesmo `slice(0, 8)`).
pub const STEAM_SUGGEST_LIMIT: usize = 8;

/// Item de `storesearch`, no formato que o `suggest` expõe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamSearchItem {
    pub appid: String,
    pub title: String,
    pub cover: String,
}

/// Nomes possíveis por tipo, na ordem de preferência do cliente.
pub const STEAM_FILES: &[(&str, &[&str])] = &[
    ("cover", &["library_600x900.jpg", "header.jpg"]),
    (
        "hero",
        &[
            "library_hero.jpg",
            "library_hero_2x.jpg",
            "page_bg_generated_v6b.jpg",
        ],
    ),
    ("logo", &["logo.png", "logo_2x.png"]),
];

/// Extrai o appid de um id Steam (`steam:346110`).
pub fn steam_app_id(game_id: &str) -> Option<String> {
    static APP_ID: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    APP_ID
        .get_or_init(|| Regex::new(r"^steam:(\d+)$").expect("regex de appid Steam"))
        .captures(game_id)
        .map(|capture| capture[1].to_string())
}

fn files_for(kind: ArtKind) -> Option<&'static [&'static str]> {
    STEAM_FILES
        .iter()
        .find(|(name, _)| *name == kind.as_str())
        .map(|(_, files)| *files)
}

/// Cliente Steam; bases configuráveis para servidor local nos testes.
#[derive(Clone)]
pub struct SteamClient {
    fetcher: Arc<dyn Fetcher>,
    cdn_base: String,
    store_base: String,
    search_base: String,
}

impl SteamClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            cdn_base: STEAM_CDN.to_string(),
            store_base: STEAM_STORE.to_string(),
            search_base: STEAM_SEARCH.to_string(),
        }
    }

    pub fn with_cdn_base(mut self, base: impl Into<String>) -> Self {
        self.cdn_base = base.into();
        self
    }

    pub fn with_store_base(mut self, base: impl Into<String>) -> Self {
        self.store_base = base.into();
        self
    }

    pub fn with_search_base(mut self, base: impl Into<String>) -> Self {
        self.search_base = base.into();
        self
    }

    /// `steamArt`: devolve só os arquivos que responderam `2xx` ao `HEAD`.
    ///
    /// Falha de rede no `HEAD` conta como arquivo ausente (o JS engole).
    pub async fn art(&self, game_id: &str, kind: ArtKind) -> Result<Vec<Artwork>> {
        let Some(app_id) = steam_app_id(game_id) else {
            return Ok(Vec::new());
        };
        let Some(files) = files_for(kind) else {
            return Ok(Vec::new());
        };
        let base = self.cdn_base.trim_end_matches('/');
        let mut artworks = Vec::new();
        for file in files {
            let url = format!("{base}/{app_id}/{file}");
            let response = self.fetcher.fetch(Request::head(url.clone())).await.ok();
            let exists = response
                .map(|response| response.is_success())
                .unwrap_or(false);
            if !exists {
                continue;
            }
            artworks.push(Artwork {
                fonte: "Steam".to_string(),
                url: url.clone(),
                thumb: url,
                largura: Some(0),
                altura: Some(0),
                animado: false,
                autor: "oficial".to_string(),
            });
        }
        Ok(artworks)
    }

    /// `steamTextos`: descrição curta e completa no idioma pedido.
    ///
    /// `appid_fallback` cobre jogo fora da Steam cujo appid foi resolvido por
    /// título pelo chamador.
    pub async fn textos(
        &self,
        game_id: &str,
        lang: &str,
        appid_fallback: &str,
    ) -> Result<Vec<TextCandidate>> {
        let app_id = steam_app_id(game_id).unwrap_or_else(|| appid_fallback.to_string());
        if app_id.is_empty() {
            return Ok(Vec::new());
        }
        let params = query_string(&[("appids", app_id.clone()), ("l", lang.to_string())]);
        let url = format!("{}?{}", self.store_base.trim_end_matches('/'), params);
        let json = get_json(self.fetcher.as_ref(), url, &[("User-Agent", MOZILLA_UA)]).await?;
        let entry = json.get(&app_id);
        let success = entry.and_then(|entry| entry.get("success"));
        if !value_truthy(success) {
            return Ok(Vec::new());
        }
        let Some(info) = entry.and_then(|entry| entry.get("data")) else {
            return Ok(Vec::new());
        };

        let mut texts = Vec::new();
        if let Some(short) = info.get("short_description").and_then(Value::as_str) {
            if !short.is_empty() {
                texts.push(TextCandidate::new("Steam (curta)", strip_html(short)));
            }
        }
        if let Some(about) = info.get("about_the_game").and_then(Value::as_str) {
            if !about.is_empty() {
                texts.push(TextCandidate::new("Steam (completa)", strip_html(about)));
            }
        }
        Ok(texts)
    }

    /// `steamTextos(gameId)` com os defaults do JS (`english`, sem fallback).
    pub async fn textos_default(&self, game_id: &str) -> Result<Vec<TextCandidate>> {
        self.textos(game_id, "english", "").await
    }

    /// `storesearch` da Steam: `term`, `cc` e `l`, como `steamstore.js`.
    ///
    /// A resposta crua não é filtrada por tipo (DLC/demo) porque o filtro do
    /// cliente usa o cache local de `items`, que não pertence a esta camada.
    pub async fn search(
        &self,
        query: &str,
        country: &str,
        language: &str,
    ) -> Result<Vec<SteamSearchItem>> {
        let params = query_string(&[
            ("term", query.to_string()),
            ("cc", country.to_string()),
            ("l", language.to_string()),
        ]);
        let url = format!("{}?{}", self.search_base.trim_end_matches('?'), params);
        let json = get_json(self.fetcher.as_ref(), url, &[("User-Agent", MOZILLA_UA)]).await?;
        let items = json
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let appid = value_str(item.get("id"));
                        let cover = match value_str(item.get("tiny_image")) {
                            cover if !cover.is_empty() => cover,
                            _ => format!("{STEAM_CDN}/{appid}/header.jpg"),
                        };
                        SteamSearchItem {
                            appid,
                            title: value_str(item.get("name")),
                            cover,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(items)
    }

    /// `suggest` da loja: exige ao menos 2 caracteres e devolve no máximo 8,
    /// como o dropdown congelado. O fallback para o catálogo local/remoto é
    /// responsabilidade do chamador (não é provedor de metadados).
    pub async fn suggest(
        &self,
        query: &str,
        country: &str,
        language: &str,
    ) -> Result<Vec<SteamSearchItem>> {
        if query.trim().chars().count() < 2 {
            return Ok(Vec::new());
        }
        let mut items = self.search(query.trim(), country, language).await?;
        items.truncate(STEAM_SUGGEST_LIMIT);
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};

    #[test]
    fn app_id_only_accepts_the_steam_prefix_and_digits() {
        assert_eq!(steam_app_id("steam:346110"), Some("346110".to_string()));
        assert_eq!(steam_app_id("steam:"), None);
        assert_eq!(steam_app_id("gog:123"), None);
        assert_eq!(steam_app_id("steam:12a"), None);
        assert_eq!(steam_app_id("steam:001"), Some("001".to_string()));
    }

    #[tokio::test]
    async fn art_checks_each_candidate_in_order() {
        let fetcher = MockFetcher::new(vec![Response::new(404, ""), Response::new(200, "")]);
        let client = SteamClient::new(fetcher.clone());
        let art = client.art("steam:10", ArtKind::Cover).await.unwrap();
        assert_eq!(art.len(), 1);
        assert_eq!(
            art[0].url,
            "https://cdn.cloudflare.steamstatic.com/steam/apps/10/header.jpg"
        );
        assert_eq!(art[0].fonte, "Steam");
        assert_eq!(art[0].autor, "oficial");
        let requests = fetcher.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, crate::http::Method::Head);
        assert!(requests[0].url.ends_with("/library_600x900.jpg"));
        assert!(requests[1].url.ends_with("/header.jpg"));
    }

    #[tokio::test]
    async fn art_network_failure_means_missing_file_and_unknown_kind_is_empty() {
        let fetcher = MockFetcher::new(vec![Response::new(500, "")]);
        let client = SteamClient::new(fetcher.clone());
        assert!(client
            .art("steam:10", ArtKind::Logo)
            .await
            .unwrap()
            .is_empty());
        // Apenas o primeiro logo foi consultado e falhou; o segundo não existe
        // na fila, então vira erro de mock engolido como ausência.
        assert!(client
            .art("gog:10", ArtKind::Cover)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(fetcher.requests().len(), 2);
    }

    #[tokio::test]
    async fn textos_parses_short_and_about_with_html_cleanup() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "10": {
                "success": true,
                "data": {
                    "short_description": "<b>Curta</b> &amp; boa",
                    "about_the_game": "<p>Linha 1</p><p>Linha 2</p>"
                }
            }
        }))]);
        let client = SteamClient::new(fetcher.clone());
        let texts = client.textos("steam:10", "portuguese", "").await.unwrap();
        assert_eq!(
            texts,
            vec![
                TextCandidate::new("Steam (curta)", "Curta & boa"),
                TextCandidate::new("Steam (completa)", "Linha 1\nLinha 2"),
            ]
        );
        assert_eq!(
            fetcher.requests()[0].url,
            "https://store.steampowered.com/api/appdetails?appids=10&l=portuguese"
        );
        assert_eq!(
            fetcher.requests()[0]
                .headers
                .iter()
                .find(|(name, _)| name == "User-Agent")
                .map(|(_, value)| value.as_str()),
            Some(MOZILLA_UA)
        );
    }

    #[tokio::test]
    async fn textos_uses_appid_fallback_and_skips_unsuccessful_entries() {
        let fetcher = MockFetcher::new(vec![
            json_response(serde_json::json!({
                "20": {"success": true, "data": {"short_description": "ok"}}
            })),
            json_response(serde_json::json!({
                "30": {"success": false}
            })),
        ]);
        let client = SteamClient::new(fetcher.clone());
        let texts = client.textos("gog:20", "english", "20").await.unwrap();
        assert_eq!(texts, vec![TextCandidate::new("Steam (curta)", "ok")]);
        assert!(client
            .textos("gog:30", "english", "30")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            fetcher.requests()[0].url,
            "https://store.steampowered.com/api/appdetails?appids=20&l=english"
        );
    }

    #[tokio::test]
    async fn textos_without_any_appid_does_not_touch_the_network() {
        let fetcher = MockFetcher::new(vec![]);
        let client = SteamClient::new(fetcher.clone());
        assert!(client
            .textos("gog:1", "english", "")
            .await
            .unwrap()
            .is_empty());
        assert!(fetcher.requests().is_empty());
    }

    #[tokio::test]
    async fn missing_success_or_data_is_empty_not_an_error() {
        let fetcher = MockFetcher::new(vec![
            json_response(serde_json::json!({"10": {}})),
            json_response(serde_json::json!({})),
        ]);
        let client = SteamClient::new(fetcher);
        assert!(client
            .textos("steam:10", "english", "")
            .await
            .unwrap()
            .is_empty());
        assert!(client
            .textos("steam:10", "english", "")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn storesearch_contract_parses_items_and_cover_fallback() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "items": [
                {"id": 10, "name": "Portal 2", "tiny_image": "https://cdn/tiny.jpg"},
                {"id": 20, "name": "Portal"}
            ]
        }))]);
        let client = SteamClient::new(fetcher.clone());
        let items = client.search("Portal 2", "br", "portuguese").await.unwrap();
        assert_eq!(
            items,
            vec![
                SteamSearchItem {
                    appid: "10".into(),
                    title: "Portal 2".into(),
                    cover: "https://cdn/tiny.jpg".into(),
                },
                SteamSearchItem {
                    appid: "20".into(),
                    title: "Portal".into(),
                    cover: "https://cdn.cloudflare.steamstatic.com/steam/apps/20/header.jpg".into(),
                },
            ]
        );
        assert_eq!(
            fetcher.requests()[0].url,
            "https://store.steampowered.com/api/storesearch/?term=Portal+2&cc=br&l=portuguese"
        );
        assert_eq!(
            fetcher.requests()[0]
                .headers
                .iter()
                .find(|(name, _)| name == "User-Agent")
                .map(|(_, value)| value.as_str()),
            Some(MOZILLA_UA)
        );
    }

    #[tokio::test]
    async fn suggest_requires_two_characters_and_caps_at_eight() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "items": (0..12)
                .map(|index| serde_json::json!({"id": index, "name": format!("Jogo {index}")}))
                .collect::<Vec<_>>()
        }))]);
        let client = SteamClient::new(fetcher.clone());
        assert!(client
            .suggest("p", "br", "english")
            .await
            .unwrap()
            .is_empty());
        assert!(fetcher.requests().is_empty());
        let items = client.suggest("portal", "br", "english").await.unwrap();
        assert_eq!(items.len(), STEAM_SUGGEST_LIMIT);
        assert_eq!(items[0].appid, "0");
    }

    #[tokio::test]
    async fn storesearch_missing_items_is_empty() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({}))]);
        let client = SteamClient::new(fetcher);
        assert!(client
            .search("Portal", "br", "english")
            .await
            .unwrap()
            .is_empty());
    }
}
