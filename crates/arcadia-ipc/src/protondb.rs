//! ProtonDB por appid — consulta pública com cache de 24 h.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn cache() -> &'static Mutex<HashMap<String, (Instant, Value)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize(appid: &str) -> String {
    appid.trim().trim_start_matches("steam:").to_string()
}

pub async fn info(appid: &str) -> Value {
    let appid = normalize(appid);
    if appid.is_empty() {
        return Value::Null;
    }
    if let Ok(guard) = cache().lock() {
        if let Some((stamp, value)) = guard.get(&appid) {
            if stamp.elapsed() < Duration::from_secs(24 * 60 * 60) {
                return value.clone();
            }
        }
    }

    let url = format!("https://www.protondb.com/api/v1/reports/summaries/{appid}.json");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(_) => return Value::Null,
    };
    let value = match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            match response.json::<Value>().await {
                Ok(data) => json!({
                    "tier": data.get("tier").and_then(Value::as_str).unwrap_or_default(),
                    "score": data.get("score").filter(|value| value.is_number()).cloned().unwrap_or(Value::Null),
                    "deckCompatibility": data
                        .get("deckCompatibility")
                        .or_else(|| data.get("steamDeckCompatibilityCategory"))
                        .cloned()
                        .unwrap_or_else(|| json!("")),
                    "total": data
                        .get("total")
                        .or_else(|| data.get("reportCount"))
                        .cloned()
                        .unwrap_or_else(|| json!(0)),
                    "url": format!("https://www.protondb.com/app/{appid}")
                }),
                Err(_) => Value::Null,
            }
        }
        _ => Value::Null,
    };

    if let Ok(mut guard) = cache().lock() {
        if guard.len() > 100 {
            guard.clear();
        }
        guard.insert(appid, (Instant::now(), value.clone()));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn strips_steam_prefix() {
        assert_eq!(normalize("steam:570"), "570");
        assert_eq!(normalize(" 570 "), "570");
        assert_eq!(normalize(""), "");
    }
}
