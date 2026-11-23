//! Biblioteca local — `library.json` global + arquivos por conta
//! (`custom_games.json`, `pending_games.json`, `overrides.json`).
//!
//! O snapshot global é a base; custom e pending entram por cima; overrides
//! (edições do usuário) vencem tudo. Campos de provider são preservados.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use arcadia_contracts::{normalize_game, normalize_library, LIBRARY_SCHEMA_VERSION};

pub fn library_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library.json")
}

pub fn account_file(data_dir: &Path, user: Option<&str>, name: &str) -> PathBuf {
    match user.map(str::trim).filter(|user| !user.is_empty()) {
        Some(user) => data_dir.join("contas").join(user).join(name),
        None => data_dir.join(name),
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Extrai os jogos normalizados de um documento de biblioteca.
/// Aceita o formato versionado (`{version:1, games:[...]}`) e o legado (array).
pub fn games_from_document(document: &Value) -> Vec<Value> {
    match document {
        Value::Array(items) => normalize_library(Value::Array(items.clone())),
        Value::Object(map) if map.get("version").and_then(Value::as_u64) == Some(LIBRARY_SCHEMA_VERSION as u64) => {
            map.get("games")
                .cloned()
                .map(normalize_library)
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn apply_overrides(games: &mut [Value], overrides: Option<Value>) {
    let Some(Value::Object(overrides)) = overrides else {
        return;
    };
    for game in games.iter_mut() {
        let Some(id) = game.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(Value::Object(patch)) = overrides.get(id) else {
            continue;
        };
        if let Some(object) = game.as_object_mut() {
            for (key, value) in patch {
                object.insert(key.clone(), value.clone());
            }
        }
    }
}

fn merge_pending(games: &mut Vec<Value>, pending: Vec<Value>) {
    let mut index: HashMap<String, usize> = HashMap::new();
    for (position, game) in games.iter().enumerate() {
        if let Some(id) = game.get("id").and_then(Value::as_str) {
            index.insert(id.to_string(), position);
        }
    }
    for item in pending {
        let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        match index.get(&id).copied() {
            Some(position) => {
                if item.get("installed").and_then(Value::as_bool) == Some(true) {
                    if let (Some(base), Some(patch)) = (games[position].as_object_mut(), item.as_object()) {
                        for (key, value) in patch {
                            base.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
            None => {
                index.insert(id, games.len());
                games.push(item);
            }
        }
    }
}

/// Lista final que o renderer recebe em `library:get`.
pub fn read_games(data_dir: &Path, user: Option<&str>) -> Vec<Value> {
    let global = read_json(&library_path(data_dir));
    let globals = global
        .as_ref()
        .map(games_from_document)
        .unwrap_or_default();
    let scope = crate::accounts::AccountScope::with_active(data_dir, user);
    let mut games = crate::owned::filter_by_ownership(&scope, &globals);

    let custom = read_json(&account_file(data_dir, user, "custom_games.json"));
    if let Some(custom) = custom {
        games.extend(normalize_library(custom));
    }

    let pending = read_json(&account_file(data_dir, user, "pending_games.json"));
    if let Some(pending) = pending {
        merge_pending(&mut games, normalize_library(pending));
    }

    let overrides = read_json(&account_file(data_dir, user, "overrides.json"));
    apply_overrides(&mut games, overrides);

    games
}

/// Normaliza uma entrada isolada preservando campos extras.
pub fn normalize_entry(value: Value) -> Option<Value> {
    normalize_game(value)
}

/// Objeto vazio auxiliar para quem precisa montar payloads.
pub fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-library-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_versioned_document_and_keeps_provider_fields() {
        let document = json!({
            "version": 1,
            "generated_at": 123,
            "sources": { "steam": 2 },
            "games": [
                { "id": "10", "title": "Half-Life", "launcher": "steam", "provider_x": { "keep": true } }
            ]
        });
        let games = games_from_document(&document);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0]["provider_x"], json!({ "keep": true }));
    }

    #[test]
    fn rejects_future_schema_instead_of_guessing() {
        let document = json!({ "version": 99, "games": [{ "id": "1", "title": "x" }] });
        assert!(games_from_document(&document).is_empty());
    }

    #[test]
    fn custom_and_overrides_apply_over_global() {
        let dir = temp_dir("overrides");
        std::fs::write(
            library_path(&dir),
            serde_json::to_string(&json!({
                "version": 1,
                "games": [{ "id": "1", "title": "Base", "launcher": "steam" }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("custom_games.json"),
            serde_json::to_string(&json!([{ "id": "2", "title": "Custom" }])).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("overrides.json"),
            serde_json::to_string(&json!({ "1": { "title": "Editado", "hidden": true } })).unwrap(),
        )
        .unwrap();

        let games = read_games(&dir, None);
        assert_eq!(games.len(), 2);
        let base = games.iter().find(|g| g["id"] == "1").unwrap();
        assert_eq!(base["title"], "Editado");
        assert_eq!(base["hidden"], true);
        assert_eq!(base["launcher"], "steam");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_installed_overrides_existing_entry() {
        let dir = temp_dir("pending");
        std::fs::write(
            library_path(&dir),
            serde_json::to_string(&json!({
                "version": 1,
                "games": [{ "id": "7", "title": "Old", "installed": false }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("pending_games.json"),
            serde_json::to_string(&json!([
                { "id": "7", "title": "Old", "installed": true, "exe": "C:/game.exe" },
                { "id": "8", "title": "New", "installed": true }
            ]))
            .unwrap(),
        )
        .unwrap();
        let games = read_games(&dir, None);
        assert_eq!(games.len(), 2);
        let updated = games.iter().find(|g| g["id"] == "7").unwrap();
        assert_eq!(updated["installed"], true);
        assert_eq!(updated["exe"], "C:/game.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn account_scope_reads_subdirectory() {
        let dir = temp_dir("account");
        let account_dir = dir.join("contas").join("zyon");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(
            account_dir.join("custom_games.json"),
            serde_json::to_string(&json!([{ "id": "9", "title": "Da conta" }])).unwrap(),
        )
        .unwrap();
        let games = read_games(&dir, Some("zyon"));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0]["title"], "Da conta");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn account_add_and_remove_persist_global_game_ownership() {
        let dir = temp_dir("owned-library");
        std::fs::write(
            library_path(&dir),
            serde_json::to_string(&json!({"version":1,"games":[
                {"id":"steam:42","title":"Global Steam game"},
                {"id":"steam:99","title":"Another account's game"}
            ]})).unwrap(),
        ).unwrap();
        let scope = crate::accounts::AccountScope::with_active(&dir, Some("player"));
        crate::owned::write_owned(&scope, &[serde_json::json!("steam:42")]).unwrap();

        let initial = read_games(&dir, Some("player"));
        assert_eq!(initial.iter().map(|game| game["id"].as_str().unwrap()).collect::<Vec<_>>(), vec!["steam:42"]);

        crate::owned::owned_add(&scope, "steam:99").unwrap();
        assert_eq!(read_games(&dir, Some("player")).len(), 2);

        crate::owned::owned_remove(&scope, "steam:42").unwrap();
        assert_eq!(read_games(&dir, Some("player")).iter().map(|game| game["id"].as_str().unwrap()).collect::<Vec<_>>(), vec!["steam:99"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
