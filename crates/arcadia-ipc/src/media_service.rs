//! Arte automática do trilho/hero: cascata PSN → Worker → Xbox → SGDB, com
//! cache em disco e overrides manuais vencendo.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use arcadia_media::art::ArtResolver;
use arcadia_media::http::{Fetcher, HttpClientFetcher};
use arcadia_media::providers::steamgriddb::SteamGridDb;

use crate::AppState;

const ART_CACHE: &str = "arte_quadrada.json";
const LOGO_CACHE: &str = "logo_auto.json";
const CACHE_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn read_cache(path: &Path) -> Map<String, Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Map::new();
    };
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(&raw) else {
        return Map::new();
    };
    let now = now_ms();
    map.retain(|_, value| {
        value
            .get("quando")
            .and_then(Value::as_i64)
            .map(|stamp| now - stamp <= CACHE_TTL_MS)
            .unwrap_or(false)
    });
    map
}

fn write_cache(path: &Path, cache: &Map<String, Value>) {
    let value = Value::Object(cache.clone());
    if let Ok(text) = serde_json::to_string(&value) {
        let _ = arcadia_store::atomic::write_atomic_text(path, &text);
    }
}

fn resolver(state: &AppState) -> Option<ArtResolver> {
    let api = state.api()?;
    let fetcher: Arc<dyn Fetcher> = Arc::new(HttpClientFetcher::new(api.http().clone()));
    let mut resolver = ArtResolver::new(Arc::clone(&fetcher));
    if let Some(url) = api
        .config()
        .url
        .strip_suffix('/')
        .map(str::to_string)
        .filter(|url| !url.is_empty())
    {
        resolver = resolver.with_worker_base(url);
    }
    let key = arcadia_store::config::read_config(&state.data_dir)
        .get("steamgriddb_api_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if !key.is_empty() {
        resolver = resolver.with_sgdb(SteamGridDb::new(fetcher, key));
    }
    Some(resolver)
}

pub fn manual_overrides_public(state: &AppState) -> Map<String, Value> {
    manual_overrides(state)
}

fn manual_overrides(state: &AppState) -> Map<String, Value> {
    let path = arcadia_store::accounts::account_file(
        &state.data_dir,
        state.account().as_deref(),
        "overrides.json",
    );
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn items(args: &[Value]) -> Vec<(String, String)> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    payload
        .get("itens")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| {
                    let id = item.get("id").and_then(Value::as_str)?.to_string();
                    let title = item.get("titulo").and_then(Value::as_str)?.to_string();
                    if id.is_empty() || title.is_empty() {
                        None
                    } else {
                        Some((id, title))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn has_manual_art(overrides: &Map<String, Value>, id: &str, key: &str) -> bool {
    overrides
        .get(id)
        .and_then(Value::as_object)
        .and_then(|entry| entry.get(key))
        .map(|value| !value.is_null())
        .unwrap_or(false)
}

pub async fn square_art(state: &AppState, args: &[Value]) -> Value {
    let list = items(args);
    if list.is_empty() {
        return json!({ "ok": true, "artes": {} });
    }
    let path = state.data_dir.join(ART_CACHE);
    let mut cache = read_cache(&path);
    let overrides = manual_overrides(state);
    let missing: Vec<(String, String)> = list
        .into_iter()
        .filter(|(id, _)| !cache.contains_key(id) && !has_manual_art(&overrides, id, "cover"))
        .collect();

    if !missing.is_empty() {
        if let Some(resolver) = resolver(state) {
            for (id, title) in missing {
                if let Some(art) = resolver.square_art(&title).await {
                    let mut entry = serde_json::to_value(&art).unwrap_or_else(|_| json!({}));
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("quando".into(), json!(now_ms()));
                    }
                    cache.insert(id, entry);
                }
            }
            write_cache(&path, &cache);
        }
    }

    let mut artes = Map::new();
    for (id, _) in items(args) {
        if let Some(url) = cache.get(&id).and_then(|entry| entry.get("url")) {
            artes.insert(id, url.clone());
        }
    }
    json!({ "ok": true, "artes": artes })
}

pub async fn logo_auto(state: &AppState, args: &[Value]) -> Value {
    let list = items(args);
    if list.is_empty() {
        return json!({ "ok": true, "logos": {} });
    }
    let path = state.data_dir.join(LOGO_CACHE);
    let mut cache = read_cache(&path);
    let overrides = manual_overrides(state);
    let missing: Vec<(String, String)> = list
        .into_iter()
        .filter(|(id, _)| !cache.contains_key(id) && !has_manual_art(&overrides, id, "logo"))
        .collect();

    if !missing.is_empty() {
        if let Some(resolver) = resolver(state) {
            for (id, title) in missing {
                if let Some(art) = resolver.logo(&title).await {
                    let mut entry = serde_json::to_value(&art).unwrap_or_else(|_| json!({}));
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("quando".into(), json!(now_ms()));
                    }
                    cache.insert(id, entry);
                }
            }
            write_cache(&path, &cache);
        }
    }

    let mut logos = BTreeMap::new();
    for (id, _) in items(args) {
        if let Some(url) = cache.get(&id).and_then(|entry| entry.get("url")) {
            logos.insert(id, url.clone());
        }
    }
    json!({ "ok": true, "logos": logos })
}

#[cfg(test)]
mod tests {
    use super::{items, read_cache};
    use serde_json::json;

    #[test]
    fn filters_items_without_id_or_title() {
        let list = items(&[json!({ "itens": [
            { "id": "1", "titulo": "A" },
            { "id": "", "titulo": "B" },
            { "id": "2", "titulo": "" }
        ]})]);
        assert_eq!(list, vec![("1".to_string(), "A".to_string())]);
    }

    #[test]
    fn cache_expires_old_entries() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-art-cache-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("arte_quadrada.json");
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "velho": { "url": "https://x/old.png", "quando": 1 },
                "novo": { "url": "https://x/new.png", "quando": super::now_ms() }
            }))
            .unwrap(),
        )
        .unwrap();
        let cache = read_cache(&path);
        assert!(!cache.contains_key("velho"));
        assert!(cache.contains_key("novo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
