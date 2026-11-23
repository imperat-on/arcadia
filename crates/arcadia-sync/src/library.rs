//! Sync de BIBLIOTECA (jogos custom) + HORAS — port de
//! `app/electron/supabase/biblioteca.js`.
//!
//! O push envia diffs contra o watermark `libPush` e deltas de horas; o pull
//! traz os jogos que faltam (com `exe` vazio), aplica remoções de outras
//! máquinas e sobe o display de horas. O pull vazio exige duas observações
//! consecutivas antes de remover, para não apagar a biblioteca se o RPC
//! responder `[]` por bug.
//!
//! Diferenças documentadas (dependem de módulos Electron fora do escopo):
//! enriquecimento de retrô via `/catalog/v1/retro/games` e arte real de stubs
//! Steam via `steamstore.itensDaLoja`. Sem eles, usamos os metadados já
//! presentes na linha/local e a arte CDN previsível (`library_600x900.jpg`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde_json::{json, Map, Value};

use arcadia_api::ApiClient;
use arcadia_api::Session;
use arcadia_contracts::{
    normalize_library_sync_item, normalize_library_sync_items, resolve_library_conflict,
    MAX_SYNC_ITEMS,
};
use arcadia_store::accounts::AccountScope;
use arcadia_store::library::{games_from_document, library_path};
use arcadia_store::owned::{read_owned, write_owned};

use crate::state::SyncStateFile;
use crate::util;
use crate::{ensure_session, map_api_error, SyncError};

pub const CUSTOM_GAMES_FILE: &str = "custom_games.json";
pub const PENDING_GAMES_FILE: &str = "pending_games.json";
pub const OVERRIDES_FILE: &str = "overrides.json";
const STEAM_CHANGES_FILE: &str = "library_sync_queue.json";
static LOCAL_LIBRARY: Mutex<()> = Mutex::new(());

/// Serializa as mudanças locais da loja com a aplicação de um pull.
pub fn lock_local_library() -> MutexGuard<'static, ()> {
    LOCAL_LIBRARY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn read_steam_changes(path: &Path) -> Result<Map<String, Value>, SyncError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| SyncError::Storage(format!("{STEAM_CHANGES_FILE}: {error}"))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error.into()),
    }
}

/// Persiste a última ação da loja para o jogo. O chamador segura a trava local.
pub fn record_steam_change(
    account_dir: &Path,
    app_id: &str,
    title: Option<&str>,
) -> Result<(), SyncError> {
    let steam_id = app_id.strip_prefix("steam:").unwrap_or(app_id);
    if steam_id.is_empty() || !steam_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SyncError::InvalidResponse("appid Steam inválido".into()));
    }
    let appid = format!("steam:{steam_id}");
    let row = match title {
        Some(title) => json!({ "appid": appid, "title": title, "platform": "windows" }),
        None => json!({ "appid": appid, "removed": true }),
    };
    let row = normalize_library_sync_item(row)
        .filter(|row| row.get("appid").and_then(Value::as_str) == Some(appid.as_str()))
        .ok_or_else(|| SyncError::InvalidResponse("appid Steam inválido".into()))?;
    let path = account_dir.join(STEAM_CHANGES_FILE);
    let mut changes = read_steam_changes(&path)?;
    changes.insert(appid, row);
    util::write_json_atomic(&path, &Value::Object(changes), false)?;
    Ok(())
}

fn is_removal(row: &Value) -> bool {
    row.get("removed").and_then(Value::as_bool) == Some(true)
}

async fn session_for_scope(client: &ApiClient, scope: &AccountScope) -> Result<Session, SyncError> {
    let session = client
        .auth()
        .get_session()
        .await
        .ok_or(SyncError::NotLoggedIn)?;
    if session.user_id().is_none() {
        return Err(SyncError::NotLoggedIn);
    }
    if let Some(account) = scope.active() {
        let username = session
            .user
            .pointer("/user_metadata/username")
            .and_then(Value::as_str)
            .or_else(|| session.user.get("username").and_then(Value::as_str))
            .map(str::trim);
        if username != Some(account) {
            return Err(SyncError::AccountSwitched);
        }
    }
    Ok(session)
}

