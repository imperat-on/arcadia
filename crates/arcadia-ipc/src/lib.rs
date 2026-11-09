//! Dispatcher IPC — um único comando Tauri (`arcadia_ipc`) roteia os 202
//! canais do preload congelado para handlers Rust. Canais não portados
//! respondem `not_ported`; canais fora do contrato respondem `unknown_channel`.

pub mod channels;

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Map, Value};

use arcadia_store::config::{merge_config, read_config, write_config_patch};
use arcadia_store::library::read_games;

#[derive(Debug, Clone, Serialize)]
pub struct IpcError {
    pub code: String,
    pub message: String,
    pub channel: String,
}

impl IpcError {
    pub fn new(code: &str, message: impl Into<String>, channel: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            channel: channel.to_string(),
        }
    }

    fn not_ported(channel: &str) -> Self {
        Self::new("not_ported", format!("canal ainda não portado: {channel}"), channel)
    }

    fn unknown(channel: &str) -> Self {
        Self::new("unknown_channel", format!("canal fora do contrato: {channel}"), channel)
    }

    fn invalid(channel: &str, message: impl Into<String>) -> Self {
        Self::new("invalid_args", message, channel)
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} ({})", self.code, self.message, self.channel)
    }
}

impl std::error::Error for IpcError {}

/// Estado compartilhado entre os handlers.
pub struct AppState {
    pub data_dir: PathBuf,
    pub mode: Mutex<String>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let mode = std::env::var("ARCADIA_MODE")
            .ok()
            .filter(|mode| mode == "console" || mode == "desktop")
            .unwrap_or_else(|| "desktop".to_string());
        Self {
            data_dir,
            mode: Mutex::new(mode),
        }
    }

    pub fn mode(&self) -> String {
        self.mode
            .lock()
            .map(|mode| mode.clone())
            .unwrap_or_else(|_| "desktop".to_string())
    }
}

/// Superfície mínima que o IPC precisa da janela. Mantém o crate testável
/// sem Tauri.
pub trait Shell: Send + Sync {
    fn minimize(&self);
    fn toggle_maximize(&self);
    fn close(&self);
    fn quit(&self);
    fn set_fullscreen(&self, on: bool) -> Result<(), String>;
    fn toggle_fullscreen(&self) -> Result<(), String>;
    fn is_focused(&self) -> bool;
    fn boot_video(&self) -> Option<Vec<u8>>;
    fn emit(&self, event: &str, payload: Value);
}

const SEGREDOS: [&str; 9] = [
    "steam_api_key",
    "steamgriddb_api_key",
    "hubcap_api_key",
    "retroachievements_token",
    "retroachievements_web_api_key",
    "realdebrid_token",
    "torbox_token",
    "alldebrid_token",
    "premiumize_token",
];

fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() > 8 {
        let prefix: String = chars.iter().take(3).collect();
        let suffix: String = chars.iter().skip(chars.len() - 2).collect();
        format!("{prefix}•••{suffix}")
    } else {
        "•••".to_string()
    }
}

/// `config:get` nunca devolve segredo em claro.
pub fn redact_config(config: &Value) -> Value {
    let Some(object) = config.as_object() else {
        return config.clone();
    };
    let mut out = object.clone();
    for key in SEGREDOS {
        if let Some(Value::String(value)) = out.get(key) {
            if !value.is_empty() {
                out.insert(key.to_string(), Value::String(mask(value)));
            }
        }
    }
    Value::Object(out)
}

