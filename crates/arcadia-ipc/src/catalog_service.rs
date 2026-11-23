//! Serviço de catálogo do IPC: cliente com espelho/ETag e estatísticas de jogo.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use arcadia_api::ApiClient;
use arcadia_catalog::catalog::{
    AuthProvider, CatalogClient, CatalogOptions, FsMirrorStore, NoAuth,
};
use arcadia_retro::catalog::RemoteCatalogProvider;
use arcadia_steamstore::store::StoreClient;

use crate::AppState;

/// Catálogo remoto consumido pelo [`arcadia_retro::catalog::RetroServerCatalog`].
pub struct CatalogRemote {
    client: CatalogClient,
}

impl CatalogRemote {
    pub fn new(client: CatalogClient) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl RemoteCatalogProvider for CatalogRemote {
    async fn catalog_get(&self, path: &str, no_mirror: bool) -> Result<Value, String> {
        let options = CatalogOptions::new().with_no_mirror(no_mirror);
        let response = self.client.get_with(path, options).await;
        match response.data {
            Some(data) => Ok(data),
            None => Err(response
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "catálogo vazio".to_string())),
        }
    }
}

/// Auth do catálogo: Bearer da sessão + apikey; refresh real no 401.
struct SessionAuth {
    api: Arc<ApiClient>,
    token: Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl AuthProvider for SessionAuth {
    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("apikey".to_string(), "arcadia-dummy-key".to_string())];
        if let Some(token) = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        headers
    }

    async fn refresh(&self) -> bool {
        let Ok(session) = self.api.auth().refresh_session().await else {
            return false;
        };
        *self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session.access_token);
        true
    }
}

pub fn client(state: &AppState) -> Option<CatalogClient> {
    let api = state.api()?;
    let store = Arc::new(FsMirrorStore::new(&state.data_dir));
    let auth: Arc<dyn AuthProvider> = match state.access_token() {
        Some(token) => Arc::new(SessionAuth {
            api: Arc::clone(api),
            token: Mutex::new(Some(token)),
        }),
        None => Arc::new(NoAuth),
    };
    Some(CatalogClient::from_http_client(
        api.http().clone(),
        store,
        auth,
    ))
}

fn first_body_data(body: &Value) -> Value {
    body.get("data").cloned().unwrap_or_else(|| body.clone())
}

