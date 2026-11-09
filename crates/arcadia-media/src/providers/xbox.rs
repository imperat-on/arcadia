//! Xbox / Microsoft Store: catálogo público, sem chave.
//!
//! A busca é fuzzy e devolve lixo para jogos que a loja não tem, então o
//! resultado passa por `titles_match` antes de chegar à UI. As imagens vêm
//! com `ImagePurpose` (Poster, SuperHeroArt, Logo…) e a URL recebe tamanho e
//! qualidade explícitos.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::http::{get_json, query_string, value_number, value_str, Fetcher, MOZILLA_UA};
use crate::providers::{ArtKind, Artwork, TextCandidate};
use crate::text::{strip_html, titles_match};

pub const XBOX_SEARCH: &str = "https://storeedgefd.dsx.mp.microsoft.com/v9.0/search";
pub const XBOX_CATALOG: &str = "https://displaycatalog.mp.microsoft.com/v7.0/products";

/// Que `ImagePurpose` serve para cada arte nossa, em ordem de preferência.
pub const XBOX_IMAGES: &[(&str, &[&str])] = &[
    ("cover", &["Poster", "BrandedKeyArt", "BoxArt"]),
    ("hero", &["SuperHeroArt", "TitledHeroArt", "Screenshot"]),
    ("logo", &["Logo"]),
];

/// Resultado da busca da loja.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XboxGame {
    pub id: String,
    pub titulo: String,
}

/// Imagem do produto localizado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XboxImage {
    pub purpose: String,
    pub uri: String,
    pub width: u32,
    pub height: u32,
}

/// `LocalizedProperties[0]` usado pela arte e pelo texto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XboxLocalizedProduct {
    pub product_title: String,
    pub product_description: String,
    pub images: Vec<XboxImage>,
}

/// Opções de [`xbox_img`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XboxImageOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub quality: u32,
}

impl Default for XboxImageOptions {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            quality: 100,
        }
    }
}

impl XboxImageOptions {
    pub fn sized(width: u32, height: u32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
            quality: 100,
        }
    }

    pub fn width_only(width: u32) -> Self {
        Self {
            width: Some(width),
            height: None,
            quality: 100,
        }
    }

    pub fn with_quality(mut self, quality: u32) -> Self {
        self.quality = quality;
        self
    }
}

fn images_for(kind: ArtKind) -> &'static [&'static str] {
    XBOX_IMAGES
        .iter()
        .find(|(name, _)| *name == kind.as_str())
        .map(|(_, purposes)| *purposes)
        .unwrap_or(&[])
}

/// As URIs vêm sem protocolo (`//store-images...`): o `<img>` aceita, o
/// `fetch` não.
pub fn xbox_uri(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        uri.to_string()
    }
}

/// `xboxImg`: `q`, `format`, e depois `w`/`h` quando presentes e não-zero.
pub fn xbox_img(uri: &str, options: XboxImageOptions) -> String {
    let base = xbox_uri(uri);
    if base.is_empty() {
        return base;
    }
    let mut params = vec![
        ("q", options.quality.to_string()),
        ("format", "jpg".to_string()),
    ];
    if let Some(width) = options.width.filter(|width| *width != 0) {
        params.push(("w", width.to_string()));
    }
    if let Some(height) = options.height.filter(|height| *height != 0) {
        params.push(("h", height.to_string()));
    }
    format!("{base}?{}", query_string(&params))
}

