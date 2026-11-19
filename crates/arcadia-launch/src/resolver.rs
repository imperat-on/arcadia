//! Launch-intent resolution and the typed launch plan.
//!
//! Two layers are ported here:
//!
//! * [`resolve_launch_request`] is a 1:1 port of
//!   `app/electron/launch-resolver.js`.  It only decides which argv to use and
//!   never trusts an arbitrary renderer-provided command.
//! * [`plan_launch`] ports the launch policy from `main.js`
//!   (`exeLaunchCmd` + `applyGameSettings`): Proton/UMU, prefix, Wine env,
//!   gamescope, GameMode, MangoHud, wrappers, game args and scripts.
//!
//! Every command is an argument array.  Nothing in this module builds a shell
//! string.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gamescope::{
    build_external_gamescope_command, GamescopeSettings, ProcessSession, SystemdSession,
};
use crate::platform::{strip_exe, strip_lower_exe, Platform};
use crate::session::should_track_game_session;

pub const ERR_GAME_NOT_FOUND: &str = "Jogo não encontrado na biblioteca.";
pub const ERR_NO_COMMAND: &str =
    "Sem comando de lançamento (cmd vazio). Verifique o executável do jogo em Configurações.";
pub const ERR_EXE_NOT_FOUND: &str = "Executável configurado não encontrado (exePath vazio).";
pub const ERR_LEGACY_REJECTED: &str = "Comando de lançamento rejeitado (padrão não permitido).";
pub const ERR_ALREADY_RUNNING: &str = "Já existe um lançamento ou jogo em execução.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchError(pub String);

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LaunchError {}

impl From<&str> for LaunchError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for LaunchError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A library record as consumed by the resolver.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub launch_cmd: Vec<String>,
    #[serde(default)]
    pub launcher: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub exe: Option<String>,
}

/// Return shape used by `customLaunchCmd`, `exeLaunchCmd` and emulator
/// resolution in the Electron main process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuiltCommand {
    pub cmd: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
}

