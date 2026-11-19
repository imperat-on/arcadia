//! Catálogo público da Steam, com HTTP/relógio injetados e cache separado dos unlocks.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};

use crate::fsx::Clock;
use crate::js::js_number_value;
use crate::loader::{steam_icon_url, AchievementsStore};
use crate::r#match::{build_index, find_item_index};

pub const STORE_TTL_MS: i64 = 30 * 24 * 3600 * 1000;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// O adaptador aplica User-Agent Mozilla/5.0, Accept-Language pt-BR e o timeout.
#[async_trait]
pub trait AchievementsHttp: Send + Sync {
    async fn get(&self, url: &str, timeout: Duration) -> Result<HttpResponse, String>;
}

static ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<div class="achieveRow[^"]*"[^>]*>(.*?)<div style="clear: both;">"#).unwrap()
});
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bid="([^"]+)""#).unwrap());
static ICON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<img src="([^"]+)""#).unwrap());
static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<h3>(.*?)</h3>").unwrap());
static DESC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<h5>(.*?)</h5>").unwrap());
static PERCENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"achievePercent">([\d.,]+)%<"#).unwrap());
static SSR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<script\b[^>]*\bid="valve-ssr-data"[^>]*>(.*?)</script>"#).unwrap()
});

fn unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Linhas do HTML legado; ainda não acrescenta estado de desbloqueio.
pub fn parse_achievements_html(page: &str) -> Vec<Value> {
    let mut items = Vec::new();
    for row in ROW.captures_iter(page) {
        let body = &row[1];
        let (Some(icon), Some(title)) = (ICON.captures(body), TITLE.captures(body)) else {
            continue;
        };
        let opening = row[0].split_once('>').map(|(tag, _)| tag).unwrap_or("");
        let apiname = ID
            .captures(opening)
            .map(|id| id[1].trim().to_string())
            .unwrap_or_default();
        let percent = PERCENT
            .captures(body)
            .and_then(|pct| pct[1].replace(',', ".").parse::<f64>().ok());
        items.push(json!({
            "apiname": apiname,
            "title": unescape(&title[1]).trim(),
            "desc": DESC.captures(body).map(|desc| unescape(&desc[1]).trim().to_string()).unwrap_or_default(),
            "icon": &icon[1],
            "icongray": &icon[1],
            "percent": percent
        }));
    }
    if items.is_empty() {
        return parse_ssr_achievements(page);
    }
    items
}

// A página atual redireciona /stats/<id>/achievements para /achievements/<id>.
// O schema e os percentuais vêm no estado público SSR, não em achieveRow.
fn parse_ssr_achievements(page: &str) -> Vec<Value> {
    let Some(script) = SSR.captures(page) else {
        return Vec::new();
    };
    let Ok(ssr) = serde_json::from_str::<Value>(&script[1]) else {
        return Vec::new();
    };
    let Some(raw_queries) = ssr
        .pointer("/renderContext/queryData")
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(raw_queries) else {
        return Vec::new();
    };
    let Some(queries) = data.get("queries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let percentages = queries.iter().find_map(|query| {
        query
            .pointer("/state/data/percentages")
            .and_then(Value::as_object)
    });
    let mut items = Vec::new();
    for query in queries {
        let Some(schema) = query.pointer("/state/data") else {
            continue;
        };
        let (Some(appid), Some(groups)) = (
            schema.get("appid").and_then(Value::as_u64),
            schema.get("groups").and_then(Value::as_array),
        ) else {
            continue;
        };
        for achievement in groups
            .iter()
            .filter_map(|group| group.get("achievements").and_then(Value::as_array))
            .flatten()
        {
            let text = |key| achievement.get(key).and_then(Value::as_str).unwrap_or("");
            if text("name").is_empty() {
                continue;
            }
            let icon =
                steam_icon_url(&appid.to_string(), text("icon_achieved")).unwrap_or_default();
            let icongray = steam_icon_url(&appid.to_string(), text("icon_unachieved"))
                .unwrap_or_else(|| icon.clone());
            let percent = achievement
                .get("internal_key")
                .and_then(Value::as_u64)
                .and_then(|key| percentages.and_then(|values| values.get(&key.to_string())))
                .and_then(Value::as_f64);
            items.push(json!({
                "apiname": text("api_name"), "title": text("name"),
                "desc": text("description"), "icon": icon, "icongray": icongray,
                "percent": percent
            }));
        }
    }
    items
}

pub fn fetch_consent_url(appid: &str) -> String {
    format!(
        "https://steamcommunity.com/stats/{appid}/achievements?l=brazilian&gid=0&birthyear=2005"
    )
}

pub fn apinames_api_url(appid: &str) -> String {
    format!("https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid={appid}")
}

fn percent_key(value: &Value) -> Option<String> {
    let number = js_number_value(value).filter(|number| number.is_finite())?;
    // toFixed(1) arredonda empates exatos para longe de zero; Rust usa half-even.
    // Nessa precisão os únicos empates binários exatos terminam em .25 ou .75.
    let fraction = number.abs().fract();
    if fraction == 0.25 || fraction == 0.75 {
        Some(format!("{:.1}", (number * 10.0).round() / 10.0))
    } else {
        Some(format!("{number:.1}"))
    }
}

