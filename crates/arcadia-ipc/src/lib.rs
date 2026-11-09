//! Dispatcher IPC — um único comando Tauri (`arcadia_ipc`) roteia os 202
//! canais do preload congelado para handlers Rust. Canais não portados
//! respondem `not_ported`; canais fora do contrato respondem `unknown_channel`.

pub mod channels;
pub mod diagnostics;
pub mod launch_runtime;
pub mod media_service;
pub mod news;
pub mod protondb;
pub mod steam_service;
pub mod trailer_service;

/// Versão-base do cliente Electron que este port substitui.
pub const APP_VERSION: &str = "1.5.5";

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::{json, Map, Value};
use tokio::sync::OnceCell;

use arcadia_api::auth::Session;
use arcadia_api::{ApiClient, Error as ApiFailure};
use arcadia_store::accounts::{account_file, AccountScope};
use arcadia_store::config::{merge_config, read_config, write_config_patch};
use arcadia_store::library::read_games;
use arcadia_media::http::{Fetcher, HttpClientFetcher};
use arcadia_plugins::ipc::{plugin_id_from_ipc, PluginIpc};
use arcadia_plugins::registry::PluginRegistry;
use arcadia_media::HltbClient;
use arcadia_social::community::{CommunityService, PageOptions};
use arcadia_social::friends::FriendsService;
use arcadia_sync::{SyncEngine, SyncError, SyncReport, SyncState};

use crate::launch_runtime::{EventSink, LaunchRuntime};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
    api: Option<Arc<ApiClient>>,
    session: Mutex<Option<Session>>,
    account: Mutex<Option<String>>,
    sync: Mutex<Option<SyncEngine>>,
    friends: Mutex<Option<FriendsService>>,
    community: Mutex<Option<CommunityService>>,
    pub launch: LaunchRuntime,
    pub news: news::NewsCache,
    plugins: Option<PluginRegistry>,
    boot_session: Mutex<Option<Session>>,
    boot_restored: OnceCell<()>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::build(data_dir, None)
    }

    pub fn with_api(data_dir: PathBuf, api: Arc<ApiClient>) -> Self {
        let state = Self::build(data_dir, Some(api));
        state.load_boot_session();
        state
    }

    fn build(data_dir: PathBuf, api: Option<Arc<ApiClient>>) -> Self {
        let config = arcadia_store::config::read_config(&data_dir);
        let config_console = ["start_in_console_mode", "launch_in_console_mode"]
            .iter()
            .any(|key| config.get(key).and_then(Value::as_bool) == Some(true));
        let mode = std::env::var("ARCADIA_MODE")
            .ok()
            .filter(|mode| mode == "console" || mode == "desktop")
            .unwrap_or_else(|| {
                if config_console {
                    "console".to_string()
                } else {
                    "desktop".to_string()
                }
            });
        let plugins = PluginRegistry::with_standard_builtins(&data_dir, false, false).ok();
        Self {
            data_dir,
            mode: Mutex::new(mode),
            api,
            session: Mutex::new(None),
            account: Mutex::new(None),
            sync: Mutex::new(None),
            friends: Mutex::new(None),
            community: Mutex::new(None),
            launch: LaunchRuntime::new(),
            news: news::NewsCache::default(),
            plugins,
            boot_session: Mutex::new(None),
            boot_restored: OnceCell::new(),
        }
    }

    pub fn mode(&self) -> String {
        lock(&self.mode).clone()
    }

    pub fn account(&self) -> Option<String> {
        lock(&self.account).clone()
    }

    pub fn api(&self) -> Option<&Arc<ApiClient>> {
        self.api.as_ref()
    }

    fn plugin_registry(&self, channel: &str) -> Result<&PluginRegistry, IpcError> {
        self.plugins.as_ref().ok_or_else(|| {
            IpcError::new("plugins_unavailable", "registro de plugins indisponível", channel)
        })
    }

    /// Eventos de background (launch/monitor) vão para a janela via este sink.
    pub fn set_event_sink(&self, sink: EventSink) {
        self.launch.set_event_sink(sink);
    }

    fn load_boot_session(&self) {
        match arcadia_store::session::load_session(&self.data_dir) {
            Ok(Some(value)) => match serde_json::from_value::<Session>(value) {
                Ok(session) => {
                    self.install_session_memory(&session);
                    *lock(&self.boot_session) = Some(session);
                }
                Err(error) => eprintln!("[boot] session.json ilegível: {error}"),
            },
            Ok(None) => {}
            Err(error) => eprintln!("[boot] sessão não restaurada: {error}"),
        }
    }

    fn install_session_memory(&self, session: &Session) {
        // Troca de conta invalida engines/serviços: escopo é imutável por sessão.
        *lock(&self.sync) = None;
        *lock(&self.friends) = None;
        *lock(&self.community) = None;
        *lock(&self.account) = account_from_session(session);
        *lock(&self.session) = Some(session.clone());
    }

    /// Engine de sync do usuário ativo. Criado sob demanda e em pull-only.
    fn sync_engine(&self, channel: &str) -> Result<SyncEngine, IpcError> {
        if let Some(engine) = lock(&self.sync).clone() {
            return Ok(engine);
        }
        let api = self.api().ok_or_else(|| {
            IpcError::new("api_unavailable", "cliente do backend indisponível", channel)
        })?;
        let account = self
            .account()
            .ok_or_else(|| IpcError::new("nao_logado", "sessão ausente", channel))?;
        let scope = AccountScope::with_active(self.data_dir.clone(), Some(&account));
        let engine = SyncEngine::new((**api).clone(), scope);
        *lock(&self.sync) = Some(engine.clone());
        Ok(engine)
    }

    fn friends_service(&self, channel: &str) -> Result<FriendsService, IpcError> {
        if let Some(service) = lock(&self.friends).clone() {
            return Ok(service);
        }
        let api = self.api().ok_or_else(|| {
            IpcError::new("api_unavailable", "cliente do backend indisponível", channel)
        })?;
        let account = self
            .account()
            .ok_or_else(|| IpcError::new("nao_logado", "sessão ausente", channel))?;
        let scope = AccountScope::with_active(self.data_dir.clone(), Some(account.as_str()));
        let service = FriendsService::new((**api).clone(), scope);
        *lock(&self.friends) = Some(service.clone());
        Ok(service)
    }

    fn community_service(&self, channel: &str) -> Result<CommunityService, IpcError> {
        if let Some(service) = lock(&self.community).clone() {
            return Ok(service);
        }
        let api = self.api().ok_or_else(|| {
            IpcError::new("api_unavailable", "cliente do backend indisponível", channel)
        })?;
        let account = self
            .account()
            .ok_or_else(|| IpcError::new("nao_logado", "sessão ausente", channel))?;
        let scope = AccountScope::with_active(self.data_dir.clone(), Some(account.as_str()));
        let service = CommunityService::new((**api).clone(), scope);
        *lock(&self.community) = Some(service.clone());
        Ok(service)
    }

    /// Instala a sessão salva no cliente HTTP uma única vez por boot.
    pub async fn ensure_boot_session(&self) {
        let Some(api) = self.api.clone() else {
            return;
        };
        self.boot_restored
            .get_or_init(|| async move {
                let session = lock(&self.boot_session).take();
                if let Some(session) = session {
                    api.auth().set_session_optimistic(session, false).await;
                }
            })
            .await;
    }

    fn current_session(&self) -> Option<Session> {
        lock(&self.session).clone()
    }

    fn install_session(&self, session: &Session) {
        let value = match serde_json::to_value(session) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("[session] serialização falhou: {error}");
                self.install_session_memory(session);
                return;
            }
        };
        if let Err(error) = arcadia_store::session::save_session(&self.data_dir, &value) {
            // Keyring indisponível não pode deslogar ninguém (mesma política do JS).
            eprintln!("[session] não foi possível persistir: {error}");
        }
        self.install_session_memory(session);
    }

    fn forget_session(&self) {
        let _ = arcadia_store::session::clear_session(&self.data_dir);
        *lock(&self.sync) = None;
        *lock(&self.friends) = None;
        *lock(&self.community) = None;
        *lock(&self.session) = None;
        *lock(&self.account) = None;
    }
}

