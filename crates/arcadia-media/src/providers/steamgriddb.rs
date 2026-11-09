//! SteamGridDB: busca de jogos e arte da comunidade.
//!
//! Endpoints e filtros idênticos a `app/electron/metadata.js`:
//! - `GET {base}/search/autocomplete/{titulo}` com `Authorization: Bearer`;
//! - `GET {base}/{grids|heroes|logos}/game/{id}` com `types`, `dimensions` e
//!   `nsfw=false`.
//!
//! A chave é sempre configuração, nunca constante: um cliente sem chave não é
//! construído pelo fluxo de arte (o chamador decide se a fonte está ativa).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{MediaError, Result};
use crate::http::{get_json, query_string, value_number, value_str, value_truthy, Fetcher};
use crate::providers::{ArtKind, Artwork};

pub const SGDB_BASE: &str = "https://www.steamgriddb.com/api/v2";

/// Endpoint da SGDB para cada arte nossa.
pub const SGDB_ENDPOINT: &[(&str, &str)] =
    &[("cover", "grids"), ("hero", "heroes"), ("logo", "logos")];

/// Dimensões que a API aceita por tipo. A lista é fechada: uma dimensão
/// inventada faz a API recusar o pedido inteiro.
pub const SGDB_DIMENSIONS: &[(&str, &[&str])] = &[
    (
        "cover",
        &["600x900", "660x930", "342x482", "920x430", "460x215"],
    ),
    ("hero", &["1920x620", "3840x1240", "1600x650"]),
    ("logo", &[]),
];

/// O padrão oferecido quando o usuário não filtra nada.
pub const SGDB_DIMENSIONS_DEFAULT: &[(&str, &[&str])] = &[
    ("cover", &["600x900", "660x930", "342x482"]),
    ("hero", &["1920x620", "3840x1240", "1600x650"]),
    ("logo", &[]),
];

/// Jogo retornado pelo autocomplete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SgdbGame {
    pub id: u64,
    pub titulo: String,
    /// `release_date` cru: a API devolve timestamp numérico e o JS repassa
    /// sem converter. `Value` preserva número/string como vieram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ano: Option<Value>,
}

/// Opções de [`sgdb_art_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SgdbArtOptions {
    /// Inclui webm/gif além das imagens paradas (default `true`).
    pub animated: bool,
    /// Resoluções pedidas; vazio usa o padrão do tipo.
    pub dimensions: Vec<String>,
}

impl Default for SgdbArtOptions {
    fn default() -> Self {
        Self {
            animated: true,
            dimensions: Vec::new(),
        }
    }
}

impl SgdbArtOptions {
    pub fn static_only() -> Self {
        Self {
            animated: false,
            dimensions: Vec::new(),
        }
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

fn endpoint_for(kind: &str) -> Option<&'static str> {
    SGDB_ENDPOINT
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, endpoint)| *endpoint)
}

fn valid_dimensions(kind: &str) -> &'static [&'static str] {
    SGDB_DIMENSIONS
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, dimensions)| *dimensions)
        .unwrap_or(&[])
}

fn default_dimensions(kind: &str) -> &'static [&'static str] {
    SGDB_DIMENSIONS_DEFAULT
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, dimensions)| *dimensions)
        .unwrap_or(&[])
}

/// Monta a URL de arte da SGDB.
///
/// `kind` é a string do JS (`cover`, `hero`, `logo`); um tipo desconhecido
/// devolve o mesmo erro do cliente congelado. Dimensões fora da lista fechada
/// são descartadas; se o filtro ficar vazio, cai no padrão do tipo.
pub fn sgdb_art_url(sgdb_id: u64, kind: &str, options: &SgdbArtOptions) -> Result<String> {
    sgdb_art_url_with_base(SGDB_BASE, sgdb_id, kind, options)
}

/// Igual a [`sgdb_art_url`], com base configurável (testes/servidor local).
pub fn sgdb_art_url_with_base(
    base: &str,
    sgdb_id: u64,
    kind: &str,
    options: &SgdbArtOptions,
) -> Result<String> {
    let endpoint = endpoint_for(kind)
        .ok_or_else(|| MediaError::message(format!("tipo de arte desconhecido: {kind}")))?;
    let valid = valid_dimensions(kind);
    let defaults: Vec<&str> = default_dimensions(kind)
        .iter()
        .copied()
        .filter(|dimension| valid.contains(dimension))
        .collect();
    let requested: Vec<&str> = options
        .dimensions
        .iter()
        .map(String::as_str)
        .filter(|dimension| valid.contains(dimension))
        .collect();
    let dimensions = if requested.is_empty() {
        defaults
    } else {
        requested
    };

    let mut params = vec![(
        "types",
        if options.animated {
            "static,animated".to_string()
        } else {
            "static".to_string()
        },
    )];
    if !dimensions.is_empty() {
        params.push(("dimensions", dimensions.join(",")));
    }
    params.push(("nsfw", "false".to_string()));
    Ok(format!(
        "{}/{}/game/{}?{}",
        base.trim_end_matches('/'),
        endpoint,
        sgdb_id,
        query_string(&params)
    ))
}