/// Só casa percentuais únicos dos dois lados; nunca depende da ordem da API.
pub fn apinames_by_percent(items: &[Value], percentages: &[Value]) -> Vec<Value> {
    let mut names: HashMap<String, Option<&str>> = HashMap::new();
    for entry in percentages {
        let (Some(key), Some(name)) = (
            entry.get("percent").and_then(percent_key),
            entry
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty()),
        ) else {
            continue;
        };
        names
            .entry(key)
            .and_modify(|value| *value = None)
            .or_insert(Some(name));
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for key in items
        .iter()
        .filter_map(|item| item.get("percent").and_then(percent_key))
    {
        *counts.entry(key).or_default() += 1;
    }
    items
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let mut item = item.clone();
            if item
                .get("apiname")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            {
                let key = item.get("percent").and_then(percent_key);
                let name = key
                    .as_ref()
                    .filter(|key| counts.get(*key) == Some(&1))
                    .and_then(|key| names.get(key))
                    .copied()
                    .flatten();
                item["apiname"] = Value::String(
                    name.map(str::to_string)
                        .unwrap_or_else(|| format!("ach_{:02}", position + 1)),
                );
            }
            item
        })
        .collect()
}

pub async fn fetch_apinames_da_api(
    appid: &str,
    http: &dyn AchievementsHttp,
    timeout: Duration,
) -> Vec<Value> {
    let Ok(response) = http.get(&apinames_api_url(appid), timeout).await else {
        return Vec::new();
    };
    if !response.is_success() {
        return Vec::new();
    }
    serde_json::from_str::<Value>(&response.body)
        .ok()
        .and_then(|data| {
            data.pointer("/achievementpercentages/achievements")?
                .as_array()
                .cloned()
        })
        .unwrap_or_default()
}

fn synthetic_apiname(item: &Value) -> bool {
    item.get("apiname")
        .and_then(Value::as_str)
        .and_then(|name| name.strip_prefix("ach_"))
        .map(|tail| !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()))
        .unwrap_or(false)
}

fn locked_items(items: &mut [Value]) {
    for item in items {
        item["achieved"] = Value::Bool(false);
        item["unlock"] = Value::from(0);
    }
}

pub async fn fetch_achievements_for_app(
    appid: &str,
    store_path: &Path,
    http: &dyn AchievementsHttp,
    clock: &dyn Clock,
) -> Result<Vec<Value>, String> {
    if appid.is_empty() || !appid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(Vec::new());
    }
    let cache = AchievementsStore::load(store_path);
    let mut items = cache.items(appid);
    items.retain(Value::is_object);
    let at = cache
        .entry(appid)
        .and_then(|entry| entry.get("at"))
        .and_then(Value::as_i64);
    if !items.is_empty()
        && !items.iter().all(synthetic_apiname)
        && at
            .map(|at| clock.now_ms().saturating_sub(at) < STORE_TTL_MS)
            .unwrap_or(false)
    {
        locked_items(&mut items);
        return Ok(items);
    }

    let deadline = Instant::now() + FETCH_TIMEOUT;
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let url = fetch_consent_url(appid);
    let response = http.get(&url, remaining()).await?;
    if !response.is_success() {
        return Err(format!("HTTP {}", response.status));
    }
    items = parse_achievements_html(&response.body);
    if items.is_empty() && response.body.contains("app_agegate") {
        let response = http
            .get(&format!("{url}&agree_to_agegate=1"), remaining())
            .await?;
        if response.is_success() {
            items = parse_achievements_html(&response.body);
        }
    }
    if items.iter().any(|item| {
        item.get("apiname")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
    }) {
        items = apinames_by_percent(
            &items,
            &fetch_apinames_da_api(appid, http, remaining()).await,
        );
    }
    for (position, item) in items.iter_mut().enumerate() {
        if item
            .get("apiname")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            item["apiname"] = Value::String(format!("ach_{:02}", position + 1));
        }
        item["block"] = Value::Null;
        item["bit"] = Value::Null;
        if item.get("percent").and_then(Value::as_f64).is_none() {
            item["percent"] = Value::from(0);
        }
    }
    locked_items(&mut items);
    if !items.is_empty() {
        // Relê depois do await para preservar entradas de outros appids.
        let mut cache = AchievementsStore::load(store_path);
        cache.set_items(appid, items.clone());
        cache
            .entry_mut(appid)
            .unwrap()
            .insert("at".into(), Value::from(clock.now_ms()));
        cache
            .save_atomic(store_path)
            .map_err(|error| error.to_string())?;
    }
    Ok(items)
}

/// O catálogo fornece os itens faltantes; estado e campos locais sempre vencem.
pub fn merge_catalog_items(catalog: Vec<Value>, local: &[Value]) -> Vec<Value> {
    if catalog.is_empty() {
        return local.to_vec();
    }
    let index = build_index(local);
    let mut used = vec![false; local.len()];
    let mut items = Vec::with_capacity(catalog.len().max(local.len()));
    for mut item in catalog {
        item["achieved"] = Value::Bool(false);
        item["unlock"] = Value::from(0);
        let previous = find_item_index(
            &index,
            local,
            item.get("apiname").and_then(Value::as_str),
            item.get("title").and_then(Value::as_str),
        );
        if let Some(position) = previous {
            used[position] = true;
            item = crate::consolidar::fundir(&local[position], &item);
        }
        items.push(item);
    }
    items.extend(
        local
            .iter()
            .enumerate()
            .filter(|(position, _)| !used[*position])
            .map(|(_, item)| item.clone()),
    );
    items
}

#[cfg(test)]
mod tests;
