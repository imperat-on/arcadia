//! `sync_state.json` por conta — espelho de `sync.js` + `biblioteca.js` +
//! `sources.js`.
//!
//! O arquivo é compartilhado pelos três domínios:
//!
//! - `lastPullAt` / `lastSyncAt` / `lastError` (cursor de conquistas e UI);
//! - `libPush` (watermark de jogos enviados);
//! - `playtimePush` (minutos já enviados por appid);
//! - `srcPush` (sources já enviadas);
//! - `steamPlaytime` (últimas linhas de `pull_steam_playtime`);
//! - `emptyPullStreak` (proteção contra pull vazio acidental).
//!
//! Chaves desconhecidas são preservadas em toda escrita.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use arcadia_store::accounts::AccountScope;

use crate::util;

pub const SYNC_STATE_FILE: &str = "sync_state.json";

/// Estado tipado exposto à UI (equivalente ao `getState()` de `sync.js`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    pub last_pull_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
    pub queue_len: usize,
}

/// Documento JSON cru do `sync_state.json`, com merge raso preservando o resto.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateDocument {
    root: Map<String, Value>,
}

impl StateDocument {
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Object(root) => Self { root },
            _ => Self::default(),
        }
    }

    pub fn as_value(&self) -> Value {
        Value::Object(self.root.clone())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.root.get(key)
    }

    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.root.insert(key.into(), value);
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.root.remove(key)
    }

    /// Objeto em `key`; se ausente/inválido devolve vazio (o JS usaria `|| {}`).
    pub fn object(&self, key: &str) -> Map<String, Value> {
        match self.root.get(key) {
            Some(Value::Object(object)) => object.clone(),
            _ => Map::new(),
        }
    }

    pub fn array(&self, key: &str) -> Vec<Value> {
        match self.root.get(key) {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        }
    }

    pub fn last_pull_at(&self) -> Option<i64> {
        self.root
            .get("lastPullAt")
            .and_then(util::js_number)
            .map(|value| value.floor() as i64)
            .filter(|value| *value > 0)
    }

    pub fn last_sync_at(&self) -> Option<i64> {
        self.root
            .get("lastSyncAt")
            .and_then(util::js_number)
            .map(|value| value.floor() as i64)
            .filter(|value| *value > 0)
    }

    pub fn last_error(&self) -> Option<String> {
        match self.root.get("lastError") {
            Some(value) if !value.is_null() => {
                let text = util::js_string(value);
                Some(text)
            }
            _ => None,
        }
    }

    pub fn set_last_pull_at(&mut self, seconds: i64) {
        self.root.insert("lastPullAt".into(), Value::from(seconds));
    }

    pub fn set_last_sync_at(&mut self, seconds: i64) {
        self.root.insert("lastSyncAt".into(), Value::from(seconds));
    }

    pub fn set_last_error(&mut self, message: Option<&str>) {
        self.root.insert(
            "lastError".into(),
            message
                .map(|message| Value::String(message.to_string()))
                .unwrap_or(Value::Null),
        );
    }

    pub fn empty_pull_streak(&self) -> i64 {
        self.root
            .get("emptyPullStreak")
            .and_then(util::js_number)
            .map(|value| value.floor() as i64)
            .unwrap_or(0)
    }

    pub fn set_empty_pull_streak(&mut self, streak: i64) {
        self.root
            .insert("emptyPullStreak".into(), Value::from(streak));
    }

    pub fn contains_watermark(&self, key: &str, id: &str) -> bool {
        self.object(key).keys().any(|existing| existing == id)
    }

    pub fn watermark_ids(&self, key: &str) -> HashSet<String> {
        self.object(key).into_iter().map(|(id, _)| id).collect()
    }
}

/// Acesso ao `sync_state.json` da conta ativa.
#[derive(Debug, Clone)]
pub struct SyncStateFile {
    path: PathBuf,
}

impl SyncStateFile {
    pub fn new(scope: &AccountScope) -> Self {
        Self {
            path: scope.account_file(SYNC_STATE_FILE),
        }
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_document(&self) -> StateDocument {
        StateDocument::from_value(util::read_json(&self.path).unwrap_or(Value::Null))
    }

    pub fn write_document(&self, document: &StateDocument) -> io::Result<()> {
        util::write_json_atomic(&self.path, &document.as_value(), true)
    }

    /// Aplica uma mutação atômica no documento (read-modify-write).
    pub fn update<F>(&self, mutate: F) -> io::Result<()>
    where
        F: FnOnce(&mut StateDocument),
    {
        let mut document = self.read_document();
        mutate(&mut document);
        self.write_document(&document)
    }

    pub fn state(&self, queue_len: usize) -> SyncState {
        let document = self.read_document();
        SyncState {
            last_pull_at: document.last_pull_at(),
            last_sync_at: document.last_sync_at(),
            last_error: document.last_error(),
            queue_len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-sync-state-{tag}-{}-{}",
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
    fn preserves_unknown_fields_and_reads_typed_state() {
        let dir = temp_dir("unknown");
        let path = dir.join(SYNC_STATE_FILE);
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "lastPullAt": 1700000000,
                "lastSyncAt": "1700000001",
                "lastError": null,
                "futureKey": { "keep": true },
                "libPush": { "steam:10": { "title": "Portal", "platform": "windows" } }
            }))
            .unwrap(),
        )
        .unwrap();

        let file = SyncStateFile::from_path(&path);
        let document = file.read_document();
        assert_eq!(document.last_pull_at(), Some(1_700_000_000));
        assert_eq!(document.last_sync_at(), Some(1_700_000_001));
        assert_eq!(document.last_error(), None);
        assert!(document.contains_watermark("libPush", "steam:10"));

        file.update(|document| document.set_last_error(Some("rede_indisponivel")))
            .unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["futureKey"], json!({ "keep": true }));
        assert_eq!(saved["lastError"], "rede_indisponivel");
        assert_eq!(saved["libPush"]["steam:10"]["title"], "Portal");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
