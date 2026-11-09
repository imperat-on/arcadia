//! Horas jogadas — parte de `biblioteca.js`.
//!
//! O servidor ACUMULA deltas; o cliente guarda em `overrides.json` o total
//! (`playtime_added_minutes`) e em `sync_state.json` o watermark
//! (`playtimePush`) já confirmado. No pull o display é `max(local, servidor)`
//! e o watermark acompanha para nunca reenviar minutos que o servidor já tem.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use arcadia_api::ApiClient;
use arcadia_contracts::{normalize_playtime_items, resolve_playtime_conflict, MAX_SYNC_MINUTES};
use arcadia_store::accounts::AccountScope;

use crate::state::SyncStateFile;
use crate::util;
use crate::{ensure_session, map_api_error, SyncError};

/// Resultado de aplicar as linhas de `pull_library` ao estado local.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaytimeApply {
    pub changed: bool,
    /// O servidor ficou atrás do local; o próximo push precisa reenviar.
    pub needs_push: bool,
}

/// Deltas a enviar em `push_library.p_playtime`.
pub fn collect_push_items(
    overrides: &Map<String, Value>,
    watermarks: &Map<String, Value>,
) -> Vec<Value> {
    let mut items = Vec::new();
    for (appid, data) in overrides {
        let total = util::js_number_or_zero(data.get("playtime_added_minutes"));
        let base = util::js_number_or_zero(watermarks.get(appid));
        if total > base {
            items.push(json!({ "appid": appid, "minutes": util::number_value(total - base) }));
        }
    }
    items
}

/// Limita/sanitiza os deltas exatamente como o contrato (`1..=999999`).
pub fn normalize_push_items(items: Vec<Value>) -> Vec<Value> {
    normalize_playtime_items(Value::Array(items))
}

pub fn watermark_minutes(watermarks: &Map<String, Value>, appid: &str) -> f64 {
    util::js_number_or_zero(watermarks.get(appid))
}

/// Aplica `row.minutes` de cada linha do pull.
///
/// Espelho de `biblioteca.js`: maior total vence localmente; se o servidor
/// recuou abaixo do local, o watermark é rebaixado para o próximo push
/// reenviar a diferença.
pub fn apply_pull_rows(
    overrides: &mut Map<String, Value>,
    watermarks: &mut Map<String, Value>,
    rows: &[Value],
) -> PlaytimeApply {
    let mut result = PlaytimeApply::default();
    for row in rows {
        let Some(appid) = util::row_appid(row) else {
            continue;
        };
        let total = util::js_number_or_zero(row.get("minutes"));
        let local = util::js_number_or_zero(
            overrides
                .get(&appid)
                .and_then(|value| value.get("playtime_added_minutes")),
        );
        let watermark = watermark_minutes(watermarks, &appid);

        let merged =
            resolve_playtime_conflict(util::number_value(local), util::number_value(total)) as f64;

        if merged > local {
            let entry = overrides
                .entry(appid.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(object) = entry.as_object_mut() {
                object.insert("playtime_added_minutes".into(), util::number_value(merged));
            }
            watermarks.insert(appid, util::number_value(merged));
            result.changed = true;
        } else if total < local && watermark > total {
            watermarks.insert(appid, util::number_value(total));
            result.needs_push = true;
            result.changed = true;
        } else if total < watermark {
            // Watermark obsoleto, mesmo sem minutos locais novos.
            watermarks.insert(appid, util::number_value(total));
            result.changed = true;
        }
    }
    result
}

/// Resultado de `push_steam_playtime`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamPush {
    pub pushed: usize,
}

fn steam_item(value: &Value) -> Option<Value> {
    let appid = value
        .get("appid")
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
        .map(|value| util::js_trim(&value).to_string())
        .filter(|value| !value.is_empty())?;
    let steamid = value
        .get("steamid")
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
        .map(|value| util::js_trim(&value).to_string())
        .filter(|value| !value.is_empty())?;
    let minutes = util::js_number_or_zero(value.get("minutes"));
    if minutes <= 0.0 || minutes > MAX_SYNC_MINUTES as f64 {
        return None;
    }
    let persona = value
        .get("persona")
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
        .unwrap_or_default();
    Some(json!({
        "appid": appid,
        "steamid": steamid,
        "minutes": util::number_value(minutes),
        "persona": persona,
    }))
}