fn account_from_session(session: &Session) -> Option<String> {
    session
        .user
        .get("user_metadata")
        .and_then(|metadata| metadata.get("username"))
        .and_then(Value::as_str)
        .or_else(|| session.user.get("username").and_then(Value::as_str))
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .map(str::to_string)
}

fn safe_session_value(session: &Session) -> Value {
    serde_json::to_value(session)
        .ok()
        .and_then(|value| arcadia_contracts::safe_account_session(&value))
        .unwrap_or(Value::Null)
}

fn api_failure_message(error: &ApiFailure) -> String {
    match error {
        ApiFailure::Api(api) => {
            if api
                .message
                .to_ascii_lowercase()
                .contains("invalid login credentials")
            {
                "credenciais_invalidas".to_string()
            } else {
                api.message.clone()
            }
        }
        ApiFailure::Transport(_) => "rede_indisponivel".to_string(),
        ApiFailure::Decode(_) => "resposta_invalida".to_string(),
    }
}

fn valid_username(username: &str) -> bool {
    let chars: Vec<char> = username.chars().collect();
    (3..=20).contains(&chars.len())
        && chars
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
}

fn valid_email(email: &str) -> bool {
    let mut parts = email.split('@');
    let (Some(user), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !user.is_empty()
        && !user.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(char::is_whitespace)
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

/// Chaves que o renderer pode gravar (espelho da allowlist F3 do main.js).
/// Sem esta guarda, um XSS no renderer poderia injetar caminhos/args arbitrários.
const ALLOWED_CONFIG: &[&str] = &[
    "language",
    "ui_scale",
    "card_scale",
    "accent",
    "default_wine_prefix_path",
    "steam_api_key",
    "steamgriddb_api_key",
    "hubcap_api_key",
    "realdebrid_token",
    "torbox_token",
    "alldebrid_token",
    "premiumize_token",
    "retroachievements_username",
    "retroachievements_token",
    "retroachievements_web_api_key",
    "slssteam_path",
    "profile",
    "steam_path",
    "theme",
    "console_wallpaper",
    "desktop_wallpaper",
    "accent_color",
    "bg_color",
    "font_size",
    "show_fps",
    "title",
    "bio",
    "avatar",
    "background",
    "hubcap_enabled",
    "hubcap_auto_connect",
    "region",
    "download_limit",
    "max_concurrent_downloads",
    "minimize_to_tray",
    "close_to_tray",
    "start_minimized",
    "launch_in_console_mode",
    "show_console_on_startup",
    "enable_controller_navigation",
    "controller_deadzone",
    "music_enabled",
    "music_volume",
    "music_auto_play",
    "system_theme",
    "notifications_enabled",
    "notification_volume",
    "trailer_auto",
    "youtube_cookies",
    "library_sidebar",
    "achievements_auto_capture",
    "theme_name",
    "content_font",
    "actions_font",
    "custom_css_path",
    "tiles_color",
    "always_titles",
    "no_click_outside",
    "no_smooth_scroll",
    "no_anim",
    "check_updates_on_start",
    "start_in_console_mode",
    "hide_changelog_on_start",
    "update_ja_avisado",
    "minimize_on_game_launch",
    "frameless_window",
    "disable_playtime_tracking",
    "discord_rich_presence",
    "discord_client_id",
    "download_cpu_cores",
    "default_install_path",
    "compatibility",
    "wine_arch",
    "wine_version",
    "steamcmd_path",
    "steam_auto_login",
    "steam_auto_launch",
    "epic_auto_login",
    "gog_auto_login",
    "sources",
];

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
    // Drift UI ↔ allowlist é descartado em silêncio, como no Electron.
    next.retain(|key, _| ALLOWED_CONFIG.contains(&key.as_str()));
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
    match channel {
        // ── Biblioteca e config ──────────────────────────────────────────
        "library:get" => {
            let games = read_games(&state.data_dir, state.account().as_deref());
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

        // ── Settings por jogo / overrides / jogos custom ─────────────────
        "gamesettings:get" => {
            let id = args.first().and_then(Value::as_str).unwrap_or_default();
            let all = read_json_object(&game_settings_path(state, id));
            let settings = all.get(id).cloned().unwrap_or_else(|| json!({}));
            Ok(json!({
                "settings": settings,
                "defaultPrefix": if id.is_empty() { String::new() } else { game_default_prefix(state, id) }
            }))
        }
        "gamesettings:set" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.is_empty() {
                return Ok(json!({}));
            }
            let patch = payload.get("patch").and_then(Value::as_object);
            let path = game_settings_path(state, id);
            let mut all = read_json_object(&path);
            let mut entry = all
                .get(id)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(patch) = patch {
                for (key, value) in patch {
                    if ALLOWED_GAME_SETTINGS.contains(&key.as_str()) {
                        entry.insert(key.clone(), value.clone());
                    }
                }
            }
            let entry = Value::Object(entry);
            all.insert(id.to_string(), entry.clone());
            write_json_object(&path, &Value::Object(all), "gamesettings:set")?;
            Ok(entry)
        }
        "overrides:set" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.is_empty() {
                return Ok(json_array(read_games(
                    &state.data_dir,
                    state.account().as_deref(),
                )));
            }
            let path = account_file(
                &state.data_dir,
                state.account().as_deref(),
                "overrides.json",
            );
            let all = read_json_object(&path);
            let mut next = all
                .get(id)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(patch) = payload.get("patch").and_then(Value::as_object) {
                for (key, value) in patch {
                    if value.is_null() {
                        next.remove(key);
                    } else {
                        next.insert(key.clone(), value.clone());
                    }
                }
            }
            let mut all = all;
            if next.is_empty() {
                all.remove(id);
            } else {
                all.insert(id.to_string(), Value::Object(next));
            }
            write_json_object(&path, &Value::Object(all), "overrides:set")?;
            Ok(json_array(read_games(
                &state.data_dir,
                state.account().as_deref(),
            )))
        }
        "app:diagnostics" => Ok(diagnostics::collect(&state.data_dir, APP_VERSION)),
        // Updater ainda não portado: a UI recebe o estado "sem suporte" em vez
        // de um erro de canal, e não oferece um download que não existe.
        "update:packaged:state" => Ok(json!({
            "canal": "fonte",
            "suportado": false,
            "versaoAtual": APP_VERSION,
            "versaoNova": null,
            "tamanho": null,
            "fase": "ocioso",
            "progresso": 0,
            "erro": null,
            "erroDeFundo": false,
            "erroAcao": null,
            "jaAvisado": false,
            "jogoRodando": false
        })),
        "update:state" => Ok(json!({
            "canal": "fonte",
            "suportado": false,
            "versao": APP_VERSION,
            "disponivel": false
        })),
        "update:check" => Ok(json!({ "ok": false, "error": "nao_suportado" })),
        "update:apply" => Ok(json!({ "ok": false, "error": "nao_suportado" })),
        "update:packaged:check" => Ok(json!({
            "ok": false, "disponivel": false, "motivo": "fonte"
        })),
        "update:packaged:download" => Ok(json!({ "ok": false, "erro": "fonte" })),
        "update:packaged:install" => Ok(json!({ "ok": false, "erro": "fonte" })),
        "update:packaged:jaAvisado" => Ok(json!({ "ok": false })),
        "app:diagnostico" => {
            let read_state = |name: &str| {
                std::fs::read_to_string(
                    account_file(&state.data_dir, state.account().as_deref(), name),
                )
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .unwrap_or(Value::Null)
            };
            Ok(json!({
                "contaAtiva": state.account().unwrap_or_else(|| "guest (raiz)".to_string()),
                "dataDir": state.data_dir.to_string_lossy(),
                "sessionExiste": state.data_dir.join("session.json").exists(),
                "sync": {
                    "estado": read_state("sync_state.json"),
                    "fila": read_state("sync_queue.json")
                },
                "report": diagnostics::collect(&state.data_dir, APP_VERSION)
            }))
        }
        "trailer:path" => Ok(trailer_service::local_path_json(
            state,
            args.first().and_then(Value::as_str).unwrap_or_default(),
        )),
        "saves:list" => {
            let game_id = args.first().and_then(Value::as_str).unwrap_or_default();
            let service =
                arcadia_store::snapshots::SnapshotService::new(state.data_dir.join("snapshots"));
            Ok(service.list(game_id))
        }
        "saves:create" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let game_id = payload.get("gameId").and_then(Value::as_str).unwrap_or_default();
            let source = payload.get("sourceDir").and_then(Value::as_str).unwrap_or_default();
            let label = payload.get("label").and_then(Value::as_str).unwrap_or_default();
            let service =
                arcadia_store::snapshots::SnapshotService::new(state.data_dir.join("snapshots"));
            Ok(service.create(game_id, source, label))
        }
        "saves:restore" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let game_id = payload.get("gameId").and_then(Value::as_str).unwrap_or_default();
            let snapshot_id = payload
                .get("snapshotId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let target = payload.get("targetDir").and_then(Value::as_str).unwrap_or_default();
            let backup = payload
                .get("backup")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let service =
                arcadia_store::snapshots::SnapshotService::new(state.data_dir.join("snapshots"));
            Ok(service.restore(game_id, snapshot_id, target, backup))
        }
        "saves:delete" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let game_id = payload.get("gameId").and_then(Value::as_str).unwrap_or_default();
            let snapshot_id = payload
                .get("snapshotId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let service =
                arcadia_store::snapshots::SnapshotService::new(state.data_dir.join("snapshots"));
            Ok(service.remove(game_id, snapshot_id))
        }
        "steam:contaStatus" => {
            let auto = read_config(&state.data_dir)
                .get("achievements_auto_capture")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok(steam_service::status(
                &state.data_dir,
                state.account().as_deref(),
                auto,
            ))
        }
        "steam:vincularConta" => Ok(steam_service::link_now(
            &state.data_dir,
            state.account().as_deref(),
        )),
        "steam:capturarAgora" => Ok(json!({
            "ok": false,
            "motivo": "captura_nao_portada"
        })),
        "steam:horasTodas" => Ok(steam_service::hours_json()),
        "steam:horasDoJogo" => Ok(steam_service::game_hours_json(
            args.first().and_then(Value::as_str).unwrap_or_default(),
        )),
        "plugins:list" => {
            let registry = state.plugin_registry(channel)?;
            Ok(serde_json::to_value(PluginIpc::new(registry).list())
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:details" => {
            let registry = state.plugin_registry(channel)?;
            Ok(serde_json::to_value(PluginIpc::new(registry).details())
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:get" => {
            let registry = state.plugin_registry(channel)?;
            let value = args.first().cloned().unwrap_or(Value::Null);
            Ok(serde_json::to_value(PluginIpc::new(registry).get_value(&value))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:register" => {
            let registry = state.plugin_registry(channel)?;
            let value = args.first().cloned().unwrap_or(Value::Null);
            Ok(serde_json::to_value(PluginIpc::new(registry).register_value(&value))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:unregister" => {
            let registry = state.plugin_registry(channel)?;
            let id = plugin_id_from_ipc(&args.first().cloned().unwrap_or(Value::Null));
            Ok(serde_json::to_value(registry.unregister(&id))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:enable" => {
            let registry = state.plugin_registry(channel)?;
            let value = args.first().cloned().unwrap_or(Value::Null);
            Ok(serde_json::to_value(PluginIpc::new(registry).enable_value(&value))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:disable" => {
            let registry = state.plugin_registry(channel)?;
            let value = args.first().cloned().unwrap_or(Value::Null);
            Ok(serde_json::to_value(PluginIpc::new(registry).disable_value(&value))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:verify" => {
            let registry = state.plugin_registry(channel)?;
            let value = args.first().cloned().unwrap_or(Value::Null);
            Ok(serde_json::to_value(PluginIpc::new(registry).verify_value(&value))
                .unwrap_or_else(|_| json!({ "ok": false })))
        }
        "plugins:install" | "plugins:remove" => {
            Ok(json!({ "ok": false, "error": "nao_suportado" }))
        }
        "integrations:status" => {
            let config = read_config(&state.data_dir);
            Ok(json!({
                "steam": config
                    .get("steam_api_key")
                    .and_then(Value::as_str)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false),
                "slssteam": 0,
                "heroic": false
            }))
        }
        "app:diskSpace" => {
            let target = args
                .first()
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(arcadia_store::paths::home_dir);
            let mut probe = target;
            while !probe.exists() {
                match probe.parent() {
                    Some(parent) if parent != probe => probe = parent.to_path_buf(),
                    _ => break,
                }
            }
            match (fs2::total_space(&probe), fs2::available_space(&probe)) {
                (Ok(total), Ok(free)) => Ok(json!({
                    "ok": true,
                    "total": total as f64 / 1024.0 / 1024.0 / 1024.0,
                    "free": free as f64 / 1024.0 / 1024.0 / 1024.0
                })),
                (Err(error), _) | (_, Err(error)) => {
                    Ok(json!({ "ok": false, "error": error.to_string() }))
                }
            }
        }
        "profile:stats" => {
            let games = read_games(&state.data_dir, state.account().as_deref());
            let steam_hours = steam_service::hours_by_appid()
                .map(|(hours, _)| hours)
                .unwrap_or_default();
            let minutes: i64 = games
                .iter()
                .map(|game| {
                    let id = game
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let steam = steam_hours
                        .get(&steam_service::normalize_appid(id))
                        .copied()
                        .unwrap_or(0) as i64;
                    let local = game
                        .get("playtime_minutes")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    steam.max(local)
                })
                .sum();
            Ok(json!({
                "jogos": games.len(),
                "playtime_hours": (minutes as f64 / 60.0).round() as i64
            }))
        }
        "customgame:add" => custom_game_add(state, args.first().cloned().unwrap_or_else(|| json!({}))),
        "customgame:update" => {
            custom_game_update(state, args.first().cloned().unwrap_or_else(|| json!({})))
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

/// Roteia os canais que precisam de rede/sessão. Os demais caem no dispatcher
/// síncrono local, mantendo a mesma superfície de 202 canais.
pub async fn dispatch_async(
    state: &AppState,
    shell: &impl Shell,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let result = dispatch_async_inner(state, shell, channel, args).await;
    if let Ok(path) = std::env::var("ARCADIA_IPC_LOG") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let outcome = match &result {
                Ok(value) => {
                    let text = value.to_string();
                    let head: String = text.chars().take(160).collect();
                    format!("ok {head}")
                }
                Err(error) => format!("err {}", error.code),
            };
            let _ = writeln!(file, "{channel} -> {outcome}");
        }
    }
    result
}

async fn dispatch_async_inner(
    state: &AppState,
    shell: &impl Shell,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    // Garante a sessão restaurada antes de qualquer leitura escopada por conta
    // (o boot do renderer pede account:status e library:get em paralelo).
    state.ensure_boot_session().await;

    match channel {
        "account:status" => {
            let session = state.current_session();
            Ok(json!({
                "session": session.as_ref().map(safe_session_value),
                "error": null
            }))
        }
        "account:signIn" => account_sign_in(state, channel, args).await,
        "account:signUp" => account_sign_up(state, channel, args).await,
        "account:signOut" => account_sign_out(state, channel).await,
        "account:profile" => account_profile(state, channel).await,
        "account:updateProfile" => account_update_profile(state, channel, args).await,
        "account:setAvatarBytes" => account_set_avatar_bytes(state, channel, args).await,
        "account:setAvatar" => account_set_avatar_path(state, channel, args).await,
        "account:setBackground" => account_set_background(state, channel, args).await,
        "art:download" => {
            let payload = args.first().cloned().unwrap_or_else(|| json!({}));
            let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
            let kind = payload.get("kind").and_then(Value::as_str).unwrap_or_default();
            let url = payload.get("url").and_then(Value::as_str).unwrap_or_default();
            if id.is_empty() || !matches!(kind, "cover" | "hero" | "logo" | "icon") {
                return Ok(json!({ "ok": false, "error": "pedido_invalido" }));
            }
            if !url.starts_with("https://") {
                return Ok(json!({ "ok": false, "error": "url rejeitada" }));
            }
            let Some(api) = state.api() else {
                return Ok(json!({ "ok": false, "error": "api_indisponivel" }));
            };
            let fetcher = arcadia_media::http::HttpClientFetcher::new(api.http().clone());
            let directory = arcadia_media::art::art_dir(&state.data_dir);
            if let Err(error) = std::fs::create_dir_all(&directory) {
                return Ok(json!({ "ok": false, "error": error.to_string() }));
            }
            let base = directory.join(format!(
                "{}-{}-{}",
                arcadia_media::art::safe_id(id),
                kind,
                chrono::Utc::now().timestamp_millis()
            ));
            match arcadia_media::art::download_to(&fetcher, url, &base).await {
                Ok(downloaded) => {
                    // Remove a arte anterior SÓ se ela vive em art/.
                    let overrides = media_service::manual_overrides_public(state);
                    if let Some(previous) = overrides
                        .get(id)
                        .and_then(Value::as_object)
                        .and_then(|entry| entry.get(kind))
                        .and_then(Value::as_str)
                    {
                        if let Some(old) =
                            arcadia_media::art::art_to_delete_in(previous, &directory)
                        {
                            let _ = std::fs::remove_file(old);
                        }
                    }
                    Ok(json!({ "ok": true, "path": downloaded.path.to_string_lossy() }))
                }
                Err(error) => Ok(json!({ "ok": false, "error": error.to_string() })),
            }
        }
        "game:protondb" => {
            let appid = args.first().and_then(Value::as_str).unwrap_or_default();
            Ok(json!({
                "ok": true,
                "info": protondb::info(appid).await
            }))
        }
        "trailer:search" => Ok(trailer_service::search(state, &args).await),
        "trailer:streamUrl" => Ok(trailer_service::stream_url(state, &args).await),
        "trailer:downloadUrl" => Ok(trailer_service::download_url(state, &args).await),
        "trailer:download" => Ok(trailer_service::download(state, &args).await),
        "trailer:downloadAll" => Ok(trailer_service::download_all(state).await),
        "meta:art" => Ok(media_service::meta_art(state, &args).await),
        "meta:text" => Ok(media_service::meta_text(state, &args).await),
        "arte:quadrada" => Ok(media_service::square_art(state, &args).await),
        "arte:logo" => Ok(media_service::logo_auto(state, &args).await),
        "hltb:get" => {
            let title = args.first().and_then(Value::as_str).unwrap_or_default();
            if title.trim().is_empty() {
                return Ok(Value::Null);
            }
            let Some(api) = state.api() else {
                return Ok(Value::Null);
            };
            let fetcher: Arc<dyn Fetcher> =
                Arc::new(HttpClientFetcher::new(api.http().clone()));
            let client = HltbClient::new(fetcher);
            Ok(client
                .search(title)
                .await
                .and_then(|times| serde_json::to_value(times).ok())
                .unwrap_or(Value::Null))
        }
        "news:get" => Ok(Value::Array(state.news.get(40).await)),
        "news:game" => {
            let appid = steam_service::normalize_appid(
                args.first().and_then(Value::as_str).unwrap_or_default(),
            );
            if appid.is_empty() {
                return Ok(json!([]));
            }
            let Some(api) = state.api() else {
                return Ok(json!([]));
            };
            let client = arcadia_steam::news::NewsClient::new(api.http().clone());
            let items = client.game_news(&appid).await;
            Ok(serde_json::to_value(items).unwrap_or_else(|_| json!([])))
        }
        "friends:search"
        | "friends:send"
        | "friends:accept"
        | "friends:cancel"
        | "friends:list"
        | "friends:achievements"
        | "friends:profile"
        | "friends:remove" => Ok(friends_channel(state, channel, args).await),
        "community:reviews"
        | "community:review:create"
        | "community:review:update"
        | "community:review:remove"
        | "community:review:report"
        | "community:collections"
        | "community:collection:get"
        | "community:collection:create"
        | "community:collection:update"
        | "community:collection:remove"
        | "community:collection:item:add"
        | "community:collection:item:replace"
        | "community:collection:item:remove"
        | "community:collection:report" => Ok(community_channel(state, channel, args).await),
        "game:launch" => {
            let payload = args.first().cloned().unwrap_or(Value::Null);
            Ok(state
                .launch
                .launch(&state.data_dir, state.account().as_deref(), &payload))
        }
        "game:close" => Ok(state.launch.close()),
        "sync:state" => Ok(match state.sync_engine(channel) {
            Ok(engine) => sync_state_json(&engine.state()),
            Err(_) => sync_state_json(&SyncState::default()),
        }),
        "sync:now" => run_sync(state, shell, channel, SyncMode::Now).await,
        "sync:full" => run_sync(state, shell, channel, SyncMode::All).await,
        "sync:all" => sync_all_local(state, shell, channel).await,
        "sync:deleteAchievement" => sync_delete_achievement(state, channel, args).await,
        _ => dispatch(state, shell, channel, args),
    }
}

#[derive(Clone, Copy)]
enum SyncMode {
    Now,
    All,
}

fn sync_state_json(state: &SyncState) -> Value {
    json!({
        "lastPullAt": state.last_pull_at,
        "lastSyncAt": state.last_sync_at,
        "lastError": state.last_error,
        "queueLen": state.queue_len
    })
}

fn sync_error_json(error: &SyncError) -> Value {
    json!({
        "ok": false,
        "error": error.code(),
        "retryable": error.is_retryable()
    })
}

fn sync_report_json(report: &SyncReport) -> Value {
    json!({
        "ok": true,
        "pushed": report.achievements_pushed,
        "pulled": report.achievements_pulled,
        "libraryPulled": report.library_pulled,
        "libraryPushed": report.library_pushed,
        "sourcesPulled": report.sources_pulled,
        "full": report.full
    })
}

fn emit_sync_state(shell: &impl Shell, engine: &SyncEngine) {
    shell.emit("sync:state", sync_state_json(&engine.state()));
}

async fn run_sync(
    state: &AppState,
    shell: &impl Shell,
    channel: &str,
    mode: SyncMode,
) -> Result<Value, IpcError> {
    let engine = match state.sync_engine(channel) {
        Ok(engine) => engine,
        Err(_) => return Ok(json!({ "ok": false, "error": "nao_logado", "retryable": false })),
    };
    let result = match mode {
        SyncMode::Now => engine.sync_now().await,
        SyncMode::All => engine.sync_all().await,
    };
    emit_sync_state(shell, &engine);
    match result {
        Ok(report) => {
            if report.library_changed {
                shell.emit("library:changed", json!({ "source": "biblioteca" }));
            }
            Ok(sync_report_json(&report))
        }
        Err(error) => Ok(sync_error_json(&error)),
    }
}

async fn sync_all_local(
    state: &AppState,
    shell: &impl Shell,
    channel: &str,
) -> Result<Value, IpcError> {
    let engine = match state.sync_engine(channel) {
        Ok(engine) => engine,
        Err(_) => return Ok(json!({ "ok": false, "error": "nao_logado", "retryable": false })),
    };
    let result = engine.push_achievements_all().await;
    emit_sync_state(shell, &engine);
    match result {
        Ok(outcome) => Ok(json!({ "ok": true, "pushed": outcome.pushed, "synced": outcome.pushed })),
        Err(error) => Ok(sync_error_json(&error)),
    }
}

fn arg_string(args: &[Value], index: usize) -> String {
    args.get(index)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn arg_value(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or_else(|| json!({}))
}

async fn friends_channel(state: &AppState, channel: &str, args: Vec<Value>) -> Value {
    let service = match state.friends_service(channel) {
        Ok(service) => service,
        Err(_) => return json!({ "ok": false, "error": "nao_logado" }),
    };
    let result = match channel {
        "friends:search" => service.search(&arg_string(&args, 0)).await,
        "friends:send" => service.send(&arg_string(&args, 0)).await,
        "friends:accept" => service.accept(&arg_string(&args, 0)).await,
        "friends:cancel" => service.cancel(&arg_string(&args, 0)).await,
        "friends:list" => {
            let forcar = arg_value(&args, 0)
                .get("forcar")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            service.list(forcar).await
        }
        "friends:achievements" => {
            service.friend_achievements_cached(&arg_string(&args, 0)).await
        }
        "friends:profile" => service.friend_profile(&arg_string(&args, 0)).await,
        "friends:remove" => service.remove_friend(&arg_string(&args, 0)).await,
        _ => return json!({ "ok": false, "error": "canal_desconhecido" }),
    };
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut envelope = error.to_friends_envelope();
            if channel == "friends:list" {
                if let Some(object) = envelope.as_object_mut() {
                    object.insert("deCache".into(), Value::Bool(false));
                }
            }
            envelope
        }
    }
}

async fn community_channel(state: &AppState, channel: &str, args: Vec<Value>) -> Value {
    let service = match state.community_service(channel) {
        Ok(service) => service,
        Err(_) => {
            return json!({
                "ok": false,
                "error": "nao_logado",
                "code": "nao_logado"
            })
        }
    };
    let result = match channel {
        "community:reviews" => {
            let appid = arg_string(&args, 0);
            let options = PageOptions::from_value(&arg_value(&args, 1));
            service.list_reviews(&appid, &options).await
        }
        "community:review:create" => service.create_review(&arg_value(&args, 0)).await,
        "community:review:update" => {
            service
                .update_review(&arg_string(&args, 0), &arg_value(&args, 1))
                .await
        }
        "community:review:remove" => service.remove_review(&arg_string(&args, 0)).await,
        "community:review:report" => {
            service
                .report_review(&arg_string(&args, 0), &arg_value(&args, 1))
                .await
        }
        "community:collections" => {
            let options = PageOptions::from_value(&arg_value(&args, 0));
            service.list_collections(&options).await
        }
        "community:collection:get" => service.get_collection(&arg_string(&args, 0)).await,
        "community:collection:create" => service.create_collection(&arg_value(&args, 0)).await,
        "community:collection:update" => {
            service
                .update_collection(&arg_string(&args, 0), &arg_value(&args, 1))
                .await
        }
        "community:collection:remove" => service.remove_collection(&arg_string(&args, 0)).await,
        "community:collection:item:add" => {
            service
                .add_collection_item(&arg_string(&args, 0), &arg_string(&args, 1))
                .await
        }
        "community:collection:item:replace" => {
            let id = arg_string(&args, 0);
            let items = arg_value(&args, 1)
                .as_array()
                .cloned()
                .unwrap_or_default();
            service.replace_collection_items(&id, &items).await
        }
        "community:collection:item:remove" => {
            service
                .remove_collection_item(&arg_string(&args, 0), &arg_string(&args, 1))
                .await
        }
        "community:collection:report" => {
            service
                .report_collection(&arg_string(&args, 0), &arg_value(&args, 1))
                .await
        }
        _ => return json!({ "ok": false, "error": "canal_desconhecido" }),
    };
    match result {
        Ok(value) => value,
        Err(error) => error.to_community_envelope(),
    }
}

const ALLOWED_GAME_SETTINGS: &[&str] = &[
    "wineVersion",
    "prefixPath",
    "uplayId",
    "upcId",
    "autoDXVK",
    "autoNVAPI",
    "autoVKD3D",
    "esync",
    "fsync",
    "wineWayland",
    "wow64",
    "fsrHack",
    "gamescope",
    "gsHdr",
    "gsWidth",
    "gsHeight",
    "gsFps",
    "gsWindowMode",
    "gsFramerateLimit",
    "dxvkHud",
    "mangohud",
    "gamemode",
    "verboseLogs",
    "gameArgs",
    "exePath",
    "scriptPre",
    "scriptPost",
    "wrappers",
    "envVars",
    "emulatorId",
    "romPath",
    "emulatorArgs",
    "emulatorCorePath",
];

fn game_settings_path(state: &AppState, _id: &str) -> PathBuf {
    account_file(
        &state.data_dir,
        state.account().as_deref(),
        "game_settings.json",
    )
}

fn game_default_prefix(state: &AppState, id: &str) -> String {
    let config = read_config(&state.data_dir);
    let base = config
        .get("default_wine_prefix_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            arcadia_store::paths::home_dir()
                .join("Games")
                .join("Arcadia")
                .join("Prefixes")
                .to_string_lossy()
                .into_owned()
        });
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Path::new(&base).join(safe).to_string_lossy().into_owned()
}

fn read_json_object(path: &std::path::Path) -> Map<String, Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn write_json_object(
    path: &std::path::Path,
    value: &Value,
    channel: &str,
) -> Result<(), IpcError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| IpcError::new("serialize", error.to_string(), channel))?;
    arcadia_store::atomic::write_atomic_text(path, &text)
        .map_err(|error| IpcError::new("io", error.to_string(), channel))
}

fn valid_custom_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("custom:") else {
        return false;
    };
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let first_ok = first.is_ascii_lowercase() || first.is_ascii_digit();
    first_ok
        && rest.chars().count() <= 101
        && rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn custom_games_result(state: &AppState) -> Value {
    json!({
        "ok": true,
        "games": read_games(&state.data_dir, state.account().as_deref())
    })
}

fn custom_game_add(state: &AppState, payload: Value) -> Result<Value, IpcError> {
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default().trim();
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let platform = payload
        .get("platform")
        .and_then(Value::as_str)
        .unwrap_or("windows");
    let is_emulator = platform == "emulator";

    if !valid_custom_id(id) || title.is_empty() || title.chars().count() > 200 {
        return Ok(json!({ "ok": false, "error": "título ou identificador inválido" }));
    }
    if is_emulator {
        let emulator = payload.get("emulatorId").and_then(Value::as_str).unwrap_or_default();
        let rom = payload.get("romPath").and_then(Value::as_str).unwrap_or_default();
        if emulator.is_empty() || rom.is_empty() {
            return Ok(json!({ "ok": false, "error": "configuração do emulador inválida" }));
        }
    } else {
        let exe = payload.get("exe").and_then(Value::as_str).unwrap_or_default();
        if exe.trim().is_empty() || exe.contains('\0') {
            return Ok(json!({ "ok": false, "error": "título e executável são obrigatórios" }));
        }
    }

    let path = account_file(
        &state.data_dir,
        state.account().as_deref(),
        "custom_games.json",
    );
    let mut games = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    if games
        .iter()
        .any(|game| game.get("id").and_then(Value::as_str) == Some(id))
    {
        return Ok(json!({ "ok": false, "error": "já existe um jogo com esse nome" }));
    }

    let mut entry = Map::new();
    entry.insert("id".into(), json!(id));
    entry.insert("title".into(), json!(title));
    entry.insert("launcher".into(), json!("custom"));
    entry.insert("platform".into(), json!(platform));
    entry.insert("installed".into(), json!(true));
    if !is_emulator {
        entry.insert(
            "exe".into(),
            payload.get("exe").cloned().unwrap_or(Value::Null),
        );
    }
    games.push(Value::Object(entry));
    write_json_object(&path, &Value::Array(games), "customgame:add")?;
    Ok(custom_games_result(state))
}

fn custom_game_update(state: &AppState, payload: Value) -> Result<Value, IpcError> {
    let id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
    let path = account_file(
        &state.data_dir,
        state.account().as_deref(),
        "custom_games.json",
    );
    let mut games = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    let Some(game) = games
        .iter_mut()
        .find(|game| game.get("id").and_then(Value::as_str) == Some(id))
    else {
        return Ok(json!({ "ok": false, "error": "jogo não encontrado" }));
    };
    let Some(object) = game.as_object_mut() else {
        return Ok(json!({ "ok": false, "error": "jogo inválido" }));
    };

    if let Some(title) = payload.get("title").and_then(Value::as_str) {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Ok(json!({ "ok": false, "error": "título inválido" }));
        }
        object.insert("title".into(), json!(title));
    }
    if let Some(platform) = payload.get("platform").and_then(Value::as_str) {
        if matches!(platform, "emulator" | "windows" | "linux") {
            object.insert("platform".into(), json!(platform));
        }
    }
    if let Some(exe) = payload.get("exe").and_then(Value::as_str) {
        if exe.contains('\0') {
            return Ok(json!({ "ok": false, "error": "executável inválido" }));
        }
        if !exe.is_empty() {
            object.insert("exe".into(), json!(exe));
        }
    }
    if object.get("platform").and_then(Value::as_str) == Some("emulator") {
        object.remove("exe");
    }

    write_json_object(&path, &Value::Array(games), "customgame:update")?;
    Ok(custom_games_result(state))
}

async fn sync_delete_achievement(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let appid = payload.get("appid").and_then(Value::as_str).unwrap_or_default();
    let apiname = payload
        .get("apiname")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if appid.is_empty() || apiname.is_empty() {
        return Ok(json!({ "ok": false, "error": "parametros_invalidos" }));
    }
    let engine = match state.sync_engine(channel) {
        Ok(engine) => engine,
        Err(_) => return Ok(json!({ "ok": false, "error": "nao_logado", "retryable": false })),
    };
    match engine.delete_achievement(appid, apiname).await {
        Ok(()) => Ok(json!({ "ok": true })),
        Err(error) => Ok(sync_error_json(&error)),
    }
}

async fn account_sign_in(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let username = payload
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let password = payload
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if !valid_username(&username) {
        return Ok(json!({ "ok": false, "error": "username_invalido" }));
    }
    if password.is_empty() {
        return Ok(json!({ "ok": false, "error": "senha_curta" }));
    }

    let api = state.api().ok_or_else(|| {
        IpcError::new("api_unavailable", "cliente do backend indisponível", channel)
    })?;
    match api.sign_in_with_username(&username, password).await {
        Ok(response) => {
            state.install_session(&response.session);
            trigger_reconcile(state, &response.session);
            Ok(json!({ "ok": true }))
        }
        Err(error) => Ok(json!({ "ok": false, "error": api_failure_message(&error) })),
    }
}

/// Pull/reconcile em background após login, como o `setImmediate` do Electron.
/// Fire-and-forget: a resposta de login não espera a rede.
fn trigger_reconcile(state: &AppState, session: &Session) {
    let Some(api) = state.api().cloned() else {
        return;
    };
    let Some(account) = account_from_session(session) else {
        return;
    };
    let data_dir = state.data_dir.clone();
    tokio::spawn(async move {
        let scope = AccountScope::with_active(data_dir, Some(account.as_str()));
        let engine = SyncEngine::new((*api).clone(), scope);
        let _ = engine.reconcile().await;
    });
}

async fn account_sign_up(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let email = payload
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let username = payload
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let password = payload
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if !valid_email(&email) {
        return Ok(json!({ "ok": false, "error": "email_invalido" }));
    }
    if !valid_username(&username) {
        return Ok(json!({ "ok": false, "error": "username_invalido" }));
    }
    if password.chars().count() < 6 {
        return Ok(json!({ "ok": false, "error": "senha_curta" }));
    }

    let api = state.api().ok_or_else(|| {
        IpcError::new("api_unavailable", "cliente do backend indisponível", channel)
    })?;

    match api
        .rpc("username_available", json!({ "p_username": username }))
        .await
    {
        Ok(Value::Bool(true)) => {}
        Ok(_) => return Ok(json!({ "ok": false, "error": "username_ocupado" })),
        Err(error) => {
            return Ok(json!({ "ok": false, "error": api_failure_message(&error) }))
        }
    }

    match api
        .auth()
        .sign_up(
            &email,
            password,
            Some(json!({ "data": { "username": username } })),
        )
        .await
    {
        Ok(response) => {
            let Some(session) = response.session else {
                return Ok(json!({ "ok": false, "error": "confirmacao_necessaria" }));
            };
            state.install_session(&session);
            trigger_reconcile(state, &session);
            let username_real = profile_username(api, &session)
                .await
                .unwrap_or_else(|| username.clone());
            Ok(json!({ "ok": true, "usernameReal": username_real }))
        }
        Err(error) => Ok(json!({ "ok": false, "error": api_failure_message(&error) })),
    }
}

async fn account_sign_out(
    state: &AppState,
    _channel: &str,
) -> Result<Value, IpcError> {
    let network_error = match state.api() {
        Some(api) => api.auth().sign_out().await.err(),
        None => None,
    };
    state.forget_session();
    match network_error {
        Some(error) => Ok(json!({ "ok": false, "error": api_failure_message(&error) })),
        None => Ok(json!({ "ok": true })),
    }
}

async fn account_profile(state: &AppState, channel: &str) -> Result<Value, IpcError> {
    let Some(session) = state.current_session() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(api) = state.api() else {
        return Err(IpcError::new(
            "api_unavailable",
            "cliente do backend indisponível",
            channel,
        ));
    };
    let Some(user_id) = session.user_id() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };

    let row = match api
        .from("profiles")
        .select(
            "username, avatar_url, display_name, summary, country, city, showcase, background_url, banner_url",
        )
        .eq("id", user_id)
        .maybe_single()
        .execute()
        .await
    {
        Ok(row) => row,
        Err(error) => return Ok(json!({ "ok": false, "error": api_failure_message(&error) })),
    };

    let text = |key: &str| row.get(key).cloned().unwrap_or(Value::Null);
    let showcase = match row.get("showcase") {
        Some(Value::Array(items)) => Value::Array(items.clone()),
        _ => json!([]),
    };
    Ok(json!({
        "ok": true,
        "profile": {
            "username": text("username"),
            "avatar_url": text("avatar_url"),
            "display_name": text("display_name"),
            "summary": text("summary"),
            "country": text("country"),
            "city": text("city"),
            "showcase": showcase,
            "background_url": text("background_url"),
            "banner_url": text("banner_url"),
        }
    }))
}

