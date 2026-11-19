//! Runner Legendary (Epic): status e biblioteca, sem setup interativo.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};

use crate::AppState;

fn binary(state: &AppState) -> PathBuf {
    let name = if cfg!(windows) {
        "legendary.exe"
    } else {
        "legendary"
    };
    state.data_dir.join("runners").join(name)
}

fn legendary_config_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| arcadia_store::paths::home_dir().join("AppData").join("Local"));
        base.join("legendary")
    } else {
        arcadia_store::paths::home_dir()
            .join(".config")
            .join("legendary")
    }
}

pub fn status(state: &AppState) -> Value {
    let bin = binary(state);
    let installed = bin.is_file();
    let mut logged = false;
    let mut user = String::new();
    if let Ok(raw) = std::fs::read_to_string(legendary_config_dir().join("user.json")) {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            logged = value
                .get("refresh_token")
                .or_else(|| value.get("access_token"))
                .map(|token| !token.is_null())
                .unwrap_or(false);
            user = value
                .get("displayName")
                .or_else(|| value.get("account_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
    }
    json!({ "installed": installed, "logged": logged, "user": user })
}

pub async fn library(state: &AppState) -> Value {
    let bin = binary(state);
    if !bin.is_file() {
        return json!({ "ok": false, "error": "legendary_ausente" });
    }
    let output = tokio::process::Command::new(bin)
        .args(["list", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let output = match tokio::time::timeout(Duration::from_secs(120), output).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return json!({ "ok": false, "error": error.to_string() }),
        Err(_) => return json!({ "ok": false, "error": "legendary list: timeout" }),
    };
    if !output.status.success() {
        return json!({
            "ok": false,
            "error": String::from_utf8_lossy(&output.stderr).trim()
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<Value>(&stdout) {
        Ok(value) => {
            let games = value
                .as_array()
                .cloned()
                .or_else(|| value.get("games").and_then(Value::as_array).cloned())
                .unwrap_or_default();
            json!({ "ok": true, "games": games })
        }
        Err(_) => json!({ "ok": false, "error": "legendary list: resposta inválida" }),
    }
}

pub fn setup_not_ported() -> Value {
    json!({ "ok": false, "error": "legendary_setup_nao_portado" })
}

#[cfg(test)]
mod tests {
    use super::status;
    use crate::AppState;

    #[test]
    fn status_without_binary_is_honest() {
        let dir = std::env::temp_dir().join(format!("arcadia-legendary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());
        let value = status(&state);
        assert_eq!(value["installed"], false);
        assert_eq!(value["logged"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
