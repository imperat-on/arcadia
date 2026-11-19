//! Sync de conquistas — port de `app/electron/supabase/sync.js`.
//!
//! Offline-first: desbloqueios entram em `sync_queue.json`, sobem quando a
//! rede permite e nunca são perdidos em falha. Merge "quem desbloqueou
//! PRIMEIRO vence"; achieved é monotônico (OR). A fila preserva operações de
//! outros domínios que ainda não existem (`kind != "achievement"`).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use arcadia_api::ApiClient;
use arcadia_contracts::{normalize_sync_timestamp, resolve_achievement_conflict, MAX_SYNC_ITEMS};
use arcadia_store::accounts::AccountScope;

use crate::state::SyncStateFile;
use crate::util;
use crate::{ensure_session, map_api_error, SyncError};

pub const QUEUE_FILE: &str = "sync_queue.json";
pub const ACHIEVEMENTS_FILE: &str = "achievements.json";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementsPush {
    pub pushed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementsPull {
    pub pulled: usize,
    pub changed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AchievementsSync {
    pub pushed: usize,
    pub pulled: usize,
    pub changed: bool,
}

/// `normalizeTs` do `sync.js` (epoch em segundos).
pub fn normalize_ts(value: &Value) -> Option<i64> {
    normalize_sync_timestamp(value)
}

pub fn queue_path(scope: &AccountScope) -> std::path::PathBuf {
    scope.account_file(QUEUE_FILE)
}

pub fn load_queue(scope: &AccountScope) -> Vec<Value> {
    util::read_array_or_empty(&queue_path(scope))
}

pub fn queue_length(scope: &AccountScope) -> usize {
    load_queue(scope).len()
}

pub fn has_local_achievements(scope: &AccountScope) -> bool {
    match util::read_json(&scope.account_file(ACHIEVEMENTS_FILE)) {
        Some(Value::Object(store)) => !store.is_empty(),
        _ => false,
    }
}

/// Enfileira desbloqueios com dedupe por `(appid, apiname)` — earliest wins.
///
/// Itens de outros domínios existentes são preservados e não interpretados.
pub fn enqueue(scope: &AccountScope, items: &[Value]) -> Result<Vec<Value>, SyncError> {
    if items.is_empty() {
        return Ok(load_queue(scope));
    }
    let existing = load_queue(scope);
    let other: Vec<Value> = existing
        .iter()
        .filter(|item| {
            item.get("kind")
                .map(util::js_truthy)
                .map(|truthy| truthy && util::js_string(item.get("kind").unwrap()) != "achievement")
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    let mut map: HashMap<String, Value> = HashMap::new();
    for item in existing.iter().chain(items.iter()) {
        if item.get("kind").map(util::js_truthy).unwrap_or(false)
            && util::js_string(item.get("kind").unwrap()) != "achievement"
        {
            continue;
        }
        let Some(appid) = item.get("appid").filter(|value| util::js_truthy(value)) else {
            continue;
        };
        let Some(apiname) = item.get("apiname").filter(|value| util::js_truthy(value)) else {
            continue;
        };
        let achieved = item.get("achieved") == Some(&Value::Bool(true));
        let unlocked_at = normalize_ts(item.get("unlocked_at").unwrap_or(&Value::Null));
        if !achieved && unlocked_at.is_none() {
            continue;
        }
        let appid = util::js_string(appid);
        let apiname = util::js_string(apiname);
        let key = format!("{appid}|{apiname}");
        let mut enhanced = item.clone();
        if let Some(object) = enhanced.as_object_mut() {
            object.insert(
                "achieved".into(),
                Value::Bool(achieved || unlocked_at.is_some()),
            );
        }
        let merged =
            resolve_achievement_conflict(map.get(&key).cloned().unwrap_or(Value::Null), enhanced)
                .unwrap_or_else(|| {
                    json!({
                        "appid": appid,
                        "apiname": apiname,
                        "achieved": achieved || unlocked_at.is_some(),
                        "unlocked_at": unlocked_at,
                    })
                });
        map.insert(key, merged);
    }

    let mut achievements: Vec<(String, Value)> = map.into_iter().collect();
    achievements.sort_by(|left, right| left.0.cmp(&right.0));
    let merged: Vec<Value> = other
        .into_iter()
        .chain(achievements.into_iter().map(|(_, value)| value))
        .collect();
    save_queue(scope, &merged)?;
    Ok(merged)
}

fn save_queue(scope: &AccountScope, queue: &[Value]) -> Result<(), SyncError> {
    util::write_json_atomic(&queue_path(scope), &Value::Array(queue.to_vec()), false)?;
    Ok(())
}

fn is_queue_achievement(item: &Value) -> bool {
    let kind_ok = match item.get("kind") {
        None => true,
        Some(kind) => !util::js_truthy(kind) || util::js_string(kind) == "achievement",
    };
    kind_ok
        && item.get("appid").map(util::js_truthy).unwrap_or(false)
        && item.get("apiname").map(util::js_truthy).unwrap_or(false)
        && (item.get("achieved") == Some(&Value::Bool(true))
            || normalize_ts(item.get("unlocked_at").unwrap_or(&Value::Null)).is_some())
}

/// `pushDelta`: sobe a fila de conquistas (RPC idempotente).
pub async fn push_queue(
    client: &ApiClient,
    scope: &AccountScope,
) -> Result<AchievementsPush, SyncError> {
    ensure_session(client).await?;

    let queue: Vec<Value> = load_queue(scope)
        .into_iter()
        .filter(is_queue_achievement)
        .collect();
    if queue.is_empty() {
        return Ok(AchievementsPush::default());
    }

    let mut pushed = 0_usize;
    for chunk in queue.chunks(MAX_SYNC_ITEMS) {
        let p_items: Vec<Value> = chunk
            .iter()
            .map(|item| {
                let unlocked_at =
                    normalize_ts(item.get("unlocked_at").unwrap_or(&Value::Null)).unwrap_or(0);
                let achieved =
                    item.get("achieved") == Some(&Value::Bool(true)) || (unlocked_at > 0);
                json!({
                    "appid": item.get("appid").cloned().unwrap_or(Value::Null),
                    "apiname": item.get("apiname").cloned().unwrap_or(Value::Null),
                    "unlocked_at": unlocked_at,
                    "achieved": achieved,
                    "title": item.get("title").cloned().unwrap_or(Value::Null),
                    "icon": item.get("icon").cloned().unwrap_or(Value::Null),
                    "percent": item.get("percent").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();

        client
            .rpc("sync_achievements", json!({ "p_items": p_items }))
            .await
            .map_err(map_api_error)?;

        let sent: HashSet<String> = chunk
            .iter()
            .filter_map(|item| {
                Some(format!(
                    "{}|{}",
                    util::js_string(item.get("appid")?),
                    util::js_string(item.get("apiname")?)
                ))
            })
            .collect();
        let remaining: Vec<Value> = load_queue(scope)
            .into_iter()
            .filter(|item| {
                if !is_queue_achievement(item) {
                    return true;
                }
                let key = format!(
                    "{}|{}",
                    item.get("appid").map(util::js_string).unwrap_or_default(),
                    item.get("apiname").map(util::js_string).unwrap_or_default()
                );
                !sent.contains(&key)
            })
            .collect();
        save_queue(scope, &remaining)?;
        pushed += chunk.len();

        let mut state = SyncStateFile::new(scope).read_document();
        state.set_last_sync_at(util::now_seconds());
        state.set_last_error(None);
        SyncStateFile::new(scope).write_document(&state)?;
    }

    Ok(AchievementsPush { pushed })
}

/// `pullDelta`: baixa desde `lastPullAt` e aplica em `achievements.json`.
pub async fn pull_delta(
    client: &ApiClient,
    scope: &AccountScope,
) -> Result<AchievementsPull, SyncError> {
    pull_since(client, scope, false).await
}

/// `fullSync`: ignora `lastPullAt` (`p_since = null`).
pub async fn pull_all(
    client: &ApiClient,
    scope: &AccountScope,
) -> Result<AchievementsPull, SyncError> {
    pull_since(client, scope, true).await
}

async fn pull_since(
    client: &ApiClient,
    scope: &AccountScope,
    full: bool,
) -> Result<AchievementsPull, SyncError> {
    ensure_session(client).await?;

    let state_file = SyncStateFile::new(scope);
    let state = state_file.read_document();
    let p_since = if full {
        Value::Null
    } else {
        match state.last_pull_at() {
            Some(seconds) => util::iso_from_epoch_seconds(seconds)
                .map(Value::String)
                .unwrap_or(Value::Null),
            None => Value::Null,
        }
    };

    let response = client
        .rpc("pull_achievements", json!({ "p_since": p_since }))
        .await
        .map_err(map_api_error)?;
    // O JS aceita qualquer resposta não-array como pull vazio.
    let rows = response.as_array().cloned().unwrap_or_default();
    let changed = apply_pulled(scope, &rows)?;

    let mut state = state_file.read_document();
    state.set_last_pull_at(util::now_seconds());
    state.set_last_sync_at(util::now_seconds());
    state.set_last_error(None);
    state_file.write_document(&state)?;

    Ok(AchievementsPull {
        pulled: rows.len(),
        changed,
    })
}

/// Merge local (earliest-wins) das linhas do servidor.
pub fn apply_pulled(scope: &AccountScope, rows: &[Value]) -> Result<bool, SyncError> {
    if rows.is_empty() {
        return Ok(false);
    }
    let path = scope.account_file(ACHIEVEMENTS_FILE);
    let mut store = match util::read_json(&path) {
        Some(Value::Object(store)) => store,
        _ => Map::new(),
    };
    let mut changed = false;

    for row in rows {
        let Some(remote) = resolve_achievement_conflict(Value::Null, row.clone()) else {
            continue;
        };
        let Some(appid) = remote.get("appid").map(util::js_string) else {
            continue;
        };
        let Some(apiname) = remote.get("apiname").map(util::js_string) else {
            continue;
        };
        let entry = store
            .entry(appid.clone())
            .or_insert_with(|| json!({ "at": util::now_millis(), "items": [] }));
        if !entry.is_object() {
            *entry = json!({ "at": util::now_millis(), "items": [] });
        }
        let Some(app) = entry.as_object_mut() else {
            continue;
        };
        if !app.get("items").map(Value::is_array).unwrap_or(false) {
            app.insert("items".into(), Value::Array(Vec::new()));
        }
        let items = app
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .expect("items garantido acima");

        let remote_achieved = remote.get("achieved") == Some(&Value::Bool(true))
            || remote
                .get("unlocked_at")
                .and_then(util::js_number)
                .map(|value| value > 0.0)
                .unwrap_or(false);
        let remote_unlock = remote
            .get("unlocked_at")
            .filter(|value| !value.is_null())
            .and_then(util::js_number)
            .map(|value| value.floor() as i64);

        if let Some(index) = find_item_index(items, &apiname, remote.get("title")) {
            let item = &mut items[index];
            if remember_alias(item, &apiname) {
                changed = true;
            }
            if item
                .get("remoteApiname")
                .filter(|value| util::js_truthy(value))
                .is_none()
            {
                if let Some(object) = item.as_object_mut() {
                    object.insert("remoteApiname".into(), Value::String(apiname.clone()));
                    changed = true;
                }
            }
            let local_achieved = item.get("achieved") == Some(&Value::Bool(true));
            let local_unlock = item.get("unlock").and_then(normalize_ts);
            let new_achieved = local_achieved || remote_achieved;
            let new_unlock = match (local_unlock, remote_unlock) {
                (Some(local), Some(remote)) => Some(local.min(remote)),
                (Some(local), None) => Some(local),
                (None, Some(remote)) => Some(remote),
                (None, None) => None,
            };
            let old_unlock_normalized = item.get("unlock").and_then(normalize_ts);
            if item.get("achieved") != Some(&Value::Bool(new_achieved))
                || old_unlock_normalized != new_unlock
            {
                if let Some(object) = item.as_object_mut() {
                    object.insert("achieved".into(), Value::Bool(new_achieved));
                    object.insert(
                        "unlock".into(),
                        new_unlock.map(Value::from).unwrap_or(Value::Null),
                    );
                    changed = true;
                }
            }
        } else if remote_achieved || remote_unlock.is_some() {
            items.push(json!({
                "apiname": apiname,
                "remoteApiname": remote.get("apiname").cloned().unwrap_or(Value::Null),
                "title": remote.get("title").filter(|value| util::js_truthy(value))
                    .cloned().unwrap_or_else(|| Value::String(apiname.clone())),
                "desc": "",
                "icon": remote.get("icon").filter(|value| util::js_truthy(value))
                    .cloned().unwrap_or_else(|| Value::String(String::new())),
                "icongray": "",
                "achieved": remote_achieved,
                "unlock": remote_unlock.unwrap_or(0),
                "percent": remote.get("percent").filter(|value| !value.is_null())
                    .cloned().unwrap_or(Value::from(100)),
            }));
            changed = true;
        }
    }

    if changed {
        util::write_json_atomic(&path, &Value::Object(store), false)?;
    }
    Ok(changed)
}

/// `syncAllLocal`: catch-up que reenvia TODAS as conquistas locais.
pub async fn sync_all_local(
    client: &ApiClient,
    scope: &AccountScope,
) -> Result<AchievementsPush, SyncError> {
    ensure_session(client).await?;

    let store = match util::read_json(&scope.account_file(ACHIEVEMENTS_FILE)) {
        Some(Value::Object(store)) => store,
        _ => Map::new(),
    };
    let mut items: Vec<Value> = Vec::new();
    for (appid, data) in &store {
        let Some(app_items) = data.get("items").and_then(Value::as_array) else {
            continue;
        };
        for item in app_items {
            if item.get("achieved") != Some(&Value::Bool(true)) {
                continue;
            }
            let Some(apiname) = item
                .get("remoteApiname")
                .filter(|value| util::js_truthy(value))
                .or_else(|| item.get("apiname"))
                .filter(|value| util::js_truthy(value))
            else {
                continue;
            };
            let Some(unlock) = item.get("unlock").filter(|value| util::js_truthy(value)) else {
                continue;
            };
            items.push(json!({
                "appid": appid,
                "apiname": apiname,
                "unlocked_at": normalize_ts(unlock),
                "title": item.get("title").cloned().unwrap_or(Value::Null),
                "icon": item.get("icon").cloned().unwrap_or(Value::Null),
                "percent": item.get("percent").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    if items.is_empty() {
        return Ok(AchievementsPush::default());
    }

    let mut pushed = 0_usize;
    for chunk in items.chunks(MAX_SYNC_ITEMS) {
        let p_items: Vec<Value> = chunk
            .iter()
            .map(|item| {
                json!({
                    "appid": item.get("appid").cloned().unwrap_or(Value::Null),
                    "apiname": item.get("apiname").cloned().unwrap_or(Value::Null),
                    "unlocked_at": item.get("unlocked_at").cloned().unwrap_or(Value::Null),
                    "achieved": true,
                    "title": item.get("title").cloned().unwrap_or(Value::Null),
                    "icon": item.get("icon").cloned().unwrap_or(Value::Null),
                    "percent": item.get("percent").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        client
            .rpc("sync_achievements", json!({ "p_items": p_items }))
            .await
            .map_err(map_api_error)?;
        pushed += chunk.len();
    }

    let mut state = SyncStateFile::new(scope).read_document();
    state.set_last_sync_at(util::now_seconds());
    state.set_last_error(None);
    SyncStateFile::new(scope).write_document(&state)?;
    Ok(AchievementsPush { pushed })
}

/// `deleteAchievement`: `unlocked_at: null` remove no servidor; a fila local
/// pendente da mesma chave é purgada para não ressuscitar.
pub async fn delete_achievement(
    client: &ApiClient,
    scope: &AccountScope,
    appid: &str,
    apiname: &str,
) -> Result<(), SyncError> {
    ensure_session(client).await?;
    client
        .rpc(
            "sync_achievements",
            json!({ "p_items": [{ "appid": appid, "apiname": apiname, "unlocked_at": null }] }),
        )
        .await
        .map_err(map_api_error)?;

    let queue = load_queue(scope);
    let original_len = queue.len();
    let filtered: Vec<Value> = queue
        .into_iter()
        .filter(|item| {
            !(item.get("appid").map(util::js_string).as_deref() == Some(appid)
                && item.get("apiname").map(util::js_string).as_deref() == Some(apiname))
        })
        .collect();
    if filtered.len() != original_len {
        save_queue(scope, &filtered)?;
    }
    Ok(())
}

/// Reconcile de conquistas (login/boot): primeira sincronização é full pull.
pub async fn reconcile(
    client: &ApiClient,
    scope: &AccountScope,
    pull_only: bool,
) -> Result<AchievementsSync, SyncError> {
    let has_local = has_local_achievements(scope);
    let mut pushed = 0_usize;
    let mut pulled = 0_usize;
    let mut changed = false;

    if pull_only {
        let result = if has_local {
            pull_delta(client, scope).await?
        } else {
            pull_all(client, scope).await?
        };
        pulled += result.pulled;
        changed |= result.changed;
        return Ok(AchievementsSync {
            pushed,
            pulled,
            changed,
        });
    }

    if has_local {
        let all = sync_all_local(client, scope).await?;
        pushed += all.pushed;
        let delta = push_queue(client, scope).await?;
        pushed += delta.pushed;
        let pull = pull_delta(client, scope).await?;
        pulled += pull.pulled;
        changed |= pull.changed;
    } else {
        let delta = push_queue(client, scope).await?;
        pushed += delta.pushed;
        let pull = pull_all(client, scope).await?;
        pulled += pull.pulled;
        changed |= pull.changed;
    }

    Ok(AchievementsSync {
        pushed,
        pulled,
        changed,
    })
}

// ───────────────────────────────────────────────────────────────────────────
// Matching de conquistas (port de `achievements/match.js`)
// ───────────────────────────────────────────────────────────────────────────

fn normalize_title(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let decomposed: String = value
        .nfd()
        .filter(|character| !('\u{0300}'..='\u{036f}').contains(character))
        .collect();
    decomposed
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn aliases_of(item: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut add = |value: Option<&Value>| {
        if let Some(value) = value {
            let text = util::js_trim(&util::js_string(value)).to_string();
            if !text.is_empty() && !out.contains(&text) {
                out.push(text);
            }
        }
    };
    add(item.get("apiname"));
    if let Some(Value::Array(aliases)) = item.get("aliases") {
        for alias in aliases {
            add(Some(alias));
        }
    }
    out
}

fn remember_alias(item: &mut Value, apiname: &str) -> bool {
    let target = util::js_trim(apiname).to_string();
    if target.is_empty() {
        return false;
    }
    let own = item
        .get("apiname")
        .map(util::js_string)
        .map(|value| util::js_trim(&value).to_string())
        .unwrap_or_default();
    if target == own {
        return false;
    }
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    let list = object
        .entry("aliases".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(aliases) = list.as_array_mut() else {
        *list = Value::Array(Vec::new());
        let aliases = list.as_array_mut().expect("array recém-criado");
        aliases.push(Value::String(target.clone()));
        return true;
    };
    if aliases.iter().any(|alias| util::js_string(alias) == target) {
        return false;
    }
    aliases.push(Value::String(target));
    true
}

fn find_item_index(items: &[Value], apiname: &str, title: Option<&Value>) -> Option<usize> {
    let key = util::js_trim(apiname).to_lowercase();
    if !key.is_empty() {
        for (index, item) in items.iter().enumerate() {
            if aliases_of(item)
                .iter()
                .any(|alias| alias.to_lowercase() == key)
            {
                return Some(index);
            }
        }
    }
    let title = title.map(util::js_string).unwrap_or_default();
    let title_key = normalize_title(&title);
    if title_key.is_empty() {
        return None;
    }
    let matches: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let candidate = item.get("title").map(util::js_string).unwrap_or_default();
            normalize_title(&candidate) == title_key
        })
        .map(|(index, _)| index)
        .collect();
    (matches.len() == 1).then(|| matches[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matching_uses_apiname_alias_then_unique_title() {
        let items = vec![
            json!({ "apiname": "ACH01", "title": "Primeira", "aliases": ["ach_01"] }),
            json!({ "apiname": "ACH02", "title": "Segunda" }),
        ];
        assert_eq!(find_item_index(&items, "ach_01", None), Some(0));
        assert_eq!(
            find_item_index(&items, "UNKNOWN", Some(&json!("Segunda"))),
            Some(1)
        );
        assert_eq!(
            find_item_index(&items, "UNKNOWN", Some(&json!("Nada"))),
            None
        );
    }

    #[test]
    fn title_match_is_ambiguous_when_repeated() {
        let items = vec![
            json!({ "apiname": "A", "title": "Igual" }),
            json!({ "apiname": "B", "title": "Igual" }),
        ];
        assert_eq!(find_item_index(&items, "X", Some(&json!("Igual"))), None);
    }

    #[test]
    fn remember_alias_does_not_duplicate() {
        let mut item = json!({ "apiname": "ACH01" });
        assert!(remember_alias(&mut item, "ach_01"));
        assert!(!remember_alias(&mut item, "ach_01"));
        assert!(!remember_alias(&mut item, "ACH01"));
        assert_eq!(item["aliases"], json!(["ach_01"]));
    }

    #[test]
    fn normalize_title_strips_accents_and_case() {
        assert_eq!(normalize_title("  Pé   de  Valsa "), "pe de valsa");
    }
}