const PROFILE_FIELDS: [&str; 7] = [
    "display_name",
    "summary",
    "country",
    "city",
    "showcase",
    "background_url",
    "banner_url",
];

async fn account_update_profile(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let Some(session) = state.current_session() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(api) = state.api() else {
        return Err(IpcError::new(
            "api_unavailable",
            "cliente do backend indisponível",
            channel,
        ));
    };
    let Some(user_id) = session.user_id() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let payload = args.first().cloned().unwrap_or_else(|| json!({}));
    let mut patch = Map::new();
    for field in PROFILE_FIELDS {
        if let Some(value) = payload.get(field) {
            patch.insert(field.to_string(), value.clone());
        }
    }
    if patch.is_empty() {
        return Ok(json!({ "ok": false, "error": "sem_campos" }));
    }
    match api
        .from("profiles")
        .update(Value::Object(patch))
        .eq("id", user_id)
        .execute()
        .await
    {
        Ok(_) => Ok(json!({ "ok": true })),
        Err(error) => Ok(json!({ "ok": false, "error": api_failure_message(&error) })),
    }
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(".png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(".jpg");
    }
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(".webp");
    }
    if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return Some(".gif");
    }
    None
}

async fn upload_avatar(
    api: &ApiClient,
    user_id: &str,
    bytes: Vec<u8>,
    mime: &str,
    extension: &str,
) -> Value {
    let name = format!(
        "{user_id}/{}{}",
        chrono::Utc::now().timestamp_millis(),
        extension
    );
    let base = api.config().url.trim_end_matches('/');
    let old = api
        .from("profiles")
        .select("avatar_url")
        .eq("id", user_id)
        .maybe_single()
        .execute()
        .await
        .ok()
        .and_then(|row| row.get("avatar_url").and_then(Value::as_str).map(str::to_string));

    if let Err(error) = api
        .storage_from("avatars")
        .upload(&name, bytes, mime, false)
        .await
    {
        return json!({ "ok": false, "error": api_failure_message(&error) });
    }
    let avatar_url = format!("{base}/storage/v1/object/public/avatars/{name}");
    if let Err(error) = api
        .from("profiles")
        .update(json!({ "avatar_url": avatar_url }))
        .eq("id", user_id)
        .execute()
        .await
    {
        return json!({ "ok": false, "error": api_failure_message(&error) });
    }
    if let Some(old) = old {
        let prefix = format!("{base}/storage/v1/object/public/avatars/");
        if let Some(previous) = old.strip_prefix(&prefix) {
            if previous != name {
                let _ = api.storage_from("avatars").remove(&[previous]).await;
            }
        }
    }
    json!({ "ok": true, "avatar_url": avatar_url })
}

