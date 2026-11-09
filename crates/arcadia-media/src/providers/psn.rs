//! PlayStation Store: busca/arte por scraping e arte quadrada via GraphQL.
//!
//! Duas superfícies, como no cliente congelado:
//!
//! - HTML público (`/en-us/search/{titulo}` e `/en-us/{tipo}/{id}`), com o
//!   JSON de telemetria embutido nos tiles e o JSON `Media` na página;
//! - GraphQL persistido (`getSearchResults`, query pública assinada) usado
//!   pela arte 1:1 (papel `MASTER`) e pelo logo (papel `LOGO`).
//!
//! Nenhuma das duas exige chave. Se a Sony mudar o HTML/query, o chamador
//! recebe `None` e a cascata de arte segue para a próxima fonte.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Result;
use crate::http::{encode_uri_component, get_json, get_text, value_str, value_truthy, Fetcher};
use crate::providers::{ArtKind, Artwork};
use crate::text::titles_match;

pub const PSN_STORE: &str = "https://store.playstation.com/en-us";
pub const PSN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

/// Prioridade de papel por tipo de arte (o 1º que existir ganha a lista).
pub const PSN_ROLES: &[(&str, &[&str])] = &[
    (
        "cover",
        &[
            "PORTRAIT_BANNER",
            "MASTER",
            "GAMEHUB_COVER_ART",
            "EDITION_KEY_ART",
            "FOUR_BY_THREE_BANNER",
        ],
    ),
    (
        "hero",
        &[
            "SIXTEEN_BY_NINE_BANNER",
            "BACKGROUND",
            "EDITION_KEY_ART",
            "MASTER",
        ],
    ),
    ("logo", &["LOGO"]),
];

/// GraphQL persistido do site (query pública, sem chave).
pub const PSN_GQL_OP: &str = "https://web.np.playstation.com/api/graphql/v1//op";
pub const PSN_GQL_HASH: &str = "4df6284f982e57bec70f23c77e2c219dc792eb19af7fb3d3a81767aa3f1958aa";
pub const PSN_GQL_APP: &str = "@sie-ppr-web-store/app";
pub const PSN_GQL_VER: &str = "0.113.0";

/// Tile da busca HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsnTile {
    pub id: String,
    /// `concept` ou `product`.
    pub tipo: String,
    pub titulo: String,
}

/// Media da ficha GraphQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsnMedia {
    #[serde(rename = "type")]
    pub media_type: String,
    pub role: String,
    pub url: String,
}

/// Resultado da busca GraphQL (só o que a arte usa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsnGame {
    pub name: String,
    pub media: Vec<PsnMedia>,
}

impl PsnGame {
    pub fn media_url(&self, role: &str) -> Option<String> {
        psn_media_de(self, role)
    }
}

fn roles_for(kind: ArtKind) -> &'static [&'static str] {
    PSN_ROLES
        .iter()
        .find(|(name, _)| *name == kind.as_str())
        .map(|(_, roles)| *roles)
        .unwrap_or(&[])
}

