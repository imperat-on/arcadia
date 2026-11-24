//! Sync do registro de sources públicas — port de
//! `app/electron/supabase/sources.js`.
//!
//! Só o registro `{source_id, url, name}` atravessa a rede. `etag`, `lastMod`,
//! `count` e chaves de API são estado local e nunca sobem. O push usa o
//! watermark `srcPush`; o pull apenas adiciona fontes desconhecidas (nunca
//! apaga as locais).

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use arcadia_api::ApiClient;
use arcadia_store::accounts::AccountScope;

use crate::state::SyncStateFile;
use crate::util;
use crate::{ensure_session, map_api_error, SyncError};

pub const SOURCES_FILE: &str = "sources.json";
const MAX_SOURCE_ID: usize = 512;
const MAX_SOURCE_URL: usize = 4096;
const MAX_SOURCE_NAME: usize = 1024;
const MAX_ITEMS: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourcesPush {
    pub pushed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourcesPull {
    pub pulled: usize,
    pub changed: bool,
}

pub fn sources_path(scope: &AccountScope) -> std::path::PathBuf {
    scope.account_file(SOURCES_FILE)
}

pub fn load_registry(scope: &AccountScope) -> Vec<Value> {
    util::read_array_or_empty(&sources_path(scope))
}

fn save_registry(scope: &AccountScope, registry: &[Value]) -> Result<(), SyncError> {
    util::write_json_atomic(&sources_path(scope), &Value::Array(registry.to_vec()), true)?;
    Ok(())
}

fn text_field(value: Option<&Value>, max: usize) -> Option<String> {
    let value = value?;
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(_) => util::js_string(value),
        _ => return None,
    };
    let trimmed = util::js_trim(&text);
    if trimmed.is_empty() || trimmed.encode_utf16().count() > max {
        return None;
    }
    Some(trimmed.to_string())
}

fn source_record(source: &Value) -> Option<(String, Value)> {
    let id = text_field(source.get("id"), MAX_SOURCE_ID)?;
    let mut item = Map::new();
    item.insert("source_id".into(), Value::String(id.clone()));
    for (key, max) in [("url", MAX_SOURCE_URL), ("name", MAX_SOURCE_NAME)] {
        let Some(value) = source.get(key) else {
            continue;
        };
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(_) => util::js_string(value),
            _ => continue,
        };
        let trimmed = util::js_trim(&text);
        if trimmed.encode_utf16().count() <= max {
            item.insert(key.into(), Value::String(trimmed.to_string()));
        }
    }
    Some((id, Value::Object(item)))
}

/// `push_sources`: novas fontes pelo watermark + tombstones das removidas.
pub async fn push(client: &ApiClient, scope: &AccountScope) -> Result<SourcesPush, SyncError> {
    ensure_session(client).await?;

    let registry = load_registry(scope);
    let state_file = SyncStateFile::new(scope);
    let mut state = state_file.read_document();
    let mut watermarks = state.object("srcPush");

    let mut p_sources: Vec<Value> = Vec::new();
    let mut current_ids: HashSet<String> = HashSet::new();
    for source in &registry {
        if let Some((id, item)) = source_record(source) {
            current_ids.insert(id.clone());
            if !watermarks.contains_key(&id) {
                p_sources.push(item);
            }
        }
    }
    for id in watermarks.keys() {
        if !current_ids.contains(id) {
            p_sources.push(json!({ "source_id": id, "removed": true }));
        }
    }
    if p_sources.is_empty() {
        return Ok(SourcesPush::default());
    }

    let mut pushed = 0_usize;
    for chunk in p_sources.chunks(MAX_ITEMS) {
        client
            .rpc(
                "push_sources",
                json!({ "p_sources": Value::Array(chunk.to_vec()) }),
            )
            .await
            .map_err(map_api_error)?;
        pushed += chunk.len();
    }

    // Só grava watermarks depois que todos os lotes foram aceitos.
    for source in &registry {
        let Some(id) = text_field(source.get("id"), MAX_SOURCE_ID) else {
            continue;
        };
        let mut watermark = Map::new();
        for key in ["url", "name"] {
            if let Some(value) = source.get(key) {
                watermark.insert(key.into(), value.clone());
            }
        }
        watermarks.insert(id, Value::Object(watermark));
    }
    for item in &p_sources {
        if item.get("removed") == Some(&Value::Bool(true)) {
            if let Some(id) = item.get("source_id").map(util::js_string) {
                watermarks.remove(&id);
            }
        }
    }
    state.insert("srcPush", Value::Object(watermarks));
    state_file.write_document(&state)?;
    Ok(SourcesPush { pushed })
}