async fn account_set_avatar_path(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let Some(session) = state.current_session() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(api) = state.api() else {
        return Err(IpcError::new(
            "api_unavailable",
            "cliente do backend indisponível",
            channel,
        ));
    };
    let Some(user_id) = session.user_id() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let raw = args.first().and_then(Value::as_str).unwrap_or_default();
    let path = raw.strip_prefix("file://").unwrap_or(raw);
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(json!({ "ok": false, "error": "avatar_ilegivel" }));
    };
    if bytes.len() > 5 * 1024 * 1024 {
        return Ok(json!({ "ok": false, "error": "avatar_grande" }));
    }
    let Some(extension) = image_extension(&bytes) else {
        return Ok(json!({ "ok": false, "error": "avatar_nao_imagem" }));
    };
    let mime = match extension {
        ".png" => "image/png",
        ".jpg" => "image/jpeg",
        ".webp" => "image/webp",
        _ => "image/gif",
    };
    Ok(upload_avatar(api, user_id, bytes, mime, extension).await)
}

fn mime_for_extension(extension: &str) -> &'static str {
    match extension {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".mp4" | ".m4v" => "video/mp4",
        ".webm" => "video/webm",
        ".mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

async fn account_set_background(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let Some(session) = state.current_session() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(api) = state.api() else {
        return Err(IpcError::new(
            "api_unavailable",
            "cliente do backend indisponível",
            channel,
        ));
    };
    let Some(user_id) = session.user_id() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let raw = args.first().and_then(Value::as_str).unwrap_or_default();
    let path = raw.strip_prefix("file://").unwrap_or(raw);
    let kind = args.get(1).and_then(Value::as_str).unwrap_or("background");
    let field = if kind == "banner" {
        "banner_url"
    } else {
        "background_url"
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(json!({ "ok": false, "error": "arquivo_ilegivel" }));
    };
    if bytes.len() > 25 * 1024 * 1024 {
        return Ok(json!({ "ok": false, "error": "arquivo_grande" }));
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))
        .unwrap_or_else(|| ".png".to_string());
    let mime = mime_for_extension(&extension);
    let name = format!(
        "{user_id}/{}-{}{}",
        if kind == "banner" { "banner" } else { "background" },
        chrono::Utc::now().timestamp_millis(),
        extension
    );
    let base = api.config().url.trim_end_matches('/');
    if let Err(error) = api
        .storage_from("avatars")
        .upload(&name, bytes, mime, false)
        .await
    {
        return Ok(json!({ "ok": false, "error": api_failure_message(&error) }));
    }
    let url = format!("{base}/storage/v1/object/public/avatars/{name}");
    if let Err(error) = api
        .from("profiles")
        .update(json!({ field: url }))
        .eq("id", user_id)
        .execute()
        .await
    {
        return Ok(json!({ "ok": false, "error": api_failure_message(&error) }));
    }
    Ok(json!({ "ok": true, "url": url }))
}