fn telemetry_decode(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn thumbnail(url: &str, width: u32) -> String {
    format!(
        "{url}{}w={width}",
        if url.contains('?') { "&" } else { "?" }
    )
}

/// Cliente PS Store; bases configuráveis para os testes.
#[derive(Clone)]
pub struct PsnClient {
    fetcher: Arc<dyn Fetcher>,
    store_base: String,
    graphql_base: String,
    user_agent: String,
}

impl PsnClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            store_base: PSN_STORE.to_string(),
            graphql_base: PSN_GQL_OP.to_string(),
            user_agent: PSN_UA.to_string(),
        }
    }

    pub fn with_store_base(mut self, base: impl Into<String>) -> Self {
        self.store_base = base.into();
        self
    }

    pub fn with_graphql_base(mut self, base: impl Into<String>) -> Self {
        self.graphql_base = base.into();
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    fn text_headers(&self) -> [(&str, &str); 1] {
        [("User-Agent", self.user_agent.as_str())]
    }

    /// `psnStoreSearch`: extrai e deduplica os tiles da página de busca.
    pub async fn store_search(&self, title: &str) -> Result<Vec<PsnTile>> {
        static TILE: OnceLock<Regex> = OnceLock::new();
        let url = format!(
            "{}/search/{}",
            self.store_base.trim_end_matches('/'),
            encode_uri_component(title)
        );
        let html = get_text(self.fetcher.as_ref(), url, &self.text_headers()).await?;
        let regex = TILE.get_or_init(|| {
            Regex::new(
                r#"data-track="web:store:(concept|product)-tile"[^>]*data-telemetry-meta="([^"]+)""#,
            )
            .expect("regex de tile da PS Store")
        });
        let mut tiles = Vec::new();
        let mut seen = HashSet::new();
        for capture in regex.captures_iter(&html) {
            let Ok(meta) = serde_json::from_str::<Value>(&telemetry_decode(&capture[2])) else {
                continue;
            };
            if !value_truthy(meta.get("id")) {
                continue;
            }
            let id = value_str(meta.get("id"));
            if !seen.insert(id.clone()) {
                continue;
            }
            tiles.push(PsnTile {
                id,
                tipo: capture[1].to_string(),
                titulo: value_str(meta.get("name")),
            });
        }
        Ok(tiles)
    }

    /// `psnStoreArt`: arte oficial da página do jogo pelo papel.
    pub async fn store_art(&self, id: &str, tipo: &str, kind: ArtKind) -> Result<Vec<Artwork>> {
        static MEDIA: OnceLock<Regex> = OnceLock::new();
        let url = format!("{}/{}/{}", self.store_base.trim_end_matches('/'), tipo, id);
        let html = get_text(self.fetcher.as_ref(), url, &self.text_headers()).await?;
        let regex = MEDIA.get_or_init(|| {
            Regex::new(
                r#"\{"__typename":"Media","role":"([A-Z_]+)","type":"IMAGE","url":"([^"]+)"\}"#,
            )
            .expect("regex de Media da PS Store")
        });
        let mut by_role: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for capture in regex.captures_iter(&html) {
            by_role
                .entry(capture[1].to_string())
                .or_insert_with(|| capture[2].to_string());
        }
        let mut artworks = Vec::new();
        for role in roles_for(kind) {
            let Some(url) = by_role.get(*role) else {
                continue;
            };
            artworks.push(Artwork {
                fonte: "PS Store".to_string(),
                url: url.clone(),
                thumb: thumbnail(url, 320),
                largura: Some(0),
                altura: Some(0),
                animado: false,
                autor: String::new(),
            });
        }
        Ok(artworks)
    }

    /// `psnJogoDaBusca`: melhor resultado da busca GraphQL por prefixo comum.
    pub async fn game_from_search(&self, title: &str) -> Result<Option<PsnGame>> {
        let variables = serde_json::to_string(&json!({
            "countryCode": "US",
            "languageCode": "en",
            "nextCursor": "",
            "pageOffset": 0,
            "pageSize": 24,
            "searchTerm": title
        }))
        .map_err(|error| crate::error::MediaError::decode(error.to_string()))?;
        let extensions = serde_json::to_string(&json!({
            "persistedQuery": {"version": 1, "sha256Hash": PSN_GQL_HASH}
        }))
        .map_err(|error| crate::error::MediaError::decode(error.to_string()))?;
        let url = format!(
            "{}?operationName=getSearchResults&variables={}&extensions={}",
            self.graphql_base,
            encode_uri_component(&variables),
            encode_uri_component(&extensions)
        );
        let correlation = uuid::Uuid::new_v4().to_string();
        let request_id = uuid::Uuid::new_v4().to_string();
        let app_version = format!("{PSN_GQL_APP}/{PSN_GQL_VER}-");
        let headers = [
            ("Accept", "application/json"),
            ("Content-Type", "application/json"),
            ("User-Agent", self.user_agent.as_str()),
            ("Origin", "https://store.playstation.com"),
            ("Referer", "https://store.playstation.com/"),
            ("apollographql-client-name", PSN_GQL_APP),
            ("apollographql-client-version", PSN_GQL_VER),
            ("X-PSN-App-Ver", app_version.as_str()),
            ("X-PSN-Correlation-ID", correlation.as_str()),
            ("X-PSN-Request-ID", request_id.as_str()),
            ("X-PSN-Store-Locale-Override", "en-US"),
        ];
        let json = get_json(self.fetcher.as_ref(), url, &headers).await?;
        let results = json
            .get("data")
            .and_then(|data| data.get("universalSearch"))
            .and_then(|search| search.get("results"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let target = letters(title);
        if target.is_empty() {
            return Ok(None);
        }
        let mut best: Option<(PsnGame, usize)> = None;
        for result in results {
            let Some(name) = result.get("name") else {
                continue;
            };
            if !value_truthy(Some(name)) {
                continue;
            }
            let name_text = value_str(Some(name));
            let normalized = letters(&name_text);
            let common = common_prefix(&normalized, &target);
            let ok = common >= 8
                && (common as f64) >= (target.len() as f64) * 0.5
                && (common as f64) >= (normalized.len() as f64) * 0.6;
            if !ok {
                continue;
            }
            // O JS ordena por n desc; `sort` é estável. Só substitui quando o
            // prefixo é estritamente maior.
            if best
                .as_ref()
                .is_none_or(|(_, best_common)| common > *best_common)
            {
                best = Some((psn_game_from_json(&result, name_text), common));
            }
        }
        Ok(best.map(|(game, _)| game))
    }
}

fn psn_game_from_json(value: &Value, name: String) -> PsnGame {
    PsnGame {
        name,
        media: value
            .get("media")
            .and_then(Value::as_array)
            .map(|media| {
                media
                    .iter()
                    .map(|media| PsnMedia {
                        media_type: value_str(media.get("type")),
                        role: value_str(media.get("role")),
                        url: value_str(media.get("url")),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// `psnMelhorResultado`: título que bate ou o primeiro tile.
pub fn best_psn_result(tiles: &[PsnTile], title: &str) -> Option<PsnTile> {
    tiles
        .iter()
        .find(|tile| titles_match(&tile.titulo, title))
        .or_else(|| tiles.first())
        .cloned()
}

/// `psnMediaDe`: primeira media `IMAGE` do papel com URL.
pub fn psn_media_de(game: &PsnGame, role: &str) -> Option<String> {
    game.media
        .iter()
        .find(|media| media.media_type == "IMAGE" && media.role == role && !media.url.is_empty())
        .map(|media| media.url.clone())
}

/// `soLetras` da busca GraphQL: minúsculas e só `[a-z0-9]`.
pub fn letters(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

/// Prefixo comum entre duas strings já reduzidas.
pub fn common_prefix(left: &str, right: &str) -> usize {
    left.bytes()
        .zip(right.bytes())
        .take_while(|(left, right)| left == right)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{json_response, MockFetcher};

    #[tokio::test]
    async fn store_search_decodes_entities_and_deduplicates() {
        let html = r#"
            <div data-track="web:store:concept-tile" data-telemetry-meta="{&quot;id&quot;:&quot;UP1&quot;,&quot;name&quot;:&quot;Portal 2&quot;}">
            <div data-track="web:store:product-tile" data-telemetry-meta="{&quot;id&quot;:&quot;UP2&quot;,&quot;name&quot;:&quot;Tom&#x27;s Game&quot;}">
            <div data-track="web:store:concept-tile" data-telemetry-meta="{&quot;id&quot;:&quot;UP1&quot;,&quot;name&quot;:&quot;Duplicado&quot;}">
            <div data-track="web:store:concept-tile" data-telemetry-meta="não é json">
        "#;
        let fetcher = MockFetcher::new(vec![crate::http::Response::text(200, html)]);
        let client = PsnClient::new(fetcher.clone());
        let tiles = client.store_search("Portal 2").await.unwrap();
        assert_eq!(
            tiles,
            vec![
                PsnTile {
                    id: "UP1".into(),
                    tipo: "concept".into(),
                    titulo: "Portal 2".into()
                },
                PsnTile {
                    id: "UP2".into(),
                    tipo: "product".into(),
                    titulo: "Tom's Game".into()
                },
            ]
        );
        let requests = fetcher.requests();
        assert_eq!(
            requests[0].url,
            "https://store.playstation.com/en-us/search/Portal%202"
        );
        assert_eq!(
            requests[0]
                .headers
                .iter()
                .find(|(name, _)| name == "User-Agent")
                .map(|(_, value)| value.as_str()),
            Some(PSN_UA)
        );
    }

    #[test]
    fn best_result_prefers_title_match_then_first() {
        let tiles = vec![
            PsnTile {
                id: "1".into(),
                tipo: "concept".into(),
                titulo: "Eldrynn".into(),
            },
            PsnTile {
                id: "2".into(),
                tipo: "concept".into(),
                titulo: "Elden Ring".into(),
            },
        ];
        assert_eq!(best_psn_result(&tiles, "Elden Ring").unwrap().id, "2");
        assert_eq!(best_psn_result(&tiles, "Nada").unwrap().id, "1");
        assert_eq!(best_psn_result(&[], "x"), None);
    }

    #[tokio::test]
    async fn store_art_uses_role_priority_and_first_occurrence() {
        let html = r#"
            {"__typename":"Media","role":"PORTRAIT_BANNER","type":"IMAGE","url":"https://img/cover.png"}
            {"__typename":"Media","role":"PORTRAIT_BANNER","type":"IMAGE","url":"https://img/cover-2.png"}
            {"__typename":"Media","role":"MASTER","type":"IMAGE","url":"https://img/master.png"}
            {"__typename":"Media","role":"LOGO","type":"IMAGE","url":"https://img/logo.png"}
            {"__typename":"Media","role":"LOGO","type":"VIDEO","url":"https://img/no.webm"}
        "#;
        let fetcher = MockFetcher::new(vec![
            crate::http::Response::text(200, html),
            crate::http::Response::text(200, html),
        ]);
        let client = PsnClient::new(fetcher.clone());
        let art = client
            .store_art("UP1", "concept", ArtKind::Cover)
            .await
            .unwrap();
        assert_eq!(art.len(), 2);
        assert_eq!(art[0].url, "https://img/cover.png");
        assert_eq!(art[0].thumb, "https://img/cover.png?w=320");
        assert_eq!(art[1].url, "https://img/master.png");
        let logo = client
            .store_art("UP1", "concept", ArtKind::Logo)
            .await
            .unwrap();
        assert_eq!(logo.len(), 1);
        assert_eq!(logo[0].thumb, "https://img/logo.png?w=320");
        assert_eq!(
            fetcher.requests()[0].url,
            "https://store.playstation.com/en-us/concept/UP1"
        );
    }

    fn graphql_body() -> Value {
        json!({
            "data": {"universalSearch": {"results": [
                {
                    "name": "Grand Theft Auto V (PS4 & PS5)",
                    "media": [
                        {"type": "IMAGE", "role": "MASTER", "url": "https://img/master.png"},
                        {"type": "IMAGE", "role": "LOGO", "url": "https://img/logo.png"}
                    ]
                },
                {
                    "name": "Grand Theft Auto: San Andreas – The Definitive Edition",
                    "media": [{"type": "IMAGE", "role": "MASTER", "url": "https://img/sa.png"}]
                }
            ]}}
        })
    }

    #[tokio::test]
    async fn graphql_search_contract_and_common_prefix_selection() {
        let fetcher = MockFetcher::new(vec![json_response(graphql_body())]);
        let client = PsnClient::new(fetcher.clone());
        let game = client
            .game_from_search("Grand Theft Auto V Legacy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(game.name, "Grand Theft Auto V (PS4 & PS5)");
        assert_eq!(
            game.media_url("MASTER").as_deref(),
            Some("https://img/master.png")
        );
        assert_eq!(
            game.media_url("LOGO").as_deref(),
            Some("https://img/logo.png")
        );
        assert_eq!(game.media_url("BACKGROUND"), None);

        let request = &fetcher.requests()[0];
        assert!(request.url.starts_with(
            "https://web.np.playstation.com/api/graphql/v1//op?operationName=getSearchResults&variables="
        ));
        assert!(request.url.contains("&extensions="));
        assert_eq!(
            request
                .headers
                .iter()
                .find(|(name, _)| name == "apollographql-client-name")
                .map(|(_, value)| value.as_str()),
            Some(PSN_GQL_APP)
        );
        assert_eq!(
            request
                .headers
                .iter()
                .find(|(name, _)| name == "X-PSN-Store-Locale-Override")
                .map(|(_, value)| value.as_str()),
            Some("en-US")
        );
        assert_eq!(
            request
                .headers
                .iter()
                .filter(|(name, _)| name == "X-PSN-Correlation-ID" || name == "X-PSN-Request-ID")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn graphql_rejects_the_series_prefix_that_would_pick_the_wrong_game() {
        let fetcher = MockFetcher::new(vec![json_response(json!({
            "data": {"universalSearch": {"results": [
                {"name": "Grand Theft Auto: San Andreas – The Definitive Edition", "media": []},
                {"name": "Grand Theft Auto V (PS4 & PS5)", "media": []}
            ]}}
        }))]);
        let client = PsnClient::new(fetcher);
        let game = client
            .game_from_search("Grand Theft Auto VI")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(game.name, "Grand Theft Auto V (PS4 & PS5)");
    }

    #[tokio::test]
    async fn graphql_empty_or_unmatched_results_are_none() {
        let fetcher = MockFetcher::new(vec![
            json_response(json!({"data": {"universalSearch": {"results": []}}})),
            json_response(
                json!({"data": {"universalSearch": {"results": [{"name": "Outro Jogo"}]}}}),
            ),
            json_response(json!({"data": {"universalSearch": {"results": []}}})),
        ]);
        let client = PsnClient::new(fetcher);
        assert_eq!(client.game_from_search("Portal 2").await.unwrap(), None);
        assert_eq!(client.game_from_search("Portal 2").await.unwrap(), None);
        assert_eq!(client.game_from_search("   ").await.unwrap(), None);
    }

    #[test]
    fn letters_and_common_prefix_match_the_javascript() {
        assert_eq!(
            letters("Grand Theft Auto V (PS4 & PS5)"),
            "grandtheftautovps4ps5"
        );
        assert_eq!(common_prefix("grandtheftautov", "grandtheftautovi"), 15);
        assert_eq!(common_prefix("abc", "abcdef"), 3);
        assert_eq!(common_prefix("", "x"), 0);
    }
}