impl BuiltCommand {
    pub fn new(cmd: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            cmd: cmd.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
            cwd: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmulatorLaunchResult {
    Failed { error: String },
    Command(BuiltCommand),
}

/// Injected lookups.  This is the Rust equivalent of the resolver's
/// `{ findGame, customLaunchCmd, getGameSettings, exeLaunchCmd, emulatorLaunch }`
/// dependency bag.
pub trait LaunchResolverDeps {
    fn find_game(&self, id: &str) -> Option<GameEntry>;
    fn custom_launch_cmd(&self, id: &str) -> Option<BuiltCommand>;
    fn get_game_settings(&self, id: &str) -> GameSettings;
    fn exe_launch_cmd(&self, id: &str, exe: &str) -> Option<BuiltCommand>;
    fn emulator_launch(
        &self,
        id: &str,
        game: &GameEntry,
        settings: &GameSettings,
    ) -> Option<EmulatorLaunchResult>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchRequest {
    #[serde(default, rename = "gameId")]
    pub game_id: Option<String>,
    #[serde(default)]
    pub cmd: Option<Vec<String>>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// The renderer may send the legacy `["steam", "steam://run/<id>"]` array or a
/// request object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchPayload {
    Command(Vec<String>),
    Request(LaunchRequest),
}

impl LaunchPayload {
    pub fn from_json(value: &Value) -> Self {
        if let Value::Array(values) = value {
            return LaunchPayload::Command(values.iter().map(js_string).collect());
        }
        let Value::Object(object) = value else {
            return LaunchPayload::Request(LaunchRequest::default());
        };
        let game_id = object
            .get("gameId")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let game_id_is_truthy_non_string = match object.get("gameId") {
            Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
            Some(Value::Bool(value)) => *value,
            Some(Value::Array(_) | Value::Object(_)) => true,
            _ => false,
        };
        let cmd = if game_id_is_truthy_non_string {
            // A non-string gameId is falsy only when null/false/empty.  When it
            // is truthy, JavaScript ignores `cmd` entirely and ends in the
            // "no command" branch.
            None
        } else {
            object
                .get("cmd")
                .and_then(Value::as_array)
                .map(|values| values.iter().map(js_string).collect::<Vec<String>>())
        };
        let mode = object
            .get("mode")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        LaunchPayload::Request(LaunchRequest { game_id, cmd, mode })
    }
}

fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchSource {
    Steam,
    Proton,
    Wine,
    Native,
    Emulator,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Library,
    Custom,
    Emulator,
    Exe,
    Legacy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunch {
    pub game_id: Option<String>,
    pub mode: Option<String>,
    pub raw_cmd: Vec<String>,
    pub env_extra: BTreeMap<String, String>,
    pub cwd: String,
    pub source: LaunchSource,
    /// `shouldTrackGameSession(rawCmd)` before any wrapper is applied.
    pub track_session: bool,
}

/// Resolve the renderer request into the argv the main process may build on.
pub fn resolve_launch_request(
    payload: &LaunchPayload,
    deps: &dyn LaunchResolverDeps,
) -> Result<ResolvedLaunch, LaunchError> {
    let (explicit_game_id, mode, mut raw_cmd) = match payload {
        LaunchPayload::Command(cmd) => (None, None, Some(cmd.clone())),
        LaunchPayload::Request(request) => {
            let game_id = request.game_id.clone().filter(|value| !value.is_empty());
            let raw_cmd = if game_id.is_some() {
                // A gameId always wins over a renderer-supplied cmd.
                None
            } else {
                request.cmd.clone()
            };
            (game_id, request.mode.clone(), raw_cmd)
        }
    };

    let mut env_extra = BTreeMap::new();
    let mut cwd = String::new();
    let mut origin = Origin::Legacy;

    if let Some(game_id) = explicit_game_id.as_deref() {
        if game_id.starts_with("custom:") {
            let settings = deps.get_game_settings(game_id);
            let custom_game = deps.find_game(game_id);
            let emulated = custom_game
                .as_ref()
                .and_then(|game| deps.emulator_launch(game_id, game, &settings));
            if let Some(result) = emulated {
                let built = apply_emulator_result(result)?;
                raw_cmd = Some(built.cmd);
                env_extra = built.env;
                cwd = built.cwd;
                origin = Origin::Emulator;
            } else {
                let built = deps
                    .custom_launch_cmd(game_id)
                    .filter(|built| !built.cmd.is_empty());
                let Some(built) = built else {
                    return Err(LaunchError(format!(
                        "Jogo custom não encontrado em custom_games.json (id: {game_id})."
                    )));
                };
                raw_cmd = Some(built.cmd);
                env_extra = built.env;
                cwd = built.cwd;
                origin = Origin::Custom;
            }
        } else {
            let Some(game) = deps.find_game(game_id) else {
                return Err(LaunchError(ERR_GAME_NOT_FOUND.to_string()));
            };
            let settings = deps.get_game_settings(game_id);

            if mode.as_deref() != Some("steam")
                && settings
                    .emulator_id
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            {
                if let Some(result) = deps.emulator_launch(game_id, &game, &settings) {
                    let built = apply_emulator_result(result)?;
                    raw_cmd = Some(built.cmd);
                    env_extra = built.env;
                    cwd = built.cwd;
                    origin = Origin::Emulator;
                }
            }

            if raw_cmd.is_none()
                && mode.as_deref() == Some("exe")
                && settings
                    .exe_path
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            {
                let exe = settings.exe_path.clone().unwrap_or_default();
                let built = deps
                    .exe_launch_cmd(game_id, &exe)
                    .filter(|built| !built.cmd.is_empty());
                let Some(built) = built else {
                    return Err(LaunchError(ERR_EXE_NOT_FOUND.to_string()));
                };
                raw_cmd = Some(built.cmd);
                env_extra = built.env;
                cwd = built.cwd;
                origin = Origin::Exe;
            }

            if raw_cmd.is_none() {
                if game.launch_cmd.is_empty() {
                    return Err(LaunchError(ERR_NO_COMMAND.to_string()));
                }
                raw_cmd = Some(game.launch_cmd);
                origin = Origin::Library;
            }
        }
    } else if let Some(command) = raw_cmd.as_ref() {
        // Legacy arrays are the only renderer-controlled command shape, and
        // only the two Steam store actions are accepted.
        let valid = command.len() == 2 && command[0] == "steam" && is_legacy_steam_uri(&command[1]);
        if !valid {
            return Err(LaunchError(ERR_LEGACY_REJECTED.to_string()));
        }
    }

    let Some(raw_cmd) = raw_cmd.filter(|command| !command.is_empty()) else {
        return Err(LaunchError(ERR_NO_COMMAND.to_string()));
    };
    let source = classify_source(&raw_cmd, &env_extra, origin);
    let track_session = should_track_game_session(&raw_cmd);
    Ok(ResolvedLaunch {
        game_id: explicit_game_id,
        mode,
        raw_cmd,
        env_extra,
        cwd,
        source,
        track_session,
    })
}

fn apply_emulator_result(result: EmulatorLaunchResult) -> Result<BuiltCommand, LaunchError> {
    match result {
        EmulatorLaunchResult::Failed { error } => Err(LaunchError(error)),
        EmulatorLaunchResult::Command(built) if built.cmd.is_empty() => {
            Err(LaunchError(ERR_NO_COMMAND.to_string()))
        }
        EmulatorLaunchResult::Command(built) => Ok(built),
    }
}

fn is_legacy_steam_uri(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("steam://install/")
        .or_else(|| value.strip_prefix("steam://run/"))
    else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn classify_source(
    command: &[String],
    env: &BTreeMap<String, String>,
    origin: Origin,
) -> LaunchSource {
    if origin == Origin::Emulator {
        return LaunchSource::Emulator;
    }
    let base = command
        .first()
        .map(|value| strip_exe(basename_any(value)))
        .unwrap_or_default();
    if base == "steam" {
        return LaunchSource::Steam;
    }
    if base == "umu-run"
        || (base == "proton" && command.get(1).map(String::as_str) == Some("run"))
        || env.contains_key("PROTONPATH")
        || env.contains_key("STEAM_COMPAT_DATA_PATH")
    {
        return LaunchSource::Proton;
    }
    if base == "wine" {
        return LaunchSource::Wine;
    }
    match origin {
        Origin::Custom => LaunchSource::Custom,
        _ => LaunchSource::Native,
    }
}

fn basename_any(value: &str) -> &str {
    value.rsplit(['/', '\\']).next().unwrap_or(value)
}

/// `steamSilencioso`: hide the client when it is opened only to launch a game.
/// Only the documented `steam://rungameid|run` shape gets `-silent`.
pub fn steam_silent(command: &[String]) -> Vec<String> {
    if command.len() < 2 {
        return command.to_vec();
    }
    let base = strip_exe(basename_any(&command[0]));
    if base != "steam" {
        return command.to_vec();
    }
    if command.iter().any(|value| value == "-silent") {
        return command.to_vec();
    }
    let opens_game = command
        .iter()
        .any(|value| value.starts_with("steam://rungameid/") || value.starts_with("steam://run/"));
    if !opens_game {
        return command.to_vec();
    }
    let mut output = Vec::with_capacity(command.len() + 1);
    output.push(command[0].clone());
    output.push("-silent".to_string());
    output.extend(command.iter().skip(1).cloned());
    output
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamLaunchKind {
    Uri(String),
    AppLaunch(String),
}

/// Detect both supported Steam launch forms.  `-applaunch` is intentionally
/// never rewritten to `-silent`: the frozen client only adds it for URIs.
pub fn steam_launch_kind(command: &[String]) -> Option<SteamLaunchKind> {
    if strip_exe(basename_any(command.first()?)) != "steam" {
        return None;
    }
    for value in command.iter().skip(1) {
        let lower = value.to_ascii_lowercase();
        if lower.starts_with("steam://rungameid/") || lower.starts_with("steam://run/") {
            return Some(SteamLaunchKind::Uri(value.clone()));
        }
    }
    let position = command.iter().position(|value| value == "-applaunch")?;
    let app_id = command.get(position + 1)?;
    if !app_id.is_empty() {
        Some(SteamLaunchKind::AppLaunch(app_id.clone()))
    } else {
        None
    }
}

pub fn steam_game_id(command: &[String]) -> Option<String> {
    match steam_launch_kind(command)? {
        SteamLaunchKind::Uri(uri) => {
            let tail = uri.rsplit('/').next().unwrap_or("");
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                None
            } else {
                Some(digits)
            }
        }
        SteamLaunchKind::AppLaunch(app_id) => Some(app_id),
    }
}

fn steam_has_launch_uri(command: &[String]) -> bool {
    command.iter().any(|value| {
        let lower = value.to_ascii_lowercase();
        lower.starts_with("steam://rungameid/") || lower.starts_with("steam://run/")
    })
}

/// `splitArgs`: whitespace tokens with single/double quote support.
pub fn split_args(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let start = index;
        let first = chars[index];
        if first == '"' || first == '\'' {
            if let Some(close) = chars[index + 1..].iter().position(|ch| *ch == first) {
                tokens.push(chars[index + 1..index + 1 + close].iter().collect());
                index = index + 1 + close + 1;
                continue;
            }
        }
        while index < chars.len() && !chars[index].is_whitespace() {
            index += 1;
        }
        let mut token: String = chars[start..index].iter().collect();
        if token.starts_with(['"', '\'']) {
            token.remove(0);
        }
        if token.ends_with(['"', '\'']) {
            token.pop();
        }
        tokens.push(token);
    }
    tokens
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Wrapper {
    pub cmd: String,
    pub args: Option<String>,
}

/// Per-game settings consumed by the planner.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameSettings {
    pub exe_path: Option<String>,
    pub wine_version: Option<String>,
    pub prefix_path: Option<String>,
    pub emulator_id: Option<String>,
    pub rom_path: Option<String>,
    pub emulator_args: Option<String>,
    pub emulator_core_path: Option<String>,
    pub esync: bool,
    pub fsync: bool,
    pub wine_wayland: bool,
    pub wow64: bool,
    pub fsr_hack: bool,
    #[serde(rename = "autoNVAPI")]
    pub auto_nvapi: bool,
    #[serde(rename = "autoDXVK")]
    pub auto_dxvk: Option<bool>,
    #[serde(rename = "autoVKD3D")]
    pub auto_vkd3d: bool,
    pub dxvk_hud: Option<String>,
    pub verbose_logs: bool,
    pub mangohud: bool,
    pub gamemode: bool,
    pub gamescope: bool,
    pub gs_hdr: bool,
    pub gs_width: Option<i64>,
    pub gs_height: Option<i64>,
    pub gs_fps: Option<i64>,
    pub gs_window_mode: Option<String>,
    pub gs_framerate_limit: Option<i64>,
    pub game_args: Option<String>,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub env_vars: Vec<EnvVar>,
    pub wrappers: Vec<Wrapper>,
}

impl GameSettings {
    fn auto_dxvk(&self) -> bool {
        self.auto_dxvk != Some(false)
    }

    fn has_truthy(&self, value: &Option<String>) -> bool {
        value.as_deref().is_some_and(|inner| !inner.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtonInstall {
    pub id: String,
    pub name: String,
    pub path: String,
    pub wine: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphicsLibsRequest {
    pub prefix: String,
    pub wine: String,
    pub dxvk: bool,
    pub nvapi: bool,
    pub vkd3d: bool,
    /// `pfx` for Proton (`<prefix>/pfx`), `root` for legendary's dedicated
    /// prefix (`<prefix>/drive_c/...`).
    pub layout: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamOverride {
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

/// Injected environment for the planner.
pub struct PlanEnvironment<'a> {
    pub platform: Platform,
    pub base_env: BTreeMap<String, String>,
    pub protons: Vec<ProtonInstall>,
    pub default_prefix: Box<dyn Fn(&str) -> String + 'a>,
    pub bin_exists: Box<dyn Fn(&str) -> bool + 'a>,
    pub fs_exists: Box<dyn Fn(&str) -> bool + 'a>,
    pub systemd: Option<SystemdSession>,
    pub steam_override: Option<SteamOverride>,
}

impl<'a> PlanEnvironment<'a> {
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            base_env: BTreeMap::new(),
            protons: Vec::new(),
            default_prefix: Box::new(|_| String::new()),
            bin_exists: Box::new(|_| false),
            fs_exists: Box::new(|_| false),
            systemd: None,
            steam_override: None,
        }
    }

    /// Environment populated from the current process, for the real spawn
    /// wiring.  Tests should use [`PlanEnvironment::new`] and fill the maps.
    pub fn from_host() -> Self {
        let platform = Platform::current();
        let base_env: BTreeMap<String, String> = std::env::vars().collect();
        let path_value = base_env.get("PATH").cloned().unwrap_or_default();
        let separator = platform.path_list_separator();
        let path_dirs: Vec<String> = path_value
            .split(separator)
            .filter(|dir| !dir.is_empty())
            .map(ToString::to_string)
            .collect();
        Self {
            platform,
            base_env,
            protons: Vec::new(),
            default_prefix: Box::new(|_| String::new()),
            bin_exists: Box::new(move |cmd: &str| {
                if cmd.is_empty() {
                    return false;
                }
                if cmd.contains('/') || cmd.contains('\\') {
                    return std::path::Path::new(cmd).exists();
                }
                path_dirs
                    .iter()
                    .any(|dir| std::path::Path::new(dir).join(cmd).exists())
            }),
            fs_exists: Box::new(|path: &str| std::path::Path::new(path).exists()),
            systemd: None,
            steam_override: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub executable: String,
    pub args: Vec<String>,
    /// Final environment (base env plus every launch override).
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub source: LaunchSource,
    pub warnings: Vec<String>,
    pub process_session: Option<ProcessSession>,
    pub steam_wrapper: bool,
    /// `shouldTrackGameSession(finalCmd)`.
    pub track_session: bool,
    /// `shouldTrackGameSession(rawCmd)` before wrappers.
    pub request_tracked: bool,
    pub game_id: String,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub graphics_libs: Option<GraphicsLibsRequest>,
}

impl LaunchPlan {
    pub fn command(&self) -> Vec<String> {
        std::iter::once(self.executable.clone())
            .chain(self.args.iter().cloned())
            .collect()
    }
}

const GAMESCOPE_ENV_KEYS: [&str; 8] = [
    "WINEPREFIX",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "GAMESCOPE_WAYLAND_DISPLAY",
    "ENABLE_GAMESCOPE_WSI",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "STEAM_GAME_DISPLAY_0",
];

/// Build the final launch plan from a resolved request and per-game settings.
pub fn plan_launch(
    resolved: &ResolvedLaunch,
    settings: &GameSettings,
    environment: &PlanEnvironment<'_>,
) -> Result<LaunchPlan, LaunchError> {
    let platform = environment.platform;
    let mut warnings = Vec::new();
    let mut command = steam_silent(&resolved.raw_cmd);
    if command.is_empty() {
        return Err(LaunchError(ERR_NO_COMMAND.to_string()));
    }
    let steam_wrapper =
        strip_exe(basename_any(&command[0])) == "steam" && steam_has_launch_uri(&command);
    let mut steam_override_warnings = Vec::new();
    let mut steam_override_applied = false;

    // SLSsteam wrapper: replace the `steam` executable but not the URI.
    if platform.is_unix() {
        if let Some(overridden) = &environment.steam_override {
            let is_plain_steam = basename_any(&command[0]) == "steam";
            if is_plain_steam && steam_has_launch_uri(&command) {
                let mut next = Vec::with_capacity(command.len());
                next.push(overridden.command.clone());
                next.extend(command.iter().skip(1).cloned());
                command = next;
                steam_override_warnings = overridden.warnings.clone();
                steam_override_applied = true;
            }
        }
    }

    let steam_target = strip_lower_exe(basename_any(&command[0])) == "steam";
    // `main.js` deletes the Electron LD_* variables only when the executable
    // is literally named `steam` (after the SLSsteam replacement).
    let steam_env_target = basename_any(&command[0]) == "steam";
    let mut env = environment.base_env.clone();

    if platform.is_unix() {
        if settings.esync {
            env.insert("WINEESYNC".into(), "1".into());
        }
        if settings.fsync {
            env.insert("WINEFSYNC".into(), "1".into());
        }
        if settings.wine_wayland {
            env.insert("PROTON_ENABLE_WAYLAND".into(), "1".into());
        }
        if settings.wow64 {
            env.insert("PROTON_USE_WOW64".into(), "1".into());
        }
        if settings.fsr_hack {
            env.insert("WINE_FULLSCREEN_FSR".into(), "1".into());
        }
        if settings.auto_nvapi {
            env.insert("DXVK_ENABLE_NVAPI".into(), "1".into());
        }
        if let Some(hud) = settings
            .dxvk_hud
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            env.insert("DXVK_HUD".into(), hud.to_string());
        }
        if settings.verbose_logs {
            if !env.get("WINEDEBUG").is_some_and(|value| !value.is_empty()) {
                env.insert("WINEDEBUG".into(), "+timestamp,+pid,+tid,+seh,+warn".into());
            }
            if !settings.has_truthy(&settings.dxvk_hud) {
                env.insert("DXVK_HUD".into(), "full".into());
            }
        }
        if settings.mangohud && !(environment.bin_exists)("mangohud") {
            env.insert("MANGOHUD".into(), "1".into());
        }
        if let Some(prefix) = settings
            .prefix_path
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            env.insert("WINEPREFIX".into(), prefix.to_string());
        }
    }

    for variable in &settings.env_vars {
        if is_env_name(&variable.name) {
            env.insert(
                variable.name.clone(),
                variable.value.clone().unwrap_or_default(),
            );
        }
    }

    let mut graphics_libs = None;
    let mut final_command = command;

    // Legendary (Epic) dedicated prefix / Wine override.
    if platform.is_unix()
        && basename_any(&final_command[0]).ends_with("legendary")
        && final_command.get(1).map(String::as_str) == Some("launch")
    {
        let prefix = settings
            .prefix_path
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                resolved
                    .game_id
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|game_id| (environment.default_prefix)(game_id))
                    .filter(|value| !value.is_empty())
            });
        if let Some(prefix) = prefix.as_deref() {
            env.insert("WINEPREFIX".into(), prefix.to_string());
            final_command.push("--wine-prefix".into());
            final_command.push(prefix.to_string());
        }
        if let Some(wine_version) = settings
            .wine_version
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            let proton = environment
                .protons
                .iter()
                .find(|proton| proton.id == wine_version);
            match proton {
                Some(proton) if (environment.fs_exists)(&proton.wine) => {
                    final_command.push("--wine".into());
                    final_command.push(proton.wine.clone());
                    if let Some(prefix) = prefix.as_deref() {
                        if (environment.fs_exists)(
                            &Platform::Linux.join(&[prefix, "drive_c", "windows", "system32"]),
                        ) {
                            graphics_libs = Some(GraphicsLibsRequest {
                                prefix: prefix.to_string(),
                                wine: proton.wine.clone(),
                                dxvk: settings.auto_dxvk(),
                                nvapi: settings.auto_nvapi,
                                vkd3d: settings.auto_vkd3d,
                                layout: "root".into(),
                            });
                        }
                    }
                }
                _ => warnings.push(format!(
                    "versão do Wine \"{wine_version}\" não encontrada — usando a do sistema"
                )),
            }
        }
    }

    // Game arguments are appended after the command, never for Steam.
    if !steam_target {
        if let Some(game_args) = settings
            .game_args
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            final_command.extend(split_args(game_args));
        }
    }

    let mut process_session: Option<ProcessSession> = None;

    if platform.is_unix() && settings.gamescope && !steam_target {
        if (environment.bin_exists)("gamescope") {
            if settings.gs_hdr {
                env.insert("ENABLE_GAMESCOPE_WSI".into(), "1".into());
            }
            let session = environment.systemd.clone();
            if session.is_none() && platform == Platform::Linux {
                warnings.push(
                    "systemd do usuário indisponível — handoff de launchers pode não ser acompanhado pelo Gamescope"
                        .to_string(),
                );
            }
            let mut environment_keys: Vec<String> = env.keys().cloned().collect();
            environment_keys.extend(resolved.env_extra.keys().cloned());
            if steam_override_applied {
                if let Some(overridden) = &environment.steam_override {
                    environment_keys.extend(overridden.env.keys().cloned());
                }
            }
            environment_keys.extend(GAMESCOPE_ENV_KEYS.iter().map(|key| (*key).to_string()));
            let gamescope_settings = GamescopeSettings {
                width: settings.gs_width.unwrap_or(0),
                height: settings.gs_height.unwrap_or(0),
                fps: settings.gs_fps.unwrap_or(0),
                hdr: settings.gs_hdr,
                window_mode: settings
                    .gs_window_mode
                    .clone()
                    .unwrap_or_else(|| "fullscreen".to_string()),
                framerate_limit: settings.gs_framerate_limit.unwrap_or(0),
                keep_alive: session.as_ref().is_some_and(|value| value.keep_alive),
                systemd_unit: session
                    .as_ref()
                    .map(|value| value.unit.clone())
                    .unwrap_or_default(),
                environment_keys,
            };
            let wrapped = build_external_gamescope_command(&final_command, &gamescope_settings)
                .map_err(|error| LaunchError(error.to_string()))?;
            final_command = wrapped.cmd;
            process_session = wrapped.process_session;
        } else {
            warnings.push("gamescope não está instalado — iniciando sem ele".to_string());
        }
    }

    if platform.is_unix() && settings.gamemode && !steam_target {
        if (environment.bin_exists)("gamemoderun") {
            final_command.insert(0, "gamemoderun".to_string());
        } else {
            warnings.push("gamemoderun não está instalado — iniciando sem ele".to_string());
        }
    }

    if platform.is_unix() && settings.mangohud && !steam_target {
        if (environment.bin_exists)("mangohud") {
            final_command.insert(0, "mangohud".to_string());
        } else {
            warnings.push("mangohud não está instalado — iniciando sem ele".to_string());
        }
    }

    // Custom wrappers: the last configured wrapper is the outermost process.
    if !steam_target {
        for wrapper in &settings.wrappers {
            if wrapper.cmd.is_empty() {
                continue;
            }
            if (environment.bin_exists)(&wrapper.cmd) {
                let mut next = vec![wrapper.cmd.clone()];
                next.extend(split_args(wrapper.args.as_deref().unwrap_or("")));
                next.extend(final_command);
                final_command = next;
            } else {
                warnings.push(format!(
                    "wrapper \"{}\" não encontrado — ignorado",
                    wrapper.cmd
                ));
            }
        }
    }

    // The launch-request env and the SLSsteam env are applied after game
    // settings, so they win over generated Wine variables.
    for (key, value) in &resolved.env_extra {
        env.insert(key.clone(), value.clone());
    }
    if steam_override_applied {
        if let Some(overridden) = &environment.steam_override {
            for (key, value) in &overridden.env {
                env.insert(key.clone(), value.clone());
            }
        }
    }
    warnings.extend(steam_override_warnings);

    if steam_env_target {
        env.remove("LD_LIBRARY_PATH");
        env.remove("LD_PRELOAD");
        env.remove("STEAM_RUNTIME_LIBRARY_PATH");
    }

    let game_id = resolved.game_id.clone().unwrap_or_default();
    if game_id.starts_with("custom:")
        && !env.get("WINEPREFIX").is_some_and(|value| !value.is_empty())
    {
        let prefix = settings
            .prefix_path
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| (environment.default_prefix)(&game_id));
        if !prefix.is_empty() {
            env.insert("WINEPREFIX".into(), prefix);
        }
    }

    let track_session = should_track_game_session(&final_command);
    let script_pre = settings
        .script_pre
        .clone()
        .filter(|value| !value.is_empty());
    let script_post = if track_session {
        settings
            .script_post
            .clone()
            .filter(|value| !value.is_empty())
    } else {
        None
    };

    let executable = final_command
        .first()
        .cloned()
        .ok_or_else(|| LaunchError(ERR_NO_COMMAND.to_string()))?;
    Ok(LaunchPlan {
        executable,
        args: final_command[1..].to_vec(),
        env,
        cwd: resolved.cwd.clone(),
        source: resolved.source,
        warnings,
        process_session,
        steam_wrapper,
        track_session,
        request_tracked: resolved.track_session,
        game_id,
        script_pre,
        script_post,
        graphics_libs,
    })
}

/// Pre-spawn binary validation from `validarBinariosLaunch`.
///
/// Only Unix-style absolute paths are checked, exactly like the Electron
/// client.  A missing wrapper is reported before any process is created.
pub fn validate_launch_binaries(
    command: &[String],
    fs_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let executable = command.first()?;
    if executable.is_empty() {
        return Some("cmd[0] vazio".to_string());
    }
    if executable.starts_with('/') && !fs_exists(executable) {
        return Some(format!("Binário não existe: {executable}"));
    }
    let game_executable = command.iter().find(|argument| {
        let lower = argument.to_ascii_lowercase();
        lower.ends_with(".exe")
            || lower.ends_with(".bat")
            || lower.ends_with(".msi")
            || (argument.starts_with('/') && argument.as_str() != executable)
    })?;
    if game_executable.starts_with('/') && !fs_exists(game_executable) {
        return Some(format!("Executável do jogo não existe: {game_executable}"));
    }
    None
}

fn is_env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first == b'_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

pub struct ExeLaunchRequest<'a> {
    pub platform: Platform,
    pub game_id: &'a str,
    pub exe: &'a str,
    pub linux: Option<bool>,
    pub settings: &'a GameSettings,
    pub protons: &'a [ProtonInstall],
    pub umu: Option<&'a str>,
    pub home: &'a str,
    pub default_prefix: &'a str,
    pub fs_exists: &'a dyn Fn(&str) -> bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExeLaunch {
    pub command: BuiltCommand,
    pub source: LaunchSource,
    pub graphics_libs: Option<GraphicsLibsRequest>,
}

