//! Serviço de catálogo do IPC: cliente com espelho/ETag e estatísticas de jogo.

use std::sync::Arc;

use serde_json::{json, Value};

use arcadia_catalog::catalog::{CatalogClient, FsMirrorStore, NoAuth};

use crate::AppState;

pub fn client(state: &AppState) -> Option<CatalogClient> {
    let api = state.api()?;
    let store = Arc::new(FsMirrorStore::new(&state.data_dir));
    Some(CatalogClient::from_http_client(
        api.http().clone(),
        store,
        Arc::new(NoAuth),
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
    let summary = body.get("query_summary").cloned().unwrap_or_else(|| json!({}));
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
                    .filter(|comment| comment.get("positive").and_then(Value::as_bool) == Some(true))
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
        assert_eq!(first_body_data(&json!({ "data": { "owners": "1" } })), json!({ "owners": "1" }));
        assert_eq!(first_body_data(&json!({ "owners": "2" })), json!({ "owners": "2" }));
    }
}