/// Aplica o patch do renderer sem apagar segredos mascarados.
fn prepare_patch(current: &Value, patch: &Value, channel: &str) -> Result<Value, IpcError> {
    let Some(object) = patch.as_object() else {
        return Err(IpcError::invalid(channel, "config:set espera um objeto"));
    };
    let mut next = Map::new();
    for (key, value) in object {
        match value {
            Value::Null => {
                next.insert(key.clone(), Value::Null);
            }
            Value::String(text) if text.is_empty() => {
                next.insert(key.clone(), Value::Null);
            }
            Value::String(text) if SEGREDOS.contains(&key.as_str()) => {
                let current_masked = current
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(mask);
                if current_masked.as_deref() == Some(text.as_str()) {
                    // Máscara devolvida intacta = "não mexi": mantém o valor real.
                    continue;
                }
                next.insert(key.clone(), value.clone());
            }
            _ => {
                next.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(next))
}

fn json_array(value: Vec<Value>) -> Value {
    Value::Array(value)
}

/// Roteia um canal. `args` é sempre o array de argumentos da chamada JS.
pub fn dispatch(
    state: &AppState,
    shell: &impl Shell,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    // Instrumentação opt-in para validar o caminho renderer → Tauri → Rust
    // durante o port. Não escreve nada sem ARCADIA_IPC_LOG.
    if let Ok(path) = std::env::var("ARCADIA_IPC_LOG") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{channel} args={}", args.len());
        }
    }

    match channel {
        // ── Biblioteca e config ──────────────────────────────────────────
        "library:get" => {
            let games = read_games(&state.data_dir, None);
            Ok(json_array(games))
        }
        "config:get" => Ok(redact_config(&read_config(&state.data_dir))),
        "config:set" => {
            let patch = args
                .first()
                .ok_or_else(|| IpcError::invalid(channel, "config:set sem payload"))?;
            let current = read_config(&state.data_dir);
            let patch = prepare_patch(&current, patch, channel)?;
            let next = merge_config(&current, &patch);
            write_config_patch(&state.data_dir, &next)
                .map_err(|error| IpcError::new("io", error.to_string(), channel))?;
            Ok(json!({ "ok": true, "config": redact_config(&next) }))
        }

        // ── Janela e ciclo de vida ───────────────────────────────────────
        "win:minimize" => {
            shell.minimize();
            Ok(Value::Null)
        }
        "win:maximize" => {
            shell.toggle_maximize();
            Ok(Value::Null)
        }
        "win:close" => {
            shell.close();
            Ok(Value::Null)
        }
        "app:quit" => {
            shell.quit();
            Ok(Value::Null)
        }
        "app:focusState" => Ok(Value::Bool(shell.is_focused())),
        "app:setFullscreen" => {
            let on = args.first().and_then(Value::as_bool).unwrap_or(false);
            shell
                .set_fullscreen(on)
                .map_err(|error| IpcError::new("window", error, channel))?;
            Ok(json!({ "ok": true }))
        }
        "app:toggleFullscreen" => {
            shell
                .toggle_fullscreen()
                .map_err(|error| IpcError::new("window", error, channel))?;
            Ok(json!({ "ok": true }))
        }
        "app:setMode" => {
            let mode = args.first().and_then(Value::as_str).unwrap_or_default();
            if mode != "console" && mode != "desktop" {
                return Ok(json!({ "ok": false, "error": "modo inválido" }));
            }
            if let Ok(mut current) = state.mode.lock() {
                *current = mode.to_string();
            }
            std::env::set_var("ARCADIA_MODE", mode);
            shell
                .set_fullscreen(mode == "console")
                .map_err(|error| IpcError::new("window", error, channel))?;
            Ok(json!({ "ok": true }))
        }
        "app:enterConsole" => {
            if let Ok(mut current) = state.mode.lock() {
                *current = "console".to_string();
            }
            shell
                .set_fullscreen(true)
                .map_err(|error| IpcError::new("window", error, channel))?;
            Ok(json!({ "ok": true }))
        }

        // ── Boot ─────────────────────────────────────────────────────────
        "boot:video" => match shell.boot_video() {
            Some(bytes) => Ok(json!({
                "ok": true,
                "mime": "video/mp4",
                "data": bytes,
                "codecs": ["avc1.42E01E", "avc1.640028", "avc1.4D401E"]
            })),
            None => Ok(json!({ "ok": false })),
        },

        // ── Leituras de boot com estado vazio seguro ─────────────────────
        "account:status" => Ok(json!({ "session": null, "error": null })),
        "dm:queue" => Ok(json!([])),
        "torrent:list" => Ok(json!({ "downloads": [] })),
        "sync:state" => Ok(json!({
            "lastPullAt": null,
            "lastSyncAt": null,
            "lastError": null,
            "queueLen": 0
        })),

        // ── Ainda não portado ────────────────────────────────────────────
        other if channels::INVOKE_CHANNELS.contains(&other) => Err(IpcError::not_ported(other)),
        other => Err(IpcError::unknown(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockShell;

    impl Shell for MockShell {
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
    }

    fn state() -> AppState {
        AppState::new(std::env::temp_dir())
    }

    #[test]
    fn manifest_matches_generated_channel_lists() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../shim/ipc-manifest.json"
        ))
        .expect("shim/ipc-manifest.json");
        let manifest: Value = serde_json::from_str(&raw).unwrap();
        let manifest_invoke: Vec<&str> = manifest["invoke"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let manifest_events: Vec<&str> = manifest["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(manifest_invoke, channels::INVOKE_CHANNELS);
        assert_eq!(manifest_events, channels::EVENT_CHANNELS);
    }

    #[test]
    fn every_contract_channel_is_registered() {
        let state = state();
        for channel in channels::INVOKE_CHANNELS {
            let result = dispatch(&state, &MockShell, channel, Vec::new());
            match result {
                Ok(_) => {}
                Err(error) => assert_ne!(error.code, "unknown_channel", "canal não roteado: {channel}"),
            }
        }
    }

    #[test]
    fn unknown_channel_is_rejected() {
        let result = dispatch(&state(), &MockShell, "nope:channel", Vec::new());
        assert_eq!(result.unwrap_err().code, "unknown_channel");
    }

    #[test]
    fn config_get_redacts_secrets() {
        let dir = std::env::temp_dir().join(format!("arcadia-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{ "steam_api_key": "ABCDEF1234567890", "language": "pt-BR" }"#,
        )
        .unwrap();
        let state = AppState::new(dir.clone());
        let value = dispatch(&state, &MockShell, "config:get", Vec::new()).unwrap();
        assert_eq!(value["steam_api_key"], "ABC•••90");
        assert_eq!(value["language"], "pt-BR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_set_keeps_masked_secret() {
        let dir = std::env::temp_dir().join(format!("arcadia-ipc-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{ "steam_api_key": "ABCDEF1234567890" }"#,
        )
        .unwrap();
        let state = AppState::new(dir.clone());
        dispatch(
            &state,
            &MockShell,
            "config:set",
            vec![json!({ "steam_api_key": "ABC•••90", "language": "en-US" })],
        )
        .unwrap();
        let stored = read_config(&dir);
        assert_eq!(stored["steam_api_key"], "ABCDEF1234567890");
        assert_eq!(stored["language"], "en-US");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_mode_switches_state() {
        let state = state();
        dispatch(
            &state,
            &MockShell,
            "app:setMode",
            vec![Value::String("console".into())],
        )
        .unwrap();
        assert_eq!(state.mode(), "console");
        dispatch(
            &state,
            &MockShell,
            "app:setMode",
            vec![Value::String("desktop".into())],
        )
        .unwrap();
        assert_eq!(state.mode(), "desktop");
    }
}