/// Cliente SteamGridDB com a chave vinda da configuração.
#[derive(Clone)]
pub struct SteamGridDb {
    fetcher: Arc<dyn Fetcher>,
    base: String,
    api_key: String,
}

impl SteamGridDb {
    pub fn new(fetcher: Arc<dyn Fetcher>, api_key: impl Into<String>) -> Self {
        Self {
            fetcher,
            base: SGDB_BASE.to_string(),
            api_key: api_key.into(),
        }
    }

    /// Base alternativa (servidor local nos testes).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    fn authorization(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// `sgdbSearch`: autocomplete por título.
    pub async fn search(&self, title: &str) -> Result<Vec<SgdbGame>> {
        let url = format!(
            "{}/search/autocomplete/{}",
            self.base.trim_end_matches('/'),
            crate::http::encode_uri_component(title)
        );
        let authorization = self.authorization();
        let json = get_json(
            self.fetcher.as_ref(),
            url,
            &[("Authorization", authorization.as_str())],
        )
        .await?;
        if !value_truthy(json.get("success")) {
            return Err(MediaError::message(
                "SteamGridDB recusou a busca (chave inválida?)",
            ));
        }
        let games = json
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|game| SgdbGame {
                        id: value_number(game.get("id")).max(0.0) as u64,
                        titulo: value_str(game.get("name")),
                        ano: match game.get("release_date") {
                            None | Some(Value::Null) => None,
                            value => Some(value_str(value)),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(games)
    }

    /// `sgdbArt`: arte de um jogo. `kind` aceita as strings do oráculo para
    /// preservar o erro de tipo desconhecido.
    pub async fn art_with_kind(
        &self,
        sgdb_id: u64,
        kind: &str,
        options: &SgdbArtOptions,
    ) -> Result<Vec<Artwork>> {
        let url = sgdb_art_url_with_base(&self.base, sgdb_id, kind, options)?;
        let authorization = self.authorization();
        let json = get_json(
            self.fetcher.as_ref(),
            url,
            &[("Authorization", authorization.as_str())],
        )
        .await?;
        if !value_truthy(json.get("success")) {
            return Err(MediaError::message("SteamGridDB recusou o pedido de arte"));
        }
        let artworks = json
            .get("data")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(artwork_from_json).collect())
            .unwrap_or_default();
        Ok(artworks)
    }

    /// Variante tipada de [`Self::art_with_kind`].
    pub async fn art(
        &self,
        sgdb_id: u64,
        kind: ArtKind,
        options: &SgdbArtOptions,
    ) -> Result<Vec<Artwork>> {
        self.art_with_kind(sgdb_id, kind.as_str(), options).await
    }
}

fn artwork_from_json(value: &Value) -> Artwork {
    let url = value_str(value.get("url"));
    let thumb = match value.get("thumb") {
        Some(Value::String(thumb)) if !thumb.is_empty() => thumb.clone(),
        _ => url.clone(),
    };
    let mime = value.get("mime").and_then(Value::as_str).unwrap_or("");
    let animated =
        !mime.is_empty() && mime != "image/png" && mime != "image/jpeg" && mime != "image/webp";
    let author = value
        .get("author")
        .and_then(|author| author.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Artwork {
        fonte: "SteamGridDB".to_string(),
        url,
        thumb,
        largura: optional_dimension(value.get("width")),
        altura: optional_dimension(value.get("height")),
        animado: animated,
        autor: author,
    }
}

fn optional_dimension(value: Option<&Value>) -> Option<u32> {
    value
        .map(|value| value_number(Some(value)))
        .filter(|value| *value > 0.0)
        .map(|value| value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};

    #[test]
    fn art_url_uses_default_portrait_dimensions_for_cover() {
        let url = sgdb_art_url(123, "cover", &SgdbArtOptions::default()).unwrap();
        assert_eq!(
            url,
            "https://www.steamgriddb.com/api/v2/grids/game/123?types=static%2Canimated&dimensions=600x900%2C660x930%2C342x482&nsfw=false"
        );
    }

    #[test]
    fn art_url_filters_invalid_dimensions_and_falls_back_to_defaults() {
        let options =
            SgdbArtOptions::default().with_dimensions(["1024x1024", "920x430", "342x482"]);
        let url = sgdb_art_url(1, "cover", &options).unwrap();
        assert!(url.contains("dimensions=920x430%2C342x482"));

        let options = SgdbArtOptions::default().with_dimensions(["1024x1024"]);
        let url = sgdb_art_url(1, "cover", &options).unwrap();
        assert!(url.contains("dimensions=600x900%2C660x930%2C342x482"));
    }

    #[test]
    fn art_url_honors_animated_and_logo_has_no_dimensions() {
        let url = sgdb_art_url(9, "logo", &SgdbArtOptions::static_only()).unwrap();
        assert_eq!(
            url,
            "https://www.steamgriddb.com/api/v2/logos/game/9?types=static&nsfw=false"
        );
    }

    #[test]
    fn art_url_supports_hero_dimensions() {
        let url = sgdb_art_url(9, "hero", &SgdbArtOptions::default()).unwrap();
        assert!(url.contains("dimensions=1920x620%2C3840x1240%2C1600x650"));
    }

    #[test]
    fn art_url_rejects_unknown_kind_like_the_client() {
        let error = sgdb_art_url(1, "grid", &SgdbArtOptions::default()).unwrap_err();
        assert_eq!(error.to_string(), "tipo de arte desconhecido: grid");
    }

    #[tokio::test]
    async fn search_contract_and_parsing() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "success": true,
            "data": [
                {"id": 10, "name": "Portal 2", "release_date": "2011-04-19"},
                {"id": 11, "name": "Portal", "release_date": null}
            ]
        }))]);
        let client = SteamGridDb::new(fetcher.clone(), "secret-key");
        let games = client.search("Portal 2").await.unwrap();
        assert_eq!(
            games,
            vec![
                SgdbGame {
                    id: 10,
                    titulo: "Portal 2".into(),
                    ano: Some("2011-04-19".into())
                },
                SgdbGame {
                    id: 11,
                    titulo: "Portal".into(),
                    ano: None
                },
            ]
        );
        let requests = fetcher.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            "https://www.steamgriddb.com/api/v2/search/autocomplete/Portal%202"
        );
        assert_eq!(
            requests[0]
                .headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.as_str()),
            Some("Bearer secret-key")
        );
    }

    #[tokio::test]
    async fn search_refusal_is_typed() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({"success": false}))]);
        let client = SteamGridDb::new(fetcher, "k");
        let error = client.search("x").await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "SteamGridDB recusou a busca (chave inválida?)"
        );
    }

    #[tokio::test]
    async fn art_contract_parses_animation_author_and_thumb_fallback() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "success": true,
            "data": [
                {
                    "url": "https://cdn2.steamgriddb.com/grid/abc.png",
                    "width": 600,
                    "height": 900,
                    "mime": "image/png",
                    "author": {"name": "artist"}
                },
                {
                    "url": "https://cdn2.steamgriddb.com/grid/def.webm",
                    "thumb": "https://cdn2.steamgriddb.com/thumb/def.jpg",
                    "width": 1920,
                    "height": 620,
                    "mime": "video/webm"
                }
            ]
        }))]);
        let client = SteamGridDb::new(fetcher.clone(), "k");
        let art = client
            .art(77, ArtKind::Cover, &SgdbArtOptions::default())
            .await
            .unwrap();
        assert_eq!(art.len(), 2);
        assert_eq!(art[0].fonte, "SteamGridDB");
        assert_eq!(art[0].thumb, art[0].url);
        assert_eq!(art[0].largura, Some(600));
        assert_eq!(art[0].autor, "artist");
        assert!(!art[0].animado);
        assert!(art[1].animado);
        assert_eq!(art[1].thumb, "https://cdn2.steamgriddb.com/thumb/def.jpg");
        assert_eq!(art[1].autor, "");
        assert_eq!(
            fetcher.requests()[0].url,
            "https://www.steamgriddb.com/api/v2/grids/game/77?types=static%2Canimated&dimensions=600x900%2C660x930%2C342x482&nsfw=false"
        );
    }

    #[tokio::test]
    async fn art_refusal_and_missing_data_are_handled() {
        let fetcher = MockFetcher::new(vec![
            json_response(serde_json::json!({"success": false})),
            json_response(serde_json::json!({"success": true})),
        ]);
        let client = SteamGridDb::new(fetcher, "k");
        assert_eq!(
            client
                .art(1, ArtKind::Logo, &SgdbArtOptions::default())
                .await
                .unwrap_err()
                .to_string(),
            "SteamGridDB recusou o pedido de arte"
        );
        assert!(client
            .art(1, ArtKind::Logo, &SgdbArtOptions::default())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn http_error_keeps_status_and_url() {
        let fetcher = MockFetcher::new(vec![Response::new(429, "rate limited")]);
        let client = SteamGridDb::new(fetcher, "k");
        let error = client.search("x").await.unwrap_err();
        assert_eq!(
            error,
            MediaError::Http {
                status: 429,
                url: "https://www.steamgriddb.com/api/v2/search/autocomplete/x".into(),
            }
        );
    }
}
