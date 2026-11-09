//! Wallhaven: wallpapers 16:9 para o fundo (hero).
//!
//! Sem chave no modo SFW. Se não houver nada em 4K, o cliente afrouxa para
//! 1440p em vez de deixar o usuário sem fundo.

use std::sync::Arc;

use crate::error::Result;
use crate::http::{get_json, query_string, value_number, value_str, Fetcher};
use crate::providers::Artwork;

pub const WALLHAVEN_BASE: &str = "https://wallhaven.cc/api/v1";
pub const WALLHAVEN_DEFAULT_ATLEAST: &str = "3840x2160";
pub const WALLHAVEN_FALLBACK_ATLEAST: &str = "2560x1440";

#[derive(Clone)]
pub struct WallhavenClient {
    fetcher: Arc<dyn Fetcher>,
    base: String,
}

impl WallhavenClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            base: WALLHAVEN_BASE.to_string(),
        }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn search_once(&self, title: &str, atleast: &str) -> Result<Vec<Artwork>> {
        let params = query_string(&[
            ("q", title.to_string()),
            ("categories", "110".to_string()),
            ("purity", "100".to_string()),
            ("ratios", "16x9".to_string()),
            ("atleast", atleast.to_string()),
            ("sorting", "relevance".to_string()),
        ]);
        let url = format!("{}/search?{}", self.base.trim_end_matches('/'), params);
        let json = get_json(self.fetcher.as_ref(), url, &[]).await?;
        Ok(json
            .get("data")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|wallpaper| {
                        let path = value_str(wallpaper.get("path"));
                        let thumb = wallpaper
                            .get("thumbs")
                            .and_then(|thumbs| {
                                let small = value_str(thumbs.get("small"));
                                if small.is_empty() {
                                    let large = value_str(thumbs.get("large"));
                                    if large.is_empty() {
                                        None
                                    } else {
                                        Some(large)
                                    }
                                } else {
                                    Some(small)
                                }
                            })
                            .unwrap_or_else(|| path.clone());
                        Artwork {
                            fonte: "Wallhaven".to_string(),
                            url: path,
                            thumb,
                            largura: optional_dimension(wallpaper.get("dimension_x")),
                            altura: optional_dimension(wallpaper.get("dimension_y")),
                            animado: false,
                            autor: String::new(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `wallhavenBusca`: tenta 4K e, se vazio, repete em 1440p.
    pub async fn search(&self, title: &str, atleast: &str) -> Result<Vec<Artwork>> {
        let mut artworks = self.search_once(title, atleast).await?;
        if artworks.is_empty() && atleast != WALLHAVEN_FALLBACK_ATLEAST {
            artworks = self.search_once(title, WALLHAVEN_FALLBACK_ATLEAST).await?;
        }
        Ok(artworks)
    }

    /// `wallhavenBusca(titulo)` com o `atleast` padrão de 4K.
    pub async fn search_default(&self, title: &str) -> Result<Vec<Artwork>> {
        self.search(title, WALLHAVEN_DEFAULT_ATLEAST).await
    }
}

fn optional_dimension(value: Option<&serde_json::Value>) -> Option<u32> {
    value
        .map(|value| value_number(Some(value)))
        .filter(|value| *value > 0.0)
        .map(|value| value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{json_response, MockFetcher};

    #[tokio::test]
    async fn search_contract_parses_wallpapers() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "data": [{
                "path": "https://w/full.jpg",
                "thumbs": {"small": "https://w/small.jpg", "large": "https://w/large.jpg"},
                "dimension_x": 3840,
                "dimension_y": 2160
            }]
        }))]);
        let client = WallhavenClient::new(fetcher.clone());
        let art = client
            .search("Portal 2", WALLHAVEN_DEFAULT_ATLEAST)
            .await
            .unwrap();
        assert_eq!(art.len(), 1);
        assert_eq!(art[0].fonte, "Wallhaven");
        assert_eq!(art[0].url, "https://w/full.jpg");
        assert_eq!(art[0].thumb, "https://w/small.jpg");
        assert_eq!(art[0].largura, Some(3840));
        assert_eq!(art[0].altura, Some(2160));
        assert_eq!(
            fetcher.requests()[0].url,
            "https://wallhaven.cc/api/v1/search?q=Portal+2&categories=110&purity=100&ratios=16x9&atleast=3840x2160&sorting=relevance"
        );
    }

    #[tokio::test]
    async fn empty_4k_falls_back_to_1440p() {
        let fetcher = MockFetcher::new(vec![
            json_response(serde_json::json!({"data": []})),
            json_response(serde_json::json!({"data": [{
                "path": "https://w/f.jpg",
                "thumbs": {"large": "https://w/l.jpg"},
                "dimension_x": 2560,
                "dimension_y": 1440
            }]})),
        ]);
        let client = WallhavenClient::new(fetcher.clone());
        let art = client.search("x", WALLHAVEN_DEFAULT_ATLEAST).await.unwrap();
        assert_eq!(art.len(), 1);
        assert_eq!(art[0].thumb, "https://w/l.jpg");
        let requests = fetcher.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].url.contains("atleast=3840x2160"));
        assert!(requests[1].url.contains("atleast=2560x1440"));
    }

    #[tokio::test]
    async fn an_explicit_1440p_search_does_not_retry() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({"data": []}))]);
        let client = WallhavenClient::new(fetcher.clone());
        assert!(client
            .search("x", WALLHAVEN_FALLBACK_ATLEAST)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(fetcher.requests().len(), 1);
    }

    #[tokio::test]
    async fn missing_thumbs_fall_back_to_the_full_path() {
        let fetcher = MockFetcher::new(vec![json_response(serde_json::json!({
            "data": [{"path": "https://w/f.jpg", "thumbs": {}}]
        }))]);
        let client = WallhavenClient::new(fetcher);
        let art = client.search("x", WALLHAVEN_DEFAULT_ATLEAST).await.unwrap();
        assert_eq!(art[0].thumb, "https://w/f.jpg");
        assert_eq!(art[0].largura, None);
    }
}
