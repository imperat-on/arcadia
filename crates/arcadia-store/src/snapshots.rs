//! Snapshots de saves — port de `snapshot-service.js`.
//!
//! Cada snapshot é `snapshots/<gameId>/<id>/{manifest.json,data/}`. Cópias
//! nunca seguem symlink; restore só aceita alvo sob o home ou temp; delete só
//! dentro da raiz de snapshots.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const SNAPSHOT_VERSION: u32 = 1;
pub const MAX_LABEL_LENGTH: usize = 120;

pub fn safe_part(value: &str, fallback: &str) -> String {
    let clean: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(160)
        .collect();
    if clean.is_empty() || clean.chars().all(|c| c == '.') {
        fallback.to_string()
    } else {
        clean
    }
}

fn absolute_directory(value: &str) -> PathBuf {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    let path = PathBuf::from(trimmed);
    if path.parent().is_none() {
        return PathBuf::new();
    }
    path
}

fn inside(parent: &Path, child: &Path) -> bool {
    let parent = normalize_path(parent);
    let child = normalize_path(child);
    child != parent && child.starts_with(&parent)
}

fn normalize_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

fn skip_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    if skip_symlink(source) {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            copy_tree(&entry.path(), &target)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

pub struct SnapshotService {
    root: PathBuf,
}

impl SnapshotService {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn game_dir(&self, game_id: &str) -> PathBuf {
        self.root.join(safe_part(game_id, "unknown"))
    }

    fn read_manifest(&self, file: &Path) -> Option<Value> {
        let raw = fs::read_to_string(file).ok()?;
        let manifest: Value = serde_json::from_str(&raw).ok()?;
        if manifest.get("version").and_then(Value::as_u64) != Some(SNAPSHOT_VERSION as u64)
            || manifest.get("id").and_then(Value::as_str).is_none()
            || manifest.get("gameId").and_then(Value::as_str).is_none()
            || manifest.get("created_at").and_then(Value::as_str).is_none()
        {
            return None;
        }
        Some(json!({
            "version": SNAPSHOT_VERSION,
            "id": manifest.get("id").cloned().unwrap_or(Value::Null),
            "gameId": manifest.get("gameId").cloned().unwrap_or(Value::Null),
            "created_at": manifest.get("created_at").cloned().unwrap_or(Value::Null),
            "label": manifest.get("label").and_then(Value::as_str).unwrap_or(""),
            "source_name": manifest.get("source_name").and_then(Value::as_str).unwrap_or(""),
        }))
    }

    pub fn list(&self, game_id: &str) -> Value {
        let directory = self.game_dir(game_id);
        let Ok(entries) = fs::read_dir(&directory) else {
            return json!([]);
        };
        let mut manifests: Vec<Value> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
            .filter_map(|entry| self.read_manifest(&entry.path().join("manifest.json")))
            .filter(|manifest| manifest.get("gameId").and_then(Value::as_str) == Some(game_id))
            .collect();
        manifests.sort_by(|left, right| {
            let left = left.get("created_at").and_then(Value::as_str).unwrap_or("");
            let right = right.get("created_at").and_then(Value::as_str).unwrap_or("");
            right.cmp(left)
        });
        Value::Array(manifests)
    }

    pub fn create(&self, game_id: &str, source_dir: &str, label: &str) -> Value {
        let source = absolute_directory(source_dir);
        let invalid = || json!({ "ok": false, "error": "origem_invalida" });
        if game_id.trim().is_empty() || source.as_os_str().is_empty() {
            return invalid();
        }
        let Ok(metadata) = fs::symlink_metadata(&source) else {
            return invalid();
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return invalid();
        }

        let safe_game = safe_part(game_id, "unknown");
        let millis = chrono::Utc::now().timestamp_millis();
        let suffix = format!("{:08x}", std::process::id().wrapping_mul(2_654_435_761) ^ (millis as u32));
        let id = format!("{millis}-{suffix}");
        let directory = self.root.join(&safe_game).join(&id);
        let temporary = self.root.join(&safe_game).join(format!("{id}.tmp"));

        let result = (|| -> io::Result<Value> {
            fs::create_dir_all(self.root.join(&safe_game))?;
            let _ = fs::remove_dir_all(&temporary);
            fs::create_dir_all(&temporary)?;
            copy_tree(&source, &temporary.join("data"))?;
            let label: String = label.trim().chars().take(MAX_LABEL_LENGTH).collect();
            let manifest = json!({
                "version": SNAPSHOT_VERSION,
                "id": id,
                "gameId": game_id,
                "created_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                "label": label,
                "source_name": source.file_name().and_then(|name| name.to_str()).unwrap_or(""),
            });
            fs::write(
                temporary.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap_or_default(),
            )?;
            fs::rename(&temporary, &directory)?;
            Ok(json!({ "ok": true, "snapshot": manifest }))
        })();

        match result {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_dir_all(&temporary);
                json!({ "ok": false, "error": error.to_string() })
            }
        }
    }

    pub fn restore(
        &self,
        game_id: &str,
        snapshot_id: &str,
        target_dir: &str,
        backup: bool,
    ) -> Value {
        let target = absolute_directory(target_dir);
        let id = safe_part(snapshot_id, "");
        let directory = self.game_dir(game_id).join(&id);
        let data = directory.join("data");
        let manifest = self.read_manifest(&directory.join("manifest.json"));

        let home = crate::paths::home_dir();
        let tmp = std::env::temp_dir();
        let target_ok = !target.as_os_str().is_empty()
            && ((inside(&home, &target) || normalize_path(&home) == normalize_path(&target))
                || (inside(&tmp, &target) || normalize_path(&tmp) == normalize_path(&target)));
        let manifest_ok = manifest
            .as_ref()
            .and_then(|manifest| manifest.get("gameId").and_then(Value::as_str))
            == Some(game_id);
        if !target_ok || id.is_empty() || !inside(&self.root, &directory) || !manifest_ok {
            return json!({ "ok": false, "error": "snapshot_invalido" });
        }
        let Ok(data_meta) = fs::symlink_metadata(&data) else {
            return json!({ "ok": false, "error": "snapshot_sem_dados" });
        };
        if !data_meta.is_dir() || data_meta.file_type().is_symlink() {
            return json!({ "ok": false, "error": "snapshot_invalido" });
        }

        let nonce = format!("{:08x}", chrono::Utc::now().timestamp_millis() as u32);
        let temporary = PathBuf::from(format!("{}.arcadia-restore-{nonce}", target.to_string_lossy()));
        let backup_path = if backup {
            PathBuf::from(format!(
                "{}.arcadia-backup-{}",
                target.to_string_lossy(),
                chrono::Utc::now().timestamp_millis()
            ))
        } else {
            PathBuf::new()
        };

        let mut previous: Option<PathBuf> = None;
        let result = (|| -> io::Result<()> {
            let _ = fs::remove_dir_all(&temporary);
            copy_tree(&data, &temporary)?;
            if target.exists() {
                let rollback = if backup {
                    backup_path.clone()
                } else {
                    PathBuf::from(format!(
                        "{}.arcadia-rollback-{nonce}",
                        target.to_string_lossy()
                    ))
                };
                fs::rename(&target, &rollback)?;
                previous = Some(rollback);
            }
            fs::rename(&temporary, &target)?;
            if let Some(previous) = previous.as_ref() {
                if !backup {
                    let _ = fs::remove_dir_all(previous);
                }
            }
            Ok(())
        })();

        match result {
            Ok(()) => json!({
                "ok": true,
                "backupPath": if backup { backup_path.to_string_lossy().into_owned() } else { String::new() },
                "snapshot": manifest
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(&temporary);
                if let Some(previous) = previous {
                    if !target.exists() && previous.exists() {
                        let _ = fs::rename(&previous, &target);
                    }
                }
                json!({
                    "ok": false,
                    "error": error.to_string(),
                    "backupPath": if backup { backup_path.to_string_lossy().into_owned() } else { String::new() }
                })
            }
        }
    }

    pub fn remove(&self, game_id: &str, snapshot_id: &str) -> Value {
        let id = safe_part(snapshot_id, "");
        let directory = self.game_dir(game_id).join(&id);
        let manifest = self.read_manifest(&directory.join("manifest.json"));
        let valid = !id.is_empty()
            && inside(&self.root, &directory)
            && manifest
                .as_ref()
                .and_then(|manifest| manifest.get("gameId").and_then(Value::as_str))
                == Some(game_id);
        if !valid {
            return json!({ "ok": false, "error": "snapshot_invalido" });
        }
        match fs::remove_dir_all(&directory) {
            Ok(()) => json!({ "ok": true }),
            Err(error) => json!({ "ok": false, "error": error.to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-snapshot-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_list_restore_remove_roundtrip() {
        let root = temp_dir("root");
        let source = temp_dir("source");
        fs::write(source.join("save.dat"), "save-v1").unwrap();
        let service = SnapshotService::new(root.join("snapshots"));

        let created = service.create("steam:1", source.to_str().unwrap(), "antes do boss");
        assert_eq!(created["ok"], true, "create: {created}");
        let id = created["snapshot"]["id"].as_str().unwrap().to_string();

        let listed = service.list("steam:1");
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["label"], "antes do boss");

        let target = temp_dir("target");
        fs::write(target.join("save.dat"), "save-v2").unwrap();
        let restored = service.restore("steam:1", &id, target.to_str().unwrap(), true);
        assert_eq!(restored["ok"], true, "restore: {restored}");
        assert_eq!(fs::read_to_string(target.join("save.dat")).unwrap(), "save-v1");

        let removed = service.remove("steam:1", &id);
        assert_eq!(removed["ok"], true, "remove: {removed}");
        assert_eq!(service.list("steam:1").as_array().unwrap().len(), 0);

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn rejects_invalid_source_and_outside_targets() {
        let root = temp_dir("reject");
        let service = SnapshotService::new(root.join("snapshots"));
        let missing = service.create("steam:1", "C:/nao-existe-arcadia", "");
        assert_eq!(missing["ok"], false);
        let outside = service.restore("steam:1", "x", if cfg!(windows) { "C:/Windows" } else { "/" }, false);
        assert_eq!(outside["ok"], false);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_part_matches_client_rules() {
        assert_eq!(safe_part("steam:990080", "unknown"), "steam_990080");
        assert_eq!(safe_part("...", "fallback"), "fallback");
        assert_eq!(safe_part("../etc", "fallback"), ".._etc");
    }
}
