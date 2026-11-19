//! Gamescope command construction and systemd user-session helpers.
//!
//! Port of `app/electron/gamescope-session.js`.  Everything here is pure: the
//! caller supplies binary existence, filesystem reads and capability probes.
//! Nothing in this module starts a process.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::platform::Platform;

pub const MAX_DIMENSION: i64 = 16_384;
pub const MAX_REFRESH: i64 = 1_000;
pub const MAX_FRAMERATE_LIMIT: i64 = 1_000;

/// Metadata kept alongside the argv; never part of the command line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSession {
    #[serde(rename = "type")]
    pub session_type: String,
    pub unit: String,
    #[serde(rename = "cgroupRoot")]
    pub cgroup_root: String,
}

/// A transient systemd unit selected for an external Gamescope launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemdSession {
    pub unit: String,
    pub keep_alive: bool,
}

/// Parsed Gamescope settings.
///
/// The Electron builder accepts JSON-shaped legacy values (numeric strings,
/// wrong types) and sanitises them without ever interpolating arbitrary text
/// into argv.  [`GamescopeSettings::from_json`] preserves that contract;
/// constructing the struct directly is the typed fast path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamescopeSettings {
    pub width: i64,
    pub height: i64,
    /// Legacy `fps` maps to the nested refresh flag `-r`.
    pub fps: i64,
    pub hdr: bool,
    pub window_mode: String,
    pub framerate_limit: i64,
    pub keep_alive: bool,
    pub systemd_unit: String,
    pub environment_keys: Vec<String>,
}

impl Default for GamescopeSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 0,
            hdr: false,
            window_mode: "windowed".to_string(),
            framerate_limit: 0,
            keep_alive: false,
            systemd_unit: String::new(),
            environment_keys: Vec::new(),
        }
    }
}

impl GamescopeSettings {
    /// Reproduce the JavaScript builder's sanitisation of a JSON object.
    pub fn from_json(value: &Value) -> Self {
        let width = safe_integer(
            value.get("width").unwrap_or(&Value::Null),
            1,
            MAX_DIMENSION,
            1920,
        );
        let height = safe_integer(
            value.get("height").unwrap_or(&Value::Null),
            1,
            MAX_DIMENSION,
            1080,
        );
        let fps = safe_integer(value.get("fps").unwrap_or(&Value::Null), 1, MAX_REFRESH, 0);
        let framerate_limit = safe_integer(
            value.get("framerateLimit").unwrap_or(&Value::Null),
            1,
            MAX_FRAMERATE_LIMIT,
            0,
        );
        let window_mode = value
            .get("windowMode")
            .and_then(Value::as_str)
            .map(valid_window_mode)
            .unwrap_or_else(|| "windowed".to_string());
        let systemd_unit = value
            .get("systemdUnit")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let environment_keys = value
            .get("environmentKeys")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            width,
            height,
            fps,
            hdr: value.get("hdr") == Some(&Value::Bool(true)),
            window_mode,
            framerate_limit,
            keep_alive: value.get("keepAlive") == Some(&Value::Bool(true)),
            systemd_unit,
            environment_keys,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamescopeLaunch {
    pub cmd: Vec<String>,
    pub process_session: Option<ProcessSession>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GamescopeError {
    EmptyCommand,
    InvalidSystemdUnit,
}

impl std::fmt::Display for GamescopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GamescopeError::EmptyCommand => write!(f, "comando Gamescope vazio"),
            GamescopeError::InvalidSystemdUnit => write!(f, "unidade systemd inválida"),
        }
    }
}

impl std::error::Error for GamescopeError {}

