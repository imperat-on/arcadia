//! `gamelog:open` — abre no visualizador do SO o log de launch já existente.
//!
//! O caminho é o mesmo que o runtime de launch usa para nomear o arquivo:
//! `data_dir/logs/<safe_name(game_id)>.log`. A sanitização vem de
//! [`arcadia_launch::safe_log_name`], a mesma do escritor do log, para que
//! nomes como `steam:440` resolvam para `steam_440.log`.
//!
//! Nada é criado aqui: ausência de log vira `no_logs` sem tocar no disco.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::Shell;

pub const ERROR_NO_LOGS: &str = "no_logs";
pub const ERROR_INVALID_ID: &str = "invalid_game_id";

/// Resolve o arquivo de log do jogo. `None` significa id inválido.
///
/// Separadores de caminho e `..` são rejeitados de propósito: mesmo que
/// [`arcadia_launch::safe_log_name`] os substitua por `_`, um id assim não é
/// um nome legítimo de jogo e não deve escolher alvo nenhum.
pub fn log_file(data_dir: &Path, game_id: &str) -> Option<PathBuf> {
    if game_id.trim().is_empty()
        || game_id == "."
        || game_id == ".."
        || game_id.contains('/')
        || game_id.contains('\\')
    {
        return None;
    }
    let name = arcadia_launch::safe_log_name(game_id);
    Some(data_dir.join("logs").join(format!("{name}.log")))
}

/// Handler do canal. `arg` é o `gameId` string enviado pela UI.
pub fn open(state: &crate::AppState, shell: &impl Shell, arg: Option<&Value>) -> Value {
    let Some(game_id) = arg.and_then(Value::as_str) else {
        return json!({ "ok": false, "error": ERROR_INVALID_ID });
    };
    let Some(path) = log_file(&state.data_dir, game_id) else {
        return json!({ "ok": false, "error": ERROR_INVALID_ID });
    };
    if !path.is_file() {
        return json!({ "ok": false, "error": ERROR_NO_LOGS });
    }
    match shell.open_path(&path) {
        Ok(()) => json!({ "ok": true, "path": path.to_string_lossy() }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    struct RecordingShell {
        opened: Mutex<Vec<PathBuf>>,
        fail: bool,
    }

    impl RecordingShell {
        fn new(fail: bool) -> Self {
            Self {
                opened: Mutex::new(Vec::new()),
                fail,
            }
        }

        fn opened(&self) -> Vec<PathBuf> {
            self.opened
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl Shell for RecordingShell {
        fn minimize(&self) {}
        fn toggle_maximize(&self) {}
        fn close(&self) {}
        fn quit(&self) {}
        fn set_fullscreen(&self, _on: bool) -> Result<(), String> {
            Ok(())
        }
        fn toggle_fullscreen(&self) -> Result<(), String> {
            Ok(())
        }
        fn is_focused(&self) -> bool {
            true
        }
        fn boot_video(&self) -> Option<Vec<u8>> {
            None
        }
        fn emit(&self, _event: &str, _payload: Value) {}
        fn open_path(&self, path: &Path) -> Result<(), String> {
            self.opened
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(path.to_path_buf());
            if self.fail {
                Err("visualizador indisponível".to_string())
            } else {
                Ok(())
            }
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "arcadia-gamelog-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn opens_the_existing_log_named_like_the_launch_writer() {
        let root = temp_root("found");
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let expected = logs.join("steam_440.log");
        std::fs::write(&expected, b"launch").unwrap();

        let state = AppState::new(root.clone());
        let shell = RecordingShell::new(false);
        let result =
            crate::dispatch(&state, &shell, "gamelog:open", vec![json!("steam:440")]).unwrap();

        assert_eq!(result["ok"], true);
        assert_eq!(
            result["path"].as_str(),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(shell.opened(), vec![expected]);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_log_reports_no_logs_without_side_effects() {
        let root = temp_root("missing");
        let state = AppState::new(root.clone());
        let shell = RecordingShell::new(false);
        let result =
            crate::dispatch(&state, &shell, "gamelog:open", vec![json!("steam:440")]).unwrap();

        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], ERROR_NO_LOGS);
        assert!(shell.opened().is_empty());
        assert!(!root.join("logs").exists());
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_and_traversal_ids_are_rejected_without_opening_anything() {
        let root = temp_root("invalid");
        std::fs::write(root.join("escape.log"), b"fora").unwrap();
        let state = AppState::new(root.clone());
        let shell = RecordingShell::new(false);

        for id in ["", "   ", ".", "..", "../escape", "a/b", "a\\b"] {
            let result = crate::dispatch(&state, &shell, "gamelog:open", vec![json!(id)]).unwrap();
            assert_eq!(result["ok"], false, "id {id:?} deveria falhar");
            assert_eq!(result["error"], ERROR_INVALID_ID, "id {id:?}");
        }
        assert!(shell.opened().is_empty());
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn real_open_failure_is_returned_to_the_ui() {
        let root = temp_root("failure");
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let expected = logs.join("steam_440.log");
        std::fs::write(&expected, b"launch").unwrap();

        let state = AppState::new(root.clone());
        let shell = RecordingShell::new(true);
        let result =
            crate::dispatch(&state, &shell, "gamelog:open", vec![json!("steam:440")]).unwrap();

        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "visualizador indisponível");
        assert_eq!(shell.opened(), vec![expected]);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
