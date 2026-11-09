#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Shell Tauri do Arcadia. A UI React é a mesma; este binário substitui o
//! processo principal do Electron.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use arcadia_api::auth::AuthEvent;
use arcadia_ipc::{AppState, IpcError, Shell};

struct TauriShell {
    window: WebviewWindow,
}

impl TauriShell {
    fn boot_video_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(dir) = self.window.app_handle().path().resource_dir() {
            candidates.push(dir.join("resources").join("boot.mp4"));
            candidates.push(dir.join("boot.mp4"));
        }
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("boot.mp4"),
        );
        candidates.push(arcadia_store::paths::resolve_data_dir().join("boot.mp4"));
        candidates
    }
}

impl Shell for TauriShell {
    fn minimize(&self) {
        let _ = self.window.minimize();
    }

    fn toggle_maximize(&self) {
        if self.window.is_maximized().unwrap_or(false) {
            let _ = self.window.unmaximize();
        } else {
            let _ = self.window.maximize();
        }
    }

    fn close(&self) {
        let _ = self.window.close();
    }

    fn quit(&self) {
        self.window.app_handle().exit(0);
    }

    fn set_fullscreen(&self, on: bool) -> Result<(), String> {
        self.window.set_fullscreen(on).map_err(|e| e.to_string())
    }

    fn toggle_fullscreen(&self) -> Result<(), String> {
        let on = self.window.is_fullscreen().unwrap_or(false);
        self.set_fullscreen(!on)
    }

    fn is_focused(&self) -> bool {
        self.window.is_focused().unwrap_or(true)
    }

    fn boot_video(&self) -> Option<Vec<u8>> {
        self.boot_video_candidates()
            .into_iter()
            .find_map(|path| std::fs::read(path).ok())
    }

    fn emit(&self, event: &str, payload: Value) {
        let _ = self.window.app_handle().emit(event, payload);
    }
}

#[tauri::command]
async fn arcadia_ipc(
    window: WebviewWindow,
    state: tauri::State<'_, AppState>,
    channel: String,
    args: Option<Vec<Value>>,
) -> Result<Value, IpcError> {
    let shell = TauriShell { window };
    arcadia_ipc::dispatch_async(state.inner(), &shell, &channel, args.unwrap_or_default()).await
}

fn main() {
    let data_dir = arcadia_store::paths::resolve_data_dir();
    let api = match arcadia_api::ApiClient::from_env() {
        Ok(api) => Some(Arc::new(api)),
        Err(error) => {
            eprintln!("[boot] cliente do backend indisponível: {error}");
            None
        }
    };
    let state = match api.clone() {
        Some(api) => AppState::with_api(data_dir.clone(), api),
        None => AppState::new(data_dir.clone()),
    };
    let data_dir_for_events = data_dir.clone();
    let api_for_events = api.clone();

    tauri::Builder::default()
        // Duas instâncias gravando o mesmo config/library corrompem dados.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(state)
        .invoke_handler(tauri::generate_handler![arcadia_ipc])
        .setup(move |app| {
            // Persiste sessão criada/renovada e espelha account:changed para a UI.
            if let Some(api) = api_for_events.clone() {
                let app_handle = app.handle().clone();
                let dir = data_dir_for_events.clone();
                tauri::async_runtime::spawn(async move {
                    let mut eventos = api.auth().on_auth_state_change();
                    loop {
                        let change = match eventos.recv().await {
                            Ok(change) => change,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        };
                        if let Some(session) = &change.session {
                            if let Ok(value) = serde_json::to_value(session) {
                                let _ = arcadia_store::session::save_session(&dir, &value);
                            }
                        }
                        if matches!(change.event, AuthEvent::SignedOut | AuthEvent::UserDeleted) {
                            let _ = arcadia_store::session::clear_session(&dir);
                        }
                        let safe = change
                            .session
                            .as_ref()
                            .and_then(|session| serde_json::to_value(session).ok())
                            .and_then(|value| arcadia_contracts::safe_account_session(&value));
                        let _ = app_handle.emit(
                            "account:changed",
                            json!({ "event": change.event.as_str(), "session": safe }),
                        );
                    }
                });
            }

            let state = app.state::<AppState>();
            let mode = state.mode();
            let boot = json!({
                "platform": arcadia_store::paths::platform(),
                "home": arcadia_store::paths::home_dir(),
                "localAppData": std::env::var("LOCALAPPDATA").ok(),
                "dataDir": state.data_dir,
                "mode": mode,
            });
            let init_script = format!("globalThis.__ARCADIA_BOOT__ = {boot};");

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("Arcadia")
            .inner_size(1600.0, 900.0)
            .min_inner_size(960.0, 540.0)
            .decorations(false)
            .fullscreen(mode == "console")
            .initialization_script(init_script)
            // Mesma ideia do ready-to-show: só aparece depois do primeiro paint.
            .on_page_load(|window, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            })
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Arcadia");
}