async fn steam_reviews(appid: &str) -> Option<Value> {
    let url = format!(
        "https://store.steampowered.com/appreviews/{appid}?json=1&language=english&purchase_type=all&filter=all&num_per_page=50"
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let body: Value = client.get(url).send().await.ok()?.json().await.ok()?;
    let summary = body
        .get("query_summary")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let total = summary
        .get("total_reviews")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let positive = summary
        .get("total_positive")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let comments: Vec<Value> = body
        .get("reviews")
        .and_then(Value::as_array)
        .map(|reviews| {
            reviews
                .iter()
                .filter_map(|review| {
                    let text = review.get("review").and_then(Value::as_str)?.trim();
                    if text.is_empty() {
                        return None;
                    }
                    let author = review
                        .get("author")
                        .and_then(|author| author.get("steamid"))
                        .and_then(Value::as_str)
                        .map(|steamid| format!("Steam {}", steamid.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>()))
                        .unwrap_or_else(|| "Steam".to_string());
                    Some(json!({
                        "author": author,
                        "text": text,
                        "positive": review.get("voted_up").and_then(Value::as_bool).unwrap_or(false),
                        "hours": review
                            .get("author")
                            .and_then(|author| author.get("playtime_forever"))
                            .and_then(Value::as_i64)
                            .map(|minutes| (minutes as f64 / 60.0).round() as i64)
                            .unwrap_or(0),
                        "helpful": review.get("votes_up").and_then(Value::as_i64).unwrap_or(0)
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(json!({
        "reviewDesc": summary.get("review_score_desc").and_then(Value::as_str).unwrap_or(""),
        "reviewPositivePct": if total > 0 { json!((positive as f64 / total as f64 * 100.0).round() as i64) } else { Value::Null },
        "totalReviews": total,
        "comments": comments
    }))
}

/// `game:sysinfo`: requisitos/tamanhos/descrição do catálogo + appdetails.
pub async fn game_sysinfo(state: &AppState, store: Option<&StoreClient>, game: &Value) -> Value {
    let id = game.get("id").and_then(Value::as_str).unwrap_or_default();
    let title = game
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut appid = id.strip_prefix("steam:").unwrap_or_default().to_string();
    if appid.is_empty() {
        if let Some(store) = store {
            if !title.is_empty() {
                let response = store.suggest(title).await;
                if let Some(jogos) = response.jogos {
                    if let Some(first) = jogos.first() {
                        appid = first.appid.clone();
                    }
                }
            }
        }
    }
    if appid.is_empty() {
        return json!({ "ok": true, "info": null });
    }

    // São fontes independentes: a descrição/trailer não deve esperar duas
    // viagens de rede em série. O catálogo continua tendo precedência.
    let (catalog, details) = tokio::join!(
        async {
            let client = client(state)?;
            client
                .get(&format!("/catalog/v1/sysinfo/{appid}"))
                .await
                .data
        },
        async { store?.app_details(&appid).await.ok().flatten() }
    );
    let mut info = serde_json::Map::new();
    if let Some(body) = catalog {
        let data = body.get("data").cloned().unwrap_or(body);
        if let Some(object) = data.as_object() {
            for (key, value) in object {
                info.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(details) = details {
        if let Ok(Value::Object(object)) = serde_json::to_value(details) {
            for (key, value) in object {
                info.entry(key).or_insert(value);
            }
        }
    }
    if info.is_empty() {
        json!({ "ok": true, "info": null })
    } else {
        info.insert("appid".into(), json!(appid));
        json!({ "ok": true, "info": Value::Object(info) })
    }
}

pub async fn game_stats(state: &AppState, appid: &str) -> Value {
    let appid = appid.trim().trim_start_matches("steam:").to_string();
    if appid.is_empty() {
        return json!({ "ok": true, "info": null });
    }
    let Some(client) = client(state) else {
        return json!({ "ok": false, "error": "api_indisponivel" });
    };

    let mut info = serde_json::Map::new();
    if let Some(data) = client
        .get(&format!("/catalog/v1/stats/{appid}"))
        .await
        .data
        .map(|body| first_body_data(&body))
    {
        if let Some(object) = data.as_object() {
            for key in ["owners", "ccu"] {
                if let Some(value) = object.get(key) {
                    info.insert(key.to_string(), value.clone());
                }
            }
        }
    }

    let reviews = client
        .get(&format!("/catalog/v1/reviews/{appid}"))
        .await
        .data;
    let mut has_reviews = false;
    if let Some(body) = reviews {
        if let Some(list) = body.get("reviews").and_then(Value::as_array) {
            if !list.is_empty() {
                has_reviews = true;
                let comments: Vec<Value> = list
                    .iter()
                    .map(|review| {
                        json!({
                            "author": review.get("username").and_then(Value::as_str).unwrap_or("Usuário"),
                            "text": review.get("text").and_then(Value::as_str).unwrap_or(""),
                            "positive": review.get("positive").and_then(Value::as_bool).unwrap_or(false),
                            "hours": review.get("hours").and_then(Value::as_i64).unwrap_or(0),
                            "helpful": 0
                        })
                    })
                    .collect();
                let total = comments.len() as i64;
                let positives = comments
                    .iter()
                    .filter(|comment| {
                        comment.get("positive").and_then(Value::as_bool) == Some(true)
                    })
                    .count() as i64;
                info.insert("comments".into(), json!(comments));
                info.insert("totalReviews".into(), json!(total));
                info.insert(
                    "reviewPositivePct".into(),
                    json!((positives as f64 / total as f64 * 100.0).round() as i64),
                );
                info.insert(
                    "reviewDesc".into(),
                    json!(if positives * 100 >= total * 70 {
                        "Muito positivas"
                    } else {
                        "Positivas"
                    }),
                );
            }
        }
    }
    if !has_reviews {
        if let Some(fallback) = steam_reviews(&appid).await {
            if let Some(object) = fallback.as_object() {
                for (key, value) in object {
                    info.insert(key.clone(), value.clone());
                }
            }
        }
    }

    if info.is_empty() {
        json!({ "ok": true, "info": null })
    } else {
        json!({ "ok": true, "info": Value::Object(info) })
    }
}

#[cfg(test)]
mod tests {
    use super::first_body_data;
    use serde_json::json;

    #[test]
    fn unwraps_nested_data_body() {
        assert_eq!(
            first_body_data(&json!({ "data": { "owners": "1" } })),
            json!({ "owners": "1" })
        );
        assert_eq!(
            first_body_data(&json!({ "owners": "2" })),
            json!({ "owners": "2" })
        );
    }
}