/// Build `gamescope … -- <command>` (optionally inside a transient systemd
/// service).  Argument-array semantics are unchanged: no shell string is ever
/// produced.
pub fn build_external_gamescope_command(
    command: &[String],
    settings: &GamescopeSettings,
) -> Result<GamescopeLaunch, GamescopeError> {
    if command.is_empty() {
        return Err(GamescopeError::EmptyCommand);
    }

    let output_width = safe_integer_value(settings.width, 1, MAX_DIMENSION, 1920);
    let output_height = safe_integer_value(settings.height, 1, MAX_DIMENSION, 1080);
    let nested_refresh = safe_integer_value(settings.fps, 1, MAX_REFRESH, 0);
    let limit = safe_integer_value(settings.framerate_limit, 1, MAX_FRAMERATE_LIMIT, 0);

    let mut args = vec![
        "-W".to_string(),
        output_width.to_string(),
        "-H".to_string(),
        output_height.to_string(),
    ];
    if nested_refresh > 0 {
        args.push("-r".to_string());
        args.push(nested_refresh.to_string());
    }
    if limit > 0 {
        args.push("--framerate-limit".to_string());
        args.push(limit.to_string());
    }
    // Only the literal boolean enables HDR; a stale string such as "false"
    // must not switch a display into HDR mode.
    if settings.hdr {
        args.push("--hdr-enabled".to_string());
    }
    let mode = valid_window_mode(&settings.window_mode);
    if mode == "fullscreen" {
        args.push("-f".to_string());
    } else if mode == "borderless" {
        args.push("-b".to_string());
    }
    // Deliberately no persistent pointer lock: Gamescope 3.16.25 can lose it
    // when the host compositor changes focus.
    if settings.keep_alive {
        args.push("--keep-alive".to_string());
    }

    let mut primary = command.to_vec();
    if !settings.systemd_unit.is_empty() {
        if !is_systemd_unit_name(&settings.systemd_unit) {
            return Err(GamescopeError::InvalidSystemdUnit);
        }
        let mut seen = HashSet::new();
        let keys: Vec<String> = settings
            .environment_keys
            .iter()
            .map(|key| key.as_str())
            .filter(|key| is_env_name(key))
            .filter(|key| seen.insert((*key).to_string()))
            .map(ToString::to_string)
            .collect();
        let mut service = vec!["systemd-run".to_string(), "--user".to_string()];
        service.extend(keys.into_iter().map(|key| format!("--setenv={key}")));
        service.extend(
            [
                "--unit",
                &settings.systemd_unit,
                "--property=RemainAfterExit=yes",
                "--property=TimeoutStopSec=5s",
                "--wait",
                "--pipe",
                "--collect",
                "--",
            ]
            .iter()
            .map(|value| (*value).to_string()),
        );
        service.extend(primary);
        primary = service;
    }

    let mut cmd = vec!["gamescope".to_string()];
    cmd.extend(args);
    cmd.push("--".to_string());
    cmd.extend(primary);

    Ok(GamescopeLaunch {
        cmd,
        process_session: if settings.systemd_unit.is_empty() {
            None
        } else {
            Some(ProcessSession {
                session_type: "systemd".to_string(),
                unit: settings.systemd_unit.clone(),
                cgroup_root: "/sys/fs/cgroup".to_string(),
            })
        },
    })
}

/// `systemd-run --user` capability probe.  No unit is created here.
pub fn can_use_systemd_session(
    platform: Platform,
    bin_exists: &dyn Fn(&str) -> bool,
    env: &BTreeMap<String, String>,
    fs_exists: &dyn Fn(&str) -> bool,
) -> bool {
    if platform != Platform::Linux {
        return false;
    }
    if !bin_exists("systemd-run") || !bin_exists("systemctl") {
        return false;
    }
    let runtime_dir = env
        .get("XDG_RUNTIME_DIR")
        .map(|value| value.trim())
        .unwrap_or("");
    let dbus = env
        .get("DBUS_SESSION_BUS_ADDRESS")
        .map(|value| value.trim())
        .unwrap_or("");
    if !dbus.is_empty() {
        return true;
    }
    if runtime_dir.is_empty() || !runtime_dir.starts_with('/') {
        return false;
    }
    fs_exists(&Platform::Linux.join(&[runtime_dir, "bus"]))
}

/// Compose the exact transient unit name used by the Electron client.
pub fn create_systemd_unit_name(pid: i64, token_id: u64) -> Option<String> {
    if pid <= 1 || pid > 9_007_199_254_740_991 {
        return None;
    }
    if token_id == 0 || token_id > 9_007_199_254_740_991 {
        return None;
    }
    Some(format!("arcadia-game-{pid}-{token_id}.service"))
}