/// Port of `exeLaunchCmd`: native on Windows/Linux, otherwise the selected
/// Steam Proton through UMU (preferred) or `proton run` (fallback).
pub fn build_exe_launch(request: ExeLaunchRequest<'_>) -> Option<ExeLaunch> {
    if request.exe.is_empty() {
        return None;
    }
    let platform = request.platform;
    let current_dir = std::env::current_dir()
        .ok()
        .and_then(|dir| dir.to_str().map(ToString::to_string))
        .unwrap_or_default();
    let executable = platform.resolve(&current_dir, request.exe);
    let cwd = platform.dirname(&executable);

    if platform.is_windows() {
        return Some(ExeLaunch {
            command: BuiltCommand {
                cmd: vec![executable],
                env: BTreeMap::new(),
                cwd,
            },
            source: LaunchSource::Native,
            graphics_libs: None,
        });
    }

    let linux = request
        .linux
        .unwrap_or_else(|| !request.exe.to_ascii_lowercase().ends_with(".exe"));
    if linux {
        return Some(ExeLaunch {
            command: BuiltCommand {
                cmd: vec![executable],
                env: BTreeMap::new(),
                cwd,
            },
            source: LaunchSource::Native,
            graphics_libs: None,
        });
    }

    let wine_version = request
        .settings
        .wine_version
        .as_deref()
        .filter(|value| !value.is_empty())?;
    let proton = request
        .protons
        .iter()
        .find(|proton| proton.id == wine_version)?;
    if proton.kind != "steam"
        || !(request.fs_exists)(&Platform::Linux.join(&[&proton.path, "proton"]))
    {
        return None;
    }

    let prefix = request
        .settings
        .prefix_path
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(request.default_prefix)
        .to_string();
    let graphics = (!prefix.is_empty()).then(|| GraphicsLibsRequest {
        prefix: Platform::Linux.join(&[&prefix, "pfx"]),
        wine: proton.wine.clone(),
        dxvk: request.settings.auto_dxvk(),
        nvapi: request.settings.auto_nvapi,
        vkd3d: request.settings.auto_vkd3d,
        layout: "pfx".into(),
    });

    if let Some(umu) = request.umu.filter(|value| !value.is_empty()) {
        let mut env = BTreeMap::new();
        env.insert("WINEPREFIX".into(), prefix);
        env.insert("GAMEID".into(), "umu-default".into());
        env.insert("PROTONPATH".into(), proton.path.clone());
        env.insert("STORE".into(), "none".into());
        env.insert("WINEARCH".into(), "win64".into());
        env.insert("PROTON_VERB".into(), "waitforexitandrun".into());
        return Some(ExeLaunch {
            command: BuiltCommand {
                cmd: vec![umu.to_string(), executable],
                env,
                cwd,
            },
            source: LaunchSource::Proton,
            graphics_libs: graphics,
        });
    }

    let mut env = BTreeMap::new();
    env.insert("STEAM_COMPAT_DATA_PATH".into(), prefix);
    env.insert(
        "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
        Platform::Linux.join(&[request.home, ".steam", "steam"]),
    );
    env.insert("STEAM_COMPAT_APP_ID".into(), "0".into());
    env.insert(
        "WINEPREFIX".into(),
        request
            .settings
            .prefix_path
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(request.default_prefix)
            .to_string(),
    );
    Some(ExeLaunch {
        command: BuiltCommand {
            cmd: vec![
                Platform::Linux.join(&[&proton.path, "proton"]),
                "run".into(),
                executable,
            ],
            env,
            cwd,
        },
        source: LaunchSource::Proton,
        graphics_libs: graphics,
    })
}

