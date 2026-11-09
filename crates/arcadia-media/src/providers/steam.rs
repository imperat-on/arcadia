//! Steam: arte oficial do CDN e descrições da loja.
//!
//! Sem chave. A arte é descoberta por `HEAD` nos nomes de arquivo conhecidos,
//! porque a Steam não tem endpoint para listar artes e a disponibilidade muda
//! de jogo para jogo. É o mesmo conjunto de `STEAM_FILES` do oráculo.

use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use crate::error::Result;
use crate::http::{get_json, query_string, value_truthy, Fetcher, Request, MOZILLA_UA};
use crate::providers::{ArtKind, Artwork, TextCandidate};
use crate::text::strip_html;

pub const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
pub const STEAM_STORE: &str = "https://store.steampowered.com/api/appdetails";

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
}

impl SteamClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            cdn_base: STEAM_CDN.to_string(),
            store_base: STEAM_STORE.to_string(),
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
}
