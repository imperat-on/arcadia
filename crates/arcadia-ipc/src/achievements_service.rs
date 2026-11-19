//! Catálogo completo sob demanda; achievements.json continua dono dos unlocks.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use arcadia_achievements::loader::{
    normalize_steam_icon_url, rebuild_app, AchievementsStore, SteamIcons,
};
use arcadia_achievements::steam_page::{
    fetch_achievements_for_app, merge_catalog_items, AchievementsHttp, HttpResponse,
};
use arcadia_achievements::{schema_from_bytes, schema_path, SystemClock};
use arcadia_store::accounts::account_file;
use async_trait::async_trait;
use serde_json::Value;

use crate::{steam_service, AppState};

#[derive(Default)]
pub(crate) struct SteamHttp {
    client: OnceLock<Result<reqwest::Client, String>>,
}

#[async_trait]
impl AchievementsHttp for SteamHttp {
    async fn get(&self, url: &str, timeout: Duration) -> Result<HttpResponse, String> {
        if timeout.is_zero() {
            return Err("timeout ao buscar conquistas".into());
        }
        let client = self
            .client
            .get_or_init(|| {
                reqwest::Client::builder()
                    .user_agent("Mozilla/5.0")
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT_LANGUAGE, "pt-BR,pt;q=0.9")
            .timeout(timeout)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|error| error.to_string())?;
        Ok(HttpResponse { status, body })
    }
}

fn local_items(path: &std::path::Path, appid: &str) -> Vec<Value> {
    let store = AchievementsStore::load(path);
    let items = store.items(appid);
    if items.is_empty() {
        store.items(&format!("steam:{appid}"))
    } else {
        items
    }
}

/// Dados antigos guardam o host legado, que devolve 404 para jogos novos.
/// Normaliza na leitura para corrigir o cache existente sem reescrever JSON.
fn normalize_icons(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(fixed) = normalize_steam_icon_url(text) {
                *text = fixed;
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_icons(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_icons(item);
            }
        }
        _ => {}
    }
}

fn items_value(items: Vec<Value>) -> Value {
    let mut value = Value::Array(items);
    normalize_icons(&mut value);
    value
}

pub(crate) async fn get(state: &AppState, raw: &str) -> Value {
    let appid = steam_service::normalize_appid(raw);
    if appid.is_empty() {
        return Value::Array(Vec::new());
    }
    let account = state.account();
    let path = account_file(&state.data_dir, account.as_deref(), "achievements.json");
    if !appid.bytes().all(|byte| byte.is_ascii_digit()) {
        return items_value(local_items(&path, &appid));
    }
    let stats_dir = state
        .with_achievements(|runtime| runtime.stats_dir())
        .unwrap_or_default();
    let schema = std::fs::read(schema_path(&stats_dir, &appid))
        .ok()
        .map(|bytes| schema_from_bytes(&appid, &bytes));
    if let Some(schema) = schema.filter(|schema| !schema.is_empty()) {
        let mut store = AchievementsStore::new();
        store.set_items(&appid, local_items(&path, &appid));
        let icons = SteamIcons::new(
            stats_dir.join("achievement_images"),
            state.data_dir.join("achievement_icons"),
        );
        // A leitura não ingere progresso nem grava unlocks: o runtime aplica o gate.
        rebuild_app(&mut store, &appid, &schema, &BTreeMap::new(), &icons);
        return items_value(store.items(&appid));
    }

    let cache_path = account_file(
        &state.data_dir,
        account.as_deref(),
        "achievements_store.json",
    );
    let catalog = match fetch_achievements_for_app(
        &appid,
        &cache_path,
        state.achievements_http.as_ref(),
        &SystemClock,
    )
    .await
    {
        Ok(items) => items,
        Err(error) => {
            eprintln!("[achievements] catálogo {appid}: {error}");
            Vec::new()
        }
    };
    // O poll pode desbloquear algo enquanto o HTTP está em andamento.
    items_value(merge_catalog_items(catalog, &local_items(&path, &appid)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_legacy_achievement_icons_in_existing_cache_payloads() {
        let mut value = json!([{
            "icon": "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/4115450/a.jpg",
            "icongray": "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/4115450/b.jpg"
        }]);
        normalize_icons(&mut value);
        assert_eq!(
            value[0]["icon"],
            "https://shared.fastly.steamstatic.com/community_assets/images/apps/4115450/a.jpg"
        );
        assert_eq!(
            value[0]["icongray"],
            "https://shared.fastly.steamstatic.com/community_assets/images/apps/4115450/b.jpg"
        );
    }
}