async fn account_set_avatar_bytes(
    state: &AppState,
    channel: &str,
    args: Vec<Value>,
) -> Result<Value, IpcError> {
    let Some(session) = state.current_session() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(api) = state.api() else {
        return Err(IpcError::new(
            "api_unavailable",
            "cliente do backend indisponível",
            channel,
        ));
    };
    let Some(user_id) = session.user_id() else {
        return Ok(json!({ "ok": false, "error": "nao_logado" }));
    };
    let Some(bytes) = args
        .first()
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_u64)
                .filter(|value| *value <= 255)
                .map(|value| value as u8)
                .collect::<Vec<u8>>()
        })
    else {
        return Ok(json!({ "ok": false, "error": "avatar_ilegivel" }));
    };
    if bytes.is_empty() {
        return Ok(json!({ "ok": false, "error": "avatar_ilegivel" }));
    }
    if bytes.len() > 5 * 1024 * 1024 {
        return Ok(json!({ "ok": false, "error": "avatar_grande" }));
    }
    let Some(extension) = image_extension(&bytes) else {
        return Ok(json!({ "ok": false, "error": "avatar_nao_imagem" }));
    };
    let mime = args
        .get(1)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(match extension {
            ".png" => "image/png",
            ".jpg" => "image/jpeg",
            ".webp" => "image/webp",
            _ => "image/gif",
        });
    Ok(upload_avatar(api, user_id, bytes, mime, extension).await)
}