pub(crate) async fn send_steam_changes(
    client: &ApiClient,
    scope: &AccountScope,
) -> Result<usize, SyncError> {
    if scope.is_guest() {
        return Ok(0);
    }
    let path = scope.account_file(STEAM_CHANGES_FILE);
    let changes = {
        let _guard = lock_local_library();
        read_steam_changes(&path)?
    };
    if changes.is_empty() {
        return Ok(0);
    }
    let session = session_for_scope(client, scope).await?;
    let user_id = session.user_id().ok_or(SyncError::NotLoggedIn)?;

    let rows: Vec<_> = changes.into_iter().collect();
    for batch in rows.chunks(MAX_SYNC_ITEMS) {
        client
            .rpc_for_user(
                "push_library",
                json!({ "p_lib": batch.iter().map(|(_, row)| row).collect::<Vec<_>>(), "p_playtime": [] }),
                user_id,
            )
            .await
            .map_err(map_api_error)?;

        let _guard = lock_local_library();
        let mut changes = read_steam_changes(&path)?;
        let state_file = SyncStateFile::new(scope);
        state_file.update(|state| {
            let mut lib_push = state.object("libPush");
            for (appid, row) in batch {
                if changes.get(appid) != Some(row) {
                    continue;
                }
                if is_removal(row) {
                    lib_push.remove(appid);
                } else {
                    let title = row.get("title").and_then(Value::as_str).unwrap_or(appid);
                    lib_push.insert(
                        appid.clone(),
                        json!({ "title": title, "platform": "windows" }),
                    );
                }
                changes.remove(appid);
            }
            state.insert("libPush", Value::Object(lib_push));
        })?;
        util::write_json_atomic(&path, &Value::Object(changes), false)?;
    }
    Ok(rows.len())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryPull {
    /// `library:changed` — inclui watermarks e horas, como o `mudou` do JS.
    pub changed: bool,
    pub pulled: usize,
    pub steam_rows: usize,
    /// O servidor recuou; o chamador (em modo push) deve rodar `push`.
    pub playtime_needs_push: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryPush {
    pub pushed: usize,
    pub playtime: usize,
}

/// `pull_library` + `pull_steam_playtime` + reconciliação local.
pub async fn pull(client: &ApiClient, scope: &AccountScope) -> Result<LibraryPull, SyncError> {
    let session = session_for_scope(client, scope).await?;
    let user_id = session.user_id().ok_or(SyncError::NotLoggedIn)?;

    let response = client
        .rpc_for_user("pull_library", Value::Null, user_id)
        .await
        .map_err(map_api_error)?;
    let mut rows = response
        .as_array()
        .cloned()
        .ok_or_else(|| SyncError::InvalidResponse("sem dados".into()))?;
    let remote_empty = rows.is_empty();

    // Horas por conta Steam: o RPC pode não existir no servidor antigo; o
    // erro é deliberadamente ignorado (o app segue como estava).
    let steam_rows = match crate::playtime::pull_steam_for_user(client, user_id).await {
        Ok(rows) => rows,
        Err(SyncError::AccountSwitched) => return Err(SyncError::AccountSwitched),
        Err(_) => Vec::new(),
    };

    let _guard = lock_local_library();
    let changes = read_steam_changes(&scope.account_file(STEAM_CHANGES_FILE))?;
    rows.retain(|row| {
        !util::row_appid(row)
            .and_then(|appid| changes.get(&appid))
            .is_some_and(is_removal)
    });

    let state_file = SyncStateFile::new(scope);
    let mut state = state_file.read_document();
    let mut lib_push = state.object("libPush");
    let mut playtime_push = state.object("playtimePush");

    let custom_path = scope.account_file(CUSTOM_GAMES_FILE);
    let pending_path = scope.account_file(PENDING_GAMES_FILE);
    let overrides_path = scope.account_file(OVERRIDES_FILE);

    let mut lib = util::read_array_or_empty(&custom_path);
    let mut pending = util::read_array_or_empty(&pending_path);
    let mut pending_ids: HashSet<String> = pending
        .iter()
        .filter_map(|item| item.get("id"))
        .map(util::js_string)
        .collect();
    let mut overrides = util::read_object(&overrides_path);

    let server_ids: HashSet<String> = rows.iter().filter_map(util::row_appid).collect();
    let initial_ids: HashSet<String> = lib
        .iter()
        .filter_map(|game| game.get("id"))
        .map(util::js_string)
        .collect();

    let raw_owned = read_owned(scope);
    let owned_known = raw_owned.is_some();
    let mut owned: Vec<Value> = raw_owned.unwrap_or_default();
    let mut owned_set: HashSet<String> = owned.iter().map(util::js_string).collect();
    let mut owned_changed = false;

    // Pull vazio com estado local cheio: exigir duas observações.
    let local_state_full = !lib_push.is_empty() || (owned_known && !owned_set.is_empty());
    let mut can_remove = true;
    if remote_empty && local_state_full {
        let streak = state.empty_pull_streak() + 1;
        can_remove = streak >= 2;
        state.set_empty_pull_streak(streak);
    } else {
        state.set_empty_pull_streak(0);
    }

    let mut changed = false;
    let mut pending_changed = false;
    let mut lib_changed = false;
    let mut overrides_changed = false;

    // Retrôs sincronizados removidos em outra máquina saem do snapshot local.
    if can_remove {
        let removals: Vec<String> = lib
            .iter()
            .filter(|game| {
                is_retro(game)
                    && game
                        .get("id")
                        .map(util::js_string)
                        .filter(|id| lib_push.contains_key(id))
                        .is_some_and(|id| !server_ids.contains(&id))
            })
            .filter_map(|game| game.get("id").map(util::js_string))
            .collect();
        if !removals.is_empty() {
            let removal_set: HashSet<String> = removals.iter().cloned().collect();
            lib.retain(|game| {
                game.get("id")
                    .map(util::js_string)
                    .map(|id| !removal_set.contains(&id))
                    .unwrap_or(true)
            });
            changed = true;
            lib_changed = true;
            for id in removals {
                if owned_known && owned_set.remove(&id) {
                    owned_changed = true;
                }
                lib_push.remove(&id);
            }
        }
    }

    // Metadados retrô: local + linha, na ordem do `{...local, ...row}`.
    let local_games: HashMap<String, Value> = lib
        .iter()
        .filter_map(|game| game.get("id").map(|id| (util::js_string(id), game.clone())))
        .collect();
    let mut retro_metadata: HashMap<String, Map<String, Value>> = HashMap::new();
    for row in &rows {
        let Some(appid) = util::row_appid(row) else {
            continue;
        };
        if !is_retro_id(&appid) {
            continue;
        }
        let mut merged = local_games
            .get(&appid)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(object) = row.as_object() {
            for (key, value) in object {
                merged.insert(key.clone(), value.clone());
            }
        }
        let metadata = retro_metadata_from(&Value::Object(merged));
        // Sem catálogo retrô, só temos o que veio local/linha.
        retro_metadata.insert(appid, metadata);
    }

    // Override de arte local (file://, /home/...) não serve em outra máquina.
    let mut overrides_retro_changed = false;
    for (appid, metadata) in &retro_metadata {
        let remote_cover = metadata.get("cover");
        let Some(remote_cover) = remote_cover.filter(|value| is_portable_artwork(value)) else {
            continue;
        };
        let Some(override_value) = overrides.get_mut(appid).and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(local_cover) = override_value.get("cover") else {
            continue;
        };
        if !is_portable_artwork(local_cover) {
            override_value.insert("cover".into(), remote_cover.clone());
            overrides_retro_changed = true;
        }
    }
    overrides_changed |= overrides_retro_changed;
    changed |= overrides_retro_changed;

    // Stubs Steam (arte CDN previsível; sem consulta à loja).
    for row in &rows {
        let Some(appid) = util::row_appid(row) else {
            continue;
        };
        if !appid.starts_with("steam:") {
            continue;
        }
        if !initial_ids.contains(&appid) && !pending_ids.contains(&appid) {
            let steam_id = appid.strip_prefix("steam:").unwrap_or(&appid);
            let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{steam_id}");
            let title = row
                .get("title")
                .filter(|title| util::js_truthy(title))
                .map(util::js_string)
                .filter(|title| title != &appid && !util::js_trim(title).is_empty())
                .unwrap_or_else(|| format!("Steam {steam_id}"));
            pending.push(json!({
                "id": appid,
                "title": title,
                "launcher": "steam",
                "launch_cmd": ["steam", format!("steam://rungameid/{steam_id}")],
                "installed": false,
                "cover": format!("{base}/library_600x900.jpg"),
                "hero": format!("{base}/library_hero.jpg"),
                "logo": format!("{base}/logo.png"),
                "icon": "",
                "pendente": true,
            }));
            pending_ids.insert(appid);
            pending_changed = true;
        } else if pending_ids.contains(&appid) {
            if let Some(entry) = pending
                .iter_mut()
                .find(|item| item.get("id").map(util::js_string).as_deref() == Some(appid.as_str()))
            {
                let current = entry.get("title").map(util::js_string).unwrap_or_default();
                let steam_id = appid.strip_prefix("steam:").unwrap_or(&appid);
                let ugly = current == format!("Steam {steam_id}") || current == appid;
                let fresh = row
                    .get("title")
                    .filter(|title| util::js_truthy(title))
                    .map(util::js_string)
                    .filter(|title| title != &appid)
                    .map(|title| util::js_trim(&title).to_string())
                    .unwrap_or_default();
                if ugly && !fresh.is_empty() && fresh != current {
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("title".into(), Value::String(fresh));
                        pending_changed = true;
                    }
                }
            }
        }
    }

    // Aplica as linhas do servidor no snapshot local/posse.
    for row in &rows {
        let Some(appid) = util::row_appid(row) else {
            continue;
        };
        if owned_known && !owned_set.contains(&appid) {
            owned_set.insert(appid.clone());
            owned.push(Value::String(appid.clone()));
            owned_changed = true;
        }

        if appid.starts_with("steam:") {
            // A posse/stub já foram tratados acima.
            continue;
        }

        if !initial_ids.contains(&appid) {
            let retro = is_retro_id(&appid);
            let metadata = retro_metadata.get(&appid).cloned().unwrap_or_default();
            let mut item = Map::new();
            item.insert("id".into(), Value::String(appid.clone()));
            item.insert(
                "title".into(),
                Value::String(
                    row.get("title")
                        .filter(|title| util::js_truthy(title))
                        .map(util::js_string)
                        .unwrap_or_else(|| appid.clone()),
                ),
            );
            item.insert(
                "launcher".into(),
                Value::String(if retro { "retro" } else { "custom" }.into()),
            );
            let platform = if retro {
                metadata
                    .get("systemId")
                    .map(util::js_string)
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        row.get("systemId")
                            .filter(|value| util::js_truthy(value))
                            .map(util::js_string)
                    })
                    .unwrap_or_else(|| "emulator".to_string())
            } else {
                row.get("platform")
                    .filter(|value| util::js_truthy(value))
                    .map(util::js_string)
                    .unwrap_or_else(|| "windows".to_string())
            };
            item.insert("platform".into(), Value::String(platform));
            item.insert("exe".into(), Value::String(String::new()));
            item.insert("installed".into(), Value::Bool(false));
            if retro {
                item.insert("retro".into(), Value::Bool(true));
                for (key, value) in metadata {
                    if !value.is_null() {
                        item.insert(key, value);
                    }
                }
            }
            lib.push(Value::Object(item));
            changed = true;
            lib_changed = true;
            continue;
        }

        let Some(index) = lib.iter().position(|game| {
            game.get("id").map(util::js_string).as_deref() == Some(appid.as_str())
        }) else {
            continue;
        };
        let existing = lib[index].clone();
        let retro = is_retro(&existing) || is_retro_id(&appid);
        let metadata = if retro {
            retro_metadata.get(&appid).cloned().unwrap_or_default()
        } else {
            Map::new()
        };
        let row_title = row
            .get("title")
            .filter(|title| util::js_truthy(title))
            .map(util::js_string)
            .unwrap_or_else(|| appid.clone());
        let row_platform = if retro {
            "emulator".to_string()
        } else {
            row.get("platform")
                .filter(|value| util::js_truthy(value))
                .map(util::js_string)
                .unwrap_or_else(|| "windows".to_string())
        };
        let merged = resolve_library_conflict(
            conflict_view(&existing, &appid, None, None),
            conflict_view(row, &appid, Some(row_title), Some(row_platform)),
        );
        let Some(merged) = merged else {
            continue;
        };
        if merged.get("removed") == Some(&Value::Bool(true)) {
            continue;
        }
        if let Some(object) = lib[index].as_object_mut() {
            if let Some(title) = merged.get("title") {
                if object.get("title") != Some(title) {
                    object.insert("title".into(), title.clone());
                    changed = true;
                    lib_changed = true;
                }
            }
            if let Some(platform) = merged.get("platform") {
                if util::js_truthy(platform) && object.get("platform") != Some(platform) {
                    object.insert("platform".into(), platform.clone());
                    changed = true;
                    lib_changed = true;
                }
            }
            if retro {
                let mut filled = Map::new();
                filled.insert("retro".into(), Value::Bool(true));
                filled.insert("launcher".into(), Value::String("retro".into()));
                if let Some(system_id) = metadata
                    .get("systemId")
                    .filter(|value| util::js_truthy(value))
                {
                    filled.insert("systemId".into(), system_id.clone());
                    filled.insert("platform".into(), system_id.clone());
                }
                for (key, value) in metadata {
                    filled.insert(key, value);
                }
                for (key, value) in filled {
                    if value.is_null() || value == Value::String(String::new()) {
                        continue;
                    }
                    if object.get(&key) != Some(&value) {
                        object.insert(key, value);
                        changed = true;
                        lib_changed = true;
                    }
                }
            }
        }
    }

    // Remoção no pull: jogos que sumiram do servidor saem da posse local.
    if owned_known && can_remove {
        let keep: Vec<Value> = owned
            .iter()
            .filter(|id| {
                let id = util::js_string(id);
                if id.starts_with("steam:") {
                    return server_ids.contains(&id)
                        || !lib_push.contains_key(&id)
                        || changes.contains_key(&id)
                        || initial_ids.contains(&id);
                }
                let custom_retro_synced = is_retro_id(&id) && lib_push.contains_key(&id);
                server_ids.contains(&id) || (initial_ids.contains(&id) && !custom_retro_synced)
            })
            .cloned()
            .collect();
        if keep.len() != owned.len() {
            owned = keep;
            owned_changed = true;
        }
    }

    // Stubs pending cujo jogo sumiu do servidor.
    if can_remove {
        let before = pending.len();
        pending.retain(|item| {
            item.get("id")
                .map(util::js_string)
                .map(|id| {
                    server_ids.contains(&id)
                        || (id.starts_with("steam:")
                            && (!lib_push.contains_key(&id) || changes.contains_key(&id)))
                })
                .unwrap_or(true)
        });
        if pending.len() != before {
            pending_changed = true;
        }
        lib_push.retain(|id, _| {
            !id.starts_with("steam:") || server_ids.contains(id) || changes.contains_key(id)
        });
    }

    // Horas: maior total vence; watermark acompanha.
    let playtime = crate::playtime::apply_pull_rows(&mut overrides, &mut playtime_push, &rows);
    changed |= playtime.changed;
    overrides_changed |= playtime.changed;

    // Watermark dos jogos que vieram do servidor (permite remoção local).
    for row in &rows {
        let Some(appid) = util::row_appid(row) else {
            continue;
        };
        let title = row
            .get("title")
            .filter(|title| util::js_truthy(title))
            .map(util::js_string)
            .filter(|title| title != &appid)
            .map(|title| util::js_trim(&title).to_string())
            .filter(|title| !title.is_empty());
        match lib_push.get_mut(&appid) {
            None => {
                let platform = row
                    .get("platform")
                    .filter(|value| util::js_truthy(value))
                    .map(util::js_string)
                    .unwrap_or_else(|| "windows".to_string());
                let fallback_title = appid.clone();
                lib_push.insert(
                    appid,
                    json!({ "title": title.unwrap_or(fallback_title), "platform": platform }),
                );
                changed = true;
            }
            Some(existing) => {
                let current = existing
                    .get("title")
                    .map(util::js_string)
                    .unwrap_or_default();
                if let Some(title) = title {
                    if current == appid {
                        if let Some(object) = existing.as_object_mut() {
                            object.insert("title".into(), Value::String(title));
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    // Escritas: só depois de todo o sucesso de rede.
    if pending_changed {
        util::write_json_atomic(&pending_path, &Value::Array(pending.clone()), true)?;
        changed = true;
    }
    if lib_changed {
        util::write_json_atomic(&custom_path, &Value::Array(lib.clone()), true)?;
    }
    if owned_changed && owned_known {
        write_owned(scope, &owned)?;
        changed = true;
    }
    if overrides_changed {
        util::write_json_atomic(&overrides_path, &Value::Object(overrides.clone()), true)?;
    }

    state.insert("steamPlaytime", Value::Array(steam_rows.clone()));
    state.insert("playtimePush", Value::Object(playtime_push));
    state.insert("libPush", Value::Object(lib_push));
    state_file.write_document(&state)?;

    Ok(LibraryPull {
        changed,
        pulled: rows.len(),
        steam_rows: steam_rows.len(),
        playtime_needs_push: playtime.needs_push,
    })
}

/// `push_library`: diffs de biblioteca + deltas de horas, em lotes de 1000.
pub async fn push(client: &ApiClient, scope: &AccountScope) -> Result<LibraryPush, SyncError> {
    ensure_session(client).await?;

    let state_file = SyncStateFile::new(scope);
    let mut state = state_file.read_document();
    let mut lib_push = state.object("libPush");
    let mut playtime_push = state.object("playtimePush");

    let custom = util::read_array_or_empty(&scope.account_file(CUSTOM_GAMES_FILE));
    let pending = util::read_array_or_empty(&scope.account_file(PENDING_GAMES_FILE));
    let overrides = util::read_object(&scope.account_file(OVERRIDES_FILE));
    let custom_ids: HashSet<String> = custom
        .iter()
        .filter_map(|game| game.get("id"))
        .map(util::js_string)
        .collect();

    let raw_owned = read_owned(scope);
    let owned_known = raw_owned.is_some();
    let owned: HashSet<String> = raw_owned
        .unwrap_or_default()
        .iter()
        .map(util::js_string)
        .collect();

    let global_titles = global_titles(scope);

    let mut p_lib: Vec<Value> = Vec::new();
    for game in &custom {
        let game_id = util::js_string(game.get("id").unwrap_or(&Value::Null));
        let Some(payload) = payload_for_game(game, overrides.get(&game_id)) else {
            continue;
        };
        let appid = payload
            .get("appid")
            .map(util::js_string)
            .unwrap_or_default();
        if appid.is_empty() {
            continue;
        }
        let previous = lib_push.get(&appid);
        if !watermark_matches(previous, &payload) {
            p_lib.push(payload);
        }
    }

    // Jogos possuídos que não são custom sobem com o título real.
    if owned_known {
        for appid in &owned {
            if custom_ids.contains(appid) {
                continue;
            }
            let title = owned_title(&pending, &global_titles, appid);
            let previous = lib_push.get(appid);
            let previous_title = previous
                .and_then(|value| value.get("title"))
                .map(util::js_string);
            if previous_title.as_deref() != Some(title.as_str()) {
                p_lib.push(json!({
                    "appid": appid,
                    "title": title,
                    "platform": "windows",
                }));
            }
        }

        // Removidos localmente: só quando a posse é conhecida.
        for appid in lib_push.keys() {
            if !custom_ids.contains(appid) && !owned.contains(appid) {
                p_lib.push(json!({ "appid": appid, "removed": true }));
            }
        }
    }

    let p_playtime = crate::playtime::collect_push_items(&overrides, &playtime_push);

    if p_lib.is_empty() && p_playtime.is_empty() {
        return Ok(LibraryPush::default());
    }

    let mut pushed = 0_usize;
    let mut playtime_pushed = 0_usize;
    let mut lib_cursor = 0_usize;
    let mut playtime_cursor = 0_usize;
    while lib_cursor < p_lib.len() || playtime_cursor < p_playtime.len() {
        let lib_end = (lib_cursor + MAX_SYNC_ITEMS).min(p_lib.len());
        let playtime_end = (playtime_cursor + MAX_SYNC_ITEMS).min(p_playtime.len());
        let lib_raw: Vec<Value> = p_lib[lib_cursor..lib_end].to_vec();
        let playtime_raw: Vec<Value> = p_playtime[playtime_cursor..playtime_end].to_vec();
        lib_cursor = lib_end;
        playtime_cursor = playtime_end;

        let lib_items = normalize_library_sync_items(Value::Array(lib_raw));
        let playtime_items = crate::playtime::normalize_push_items(playtime_raw);
        if lib_items.is_empty() && playtime_items.is_empty() {
            continue;
        }

        let previous_playtime: HashMap<String, f64> = playtime_push
            .iter()
            .map(|(appid, value)| (appid.clone(), util::js_number_or_zero(Some(value))))
            .collect();

        client
            .rpc(
                "push_library",
                json!({ "p_lib": lib_items, "p_playtime": playtime_items }),
            )
            .await
            .map_err(map_api_error)?;

        for item in &lib_items {
            let Some(appid) = item.get("appid").map(util::js_string) else {
                continue;
            };
            if item.get("removed") == Some(&Value::Bool(true)) {
                lib_push.remove(&appid);
                continue;
            }
            let mut watermark = Map::new();
            watermark.insert(
                "title".into(),
                item.get("title")
                    .cloned()
                    .unwrap_or_else(|| Value::String(appid.clone())),
            );
            watermark.insert(
                "platform".into(),
                item.get("platform")
                    .cloned()
                    .unwrap_or_else(|| Value::String("windows".into())),
            );
            for key in ["cover", "hero", "icon"] {
                if let Some(value) = item
                    .get(key)
                    .filter(|value| value.as_str().map(|text| !text.is_empty()).unwrap_or(false))
                {
                    watermark.insert(key.into(), value.clone());
                }
            }
            lib_push.insert(appid, Value::Object(watermark));
        }
        pushed += lib_items.len();
        crate::playtime::confirm_push(&mut playtime_push, &playtime_items, &previous_playtime);
        playtime_pushed += playtime_items.len();

        state.insert("libPush", Value::Object(lib_push.clone()));
        state.insert("playtimePush", Value::Object(playtime_push.clone()));
        state_file.write_document(&state)?;
    }

    Ok(LibraryPush {
        pushed,
        playtime: playtime_pushed,
    })
}

fn global_titles(scope: &AccountScope) -> HashMap<String, String> {
    let Some(document) = util::read_json(&library_path(scope.data_dir())) else {
        return HashMap::new();
    };
    games_from_document(&document)
        .into_iter()
        .filter_map(|game| {
            let id = game.get("id").map(util::js_string)?;
            let title = game
                .get("title")
                .filter(|value| util::js_truthy(value))
                .map(util::js_string)?;
            Some((id, title))
        })
        .collect()
}

fn owned_title(pending: &[Value], global_titles: &HashMap<String, String>, appid: &str) -> String {
    let pending_match = pending
        .iter()
        .find(|item| item.get("id").map(util::js_string).as_deref() == Some(appid));
    if let Some(title) = pending_match
        .and_then(|item| item.get("title"))
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
    {
        if title != appid && !title.starts_with("Steam ") {
            return title;
        }
    }
    if let Some(title) = global_titles.get(appid) {
        return title.clone();
    }
    pending_match
        .and_then(|item| item.get("title"))
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
        .unwrap_or_else(|| appid.to_string())
}

fn watermark_matches(previous: Option<&Value>, payload: &Value) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    let same = |key: &str| {
        let left = previous.get(key).map(util::js_string);
        let right = payload.get(key).map(util::js_string);
        left == right
    };
    same("title") && same("platform") && same("cover") && same("hero") && same("icon")
}

/// Campos que `resolveLibraryConflict` consulta; evita que chaves alheias
/// (ex.: `id`) mudem o desempate canônico por JSON.
fn conflict_view(
    value: &Value,
    appid: &str,
    title: Option<String>,
    platform: Option<String>,
) -> Value {
    let mut out = Map::new();
    out.insert("appid".into(), Value::String(appid.to_string()));
    if let Some(title) = title.or_else(|| value.get("title").map(util::js_string)) {
        out.insert("title".into(), Value::String(title));
    }
    if let Some(platform) = platform.or_else(|| value.get("platform").map(util::js_string)) {
        out.insert("platform".into(), Value::String(platform));
    }
    for key in [
        "revision",
        "lamport",
        "version",
        "updated_at",
        "updatedAt",
        "operation_at",
        "operationAt",
        "created_at",
        "createdAt",
    ] {
        if let Some(item) = value.get(key) {
            out.insert(key.into(), item.clone());
        }
    }
    if let Some(removed) = value.get("removed") {
        out.insert("removed".into(), removed.clone());
    }
    Value::Object(out)
}

fn payload_for_game(game: &Value, override_value: Option<&Value>) -> Option<Value> {
    let id = game.get("id")?;
    let appid = util::js_string(id);
    if appid.is_empty() {
        return None;
    }
    let retro = is_retro(game);
    let mut merged = game.clone();
    if retro {
        if let Some(override_object) = override_value.and_then(Value::as_object) {
            if let Some(object) = merged.as_object_mut() {
                for key in ["cover", "hero", "icon"] {
                    if let Some(value) = override_object
                        .get(key)
                        .filter(|value| is_portable_artwork(value))
                    {
                        object.insert(
                            key.into(),
                            Value::String(util::js_trim(&util::js_string(value)).to_string()),
                        );
                    }
                }
            }
        }
    }
    let metadata = retro_metadata_from(&merged);
    let title = game
        .get("title")
        .filter(|value| util::js_truthy(value))
        .map(util::js_string)
        .unwrap_or_else(|| appid.clone());
    let platform = if retro {
        "emulator".to_string()
    } else {
        game.get("platform")
            .filter(|value| util::js_truthy(value))
            .map(util::js_string)
            .unwrap_or_else(|| "windows".to_string())
    };
    let mut payload = Map::new();
    payload.insert("appid".into(), Value::String(appid));
    payload.insert("title".into(), Value::String(title));
    payload.insert("platform".into(), Value::String(platform));
    if retro {
        payload.insert("retro".into(), Value::Bool(true));
        for (key, value) in metadata {
            if ["cover", "hero", "icon"].contains(&key.as_str()) && !is_portable_artwork(&value) {
                continue;
            }
            payload.insert(key, value);
        }
    }
    Some(Value::Object(payload))
}

fn is_portable_artwork(value: &Value) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    let lower = util::js_trim(text).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_retro(value: &Value) -> bool {
    if value.get("retro").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    if value
        .get("launcher")
        .and_then(Value::as_str)
        .map(|launcher| launcher == "retro")
        .unwrap_or(false)
    {
        return true;
    }
    value
        .get("id")
        .map(util::js_string)
        .map(|id| id.starts_with("retro:"))
        .unwrap_or(false)
}

fn is_retro_id(id: &str) -> bool {
    id.starts_with("retro:")
}

fn truthy_field(object: &Map<String, Value>, key: &str) -> Option<Value> {
    object
        .get(key)
        .filter(|value| util::js_truthy(value))
        .cloned()
}

fn retro_metadata_from(value: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(object) = value.as_object() else {
        return out;
    };
    let artwork = object
        .get("artwork")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let first_truthy = |keys: &[&str]| -> Option<Value> {
        for key in keys {
            if let Some(value) = object.get(*key) {
                if util::js_truthy(value) {
                    return Some(value.clone());
                }
            }
        }
        for key in keys {
            if let Some(value) = artwork.get(*key) {
                if util::js_truthy(value) {
                    return Some(value.clone());
                }
            }
        }
        None
    };
    let cover = first_truthy(&["cover", "capa", "fallbackCover"])
        .or_else(|| truthy_field(&artwork, "cover"));
    let cover_text = cover.as_ref().map(util::js_string).unwrap_or_default();
    let hero = first_truthy(&["hero", "heroi"])
        .or_else(|| truthy_field(&artwork, "hero"))
        .or_else(|| truthy_field(&artwork, "background"))
        .map(|value| util::js_string(&value))
        .or_else(|| {
            if cover_text.is_empty() {
                None
            } else {
                Some(cover_text.clone())
            }
        })
        .unwrap_or_default();
    let icon = if cover_text.is_empty() {
        first_truthy(&["icon", "logo"])
            .or_else(|| truthy_field(&artwork, "icon"))
            .map(|value| util::js_string(&value))
            .unwrap_or_default()
    } else {
        cover_text.clone()
    };
    if !cover_text.is_empty() {
        out.insert(
            "cover".into(),
            Value::String(util::js_slice_utf16(&cover_text, 2_000)),
        );
    }
    if !hero.is_empty() {
        out.insert(
            "hero".into(),
            Value::String(util::js_slice_utf16(&hero, 2_000)),
        );
    }
    if !icon.is_empty() {
        out.insert(
            "icon".into(),
            Value::String(util::js_slice_utf16(&icon, 2_000)),
        );
    }
    let description = ["description", "summary"]
        .iter()
        .find_map(|key| object.get(*key).filter(|value| util::js_truthy(value)))
        .map(util::js_string);
    if let Some(description) = description {
        out.insert(
            "description".into(),
            Value::String(util::js_slice_utf16(&description, 10_000)),
        );
    }
    if let Some(Value::Array(genres)) = object.get("genres") {
        let genres: Vec<Value> = genres
            .iter()
            .filter_map(|item| item.as_str())
            .map(util::js_trim)
            .filter(|genre| !genre.is_empty())
            .take(32)
            .map(|genre| Value::String(genre.to_string()))
            .collect();
        if !genres.is_empty() {
            out.insert("genres".into(), Value::Array(genres));
        }
    }
    if let Some(system_id) = object
        .get("systemId")
        .filter(|value| util::js_truthy(value))
    {
        out.insert(
            "systemId".into(),
            Value::String(util::js_slice_utf16(&util::js_string(system_id), 120)),
        );
    }
    if let Some(Value::Number(number)) = object.get("releaseYear") {
        if number
            .as_f64()
            .map(|value| value.fract() == 0.0)
            .unwrap_or(false)
        {
            out.insert("releaseYear".into(), Value::Number(number.clone()));
        }
    }
    for key in ["developer", "publisher"] {
        if let Some(value) = object.get(key).filter(|value| util::js_truthy(value)) {
            let text = match value {
                Value::Array(items) => items
                    .first()
                    .filter(|item| util::js_truthy(item))
                    .map(util::js_string)
                    .unwrap_or_default(),
                other => util::js_string(other),
            };
            out.insert(key.into(), Value::String(text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_custom_game_has_contract_fields_only() {
        let payload = payload_for_game(
            &json!({ "id": "custom:1", "title": "Jogo", "launcher": "custom", "unknown": true }),
            None,
        )
        .unwrap();
        assert_eq!(
            payload,
            json!({ "appid": "custom:1", "title": "Jogo", "platform": "windows" })
        );
    }

    #[test]
    fn payload_retro_keeps_portable_art_and_drops_file_paths() {
        let game = json!({
            "id": "retro:sonic",
            "title": "Sonic",
            "launcher": "retro",
            "cover": "file:///C:/capa.png",
            "hero": "https://cdn.example/hero.jpg",
            "systemId": "megadrive",
            "releaseYear": 1991,
        });
        let payload = payload_for_game(&game, None).unwrap();
        assert_eq!(payload["appid"], "retro:sonic");
        assert_eq!(payload["platform"], "emulator");
        assert_eq!(payload["retro"], true);
        assert!(payload.get("cover").is_none());
        assert_eq!(payload["hero"], "https://cdn.example/hero.jpg");
        // O ícone retrô cai na capa local (file://), que é descartada no payload.
        assert!(payload.get("icon").is_none());
        assert_eq!(payload["systemId"], "megadrive");
        assert_eq!(payload["releaseYear"], 1991);
    }

    #[test]
    fn owned_title_prefers_real_pending_title_then_global() {
        let pending = vec![json!({ "id": "steam:1", "title": "Steam 1" })];
        let globals = HashMap::from([("steam:1".to_string(), "Portal".to_string())]);
        assert_eq!(owned_title(&pending, &globals, "steam:1"), "Portal");
        let pending = vec![json!({ "id": "steam:2", "title": "Half-Life" })];
        assert_eq!(
            owned_title(&pending, &HashMap::new(), "steam:2"),
            "Half-Life"
        );
    }
}
