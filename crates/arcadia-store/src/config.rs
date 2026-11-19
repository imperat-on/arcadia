//! `config.json` — leitura, merge e escrita atômica preservando chaves
//! desconhecidas. Espelho de `readConfig`/`writeConfig` do main process.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::atomic::write_atomic_text;

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.json")
}

pub fn read_config(data_dir: &Path) -> Value {
    read_config_at(&config_path(data_dir))
}

pub fn read_config_at(path: &Path) -> Value {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Value::Object(Map::new());
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => Value::Object(Map::new()),
    }
}

/// Merge de um patch no config atual.
///
/// Regras idênticas ao cliente Electron: spread raso no topo; `profile` e
/// `sources` fazem merge raso interno; chave presente no patch vence; chave
/// com `null` é removida. Campos desconhecidos do arquivo são preservados.
pub fn merge_config(current: &Value, patch: &Value) -> Value {
    let mut next = current.as_object().cloned().unwrap_or_default();
    let Some(patch_map) = patch.as_object() else {
        return Value::Object(next);
    };
    for (key, value) in patch_map {
        if value.is_null() {
            next.remove(key);
            continue;
        }
        if (key == "profile" || key == "sources") && value.is_object() {
            let mut nested = next
                .get(key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for (nested_key, nested_value) in value.as_object().expect("checado acima") {
                nested.insert(nested_key.clone(), nested_value.clone());
            }
            next.insert(key.clone(), Value::Object(nested));
            continue;
        }
        next.insert(key.clone(), value.clone());
    }
    Value::Object(next)
}

pub fn write_config_patch(data_dir: &Path, patch: &Value) -> io::Result<Value> {
    // Primeira escrita do Rust neste perfil: preserva o estado do Electron.
    crate::backup::ensure_pre_rust_backup(data_dir)?;
    let current = read_config(data_dir);
    let next = merge_config(&current, patch);
    let serialized = serde_json::to_string_pretty(&next)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_atomic_text(&config_path(data_dir), &serialized)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_is_shallow_except_profile_and_sources() {
        let current = json!({
            "language": "pt-BR",
            "profile": { "name": "a", "avatar": "x" },
            "sources": { "a": 1 },
            "unknown": { "keep": true }
        });
        let patch = json!({
            "profile": { "name": "b" },
            "sources": { "b": 2 },
            "new_key": 7
        });
        let out = merge_config(&current, &patch);
        assert_eq!(out["profile"]["name"], "b");
        assert_eq!(out["profile"]["avatar"], "x");
        assert_eq!(out["sources"], json!({ "a": 1, "b": 2 }));
        assert_eq!(out["unknown"], json!({ "keep": true }));
        assert_eq!(out["new_key"], 7);
        assert_eq!(out["language"], "pt-BR");
    }

    #[test]
    fn null_removes_key() {
        let out = merge_config(&json!({ "a": 1, "b": 2 }), &json!({ "a": null }));
        assert_eq!(out, json!({ "b": 2 }));
    }

    #[test]
    fn write_read_roundtrip_preserves_unknown_fields() {
        let dir = std::env::temp_dir().join(format!("arcadia-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            config_path(&dir),
            r#"{ "language": "pt-BR", "provider_future": { "x": 1 } }"#,
        )
        .unwrap();
        let next = write_config_patch(&dir, &json!({ "language": "en-US" })).unwrap();
        assert_eq!(next["provider_future"], json!({ "x": 1 }));
        let reread = read_config(&dir);
        assert_eq!(reread["language"], "en-US");
        assert_eq!(reread["provider_future"], json!({ "x": 1 }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