async fn profile_username(api: &ApiClient, session: &Session) -> Option<String> {
    let user_id = session.user_id()?;
    let row = api
        .from("profiles")
        .select("username")
        .eq("id", user_id)
        .maybe_single()
        .execute()
        .await
        .ok()?;
    row.get("username")
        .and_then(Value::as_str)
        .map(str::to_string)
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
    fn config_set_drops_keys_outside_allowlist() {
        let dir = std::env::temp_dir().join(format!("arcadia-ipc-allow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());
        dispatch(
            &state,
            &MockShell,
            "config:set",
            vec![json!({ "language": "en-US", "evil_path": "C:/tmp/evil", "wine_arch": "x32" })],
        )
        .unwrap();
        let stored = read_config(&dir);
        assert_eq!(stored["language"], "en-US");
        assert_eq!(stored["wine_arch"], "x32");
        assert!(stored.get("evil_path").is_none(), "chave fora do allowlist não pode ser gravada");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn game_settings_set_filters_allowlist_and_persists() {
        let dir = std::env::temp_dir().join(format!("arcadia-gamesettings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());
        dispatch(
            &state,
            &MockShell,
            "gamesettings:set",
            vec![json!({
                "id": "steam:10",
                "patch": { "wineVersion": "GE-Proton9", "prefixPath": "C:/pfx", "evil": "x" }
            })],
        )
        .unwrap();
        let stored = read_json_object(&dir.join("game_settings.json"));
        assert_eq!(stored["steam:10"]["wineVersion"], "GE-Proton9");
        assert_eq!(stored["steam:10"]["prefixPath"], "C:/pfx");
        assert!(stored["steam:10"].get("evil").is_none());

        let got = dispatch(
            &state,
            &MockShell,
            "gamesettings:get",
            vec![json!("steam:10")],
        )
        .unwrap();
        assert_eq!(got["settings"]["wineVersion"], "GE-Proton9");
        assert!(got["defaultPrefix"].as_str().unwrap().contains("Prefixes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plugins_list_and_invalid_register_use_envelopes() {
        let dir = std::env::temp_dir().join(format!("arcadia-plugins-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());
        let list = dispatch(&state, &MockShell, "plugins:list", Vec::new()).unwrap();
        assert_eq!(list["ok"], true);
        assert!(list["plugins"].is_array());
        let registered = dispatch(
            &state,
            &MockShell,
            "plugins:register",
            vec![json!("C:/nao-existe-arcadia")],
        )
        .unwrap();
        assert_eq!(registered["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_space_reports_existing_or_parent_path() {
        let state = state();
        let value = dispatch(
            &state,
            &MockShell,
            "app:diskSpace",
            vec![json!(std::env::temp_dir().join("arcadia-nao-existe-xyz"))],
        )
        .unwrap();
        assert_eq!(value["ok"], true, "diskSpace falhou: {value}");
        assert!(value["total"].as_f64().unwrap_or(0.0) > 0.0);
        assert!(value["free"].as_f64().unwrap_or(0.0) >= 0.0);
    }

    #[test]
    fn profile_stats_counts_library_games() {
        let dir = std::env::temp_dir().join(format!("arcadia-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("library.json"),
            r#"{"version":1,"games":[{"id":"a","title":"A","playtime_minutes":90},{"id":"b","title":"B","playtime_minutes":30}]}"#,
        )
        .unwrap();
        let state = AppState::new(dir.clone());
        let value = dispatch(&state, &MockShell, "profile:stats", Vec::new()).unwrap();
        assert_eq!(value["jogos"], 2);
        assert_eq!(value["playtime_hours"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_game_add_validates_and_returns_library() {
        let dir = std::env::temp_dir().join(format!("arcadia-custom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());

        let bad = dispatch(
            &state,
            &MockShell,
            "customgame:add",
            vec![json!({ "id": "Errado", "title": "x", "exe": "C:/g.exe" })],
        )
        .unwrap();
        assert_eq!(bad["ok"], false);

        let good = dispatch(
            &state,
            &MockShell,
            "customgame:add",
            vec![json!({
                "id": "custom:meu-jogo",
                "title": "Meu Jogo",
                "platform": "windows",
                "exe": "C:/Games/meu.exe"
            })],
        )
        .unwrap();
        assert_eq!(good["ok"], true);
        assert_eq!(good["games"][0]["id"], "custom:meu-jogo");
        assert_eq!(good["games"][0]["installed"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn profile_edits_require_session() {
        let state = state();
        let update = dispatch_async(
            &state,
            &MockShell,
            "account:updateProfile",
            vec![json!({ "display_name": "Zyon" })],
        )
        .await
        .unwrap();
        assert_eq!(update["ok"], false);
        assert_eq!(update["error"], "nao_logado");
        let avatar = dispatch_async(
            &state,
            &MockShell,
            "account:setAvatarBytes",
            vec![json!([1, 2, 3]), json!("image/png"), json!(".png")],
        )
        .await
        .unwrap();
        assert_eq!(avatar["ok"], false);
        assert_eq!(avatar["error"], "nao_logado");
        let avatar_path = dispatch_async(
            &state,
            &MockShell,
            "account:setAvatar",
            vec![json!("C:/nao-existe.png")],
        )
        .await
        .unwrap();
        assert_eq!(avatar_path["ok"], false);
        let background = dispatch_async(
            &state,
            &MockShell,
            "account:setBackground",
            vec![json!("C:/nao-existe.png"), json!("banner")],
        )
        .await
        .unwrap();
        assert_eq!(background["ok"], false);
    }

    #[tokio::test]
    async fn hltb_without_api_returns_null() {
        let state = state();
        let value = dispatch_async(&state, &MockShell, "hltb:get", vec![json!("Half-Life")])
            .await
            .unwrap();
        assert!(value.is_null());
    }

    #[tokio::test]
    async fn sync_state_signed_out_is_zeroed() {
        let state = state();
        let value = dispatch_async(&state, &MockShell, "sync:state", Vec::new())
            .await
            .unwrap();
        assert!(value["lastPullAt"].is_null());
        assert!(value["lastSyncAt"].is_null());
        assert!(value["lastError"].is_null());
        assert_eq!(value["queueLen"], 0);
    }

    #[tokio::test]
    async fn sync_now_signed_out_reports_nao_logado() {
        let state = state();
        let value = dispatch_async(&state, &MockShell, "sync:now", Vec::new())
            .await
            .unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "nao_logado");
    }

    #[tokio::test]
    async fn account_status_signed_out_is_safe() {
        let state = state();
        let value = dispatch_async(&state, &MockShell, "account:status", Vec::new())
            .await
            .unwrap();
        assert!(value["session"].is_null());
        assert!(value["error"].is_null());
    }

    #[tokio::test]
    async fn sign_in_without_api_is_typed_error() {
        let state = state();
        let error = dispatch_async(
            &state,
            &MockShell,
            "account:signIn",
            vec![json!({ "username": "zyon", "password": "segredo" })],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "api_unavailable");
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
