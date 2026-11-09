#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Shell Tauri do Arcadia. A UI React é a mesma; este binário substitui o
//! processo principal do Electron.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

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
    let args = args.unwrap_or_default();
    if let Some(result) = handle_shell_channel(&window, state.inner(), &channel, &args) {
        return result;
    }
    let shell = TauriShell { window };
    arcadia_ipc::dispatch_async(state.inner(), &shell, &channel, args).await
}

/// Canais que dependem de janela/SO e não pertencem ao dispatcher puro.
fn handle_shell_channel(
    window: &WebviewWindow,
    state: &AppState,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, IpcError>> {
    match channel {
        "app:openExternal" => {
            let target = args.first().and_then(Value::as_str).unwrap_or_default();
            if target.starts_with("http://") || target.starts_with("https://") {
                let _ = window.opener().open_url(target, None::<&str>);
            }
            Some(Ok(Value::Null))
        }
        "app:pickFolder" => Some(Ok(dialog_result(
            window.dialog().file().blocking_pick_folder(),
        ))),
        "app:pickFile" => Some(Ok(dialog_result(
            window.dialog().file().blocking_pick_file(),
        ))),
        "profile:pickImage" => Some(Ok(pick_profile_image(window, state, args))),
        "art:pick" => Some(Ok(pick_art(window, state, args))),
        "trailer:pickCookies" => {
            let Some(picked) = window
                .dialog()
                .file()
                .add_filter("Cookies", &["txt"])
                .blocking_pick_file()
            else {
                return Some(Ok(json!({ "ok": false })));
            };
            let Ok(path) = picked.into_path() else {
                return Some(Ok(json!({ "ok": false })));
            };
            let text = path.to_string_lossy().into_owned();
            let _ = arcadia_store::config::write_config_patch(
                &state.data_dir,
                &json!({ "youtube_cookies": text }),
            );
            Some(Ok(json!({ "ok": true, "path": path.to_string_lossy() })))
        }
        "app:diagnosticsExport" => {
            let Some(directory) = window.dialog().file().blocking_pick_folder() else {
                return Some(Ok(json!({ "ok": false, "canceled": true })));
            };
            let Ok(directory) = directory.into_path() else {
                return Some(Ok(json!({ "ok": false, "error": "destino_invalido" })));
            };
            let report =
                arcadia_ipc::diagnostics::collect(&state.data_dir, arcadia_ipc::APP_VERSION);
            Some(Ok(arcadia_ipc::diagnostics::create_support_bundle(
                &state.data_dir,
                &directory,
                &report,
            )))
        }
        "gamelog:open" => {
            let id = args.first().and_then(Value::as_str).unwrap_or_default();
            let safe: String = id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let path = state.data_dir.join("logs").join(format!("{safe}.log"));
            if !path.exists() {
                return Some(Ok(json!({ "ok": false, "error": "sem registros" })));
            }
            match window.opener().open_path(path.to_string_lossy(), None::<&str>) {
                Ok(()) => Some(Ok(json!({ "ok": true }))),
                Err(error) => Some(Ok(json!({ "ok": false, "error": error.to_string() }))),
            }
        }
        _ => None,
    }
}

fn dialog_result(path: Option<tauri_plugin_dialog::FilePath>) -> Value {
    let Some(path) = path else {
        return json!({ "ok": false });
    };
    match path.into_path() {
        Ok(path) => json!({ "ok": true, "path": path.to_string_lossy() }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

fn config_flag(window: &WebviewWindow, key: &str) -> bool {
    let Some(state) = window.app_handle().try_state::<AppState>() else {
        return false;
    };
    arcadia_store::config::read_config(&state.data_dir)
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn close_to_tray(window: &WebviewWindow) -> bool {
    config_flag(window, "close_to_tray")
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Mostrar Arcadia", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("Arcadia")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    } else if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png")) {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// Fator automático do `ui-scale.js`: menor dimensão lógica contra 1920×1080,
/// limitado a 1..1.7. `ARCADIA_UI_SCALE` sobrepõe (0.7..2).
fn automatic_scale(width: f64, height: f64) -> f64 {
    let ratio = (width / 1920.0).min(height / 1080.0);
    if !ratio.is_finite() || ratio <= 0.0 {
        return 1.0;
    }
    ratio.clamp(1.0, 1.7)
}

fn effective_scale(width: f64, height: f64) -> f64 {
    if let Ok(value) = std::env::var("ARCADIA_UI_SCALE") {
        let text = value.trim().to_ascii_lowercase();
        if !text.is_empty() && text != "auto" {
            if let Ok(number) = text.parse::<f64>() {
                if number > 0.0 {
                    return number.clamp(0.7, 2.0);
                }
            }
        }
    }
    automatic_scale(width, height)
}

fn apply_ui_scale(window: &WebviewWindow) {
    let Ok(size) = window.inner_size() else {
        return;
    };
    let factor = window.scale_factor().unwrap_or(1.0);
    let width = size.width as f64 / factor;
    let height = size.height as f64 / factor;
    let scale = effective_scale(width, height);
    let mode = window
        .app_handle()
        .try_state::<AppState>()
        .map(|state| state.mode())
        .unwrap_or_else(|| "desktop".to_string());
    let base = if mode == "console" { 1.3 } else { 1.2 };
    let zoom = (base * scale).clamp(0.7, 2.4);
    let _ = window.set_zoom(zoom);
    let capa = (1.6 * scale).clamp(0.9, 1.9);
    let _ = window.emit("ui:escala", json!({ "escala": scale, "capa": capa }));
}

fn pick_art(window: &WebviewWindow, state: &AppState, args: &[Value]) -> Value {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let kind = payload.get("kind").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() || !matches!(kind, "cover" | "hero" | "logo") {
        return json!({ "ok": false });
    }
    let mut dialog = window.dialog().file();
    dialog = if kind == "hero" {
        dialog
            .add_filter(
                "Imagens e vídeos",
                &["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"],
            )
            .add_filter("Vídeos (fundo animado)", &["webm", "mp4", "m4v", "mov"])
    } else {
        dialog.add_filter("Imagens", &["png", "jpg", "jpeg", "gif", "webp"])
    };
    let Some(picked) = dialog.blocking_pick_file() else {
        return json!({ "ok": false });
    };
    let Ok(source) = picked.into_path() else {
        return json!({ "ok": false });
    };
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_else(|| ".png".to_string());
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let directory = arcadia_media::art::art_dir(&state.data_dir);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return json!({ "ok": false, "error": error.to_string() });
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let destination = directory.join(format!("{safe}-{kind}-{stamp}{extension}"));
    match std::fs::copy(&source, &destination) {
        Ok(_) => json!({ "ok": true, "path": destination.to_string_lossy() }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

fn pick_profile_image(window: &WebviewWindow, state: &AppState, args: &[Value]) -> Value {
    let kind = args.first().and_then(Value::as_str).unwrap_or("avatar");
    let is_background = kind == "background" || kind == "banner";
    let mut dialog = window.dialog().file();
    if is_background {
        dialog = dialog
            .add_filter(
                "Imagens e vídeos",
                &["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"],
            )
            .add_filter("Vídeos (fundo animado)", &["webm", "mp4", "m4v", "mov"])
            .add_filter("Imagens", &["png", "jpg", "jpeg", "gif", "webp"]);
    } else {
        dialog = dialog.add_filter("Imagens", &["png", "jpg", "jpeg", "gif", "webp"]);
    }
    let Some(picked) = dialog.blocking_pick_file() else {
        return json!({ "ok": false });
    };
    let Ok(source) = picked.into_path() else {
        return json!({ "ok": false, "error": "arquivo inválido" });
    };
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_else(|| ".png".to_string());
    let name = if kind == "banner" {
        "banner"
    } else if is_background {
        "background"
    } else {
        "avatar"
    };
    let destination = state.data_dir.join(format!("{name}{extension}"));
    if let Err(error) = std::fs::copy(&source, &destination) {
        return json!({ "ok": false, "error": error.to_string() });
    }
    let clean = format!("file://{}", destination.to_string_lossy());
    let _ = arcadia_store::config::write_config_patch(
        &state.data_dir,
        &json!({ "profile": { name: clean } }),
    );
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    json!({ "ok": true, "path": format!("{clean}?t={stamp}") })
}

#[cfg(test)]
mod tests {
    use super::{automatic_scale, effective_scale};

    #[test]
    fn automatic_scale_clamps_to_reference_range() {
        assert_eq!(automatic_scale(1920.0, 1080.0), 1.0);
        assert_eq!(automatic_scale(960.0, 540.0), 1.0);
        assert!((automatic_scale(2560.0, 1440.0) - 1.333).abs() < 0.01);
        assert_eq!(automatic_scale(3840.0, 2160.0), 1.7);
    }

    #[test]
    fn effective_scale_uses_automatic_without_env() {
        // Sem ARCADIA_UI_SCALE no ambiente de teste.
        if std::env::var_os("ARCADIA_UI_SCALE").is_none() {
            assert_eq!(effective_scale(1920.0, 1080.0), 1.0);
        }
    }
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![arcadia_ipc])
        .setup(move |app| {
            // Eventos de background do launch (game:running/active/launchState)
            // saem pelo AppHandle, fora do ciclo request/response do IPC.
            {
                let state = app.state::<AppState>();
                let handle = app.handle().clone();
                state.set_event_sink(Arc::new(move |event, payload| {
                    let _ = handle.emit(event, payload);
                }));
                state.downloads.start_monitor();
            }

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

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("Arcadia")
            .inner_size(1600.0, 900.0)
            .min_inner_size(960.0, 540.0)
            .decorations(false)
            .fullscreen(mode == "console")
            .initialization_script(init_script)
            // Mesma ideia do ready-to-show: só aparece depois do primeiro paint.
            .on_page_load(|window, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && !config_flag(&window, "start_minimized")
                {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            })
            .build()?;

            let scale_window = window.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    apply_ui_scale(&scale_window);
                    if config_flag(&scale_window, "minimize_to_tray")
                        && scale_window.is_minimized().unwrap_or(false)
                    {
                        let _ = scale_window.hide();
                    }
                }
                WindowEvent::CloseRequested { api, .. } => {
                    if close_to_tray(&scale_window) {
                        api.prevent_close();
                        let _ = scale_window.hide();
                    }
                }
                _ => {}
            });
            install_tray(app)?;
            apply_ui_scale(&window);
            // O React lazy pode montar depois do primeiro Finished; reemite uma
            // vez com a janela estável para o console receber capa/escala.
            let late_window = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                apply_ui_scale(&late_window);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Arcadia");
}