/// `push_steam_playtime`: TOTAIS absolutos por `(appid, steamid)` em `p_items`.
///
/// A leitura das contas Steam (`localconfig.vdf`) fica fora desta camada; o
/// chamador entrega os itens prontos (mesmo shape de `itensParaEnviar`).
pub async fn push_steam(client: &ApiClient, items: &[Value]) -> Result<SteamPush, SyncError> {
    ensure_session(client).await?;
    let normalized: Vec<Value> = items.iter().filter_map(steam_item).collect();
    if normalized.is_empty() {
        return Ok(SteamPush::default());
    }
    let mut pushed = 0_usize;
    for chunk in normalized.chunks(arcadia_contracts::MAX_SYNC_ITEMS) {
        client
            .rpc(
                "push_steam_playtime",
                json!({ "p_items": Value::Array(chunk.to_vec()) }),
            )
            .await
            .map_err(map_api_error)?;
        pushed += chunk.len();
    }
    Ok(SteamPush { pushed })
}

/// `linhasSteamDoServidor`: linhas cruas de `pull_steam_playtime` guardadas no
/// `sync_state.json` (sobrevivem a reinícios; a tela soma as contas).
pub fn steam_rows(scope: &AccountScope) -> Vec<Value> {
    SyncStateFile::new(scope)
        .read_document()
        .array("steamPlaytime")
}

/// `pull_steam_playtime`: linhas por conta Steam, guardadas cruas no estado.
///
/// Falha desse RPC nunca derruba o pull da biblioteca (o cliente antigo
/// simplesmente ignora o erro), por isso o chamador pode optar por ignorar.
pub async fn pull_steam(client: &ApiClient) -> Result<Vec<Value>, SyncError> {
    let response = client
        .rpc("pull_steam_playtime", Value::Null)
        .await
        .map_err(map_api_error)?;
    Ok(match response {
        Value::Array(rows) => rows,
        _ => Vec::new(),
    })
}

/// Minutos enviados em um item já normalizado.
pub fn item_minutes(item: &Value) -> i64 {
    item.get("minutes")
        .and_then(util::js_number)
        .map(|value| value.floor() as i64)
        .unwrap_or(0)
        .clamp(0, MAX_SYNC_MINUTES)
}

/// Atualiza o watermark com o que o servidor confirmou.
pub fn confirm_push(
    watermarks: &mut Map<String, Value>,
    sent_items: &[Value],
    previous: &HashMap<String, f64>,
) {
    for item in sent_items {
        let Some(appid) = item.get("appid").and_then(Value::as_str) else {
            continue;
        };
        let base = previous.get(appid).copied().unwrap_or(0.0);
        let total = base + item_minutes(item) as f64;
        watermarks.insert(appid.to_string(), util::number_value(total));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_only_positive_deltas() {
        let overrides = Map::from_iter([
            ("a".to_string(), json!({ "playtime_added_minutes": 120 })),
            ("b".to_string(), json!({ "playtime_added_minutes": 30 })),
            ("c".to_string(), json!({})),
        ]);
        let watermarks =
            Map::from_iter([("a".to_string(), json!(20)), ("b".to_string(), json!(30))]);
        assert_eq!(
            collect_push_items(&overrides, &watermarks),
            vec![json!({ "appid": "a", "minutes": 100 })]
        );
    }

    #[test]
    fn pull_raises_display_and_resets_watermark_when_server_lost_minutes() {
        let mut overrides = Map::from_iter([(
            "a".to_string(),
            json!({ "playtime_added_minutes": 50, "unknown": true }),
        )]);
        let mut watermarks = Map::from_iter([("a".to_string(), json!(80))]);
        let applied = apply_pull_rows(
            &mut overrides,
            &mut watermarks,
            &[json!({ "appid": "a", "minutes": 100 })],
        );
        assert!(applied.changed);
        assert!(!applied.needs_push);
        assert_eq!(overrides["a"]["playtime_added_minutes"], 100);
        assert_eq!(watermarks["a"], 100);
        assert_eq!(overrides["a"]["unknown"], true);

        let applied = apply_pull_rows(
            &mut overrides,
            &mut watermarks,
            &[json!({ "appid": "a", "minutes": 40 })],
        );
        assert!(applied.needs_push);
        assert_eq!(overrides["a"]["playtime_added_minutes"], 100);
        assert_eq!(watermarks["a"], 40);
    }

    #[test]
    fn playtime_resolver_is_monotonic_for_equal_ties() {
        let mut overrides = Map::new();
        let mut watermarks = Map::new();
        let rows = [
            json!({ "appid": "a", "minutes": 10 }),
            json!({ "appid": "a", "minutes": 15 }),
            json!({ "appid": "a", "minutes": 12 }),
        ];
        let applied = apply_pull_rows(&mut overrides, &mut watermarks, &rows);
        assert!(applied.changed);
        assert_eq!(overrides["a"]["playtime_added_minutes"], 15);
        assert_eq!(watermarks["a"], 12);
    }
}