/// `pull_sources`: adiciona ao registro local as fontes desconhecidas.
pub async fn pull(client: &ApiClient, scope: &AccountScope) -> Result<SourcesPull, SyncError> {
    ensure_session(client).await?;

    let response = client
        .rpc("pull_sources", Value::Null)
        .await
        .map_err(map_api_error)?;
    let rows = response
        .as_array()
        .cloned()
        .ok_or_else(|| SyncError::InvalidResponse("sem dados".into()))?;

    let mut registry = load_registry(scope);
    let existing: HashSet<String> = registry
        .iter()
        .filter_map(|source| source.get("id"))
        .map(util::js_string)
        .collect();
    let state_file = SyncStateFile::new(scope);
    let mut state = state_file.read_document();
    let mut watermarks = state.object("srcPush");
    let mut state_changed = false;
    let mut changed = false;
    let mut inserted: HashSet<String> = HashSet::new();
    for row in &rows {
        // Uma linha removida no servidor não vira fonte local nem watermark.
        if row.get("removed") == Some(&Value::Bool(true)) {
            continue;
        }
        let Some(id) = row.get("source_id").and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(_) => Some(util::js_string(value)),
            _ => None,
        }) else {
            continue;
        };

        // O servidor confirmou que a fonte existe na conta. Registrar o
        // watermark também para fontes já locais garante que removê-las aqui
        // envie o tombstone correto no próximo push.
        if !watermarks.contains_key(&id) {
            let mut watermark = Map::new();
            watermark.insert("url".into(), row.get("url").cloned().unwrap_or(Value::Null));
            watermark.insert(
                "name".into(),
                row.get("name").cloned().unwrap_or(Value::Null),
            );
            watermarks.insert(id.clone(), Value::Object(watermark));
            state_changed = true;
        }

        if existing.contains(&id) || !inserted.insert(id.clone()) {
            continue;
        }
        let mut item = Map::new();
        item.insert("id".into(), Value::String(id));
        item.insert("url".into(), row.get("url").cloned().unwrap_or(Value::Null));
        item.insert(
            "name".into(),
            row.get("name").cloned().unwrap_or(Value::Null),
        );
        item.insert("etag".into(), Value::String(String::new()));
        item.insert("lastMod".into(), Value::String(String::new()));
        item.insert("addedAt".into(), Value::from(util::now_millis()));
        item.insert("count".into(), Value::from(0));
        registry.push(Value::Object(item));
        changed = true;
    }
    if changed {
        save_registry(scope, &registry)?;
    }
    if state_changed {
        state.insert("srcPush", Value::Object(watermarks));
        state_file.write_document(&state)?;
    }
    Ok(SourcesPull {
        pulled: rows.len(),
        changed,
    })
}

/// Reconcile de sources (login): push e depois pull, como o cliente.
pub async fn reconcile(
    client: &ApiClient,
    scope: &AccountScope,
    pull_only: bool,
) -> Result<(SourcesPush, SourcesPull), SyncError> {
    let pushed = if pull_only {
        SourcesPush::default()
    } else {
        push(client, scope).await?
    };
    let pulled = pull(client, scope).await?;
    Ok((pushed, pulled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_record_sends_only_network_fields() {
        let source = json!({
            "id": "abc",
            "url": "https://example/source.json",
            "name": "Fonte",
            "etag": "W/\"1\"",
            "lastMod": "hoje",
            "count": 42,
            "apiKey": "segredo",
        });
        let (id, item) = source_record(&source).unwrap();
        assert_eq!(id, "abc");
        assert_eq!(
            item,
            json!({ "source_id": "abc", "url": "https://example/source.json", "name": "Fonte" })
        );
    }

    #[test]
    fn source_record_rejects_missing_id_or_oversized_values() {
        assert!(source_record(&json!({ "url": "https://x" })).is_none());
        let huge = "x".repeat(MAX_SOURCE_URL + 1);
        let item = source_record(&json!({ "id": "abc", "url": huge }))
            .unwrap()
            .1;
        assert!(item.get("url").is_none());
    }
}