pub fn is_systemd_unit_name(value: &str) -> bool {
    let Some(inner) = value
        .strip_prefix("arcadia-game-")
        .and_then(|v| v.strip_suffix(".service"))
    else {
        return false;
    };
    let mut parts = inner.split('-');
    let (Some(pid), Some(token), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !pid.is_empty()
        && !token.is_empty()
        && pid.bytes().all(|b| b.is_ascii_digit())
        && token.bytes().all(|b| b.is_ascii_digit())
}

pub fn systemd_stop_args(unit: &str) -> Option<Vec<String>> {
    if !is_systemd_unit_name(unit) {
        return None;
    }
    Some(vec![
        "--user".to_string(),
        "stop".to_string(),
        "--no-block".to_string(),
        unit.to_string(),
    ])
}

/// A ready systemd-backed external session, or `None` when any capability is
/// missing.  Mirrors `createExternalProcessSession` in `main.js`.
pub fn create_process_session(
    platform: Platform,
    owner_pid: i64,
    token_id: u64,
    can_use_systemd: bool,
    user_manager_ready: bool,
    keep_alive: bool,
) -> Option<SystemdSession> {
    if platform != Platform::Linux || !can_use_systemd || !user_manager_ready {
        return None;
    }
    let unit = create_systemd_unit_name(owner_pid, token_id)?;
    Some(SystemdSession { unit, keep_alive })
}

pub fn parse_systemd_show(output: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for line in output.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(separator) = line.find('=') else {
            continue;
        };
        if separator == 0 {
            continue;
        }
        result.insert(
            line[..separator].to_string(),
            line[separator + 1..].to_string(),
        );
    }
    result
}

/// Read PIDs from a cgroup, refusing traversal outside `cgroup_root`.
pub fn read_cgroup_pids(
    cgroup_path: &str,
    cgroup_root: &str,
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Option<Vec<u32>> {
    if !cgroup_path.starts_with('/') || cgroup_path.contains('\0') {
        return None;
    }
    if cgroup_path.split('/').any(|part| part == "..") {
        return None;
    }

    let root = posix_resolve(cgroup_root, ".");
    let target = posix_resolve(&root, &format!(".{cgroup_path}"));
    if target != root && !target.starts_with(&format!("{root}/")) {
        return None;
    }

    let content = read_file(&format!("{target}/cgroup.procs"))?;
    Some(
        content
            .split_whitespace()
            .filter(|value| value.bytes().all(|b| b.is_ascii_digit()))
            .filter_map(|value| value.parse::<u32>().ok())
            .filter(|pid| *pid > 1)
            .collect(),
    )
}

/// Case-insensitive-ish Gamescope capability parse (the client only needs a
/// literal `--keep-alive` token in `gamescope --help`).
pub fn gamescope_supports_keep_alive(help: &str) -> bool {
    help.split_whitespace().any(|token| token == "--keep-alive")
}

/// JavaScript `safeInteger`: integers and numeric strings only, within range.
pub fn safe_integer(value: &Value, min: i64, max: i64, fallback: i64) -> i64 {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return fallback;
            }
            let digits = text
                .strip_prefix('+')
                .or_else(|| text.strip_prefix('-'))
                .unwrap_or(text);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return fallback;
            }
            match text.parse::<i64>() {
                Ok(number) if number >= min && number <= max => number,
                _ => fallback,
            }
        }
        Value::Number(number) => {
            let parsed = number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|v| i64::try_from(v).ok()))
                .or_else(|| {
                    number.as_f64().and_then(|v| {
                        if v.is_finite() && v.fract() == 0.0 {
                            Some(v as i64)
                        } else {
                            None
                        }
                    })
                });
            match parsed {
                Some(number) if number >= min && number <= max => number,
                _ => fallback,
            }
        }
        _ => fallback,
    }
}

pub fn valid_window_mode(value: &str) -> String {
    match value {
        "fullscreen" | "borderless" | "windowed" => value.to_string(),
        _ => "windowed".to_string(),
    }
}

fn safe_integer_value(value: i64, min: i64, max: i64, fallback: i64) -> i64 {
    if value < min || value > max {
        fallback
    } else {
        value
    }
}