/// Port of `customLaunchCmd`: `platform === "linux"` selects native Linux.
pub fn build_custom_launch(request: ExeLaunchRequest<'_>, game: &GameEntry) -> Option<ExeLaunch> {
    let exe = game.exe.as_deref()?;
    build_exe_launch(ExeLaunchRequest {
        exe,
        linux: Some(game.platform.as_deref() == Some("linux")),
        ..request
    })
}

/// Convenience path: resolve the request, then plan it with the same
/// dependency bag's settings.
pub fn plan_from_request(
    payload: &LaunchPayload,
    deps: &dyn LaunchResolverDeps,
    environment: &PlanEnvironment<'_>,
) -> Result<LaunchPlan, LaunchError> {
    let resolved = resolve_launch_request(payload, deps)?;
    let settings = resolved
        .game_id
        .as_deref()
        .map(|game_id| deps.get_game_settings(game_id))
        .unwrap_or_default();
    plan_launch(&resolved, &settings, environment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps() -> TestDeps {
        TestDeps::default()
    }

    #[derive(Default)]
    struct TestDeps {
        game: Option<GameEntry>,
        settings: GameSettings,
        custom: Option<BuiltCommand>,
        exe: Option<BuiltCommand>,
        emulator: Option<EmulatorLaunchResult>,
    }

    impl LaunchResolverDeps for TestDeps {
        fn find_game(&self, _id: &str) -> Option<GameEntry> {
            self.game.clone()
        }

        fn custom_launch_cmd(&self, _id: &str) -> Option<BuiltCommand> {
            self.custom.clone()
        }

        fn get_game_settings(&self, _id: &str) -> GameSettings {
            self.settings.clone()
        }

        fn exe_launch_cmd(&self, _id: &str, _exe: &str) -> Option<BuiltCommand> {
            self.exe.clone()
        }

        fn emulator_launch(
            &self,
            _id: &str,
            _game: &GameEntry,
            _settings: &GameSettings,
        ) -> Option<EmulatorLaunchResult> {
            self.emulator.clone()
        }
    }

    fn steam_deps() -> TestDeps {
        TestDeps {
            game: Some(GameEntry {
                id: "steam:440".into(),
                launch_cmd: vec!["steam".into(), "steam://rungameid/440".into()],
                launcher: "steam".into(),
                ..GameEntry::default()
            }),
            settings: GameSettings::default(),
            ..TestDeps::default()
        }
    }

    #[test]
    fn game_id_always_wins_over_renderer_cmd() {
        let result = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:440".into()),
                cmd: Some(vec!["rm".into(), "-rf".into(), "/".into()]),
                mode: None,
            }),
            &steam_deps(),
        )
        .unwrap();
        assert_eq!(result.raw_cmd, vec!["steam", "steam://rungameid/440"]);
        assert_eq!(result.source, LaunchSource::Steam);
    }

    #[test]
    fn unknown_game_and_empty_command_are_rejected() {
        let mut deps = steam_deps();
        deps.game = None;
        let error = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:999".into()),
                cmd: Some(vec!["evil".into()]),
                mode: None,
            }),
            &deps,
        )
        .unwrap_err();
        assert_eq!(error.0, ERR_GAME_NOT_FOUND);

        let mut deps = steam_deps();
        deps.game = Some(GameEntry {
            id: "steam:999".into(),
            launch_cmd: vec![],
            ..GameEntry::default()
        });
        let error = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:999".into()),
                cmd: Some(vec!["evil".into()]),
                mode: None,
            }),
            &deps,
        )
        .unwrap_err();
        assert!(error.0.contains("Sem comando"));
    }

    #[test]
    fn legacy_accepts_only_numeric_steam_store_uris() {
        for command in [
            vec!["steam".into(), "steam://run/440".into()],
            vec!["steam".into(), "steam://install/440".into()],
        ] {
            assert!(resolve_launch_request(&LaunchPayload::Command(command), &deps()).is_ok());
        }
        for command in [
            vec!["bash".into(), "-c".into(), "id".into()],
            vec!["steam".into(), "steam://run/abc".into()],
            vec!["steam".into(), "STEAM://RUN/440".into()],
        ] {
            let error =
                resolve_launch_request(&LaunchPayload::Command(command), &deps()).unwrap_err();
            assert_eq!(error.0, ERR_LEGACY_REJECTED);
        }
    }

    #[test]
    fn custom_and_exe_use_controlled_callbacks() {
        let deps = TestDeps {
            custom: Some(BuiltCommand {
                cmd: vec!["wine".into(), "/games/custom.exe".into()],
                env: BTreeMap::from([("WINEPREFIX".into(), "/tmp/pfx".into())]),
                cwd: String::new(),
            }),
            ..TestDeps::default()
        };
        let custom = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("custom:one".into()),
                cmd: Some(vec!["evil".into()]),
                mode: None,
            }),
            &deps,
        )
        .unwrap();
        assert_eq!(custom.raw_cmd, vec!["wine", "/games/custom.exe"]);
        assert_eq!(
            custom.env_extra.get("WINEPREFIX").map(String::as_str),
            Some("/tmp/pfx")
        );
        assert_eq!(custom.source, LaunchSource::Wine);

        let deps = TestDeps {
            exe: Some(BuiltCommand {
                cmd: vec!["wine".into(), "/games/override.exe".into()],
                env: BTreeMap::from([("WINEPREFIX".into(), "/tmp/pfx".into())]),
                cwd: String::new(),
            }),
            game: steam_deps().game,
            settings: GameSettings {
                exe_path: Some("/games/override.exe".into()),
                ..GameSettings::default()
            },
            ..TestDeps::default()
        };
        let exe = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:440".into()),
                mode: Some("exe".into()),
                ..LaunchRequest::default()
            }),
            &deps,
        )
        .unwrap();
        assert_eq!(exe.raw_cmd, vec!["wine", "/games/override.exe"]);
        let steam = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:440".into()),
                mode: Some("steam".into()),
                ..LaunchRequest::default()
            }),
            &deps,
        )
        .unwrap();
        assert_eq!(steam.raw_cmd, vec!["steam", "steam://rungameid/440"]);
    }

    #[test]
    fn emulator_registry_builds_argv_without_renderer_cmd() {
        let deps = TestDeps {
            game: Some(GameEntry {
                id: "custom:emu".into(),
                launch_cmd: vec!["placeholder".into()],
                launcher: "emulator".into(),
                ..GameEntry::default()
            }),
            settings: GameSettings {
                emulator_id: Some("pcsx2".into()),
                rom_path: Some("/games/demo.iso".into()),
                ..GameSettings::default()
            },
            emulator: Some(EmulatorLaunchResult::Command(BuiltCommand::new(vec![
                "pcsx2-qt",
                "/games/demo.iso",
            ]))),
            ..TestDeps::default()
        };
        let result = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("custom:emu".into()),
                ..LaunchRequest::default()
            }),
            &deps,
        )
        .unwrap();
        assert_eq!(result.raw_cmd, vec!["pcsx2-qt", "/games/demo.iso"]);
        assert_eq!(result.source, LaunchSource::Emulator);

        let normal = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("emu:pcsx2:demo".into()),
                cmd: Some(vec!["evil".into()]),
                ..LaunchRequest::default()
            }),
            &TestDeps {
                game: Some(GameEntry {
                    id: "emu:pcsx2:demo".into(),
                    launch_cmd: vec!["placeholder".into()],
                    launcher: "emulator".into(),
                    ..GameEntry::default()
                }),
                settings: GameSettings {
                    emulator_id: Some("pcsx2".into()),
                    rom_path: Some("/games/demo.iso".into()),
                    ..GameSettings::default()
                },
                emulator: Some(EmulatorLaunchResult::Command(BuiltCommand::new(vec![
                    "pcsx2-qt",
                    "/games/demo.iso",
                ]))),
                ..TestDeps::default()
            },
        )
        .unwrap();
        assert_eq!(normal.raw_cmd, vec!["pcsx2-qt", "/games/demo.iso"]);
    }

    #[test]
    fn emulator_profile_for_missing_custom_game_is_rejected() {
        let deps = TestDeps {
            game: None,
            settings: GameSettings {
                emulator_id: Some("pcsx2".into()),
                rom_path: Some("/games/demo.iso".into()),
                ..GameSettings::default()
            },
            emulator: Some(EmulatorLaunchResult::Command(BuiltCommand::new(vec![
                "pcsx2-qt",
                "/games/demo.iso",
            ]))),
            ..TestDeps::default()
        };
        let result = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("custom:missing".into()),
                ..LaunchRequest::default()
            }),
            &deps,
        );
        assert!(result.is_err());
    }

    #[test]
    fn emulator_failure_stops_before_legacy_command() {
        let deps = TestDeps {
            game: Some(GameEntry {
                id: "emu:rpcs3:demo".into(),
                launch_cmd: vec!["placeholder".into()],
                ..GameEntry::default()
            }),
            settings: GameSettings {
                emulator_id: Some("rpcs3".into()),
                rom_path: Some("/missing/game.rap".into()),
                ..GameSettings::default()
            },
            emulator: Some(EmulatorLaunchResult::Failed {
                error: "rom_invalida".into(),
            }),
            ..TestDeps::default()
        };
        let error = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("emu:rpcs3:demo".into()),
                ..LaunchRequest::default()
            }),
            &deps,
        )
        .unwrap_err();
        assert_eq!(error.0, "rom_invalida");
    }

    #[test]
    fn steam_plan_is_silent_and_untouched_by_wrappers() {
        let resolved = resolve_launch_request(
            &LaunchPayload::Request(LaunchRequest {
                game_id: Some("steam:440".into()),
                ..LaunchRequest::default()
            }),
            &steam_deps(),
        )
        .unwrap();
        let environment = PlanEnvironment {
            platform: Platform::Windows,
            bin_exists: Box::new(|_| true),
            ..PlanEnvironment::new(Platform::Windows)
        };
        let settings = GameSettings {
            gamescope: true,
            gamemode: true,
            mangohud: true,
            game_args: Some("--ignored".into()),
            wrappers: vec![Wrapper {
                cmd: "wrapper".into(),
                args: None,
            }],
            ..GameSettings::default()
        };
        let plan = plan_launch(&resolved, &settings, &environment).unwrap();
        assert_eq!(
            plan.command(),
            vec!["steam", "-silent", "steam://rungameid/440"]
        );
        assert!(plan.steam_wrapper);
        assert!(plan.track_session);
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn windows_native_plan_preserves_argv_and_cwd() {
        let resolved = ResolvedLaunch {
            game_id: Some("custom:one".into()),
            mode: None,
            raw_cmd: vec!["C:\\Games\\Demo\\demo.exe".into()],
            env_extra: BTreeMap::new(),
            cwd: "C:\\Games\\Demo".into(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let settings = GameSettings {
            esync: true,
            fsync: true,
            game_args: Some("--fullscreen \"two words\"".into()),
            ..GameSettings::default()
        };
        let plan = plan_launch(
            &resolved,
            &settings,
            &PlanEnvironment::new(Platform::Windows),
        )
        .unwrap();
        assert_eq!(
            plan.command(),
            vec!["C:\\Games\\Demo\\demo.exe", "--fullscreen", "two words"]
        );
        assert_eq!(plan.cwd, "C:\\Games\\Demo");
        assert!(!plan.env.contains_key("WINEESYNC"));
    }

    fn proton_environment<'a>() -> PlanEnvironment<'a> {
        PlanEnvironment {
            platform: Platform::Linux,
            base_env: BTreeMap::from([
                ("PATH".to_string(), "/usr/bin".to_string()),
                ("HOME".to_string(), "/home/z".to_string()),
                ("LD_PRELOAD".to_string(), "/opt/electron/lib.so".to_string()),
            ]),
            protons: vec![ProtonInstall {
                id: "steam:Proton 9.0".into(),
                name: "Proton 9.0 (Steam)".into(),
                path: "/steam/steamapps/common/Proton 9.0".into(),
                wine: "/steam/steamapps/common/Proton 9.0/files/bin/wine".into(),
                kind: "steam".into(),
            }],
            default_prefix: Box::new(|game_id| format!("/prefixes/{game_id}")),
            bin_exists: Box::new(|cmd| {
                matches!(cmd, "gamescope" | "gamemoderun" | "mangohud" | "wrapper-a")
            }),
            fs_exists: Box::new(|_| true),
            systemd: Some(SystemdSession {
                unit: "arcadia-game-1234-9.service".into(),
                keep_alive: true,
            }),
            steam_override: None,
        }
    }

    #[test]
    fn proton_gamescope_gamemode_mangohud_and_wrappers_plan() {
        let settings = GameSettings {
            wine_version: Some("steam:Proton 9.0".into()),
            prefix_path: Some("/prefixes/steam_440".into()),
            esync: true,
            fsync: true,
            auto_nvapi: true,
            auto_vkd3d: true,
            verbose_logs: true,
            gs_width: Some(2560),
            gs_height: Some(1600),
            gs_fps: Some(60),
            gs_hdr: true,
            gs_window_mode: Some("borderless".into()),
            gs_framerate_limit: Some(45),
            gamescope: true,
            gamemode: true,
            mangohud: true,
            game_args: Some("--foo \"bar baz\"".into()),
            wrappers: vec![Wrapper {
                cmd: "wrapper-a".into(),
                args: Some("--wrap x".into()),
            }],
            ..GameSettings::default()
        };
        let exe = build_exe_launch(ExeLaunchRequest {
            platform: Platform::Linux,
            game_id: "steam:440",
            exe: "/games/Demo/demo.exe",
            linux: Some(false),
            settings: &settings,
            protons: &proton_environment().protons,
            umu: Some("/data/bin/umu/1.4.4/umu-run"),
            home: "/home/z",
            default_prefix: "/prefixes/steam_440",
            fs_exists: &|_| true,
        })
        .unwrap();
        assert_eq!(
            exe.command.cmd,
            vec!["/data/bin/umu/1.4.4/umu-run", "/games/Demo/demo.exe"]
        );
        assert_eq!(
            exe.command.env.get("PROTONPATH").map(String::as_str),
            Some("/steam/steamapps/common/Proton 9.0")
        );
        assert_eq!(
            exe.command.env.get("PROTON_VERB").map(String::as_str),
            Some("waitforexitandrun")
        );
        let environment = proton_environment();
        let resolved = ResolvedLaunch {
            game_id: Some("steam:440".into()),
            mode: None,
            raw_cmd: exe.command.cmd.clone(),
            env_extra: exe.command.env.clone(),
            cwd: exe.command.cwd.clone(),
            source: LaunchSource::Proton,
            track_session: true,
        };
        let plan = plan_launch(&resolved, &settings, &environment).unwrap();
        let expected_prefix = vec![
            "wrapper-a".to_string(),
            "--wrap".to_string(),
            "x".to_string(),
            "mangohud".to_string(),
            "gamemoderun".to_string(),
            "gamescope".to_string(),
            "-W".to_string(),
            "2560".to_string(),
            "-H".to_string(),
            "1600".to_string(),
            "-r".to_string(),
            "60".to_string(),
            "--framerate-limit".to_string(),
            "45".to_string(),
            "--hdr-enabled".to_string(),
            "-b".to_string(),
            "--keep-alive".to_string(),
            "--".to_string(),
            "systemd-run".to_string(),
            "--user".to_string(),
        ];
        // Every environment key becomes --setenv=KEY, sorted because the
        // environment is a BTreeMap.  Assert the meaningful subset.
        assert!(plan.executable == expected_prefix[0]);
        assert_eq!(
            &plan.args[..expected_prefix.len() - 1],
            &expected_prefix[1..]
        );
        let command = plan.command();
        assert!(command.contains(&"--setenv=WINEPREFIX".to_string()));
        assert!(command.contains(&"--setenv=DISPLAY".to_string()));
        assert_eq!(
            plan.env.get("PROTONPATH").map(String::as_str),
            Some("/steam/steamapps/common/Proton 9.0")
        );
        assert_eq!(plan.source, LaunchSource::Proton);
        assert_eq!(
            plan.process_session,
            Some(ProcessSession {
                session_type: "systemd".into(),
                unit: "arcadia-game-1234-9.service".into(),
                cgroup_root: "/sys/fs/cgroup".into(),
            })
        );
        assert_eq!(plan.env.get("WINEESYNC").map(String::as_str), Some("1"));
        assert_eq!(plan.env.get("WINEFSYNC").map(String::as_str), Some("1"));
        assert_eq!(
            plan.env.get("DXVK_ENABLE_NVAPI").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            plan.env.get("ENABLE_GAMESCOPE_WSI").map(String::as_str),
            Some("1")
        );
        assert_eq!(plan.env.get("DXVK_HUD").map(String::as_str), Some("full"));
        assert_eq!(
            plan.env.get("WINEDEBUG").map(String::as_str),
            Some("+timestamp,+pid,+tid,+seh,+warn")
        );
        assert_eq!(
            plan.env.get("LD_PRELOAD").map(String::as_str),
            Some("/opt/electron/lib.so")
        );
        assert_eq!(
            plan.env.get("WINEPREFIX").map(String::as_str),
            Some("/prefixes/steam_440")
        );
    }

    #[test]
    fn verbose_logging_respects_existing_winedebug_and_dxvk_hud() {
        let resolved = ResolvedLaunch {
            game_id: None,
            mode: None,
            raw_cmd: vec!["/games/native".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let mut environment = PlanEnvironment::new(Platform::Linux);
        environment
            .base_env
            .insert("WINEDEBUG".into(), "+custom".into());
        let settings = GameSettings {
            verbose_logs: true,
            dxvk_hud: Some("fps".into()),
            ..GameSettings::default()
        };
        let plan = plan_launch(&resolved, &settings, &environment).unwrap();
        assert_eq!(
            plan.env.get("WINEDEBUG").map(String::as_str),
            Some("+custom")
        );
        assert_eq!(plan.env.get("DXVK_HUD").map(String::as_str), Some("fps"));
    }

    #[test]
    fn env_vars_are_validated_and_launch_env_wins() {
        let resolved = ResolvedLaunch {
            game_id: None,
            mode: None,
            raw_cmd: vec!["/games/native".into()],
            env_extra: BTreeMap::from([
                ("WINEPREFIX".to_string(), "/extra/prefix".to_string()),
                ("ONLY_EXTRA".to_string(), "1".to_string()),
            ]),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let settings = GameSettings {
            env_vars: vec![
                EnvVar {
                    name: "GOOD_NAME".into(),
                    value: Some("value".into()),
                },
                EnvVar {
                    name: "BAD-NAME".into(),
                    value: Some("ignored".into()),
                },
                EnvVar {
                    name: "EMPTY".into(),
                    value: None,
                },
            ],
            ..GameSettings::default()
        };
        let plan =
            plan_launch(&resolved, &settings, &PlanEnvironment::new(Platform::Linux)).unwrap();
        assert_eq!(plan.env.get("GOOD_NAME").map(String::as_str), Some("value"));
        assert!(!plan.env.contains_key("BAD-NAME"));
        assert_eq!(plan.env.get("EMPTY").map(String::as_str), Some(""));
        assert_eq!(
            plan.env.get("WINEPREFIX").map(String::as_str),
            Some("/extra/prefix")
        );
        assert_eq!(plan.env.get("ONLY_EXTRA").map(String::as_str), Some("1"));
    }

    #[test]
    fn steam_override_applies_before_ld_cleanup() {
        let resolved = ResolvedLaunch {
            game_id: Some("steam:440".into()),
            mode: None,
            raw_cmd: vec!["steam".into(), "steam://rungameid/440".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Steam,
            track_session: true,
        };
        let mut environment = PlanEnvironment::new(Platform::Linux);
        environment
            .base_env
            .insert("LD_PRELOAD".into(), "/electron/lib.so".into());
        environment.steam_override = Some(SteamOverride {
            command: "/opt/slssteam/steam".into(),
            env: BTreeMap::from([("LD_AUDIT".into(), "/sls.so".into())]),
            warnings: vec!["SLS warning".into()],
        });
        let plan = plan_launch(&resolved, &GameSettings::default(), &environment).unwrap();
        assert_eq!(plan.executable, "/opt/slssteam/steam");
        assert_eq!(plan.args, vec!["-silent", "steam://rungameid/440"]);
        assert!(!plan.env.contains_key("LD_PRELOAD"));
        assert_eq!(
            plan.env.get("LD_AUDIT").map(String::as_str),
            Some("/sls.so")
        );
        assert_eq!(plan.warnings, vec!["SLS warning"]);
        assert!(plan.steam_wrapper);
    }

    #[test]
    fn legendary_gets_dedicated_prefix_and_wine_override() {
        let resolved = ResolvedLaunch {
            game_id: Some("epic:demo".into()),
            mode: None,
            raw_cmd: vec!["legendary".into(), "launch".into(), "demo".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let settings = GameSettings {
            wine_version: Some("steam:Proton 9.0".into()),
            ..GameSettings::default()
        };
        let mut environment = PlanEnvironment::new(Platform::Linux);
        environment.protons = proton_environment().protons;
        environment.default_prefix = Box::new(|game_id| format!("/prefixes/{game_id}"));
        environment.fs_exists =
            Box::new(|path| path.contains("system32") || path.ends_with("wine"));
        let plan = plan_launch(&resolved, &settings, &environment).unwrap();
        assert_eq!(
            plan.command(),
            vec![
                "legendary",
                "launch",
                "demo",
                "--wine-prefix",
                "/prefixes/epic:demo",
                "--wine",
                "/steam/steamapps/common/Proton 9.0/files/bin/wine",
            ]
        );
        assert_eq!(
            plan.env.get("WINEPREFIX").map(String::as_str),
            Some("/prefixes/epic:demo")
        );
        assert!(plan.graphics_libs.is_some());
    }

    #[test]
    fn missing_wrappers_are_warnings_not_failures() {
        let resolved = ResolvedLaunch {
            game_id: None,
            mode: None,
            raw_cmd: vec!["/games/native".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let settings = GameSettings {
            gamescope: true,
            gamemode: true,
            mangohud: true,
            wrappers: vec![Wrapper {
                cmd: "missing-wrapper".into(),
                args: None,
            }],
            ..GameSettings::default()
        };
        let plan =
            plan_launch(&resolved, &settings, &PlanEnvironment::new(Platform::Linux)).unwrap();
        assert_eq!(plan.command(), vec!["/games/native"]);
        assert_eq!(plan.env.get("MANGOHUD").map(String::as_str), Some("1"));
        assert_eq!(plan.warnings.len(), 4);
        assert!(plan.warnings[0].contains("gamescope"));
        assert!(plan.warnings[3].contains("missing-wrapper"));
    }

    #[test]
    fn gamescope_plan_without_systemd_is_exact() {
        let resolved = ResolvedLaunch {
            game_id: None,
            mode: None,
            raw_cmd: vec!["/games/native".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: true,
        };
        let settings = GameSettings {
            gamescope: true,
            ..GameSettings::default()
        };
        let mut environment = PlanEnvironment::new(Platform::Linux);
        environment.bin_exists = Box::new(|cmd| cmd == "gamescope");
        let plan = plan_launch(&resolved, &settings, &environment).unwrap();
        assert_eq!(
            plan.command(),
            vec![
                "gamescope",
                "-W",
                "1920",
                "-H",
                "1080",
                "-f",
                "--",
                "/games/native"
            ]
        );
        assert_eq!(plan.process_session, None);
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains("systemd"));
    }

    #[test]
    fn custom_game_always_gets_a_prefix() {
        let resolved = ResolvedLaunch {
            game_id: Some("custom:demo".into()),
            mode: None,
            raw_cmd: vec!["/games/native".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Custom,
            track_session: true,
        };
        let mut environment = PlanEnvironment::new(Platform::Linux);
        environment.default_prefix = Box::new(|game_id| format!("/prefixes/{game_id}"));
        let plan = plan_launch(&resolved, &GameSettings::default(), &environment).unwrap();
        assert_eq!(
            plan.env.get("WINEPREFIX").map(String::as_str),
            Some("/prefixes/custom:demo")
        );
        assert_eq!(plan.source, LaunchSource::Custom);
    }

    #[test]
    fn steam_install_uri_does_not_track_a_game_session() {
        let resolved = resolve_launch_request(
            &LaunchPayload::Command(vec!["steam".into(), "steam://install/440".into()]),
            &deps(),
        )
        .unwrap();
        assert!(!resolved.track_session);
        let plan = plan_launch(
            &resolved,
            &GameSettings::default(),
            &PlanEnvironment::new(Platform::Windows),
        )
        .unwrap();
        assert!(!plan.track_session);
        assert_eq!(plan.script_post, None);
    }

    #[test]
    fn split_args_matches_the_client_tokenizer() {
        assert_eq!(split_args("a \"b c\" d"), vec!["a", "b c", "d"]);
        assert_eq!(split_args("'one two' three"), vec!["one two", "three"]);
        assert_eq!(split_args("--x=\"a b\""), vec!["--x=\"a", "b"]);
    }

    #[test]
    fn steam_launch_forms_are_detected() {
        assert_eq!(
            steam_game_id(&["steam".into(), "steam://rungameid/440".into()]),
            Some("440".into())
        );
        assert_eq!(
            steam_game_id(&["steam".into(), "-applaunch".into(), "440".into()]),
            Some("440".into())
        );
        assert!(steam_launch_kind(&["not-steam".into(), "440".into()]).is_none());
    }

    #[test]
    fn proton_fallback_without_umu_uses_compat_data() {
        let settings = GameSettings {
            wine_version: Some("steam:Proton 9.0".into()),
            ..GameSettings::default()
        };
        let protons = proton_environment().protons;
        let launch = build_exe_launch(ExeLaunchRequest {
            platform: Platform::Linux,
            game_id: "steam:440",
            exe: "/games/demo.exe",
            linux: Some(false),
            settings: &settings,
            protons: &protons,
            umu: None,
            home: "/home/z",
            default_prefix: "/prefixes/steam_440",
            fs_exists: &|_| true,
        })
        .unwrap();
        assert_eq!(
            launch.command.cmd,
            vec![
                "/steam/steamapps/common/Proton 9.0/proton",
                "run",
                "/games/demo.exe"
            ]
        );
        assert_eq!(
            launch
                .command
                .env
                .get("STEAM_COMPAT_DATA_PATH")
                .map(String::as_str),
            Some("/prefixes/steam_440")
        );
        assert_eq!(
            launch
                .command
                .env
                .get("STEAM_COMPAT_CLIENT_INSTALL_PATH")
                .map(String::as_str),
            Some("/home/z/.steam/steam")
        );
        assert_eq!(
            launch
                .command
                .env
                .get("STEAM_COMPAT_APP_ID")
                .map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn custom_launch_uses_game_platform_flag() {
        let game = GameEntry {
            id: "custom:linux".into(),
            exe: Some("/games/native".into()),
            platform: Some("linux".into()),
            ..GameEntry::default()
        };
        let settings = GameSettings {
            wine_version: Some("steam:Proton 9.0".into()),
            ..GameSettings::default()
        };
        let launch = build_custom_launch(
            ExeLaunchRequest {
                platform: Platform::Linux,
                game_id: "custom:linux",
                exe: "",
                linux: None,
                settings: &settings,
                protons: &[],
                umu: None,
                home: "/home/z",
                default_prefix: "/prefixes/custom:linux",
                fs_exists: &|_| true,
            },
            &game,
        )
        .unwrap();
        assert_eq!(launch.command.cmd, vec!["/games/native"]);
        assert!(launch.command.env.is_empty());
    }

    #[test]
    fn missing_launch_binaries_are_rejected_before_spawn() {
        let exists = |path: &str| path != "/missing/game.exe" && path != "/missing/wrapper";
        assert_eq!(
            validate_launch_binaries(
                &["/usr/bin/steam".into(), "steam://rungameid/1".into()],
                &exists
            ),
            None
        );
        assert_eq!(
            validate_launch_binaries(
                &["/missing/wrapper".into(), "/games/demo.exe".into()],
                &exists
            )
            .as_deref(),
            Some("Binário não existe: /missing/wrapper")
        );
        assert_eq!(
            validate_launch_binaries(
                &["/usr/bin/wine".into(), "/missing/game.exe".into()],
                &exists
            )
            .as_deref(),
            Some("Executável do jogo não existe: /missing/game.exe")
        );
        assert_eq!(
            validate_launch_binaries(&["".into()], &exists).as_deref(),
            Some("cmd[0] vazio")
        );
    }

    #[test]
    fn settings_deserialize_with_the_renderer_camel_case_fields() {
        let settings: GameSettings = serde_json::from_value(serde_json::json!({
            "exePath": "/games/demo.exe",
            "wineVersion": "steam:Proton 9.0",
            "autoDXVK": false,
            "autoNVAPI": true,
            "autoVKD3D": true,
            "gsHdr": true,
            "gsWidth": 2560,
            "gsWindowMode": "borderless",
            "dxvkHud": "fps",
            "envVars": [{"name": "FOO", "value": "bar"}],
            "wrappers": [{"cmd": "gamescope", "args": "--"}]
        }))
        .unwrap();
        assert_eq!(settings.exe_path.as_deref(), Some("/games/demo.exe"));
        assert_eq!(settings.auto_dxvk, Some(false));
        assert!(settings.auto_nvapi);
        assert!(settings.auto_vkd3d);
        assert!(settings.gs_hdr);
        assert_eq!(settings.gs_width, Some(2560));
        assert_eq!(settings.gs_window_mode.as_deref(), Some("borderless"));
        assert_eq!(settings.dxvk_hud.as_deref(), Some("fps"));
        assert_eq!(settings.env_vars[0].name, "FOO");
        assert_eq!(settings.wrappers[0].cmd, "gamescope");
    }

    #[test]
    fn locale_independent_json_payload_parsing() {
        let payload = LaunchPayload::from_json(&serde_json::json!({
            "gameId": "steam:440",
            "cmd": ["evil"],
            "mode": "steam"
        }));
        match payload {
            LaunchPayload::Request(request) => {
                assert_eq!(request.game_id.as_deref(), Some("steam:440"));
                assert_eq!(request.mode.as_deref(), Some("steam"));
            }
            LaunchPayload::Command(_) => panic!("expected request"),
        }
        let legacy = LaunchPayload::from_json(&serde_json::json!(["steam", 10, null]));
        assert_eq!(
            legacy,
            LaunchPayload::Command(vec!["steam".into(), "10".into(), "null".into()])
        );
        // A truthy non-string gameId makes the client ignore `cmd` entirely,
        // ending in the no-command rejection rather than the legacy path.
        let ignored = LaunchPayload::from_json(&serde_json::json!({
            "gameId": 10,
            "cmd": ["steam", "steam://run/440"]
        }));
        match ignored {
            LaunchPayload::Request(request) => assert!(request.cmd.is_none()),
            LaunchPayload::Command(_) => panic!("expected request"),
        }
    }

    #[test]
    fn steam_applaunch_is_a_steam_plan_without_rewriting() {
        let resolved = resolve_launch_request(
            &LaunchPayload::Command(vec!["steam".into(), "-applaunch".into(), "440".into()]),
            &deps(),
        );
        // Legacy renderer commands only allow the two store URIs.
        assert!(resolved.is_err());

        let resolved = ResolvedLaunch {
            game_id: Some("steam:440".into()),
            mode: None,
            raw_cmd: vec!["steam".into(), "-applaunch".into(), "440".into()],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Steam,
            track_session: true,
        };
        let plan = plan_launch(
            &resolved,
            &GameSettings::default(),
            &PlanEnvironment::new(Platform::Linux),
        )
        .unwrap();
        assert_eq!(plan.command(), vec!["steam", "-applaunch", "440"]);
        assert!(!plan.steam_wrapper);
        assert_eq!(plan.source, LaunchSource::Steam);
    }

    #[test]
    fn empty_plan_command_is_rejected() {
        let resolved = ResolvedLaunch {
            game_id: None,
            mode: None,
            raw_cmd: vec![],
            env_extra: BTreeMap::new(),
            cwd: String::new(),
            source: LaunchSource::Native,
            track_session: false,
        };
        assert_eq!(
            plan_launch(
                &resolved,
                &GameSettings::default(),
                &PlanEnvironment::new(Platform::Linux)
            )
            .unwrap_err()
            .0,
            ERR_NO_COMMAND
        );
    }
}