fn localized_product_from_json(value: &Value) -> XboxLocalizedProduct {
    XboxLocalizedProduct {
        product_title: value_str(value.get("ProductTitle")),
        product_description: value_str(value.get("ProductDescription")),
        images: value
            .get("Images")
            .and_then(Value::as_array)
            .map(|images| {
                images
                    .iter()
                    .map(|image| XboxImage {
                        purpose: value_str(image.get("ImagePurpose")),
                        uri: value_str(image.get("Uri")),
                        width: value_number(image.get("Width")).max(0.0) as u32,
                        height: value_number(image.get("Height")).max(0.0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Cliente Xbox; bases configuráveis para os testes.
#[derive(Clone)]
pub struct XboxClient {
    fetcher: Arc<dyn Fetcher>,
    search_base: String,
    catalog_base: String,
}

impl XboxClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            search_base: XBOX_SEARCH.to_string(),
            catalog_base: XBOX_CATALOG.to_string(),
        }
    }

    pub fn with_search_base(mut self, base: impl Into<String>) -> Self {
        self.search_base = base.into();
        self
    }

    pub fn with_catalog_base(mut self, base: impl Into<String>) -> Self {
        self.catalog_base = base.into();
        self
    }

    /// `xboxSearch`: resultados filtrados pelo título normalizado.
    pub async fn search(&self, title: &str, market: &str, locale: &str) -> Result<Vec<XboxGame>> {
        let params = query_string(&[
            ("query", title.to_string()),
            ("market", market.to_string()),
            ("locale", locale.to_string()),
            ("deviceFamily", "Windows.Desktop".to_string()),
        ]);
        let url = format!("{}?{}", self.search_base.trim_end_matches('/'), params);
        let json = get_json(self.fetcher.as_ref(), url, &[("User-Agent", MOZILLA_UA)]).await?;
        let games = json
            .get("Payload")
            .and_then(|payload| payload.get("SearchResults"))
            .and_then(Value::as_array)
            .map(|results| {
                results
                    .iter()
                    .filter(|result| titles_match(&value_str(result.get("Title")), title))
                    .map(|result| XboxGame {
                        id: value_str(result.get("ProductId")),
                        titulo: value_str(result.get("Title")),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(games)
    }

    /// `xboxSearch` com os defaults do cliente (`US`/`en-us`).
    pub async fn search_default(&self, title: &str) -> Result<Vec<XboxGame>> {
        self.search(title, "US", "en-us").await
    }

    /// `xboxProduto` com os defaults do cliente (`US`/`en-us`).
    pub async fn product_default(&self, product_id: &str) -> Result<Option<XboxLocalizedProduct>> {
        self.product(product_id, "US", "en-us").await
    }

    /// `xboxProduto`: primeira `LocalizedProperties` do primeiro produto.
    pub async fn product(
        &self,
        product_id: &str,
        market: &str,
        languages: &str,
    ) -> Result<Option<XboxLocalizedProduct>> {
        let params = query_string(&[
            ("bigIds", product_id.to_string()),
            ("market", market.to_string()),
            ("languages", languages.to_string()),
            ("MS-CV", "x".to_string()),
        ]);
        let url = format!("{}?{}", self.catalog_base.trim_end_matches('/'), params);
        let json = get_json(self.fetcher.as_ref(), url, &[("User-Agent", MOZILLA_UA)]).await?;
        Ok(json
            .get("Products")
            .and_then(Value::as_array)
            .and_then(|products| products.first())
            .and_then(|product| product.get("LocalizedProperties"))
            .and_then(Value::as_array)
            .and_then(|localized| localized.first())
            .filter(|localized| localized.is_object())
            .map(localized_product_from_json))
    }
}

/// `xboxArtDe`: mantém a ordem de preferência do tipo e, dentro dela, a maior
/// imagem primeiro.
pub fn xbox_art_de(product: &XboxLocalizedProduct, kind: ArtKind) -> Vec<Artwork> {
    let wanted = images_for(kind);
    let mut images: Vec<&XboxImage> = product
        .images
        .iter()
        .filter(|image| wanted.contains(&image.purpose.as_str()))
        .collect();
    images.sort_by(|left, right| {
        let left_index = wanted.iter().position(|purpose| *purpose == left.purpose);
        let right_index = wanted.iter().position(|purpose| *purpose == right.purpose);
        left_index
            .cmp(&right_index)
            .then_with(|| right.width.cmp(&left.width))
    });
    images
        .into_iter()
        .map(|image| Artwork {
            fonte: "Xbox".to_string(),
            url: xbox_img(
                &image.uri,
                XboxImageOptions {
                    width: Some(image.width),
                    height: Some(image.height),
                    quality: 100,
                },
            ),
            thumb: xbox_img(
                &image.uri,
                XboxImageOptions::width_only(320).with_quality(90),
            ),
            largura: Some(image.width),
            altura: Some(image.height),
            animado: false,
            autor: "oficial".to_string(),
        })
        .collect()
}

/// `xboxTextoDe`: descrição no idioma da loja.
pub fn xbox_texto_de(product: &XboxLocalizedProduct) -> Vec<TextCandidate> {
    let text = strip_html(&product.product_description);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![TextCandidate::new(
            format!("Xbox — {}", product.product_title),
            text,
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};

    fn local_product() -> XboxLocalizedProduct {
        XboxLocalizedProduct {
            product_title: "Portal 2".into(),
            product_description: "<p>Descrição</p>".into(),
            images: vec![
                XboxImage {
                    purpose: "Poster".into(),
                    uri: "//img/poster.jpg".into(),
                    width: 720,
                    height: 1080,
                },
                XboxImage {
                    purpose: "Poster".into(),
                    uri: "//img/poster2.jpg".into(),
                    width: 1440,
                    height: 2160,
                },
                XboxImage {
                    purpose: "Logo".into(),
                    uri: "//img/logo.png".into(),
                    width: 0,
                    height: 0,
                },
                XboxImage {
                    purpose: "BoxArt".into(),
                    uri: "//img/box.jpg".into(),
                    width: 1080,
                    height: 1080,
                },
            ],
        }
    }

    #[test]
    fn uri_and_image_size_follow_the_client_query() {
        assert_eq!(
            xbox_uri("//store-images/x.jpg"),
            "https://store-images/x.jpg"
        );
        assert_eq!(xbox_uri("https://x/y.jpg"), "https://x/y.jpg");
        assert_eq!(xbox_uri(""), "");
        assert_eq!(
            xbox_img("//img/p.jpg", XboxImageOptions::sized(720, 1080)),
            "https://img/p.jpg?q=100&format=jpg&w=720&h=1080"
        );
        assert_eq!(
            xbox_img(
                "//img/p.jpg",
                XboxImageOptions::width_only(320).with_quality(90)
            ),
            "https://img/p.jpg?q=90&format=jpg&w=320"
        );
        assert_eq!(xbox_img("", XboxImageOptions::default()), "");
        assert_eq!(
            xbox_img("//img/p.jpg", XboxImageOptions::sized(0, 0)),
            "https://img/p.jpg?q=100&format=jpg"
        );
    }

    #[tokio::test]
    async fn search_contract_filters_fuzzy_results() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "Payload": {
                "SearchResults": [
                    {"ProductId": "9A", "Title": "Portal 2"},
                    {"ProductId": "9B", "Title": "Portal 2: Still Alive"},
                    {"ProductId": "9C", "Title": "Eldrynn"}
                ]
            }
        }))]);
        let client = XboxClient::new(fetcher.clone());
        let games = client.search("Portal 2", "BR", "pt-br").await.unwrap();
        assert_eq!(
            games,
            vec![
                XboxGame {
                    id: "9A".into(),
                    titulo: "Portal 2".into()
                },
                XboxGame {
                    id: "9B".into(),
                    titulo: "Portal 2: Still Alive".into()
                },
            ]
        );
        let requests = fetcher.requests();
        assert_eq!(
            requests[0].url,
            "https://storeedgefd.dsx.mp.microsoft.com/v9.0/search?query=Portal+2&market=BR&locale=pt-br&deviceFamily=Windows.Desktop"
        );
        assert_eq!(
            requests[0]
                .headers
                .iter()
                .find(|(name, _)| name == "User-Agent")
                .map(|(_, value)| value.as_str()),
            Some(MOZILLA_UA)
        );
    }

    #[tokio::test]
    async fn search_without_payload_is_empty() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({}))]);
        let client = XboxClient::new(fetcher);
        assert!(client.search("x", "US", "en-us").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn product_contract_reads_first_localized_property() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "Products": [{
                "LocalizedProperties": [{
                    "ProductTitle": "Portal 2",
                    "ProductDescription": "<b>Desc</b>",
                    "Images": [{"ImagePurpose": "Poster", "Uri": "//i/p.jpg", "Width": 720, "Height": 1080}]
                }]
            }]
        }))]);
        let client = XboxClient::new(fetcher.clone());
        let product = client.product("9A", "US", "en-us").await.unwrap().unwrap();
        assert_eq!(product.product_title, "Portal 2");
        assert_eq!(product.images[0].width, 720);
        assert_eq!(
            fetcher.requests()[0].url,
            "https://displaycatalog.mp.microsoft.com/v7.0/products?bigIds=9A&market=US&languages=en-us&MS-CV=x"
        );
    }

    #[tokio::test]
    async fn product_missing_products_is_none_and_http_error_propagates() {
        let fetcher = MockFetcher::new(vec![
            json_response(serde_json::json!({"Products": []})),
            Response::new(500, ""),
        ]);
        let client = XboxClient::new(fetcher);
        assert_eq!(client.product("x", "US", "en-us").await.unwrap(), None);
        assert!(client.product("x", "US", "en-us").await.is_err());
    }

    #[test]
    fn art_prefers_purpose_order_then_largest_width() {
        let art = xbox_art_de(&local_product(), ArtKind::Cover);
        assert_eq!(art.len(), 3);
        assert!(art[0].url.contains("poster2.jpg"));
        assert!(art[1].url.contains("poster.jpg"));
        assert!(art[2].url.contains("box.jpg"));
        assert_eq!(
            art[0].thumb,
            "https://img/poster2.jpg?q=90&format=jpg&w=320"
        );
        assert_eq!(art[0].largura, Some(1440));
        assert_eq!(art[0].fonte, "Xbox");
        assert_eq!(art[0].autor, "oficial");
        assert!(xbox_art_de(&local_product(), ArtKind::Logo)[0]
            .url
            .contains("logo.png"));
        assert!(xbox_art_de(&local_product(), ArtKind::Hero).is_empty());
    }

    #[test]
    fn texts_strip_html_and_use_product_title() {
        assert_eq!(
            xbox_texto_de(&local_product()),
            vec![TextCandidate::new("Xbox — Portal 2", "Descrição")]
        );
        let empty = XboxLocalizedProduct {
            product_title: "X".into(),
            product_description: "<p></p>".into(),
            images: vec![],
        };
        assert!(xbox_texto_de(&empty).is_empty());
    }
}