fn is_env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first == b'_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
}

fn posix_resolve(base: &str, value: &str) -> String {
    let combined = if value.starts_with('/') || base.is_empty() {
        value.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), value)
    };
    let mut stack: Vec<&str> = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    format!("/{}", stack.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bins(name: &str) -> bool {
        matches!(name, "gamescope" | "systemd-run" | "systemctl")
    }

    #[test]
    fn builds_persistent_systemd_gamescope() {
        let result = build_external_gamescope_command(
            &["proton".into(), "run".into(), "/tmp/plutonium.exe".into()],
            &GamescopeSettings {
                width: 2560,
                height: 1600,
                fps: 60,
                keep_alive: true,
                systemd_unit: "arcadia-game-1234-9.service".into(),
                environment_keys: vec!["DISPLAY".into(), "DISPLAY".into(), "BAD-KEY".into()],
                ..GamescopeSettings::default()
            },
        )
        .unwrap();
        assert_eq!(
            result.cmd,
            vec![
                "gamescope",
                "-W",
                "2560",
                "-H",
                "1600",
                "-r",
                "60",
                "--keep-alive",
                "--",
                "systemd-run",
                "--user",
                "--setenv=DISPLAY",
                "--unit",
                "arcadia-game-1234-9.service",
                "--property=RemainAfterExit=yes",
                "--property=TimeoutStopSec=5s",
                "--wait",
                "--pipe",
                "--collect",
                "--",
                "proton",
                "run",
                "/tmp/plutonium.exe",
            ]
        );
        assert_eq!(
            result.process_session,
            Some(ProcessSession {
                session_type: "systemd".into(),
                unit: "arcadia-game-1234-9.service".into(),
                cgroup_root: "/sys/fs/cgroup".into(),
            })
        );
    }

    #[test]
    fn keeps_direct_command_without_systemd() {
        let result =
            build_external_gamescope_command(&["game".into()], &GamescopeSettings::default())
                .unwrap();
        assert_eq!(
            result.cmd,
            vec!["gamescope", "-W", "1920", "-H", "1080", "--", "game"]
        );
        assert_eq!(result.process_session, None);
    }

    #[test]
    fn capability_requires_linux_binaries_and_session_bus() {
        let exists = |file: &str| file == "/run/user/1000/bus";
        let mut env = BTreeMap::new();
        env.insert("XDG_RUNTIME_DIR".into(), "/run/user/1000".into());
        assert!(can_use_systemd_session(
            Platform::Linux,
            &bins,
            &env,
            &exists
        ));
        assert!(!can_use_systemd_session(
            Platform::Linux,
            &bins,
            &BTreeMap::new(),
            &exists
        ));
        assert!(!can_use_systemd_session(
            Platform::Windows,
            &bins,
            &BTreeMap::from([(
                "DBUS_SESSION_BUS_ADDRESS".into(),
                "unix:path=/tmp/bus".into()
            )]),
            &exists
        ));
    }

    #[test]
    fn unit_name_is_deterministic_and_strict() {
        assert_eq!(
            create_systemd_unit_name(1234, 9).as_deref(),
            Some("arcadia-game-1234-9.service")
        );
        assert!(is_systemd_unit_name("arcadia-game-1234-9.service"));
        assert!(!is_systemd_unit_name("arcadia-game-1234-9.scope"));
        assert_eq!(
            systemd_stop_args("arcadia-game-1234-9.service"),
            Some(vec![
                "--user".to_string(),
                "stop".to_string(),
                "--no-block".to_string(),
                "arcadia-game-1234-9.service".to_string(),
            ])
        );
        assert_eq!(systemd_stop_args("other.service"), None);
    }

    #[test]
    fn parses_show_and_reads_only_inside_cgroup() {
        assert_eq!(
            parse_systemd_show("ActiveState=active\r\nControlGroup=/user.slice/x.service\n"),
            BTreeMap::from([
                ("ActiveState".to_string(), "active".to_string()),
                (
                    "ControlGroup".to_string(),
                    "/user.slice/x.service".to_string()
                ),
            ])
        );
        let wanted = "/sys/fs/cgroup/user.slice/x.service/cgroup.procs";
        let reader = |file: &str| {
            if file == wanted {
                Some("101\n202\ninvalid\n0\n".to_string())
            } else {
                None
            }
        };
        assert_eq!(
            read_cgroup_pids("/user.slice/x.service", "/sys/fs/cgroup", &reader),
            Some(vec![101, 202])
        );
        assert_eq!(
            read_cgroup_pids("/user.slice/../other", "/sys/fs/cgroup", &reader),
            None
        );
    }

    #[test]
    fn builds_hdr_limit_and_window_mode_in_order() {
        let result = build_external_gamescope_command(
            &["proton".into(), "run".into(), "/tmp/game.exe".into()],
            &GamescopeSettings {
                width: 2560,
                height: 1600,
                fps: 60,
                framerate_limit: 45,
                hdr: true,
                window_mode: "fullscreen".into(),
                keep_alive: true,
                ..GamescopeSettings::default()
            },
        )
        .unwrap();
        assert_eq!(
            result.cmd,
            vec![
                "gamescope",
                "-W",
                "2560",
                "-H",
                "1600",
                "-r",
                "60",
                "--framerate-limit",
                "45",
                "--hdr-enabled",
                "-f",
                "--keep-alive",
                "--",
                "proton",
                "run",
                "/tmp/game.exe",
            ]
        );
    }

    #[test]
    fn window_mode_adds_exactly_one_flag() {
        for (mode, expected) in [
            ("fullscreen", Some("-f")),
            ("borderless", Some("-b")),
            ("windowed", None),
            ("valor-invalido", None),
        ] {
            let cmd = build_external_gamescope_command(
                &["game".into()],
                &GamescopeSettings {
                    window_mode: mode.into(),
                    ..GamescopeSettings::default()
                },
            )
            .unwrap()
            .cmd;
            let flags: Vec<&String> = cmd
                .iter()
                .filter(|value| value.as_str() == "-f" || value.as_str() == "-b")
                .collect();
            assert_eq!(flags.len(), usize::from(expected.is_some()), "{mode}");
            if let Some(expected) = expected {
                assert_eq!(flags[0], expected);
            }
        }
    }

    #[test]
    fn invalid_values_never_become_arguments() {
        let settings = GamescopeSettings::from_json(&json!({
            "width": "2560 --hdr-enabled",
            "height": -1,
            "fps": "60;evil",
            "framerateLimit": "45 --keep-alive",
            "hdr": "true",
            "windowMode": "--evil",
        }));
        let cmd = build_external_gamescope_command(&["game".into()], &settings)
            .unwrap()
            .cmd;
        assert_eq!(
            cmd,
            vec!["gamescope", "-W", "1920", "-H", "1080", "--", "game"]
        );
    }

    #[test]
    fn optional_legacy_values_do_not_change_contract() {
        let settings = GamescopeSettings {
            width: 1280,
            height: 720,
            fps: 30,
            hdr: false,
            framerate_limit: 0,
            window_mode: "windowed".into(),
            ..GamescopeSettings::default()
        };
        let cmd = build_external_gamescope_command(&["game".into()], &settings)
            .unwrap()
            .cmd;
        assert_eq!(
            cmd,
            vec![
                "gamescope",
                "-W",
                "1280",
                "-H",
                "720",
                "-r",
                "30",
                "--",
                "game"
            ]
        );
    }

    #[test]
    fn empty_command_and_bad_unit_are_rejected() {
        assert_eq!(
            build_external_gamescope_command(&[], &GamescopeSettings::default()),
            Err(GamescopeError::EmptyCommand)
        );
        assert_eq!(
            build_external_gamescope_command(
                &["game".into()],
                &GamescopeSettings {
                    systemd_unit: "evil.service".into(),
                    ..GamescopeSettings::default()
                }
            ),
            Err(GamescopeError::InvalidSystemdUnit)
        );
    }

    #[test]
    fn keep_alive_probe_needs_a_literal_token() {
        assert!(gamescope_supports_keep_alive("Usage:\n  --keep-alive\n"));
        assert!(!gamescope_supports_keep_alive("--keep-alive-extra"));
    }
}
