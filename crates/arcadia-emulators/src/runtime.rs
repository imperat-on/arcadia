//! Runtime: detecção de emulador já em execução e preflight de BIOS/firmware.
//!
//! Port 1:1 de `app/electron/emulator-runtime.js` e
//! `app/electron/emulator-status.js`. O módulo só lê metadados/arquivos
//! locais; não cria processos, não baixa nada e não segue symlinks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::fs::{
    basename, has_symlink_component, is_absolute, js_lower, js_trim, normalize_path, path_string,
    resolve_from_cwd, FsKind, PlatformKind,
};

pub const PS1_MIN: usize = 256 * 1024;
pub const PS1_MAX: usize = 16 * 1024 * 1024;
pub const PS2_MIN: usize = 4 * 1024 * 1024;
pub const PS2_MAX: usize = 8 * 1024 * 1024;
pub const MAX_READ: usize = 512 * 1024;
pub const PS1_SIGNATURE: &[u8] = b"Sony Computer Entertainment";
pub const PS2_RESET: &[u8] = b"RESET";
pub const PS2_ROMVER: &[u8] = b"ROMVER";

/// Código estável de "emulador já rodando" (`RUNNING_CODES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunningCodes {
    pub emulator_already_running: &'static str,
}

pub const RUNNING_CODES: RunningCodes = RunningCodes {
    emulator_already_running: "EMULATOR_ALREADY_RUNNING",
};

pub const EMULATOR_ALREADY_RUNNING: &str = RUNNING_CODES.emulator_already_running;

/// Códigos do preflight (`PREFLIGHT_CODES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreflightCodes {
    pub emulator_unknown: &'static str,
    pub bios_not_configured: &'static str,
}

pub const PREFLIGHT_CODES: PreflightCodes = PreflightCodes {
    emulator_unknown: "EMULATOR_UNKNOWN",
    bios_not_configured: "BIOS_NOT_CONFIGURED",
};

pub const EMULATOR_UNKNOWN: &str = PREFLIGHT_CODES.emulator_unknown;
pub const BIOS_NOT_CONFIGURED: &str = PREFLIGHT_CODES.bios_not_configured;
pub const LEGACY_BIOS_NOT_CONFIGURED: &str = "bios_nao_configurado";

/// Nomes de processo por emulador (`PROCESS_NAMES`). O `dolphin` do KDE é um
/// gerenciador de arquivos, por isso só `dolphin-emu` conta.
pub const PROCESS_NAMES: &[(&str, &[&str])] = &[
    ("pcsx2", &["pcsx2", "pcsx2-qt"]),
    ("rpcs3", &["rpcs3"]),
    ("dolphin", &["dolphin-emu"]),
    ("ppsspp", &["ppsspp", "ppssppheadless"]),
    ("duckstation", &["duckstation", "duckstation-qt"]),
    ("retroarch", &["retroarch"]),
    ("melonds", &["melonds"]),
    ("desmume", &["desmume"]),
];

pub fn process_names(emulator_id: &str) -> &'static [&'static str] {
    PROCESS_NAMES
        .iter()
        .find(|(id, _)| *id == emulator_id)
        .map(|(_, names)| *names)
        .unwrap_or(&[])
}

fn normalize_runtime_id(value: &str) -> String {
    js_lower(js_trim(value))
}

fn normalize_names(emulator_id: &str, executable_path: &str) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = process_names(emulator_id)
        .iter()
        .map(|name| js_lower(name))
        .collect();
    if !js_trim(executable_path).is_empty() {
        names.insert(js_lower(&basename(Path::new(js_trim(executable_path)))));
    }
    names
}

/// `readProcessName`: `comm` primeiro, depois o basename de `exe`.
pub fn read_process_name(proc_dir: &Path, fs: &dyn crate::fs::EmulatorFs) -> String {
    let comm_path = proc_dir.join("comm");
    if let Some(bytes) = fs.read(&comm_path) {
        let comm = js_lower(js_trim(&String::from_utf8_lossy(&bytes)));
        if !comm.is_empty() {
            return comm;
        }
    }
    match fs.read_link(&proc_dir.join("exe")) {
        Some(target) => js_lower(&basename(&target)),
        None => String::new(),
    }
}

/// `{ ok: true, emulatorId, running, pid }`; `pid` é `null` quando parado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningEmulatorStatus {
    pub ok: bool,
    pub emulator_id: String,
    pub running: bool,
    pub pid: Option<u64>,
}

impl RunningEmulatorStatus {
    fn stopped(emulator_id: String) -> Self {
        Self {
            ok: true,
            emulator_id,
            running: false,
            pid: None,
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// `getRunningEmulatorStatus`: varre `/proc` (ou uma raiz injetada) em ordem
/// numérica de PID e devolve o primeiro processo cujo nome bate.
pub fn get_running_emulator_status(
    emulator_id: &str,
    executable_path: &str,
    proc_root: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> RunningEmulatorStatus {
    let id = normalize_runtime_id(emulator_id);
    let names = normalize_names(&id, executable_path);
    if id.is_empty() || names.is_empty() {
        return RunningEmulatorStatus::stopped(id);
    }
    let Some(entries) = fs.read_dir(proc_root) else {
        return RunningEmulatorStatus::stopped(id);
    };
    let mut pids: Vec<(u128, String)> = entries
        .into_iter()
        .filter(|entry| {
            !entry.name.is_empty() && entry.name.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map(|entry| {
            let numeric = entry.name.parse::<u128>().unwrap_or(u128::MAX);
            (numeric, entry.name)
        })
        .collect();
    pids.sort_by_key(|entry| entry.0);
    for (_, pid) in pids {
        let name = read_process_name(&proc_root.join(&pid), fs);
        if names.contains(&name) {
            return RunningEmulatorStatus {
                ok: true,
                emulator_id: id,
                running: true,
                pid: Some(pid.parse::<u64>().unwrap_or(u64::MAX)),
            };
        }
    }
    RunningEmulatorStatus::stopped(id)
}

/// Conveniência: usa `/proc` real.
pub fn get_running_emulator_status_system(
    emulator_id: &str,
    executable_path: &str,
) -> RunningEmulatorStatus {
    get_running_emulator_status(
        emulator_id,
        executable_path,
        Path::new("/proc"),
        &crate::fs::SystemFs,
    )
}

/// Recusa estruturada quando já existe processo do emulador.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorAlreadyRunning {
    pub ok: bool,
    pub error: String,
    pub code: String,
    pub emulator_id: String,
    pub pid: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RunningPreflight {
    Status(RunningEmulatorStatus),
    Blocked(EmulatorAlreadyRunning),
}

impl RunningPreflight {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn ok(&self) -> bool {
        matches!(
            self,
            RunningPreflight::Status(RunningEmulatorStatus { running: false, .. })
        )
    }

    pub fn blocked(&self) -> Option<&EmulatorAlreadyRunning> {
        match self {
            RunningPreflight::Blocked(blocked) => Some(blocked),
            RunningPreflight::Status(_) => None,
        }
    }
}

/// `preflightRunningEmulator`.
pub fn preflight_running_emulator(
    emulator_id: &str,
    executable_path: &str,
    proc_root: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> RunningPreflight {
    let status = get_running_emulator_status(emulator_id, executable_path, proc_root, fs);
    if !status.ok || !status.running {
        return RunningPreflight::Status(status);
    }
    RunningPreflight::Blocked(EmulatorAlreadyRunning {
        ok: false,
        error: EMULATOR_ALREADY_RUNNING.to_string(),
        code: EMULATOR_ALREADY_RUNNING.to_string(),
        emulator_id: status.emulator_id,
        pid: status.pid,
    })
}

/// Conveniência: usa `/proc` real.
pub fn preflight_running_emulator_system(
    emulator_id: &str,
    executable_path: &str,
) -> RunningPreflight {
    preflight_running_emulator(
        emulator_id,
        executable_path,
        Path::new("/proc"),
        &crate::fs::SystemFs,
    )
}

// ---------------------------------------------------------------------------
// Status / preflight de BIOS e firmware
// ---------------------------------------------------------------------------

fn split_js_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// `normalizeAbsolute` do emulator-status: aceita caminho absoluto e rejeita
/// `..` separado pelo separador do host (quirk preservado do oráculo).
pub fn normalize_status_absolute(value: &str) -> String {
    if value.contains('\0') {
        return String::new();
    }
    let raw = js_trim(value);
    if raw.is_empty() {
        return String::new();
    }
    let separator = PlatformKind::path_separator();
    if !is_absolute(Path::new(raw)) || raw.split(separator).any(|part| part == "..") {
        return String::new();
    }
    path_string(&normalize_path(Path::new(raw)))
}

fn safe_directory(value: &str, fs: &dyn crate::fs::EmulatorFs) -> String {
    let normalized = normalize_status_absolute(value);
    if normalized.is_empty() || has_symlink_component(Path::new(&normalized), fs) {
        return String::new();
    }
    match fs.lstat(Path::new(&normalized)) {
        Some(meta) if meta.kind == FsKind::Directory => normalized,
        _ => String::new(),
    }
}

fn safe_file(value: &str, fs: &dyn crate::fs::EmulatorFs) -> bool {
    if has_symlink_component(Path::new(value), fs) {
        return false;
    }
    fs.lstat(Path::new(value))
        .map(|meta| meta.kind == FsKind::File)
        .unwrap_or(false)
}

fn parse_ini_section(line: &str) -> Option<String> {
    let trimmed = js_trim(line);
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') || trimmed.len() < 3 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty() || inner.contains(']') {
        return None;
    }
    Some(js_trim(inner).to_string())
}

fn read_ini_value(
    file: &str,
    section_name: &str,
    key_name: &str,
    root: &str,
    fs: &dyn crate::fs::EmulatorFs,
) -> String {
    if !safe_file(file, fs) {
        return String::new();
    }
    let Some(bytes) = fs.read(Path::new(file)) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut section = String::new();
    let wanted_section = js_lower(section_name);
    for line in split_js_lines(&text) {
        if let Some(header) = parse_ini_section(line) {
            section = js_lower(&header);
            continue;
        }
        if section != wanted_section {
            continue;
        }
        let trimmed = line.trim_start_matches(crate::fs::is_js_whitespace);
        let key_matches = trimmed
            .get(..key_name.len())
            .map(|prefix| prefix.eq_ignore_ascii_case(key_name))
            .unwrap_or(false);
        if !key_matches {
            continue;
        }
        let rest = &trimmed[key_name.len()..];
        let rest = rest.trim_start_matches(crate::fs::is_js_whitespace);
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let value = js_trim(rest);
        if value.is_empty() {
            continue;
        }
        let absolute = normalize_status_absolute(value);
        if !absolute.is_empty() {
            return absolute;
        }
        return path_string(&normalize_path(&Path::new(root).join(value)));
    }
    String::new()
}

fn executable_dir(executable_path: &str) -> String {
    let normalized = normalize_status_absolute(executable_path);
    if normalized.is_empty() {
        return String::new();
    }
    Path::new(&normalized)
        .parent()
        .map(path_string)
        .unwrap_or_default()
}

fn unique_existing_directories(values: &[String], fs: &dyn crate::fs::EmulatorFs) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let directory = safe_directory(value, fs);
        if directory.is_empty() || !seen.insert(directory.clone()) {
            continue;
        }
        result.push(directory);
    }
    result
}

fn bios_candidates(
    emulator_id: &str,
    executable_path: &str,
    manual_path: &str,
    home: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> Vec<String> {
    let manual = normalize_status_absolute(manual_path);
    let executable = executable_dir(executable_path);
    let mut candidates = if manual.is_empty() {
        Vec::new()
    } else {
        vec![manual]
    };
    if emulator_id == "duckstation" {
        let config_files = [
            home.join(".local")
                .join("share")
                .join("duckstation")
                .join("settings.ini"),
            home.join(".var")
                .join("app")
                .join("org.duckstation.DuckStation")
                .join("data")
                .join("duckstation")
                .join("settings.ini"),
        ];
        for file in config_files {
            let file_text = path_string(&file);
            let root = path_string(file.parent().unwrap_or(Path::new("")));
            let configured = read_ini_value(&file_text, "bios", "SearchDirectory", &root, fs);
            if !configured.is_empty() {
                candidates.push(configured);
            }
            candidates.push(path_string(
                &file.parent().unwrap_or(Path::new("")).join("bios"),
            ));
        }
    }
    if emulator_id == "pcsx2" {
        let mut config_files = vec![
            home.join(".config")
                .join("PCSX2")
                .join("inis")
                .join("PCSX2.ini"),
            home.join(".var")
                .join("app")
                .join("net.pcsx2.PCSX2")
                .join("config")
                .join("PCSX2")
                .join("inis")
                .join("PCSX2.ini"),
        ];
        if !executable.is_empty() {
            config_files.push(Path::new(&executable).join("inis").join("PCSX2.ini"));
        }
        for file in config_files {
            let file_text = path_string(&file);
            let root = file
                .parent()
                .and_then(Path::parent)
                .map(path_string)
                .unwrap_or_default();
            let configured = read_ini_value(&file_text, "folders", "Bios", &root, fs);
            if !configured.is_empty() {
                candidates.push(configured);
            }
            candidates.push(path_string(&Path::new(&root).join("bios")));
        }
    }
    if !executable.is_empty() {
        candidates.push(path_string(&Path::new(&executable).join("bios")));
    }
    unique_existing_directories(&candidates, fs)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `plausibleBios`: tamanho plausível + assinatura PS1/PS2.
pub fn plausible_bios(directory: &str, emulator_id: &str, fs: &dyn crate::fs::EmulatorFs) -> bool {
    let (min, max) = if emulator_id == "duckstation" {
        (PS1_MIN, PS1_MAX)
    } else {
        (PS2_MIN, PS2_MAX)
    };
    let Some(entries) = fs.read_dir(Path::new(directory)) else {
        return false;
    };
    for entry in entries {
        if entry.kind != FsKind::File || entry.is_symlink() {
            continue;
        }
        let file = Path::new(directory).join(&entry.name);
        let file_text = path_string(&file);
        if !safe_file(&file_text, fs) {
            continue;
        }
        let Some(size) = fs.lstat(&file).map(|meta| meta.size) else {
            continue;
        };
        let size = size as usize;
        if size < min || size > max {
            continue;
        }
        let bytes = if emulator_id == "duckstation" {
            64 * 1024
        } else {
            MAX_READ
        };
        let Some(head) = fs.read_prefix(&file, bytes) else {
            continue;
        };
        if emulator_id == "duckstation" && contains(&head, PS1_SIGNATURE) {
            return true;
        }
        if emulator_id == "pcsx2" && contains(&head, PS2_RESET) && contains(&head, PS2_ROMVER) {
            return true;
        }
    }
    false
}

fn firmware_candidates(
    executable_path: &str,
    home: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> Vec<String> {
    let executable = executable_dir(executable_path);
    let mut values = vec![
        path_string(
            &home
                .join(".config")
                .join("rpcs3")
                .join("dev_flash")
                .join("sys")
                .join("external"),
        ),
        path_string(
            &home
                .join(".var")
                .join("app")
                .join("net.rpcs3.RPCS3")
                .join("config")
                .join("rpcs3")
                .join("dev_flash")
                .join("sys")
                .join("external"),
        ),
    ];
    if !executable.is_empty() {
        values.push(path_string(
            &Path::new(&executable)
                .join("dev_flash")
                .join("sys")
                .join("external"),
        ));
    }
    unique_existing_directories(&values, fs)
}

fn has_firmware(directory: &str, fs: &dyn crate::fs::EmulatorFs) -> bool {
    match fs.read_dir(Path::new(directory)) {
        Some(entries) => entries
            .iter()
            .any(|entry| entry.kind == FsKind::File && !entry.is_symlink()),
        None => false,
    }
}

/// Status de BIOS/firmware de um emulador.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorStatus {
    pub ok: bool,
    pub emulator_id: String,
    pub kind: Option<String>,
    pub required: bool,
    pub installed: bool,
    pub detected_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorStatusError {
    pub ok: bool,
    pub error: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum EmulatorStatusResult {
    Status(EmulatorStatus),
    Error(EmulatorStatusError),
}

impl EmulatorStatusResult {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, EmulatorStatusResult::Status(status) if status.ok)
    }
}

/// `getEmulatorStatus`: BIOS PS1/PS2, firmware RPCS3 ou status neutro.
pub fn get_emulator_status(
    emulator_id: &str,
    executable_path: &str,
    bios_path: &str,
    home: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> EmulatorStatusResult {
    let id = normalize_runtime_id(emulator_id);
    if id.is_empty() {
        return EmulatorStatusResult::Error(EmulatorStatusError {
            ok: false,
            error: EMULATOR_UNKNOWN.to_string(),
            code: EMULATOR_UNKNOWN.to_string(),
        });
    }
    if id == "duckstation" || id == "pcsx2" {
        let candidates = bios_candidates(&id, executable_path, bios_path, home, fs);
        let detected = candidates
            .into_iter()
            .find(|candidate| plausible_bios(candidate, &id, fs));
        return EmulatorStatusResult::Status(EmulatorStatus {
            ok: true,
            emulator_id: id,
            kind: Some("bios".to_string()),
            required: true,
            installed: detected.is_some(),
            detected_path: detected,
        });
    }
    if id == "rpcs3" {
        let candidates = firmware_candidates(executable_path, home, fs);
        let detected = candidates
            .into_iter()
            .find(|candidate| has_firmware(candidate, fs));
        return EmulatorStatusResult::Status(EmulatorStatus {
            ok: true,
            emulator_id: id,
            kind: Some("firmware".to_string()),
            required: false,
            installed: detected.is_some(),
            detected_path: detected,
        });
    }
    EmulatorStatusResult::Status(EmulatorStatus {
        ok: true,
        emulator_id: id,
        kind: None,
        required: false,
        installed: true,
        detected_path: None,
    })
}

/// Conveniência: usa o home do processo e `SystemFs`.
pub fn get_emulator_status_system(
    emulator_id: &str,
    executable_path: &str,
    bios_path: &str,
) -> EmulatorStatusResult {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    get_emulator_status(
        emulator_id,
        executable_path,
        bios_path,
        &home,
        &crate::fs::SystemFs,
    )
}

/// Bloqueio de preflight com os dois códigos (atual e legado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightBlocked {
    pub ok: bool,
    pub error: String,
    pub code: String,
    pub legacy_error: String,
    pub emulator_id: String,
    pub kind: Option<String>,
    pub detected_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum PreflightEmulatorStatus {
    Status(EmulatorStatusResult),
    Blocked(PreflightBlocked),
}

impl PreflightEmulatorStatus {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, PreflightEmulatorStatus::Status(status) if status.is_ok())
    }

    pub fn blocked(&self) -> Option<&PreflightBlocked> {
        match self {
            PreflightEmulatorStatus::Blocked(blocked) => Some(blocked),
            PreflightEmulatorStatus::Status(_) => None,
        }
    }
}

/// `preflightEmulator`: bloqueia somente BIOS obrigatório ausente.
pub fn preflight_emulator(
    emulator_id: &str,
    executable_path: &str,
    bios_path: &str,
    home: &Path,
    fs: &dyn crate::fs::EmulatorFs,
) -> PreflightEmulatorStatus {
    let status = get_emulator_status(emulator_id, executable_path, bios_path, home, fs);
    match &status {
        EmulatorStatusResult::Error(_) => PreflightEmulatorStatus::Status(status),
        EmulatorStatusResult::Status(info) => {
            if info.required && !info.installed {
                PreflightEmulatorStatus::Blocked(PreflightBlocked {
                    ok: false,
                    error: BIOS_NOT_CONFIGURED.to_string(),
                    code: BIOS_NOT_CONFIGURED.to_string(),
                    legacy_error: LEGACY_BIOS_NOT_CONFIGURED.to_string(),
                    emulator_id: info.emulator_id.clone(),
                    kind: info.kind.clone(),
                    detected_path: None,
                })
            } else {
                PreflightEmulatorStatus::Status(status)
            }
        }
    }
}

/// Conveniência: usa o home do processo e `SystemFs`.
pub fn preflight_emulator_system(
    emulator_id: &str,
    executable_path: &str,
    bios_path: &str,
) -> PreflightEmulatorStatus {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    preflight_emulator(
        emulator_id,
        executable_path,
        bios_path,
        &home,
        &crate::fs::SystemFs,
    )
}

/// `emulators:status`: catálogo + BIOS/firmware + processo em execução.
///
/// Reproduz o handler do processo principal: para cada item de `list()`, usa o
/// executável resolvido (ou o do perfil) e o `biosPath` do perfil, e acrescenta
/// `running`/`runningPid` ao status.
pub fn emulator_statuses(
    registry: &crate::registry::EmulatorRegistry,
    home: &Path,
    proc_root: &Path,
) -> Value {
    let items = registry.list();
    let mut statuses = Vec::with_capacity(items.len());
    for item in items {
        let id = item.id().to_string();
        let profile = item.profile.clone().or_else(|| registry.get_profile(&id));
        let executable_path = if item.executable.is_empty() {
            profile
                .as_ref()
                .map(|profile| profile.executable.clone())
                .unwrap_or_default()
        } else {
            item.executable.clone()
        };
        let bios_path = profile
            .as_ref()
            .map(|profile| profile.bios_path.clone())
            .unwrap_or_default();
        let status = get_emulator_status(&id, &executable_path, &bios_path, home, registry.fs());
        let running = get_running_emulator_status(&id, &executable_path, proc_root, registry.fs());
        let mut value = status.to_value();
        if let Value::Object(object) = &mut value {
            object.insert("running".to_string(), Value::Bool(running.running));
            object.insert(
                "runningPid".to_string(),
                running
                    .pid
                    .map(|pid| Value::Number(serde_json::Number::from(pid)))
                    .unwrap_or(Value::Null),
            );
        }
        statuses.push(value);
    }
    json!({ "ok": true, "statuses": statuses })
}

/// Conveniência: usa home do processo e `/proc` real.
pub fn emulator_statuses_system(registry: &crate::registry::EmulatorRegistry) -> Value {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    emulator_statuses(registry, &home, Path::new("/proc"))
}

/// Caminho absoluto do diretório do executável, como `executableDir`.
pub fn emulator_executable_dir(executable_path: &str) -> String {
    executable_dir(executable_path)
}

/// Diretório de config de BIOS por emulador (helpers de UI/teste).
pub fn bios_config_files(emulator_id: &str, home: &Path) -> Vec<String> {
    match emulator_id {
        "duckstation" => vec![
            path_string(
                &home
                    .join(".local")
                    .join("share")
                    .join("duckstation")
                    .join("settings.ini"),
            ),
            path_string(
                &home
                    .join(".var")
                    .join("app")
                    .join("org.duckstation.DuckStation")
                    .join("data")
                    .join("duckstation")
                    .join("settings.ini"),
            ),
        ],
        "pcsx2" => vec![
            path_string(
                &home
                    .join(".config")
                    .join("PCSX2")
                    .join("inis")
                    .join("PCSX2.ini"),
            ),
            path_string(
                &home
                    .join(".var")
                    .join("app")
                    .join("net.pcsx2.PCSX2")
                    .join("config")
                    .join("PCSX2")
                    .join("inis")
                    .join("PCSX2.ini"),
            ),
        ],
        _ => Vec::new(),
    }
}

/// Normalização de caminho exposta para os mesmos seletores usados no status.
pub fn resolve_status_path(value: &str) -> PathBuf {
    resolve_from_cwd(Path::new(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemoryFs;
    use crate::test_support::TempDir;

    fn proc_fixture() -> MemoryFs {
        let fs = MemoryFs::new();
        fs.add_file(Path::new("/proc/4242/comm"), b"rpcs3\n");
        fs
    }

    #[test]
    fn detecta_rpcs3_executando_sem_criar_processos() {
        let fs = proc_fixture();
        let status = get_running_emulator_status("rpcs3", "", Path::new("/proc"), &fs);
        assert_eq!(
            status,
            RunningEmulatorStatus {
                ok: true,
                emulator_id: "rpcs3".into(),
                running: true,
                pid: Some(4242),
            }
        );
        let blocked = preflight_running_emulator("rpcs3", "", Path::new("/proc"), &fs);
        let value = blocked.to_value();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "EMULATOR_ALREADY_RUNNING");
        assert_eq!(value["code"], "EMULATOR_ALREADY_RUNNING");
        assert_eq!(value["pid"], 4242);
    }

    #[test]
    fn ignora_nomes_parecidos_pid_invalido_e_proc_indisponivel() {
        let fs = proc_fixture();
        fs.add_file(Path::new("/proc/bad/comm"), b"rpcs3\n");
        fs.add_file(Path::new("/proc/4242/comm"), b"not-rpcs3\n");
        assert!(!get_running_emulator_status("rpcs3", "", Path::new("/proc"), &fs).running);
        assert!(!get_running_emulator_status("rpcs3", "", Path::new("/missing"), &fs).running);
    }

    #[test]
    fn dolphin_ignora_file_manager_do_kde() {
        let fs = MemoryFs::new();
        fs.add_file(Path::new("/proc/4242/comm"), b"dolphin\n");
        assert!(!get_running_emulator_status("dolphin", "", Path::new("/proc"), &fs).running);
        fs.add_file(Path::new("/proc/4242/comm"), b"dolphin-emu\n");
        assert!(get_running_emulator_status("dolphin", "", Path::new("/proc"), &fs).running);
    }

    #[test]
    fn read_process_name_cai_para_exe_quando_comm_ausente() {
        let fs = MemoryFs::new();
        fs.add_dir(Path::new("/proc/7"));
        fs.add_symlink(Path::new("/proc/7/exe"), Path::new("/usr/bin/duckstation"));
        assert_eq!(read_process_name(Path::new("/proc/7"), &fs), "duckstation");
        assert!(get_running_emulator_status("duckstation", "", Path::new("/proc"), &fs).running);
    }

    #[test]
    fn executable_path_permite_detectar_emulador_desconhecido() {
        let fs = MemoryFs::new();
        fs.add_file(Path::new("/proc/99/comm"), b"myemu\n");
        let status = get_running_emulator_status("custom", "/opt/MyEmu", Path::new("/proc"), &fs);
        assert!(status.running);
        assert_eq!(status.pid, Some(99));
    }

    #[test]
    fn detecta_bios_ps1_por_tamanho_e_assinatura() {
        let temp = TempDir::new("arcadia-bios");
        let home = temp.mkdir("home");
        let exe = temp.write("bin/pcsx2", b"exe");
        let bios = temp.mkdir("duck/bios");
        let mut dump = vec![0u8; PS1_MIN];
        dump[32..32 + PS1_SIGNATURE.len()].copy_from_slice(PS1_SIGNATURE);
        std::fs::write(bios.join("scph-no-extension"), dump).unwrap();
        let status = get_emulator_status(
            "duckstation",
            &path_string(&exe),
            &path_string(&bios),
            &home,
            &crate::fs::SystemFs,
        );
        let value = status.to_value();
        assert_eq!(value["ok"], true);
        assert_eq!(value["installed"], true);
        assert_eq!(value["detectedPath"], path_string(&bios));
        assert!(preflight_emulator(
            "duckstation",
            &path_string(&exe),
            &path_string(&bios),
            &home,
            &crate::fs::SystemFs
        )
        .is_ok());
    }

    #[test]
    fn detecta_bios_ps2_por_reset_romver_e_bloqueia_ausencia() {
        let temp = TempDir::new("arcadia-bios");
        let home = temp.mkdir("home");
        let exe = temp.write("bin/pcsx2", b"exe");
        let bios = temp.mkdir("pcsx2/bios");
        let mut dump = vec![0u8; PS2_MIN];
        dump[64..69].copy_from_slice(PS2_RESET);
        dump[128..134].copy_from_slice(PS2_ROMVER);
        std::fs::write(bios.join("bios.bin"), dump).unwrap();
        let installed = get_emulator_status(
            "pcsx2",
            &path_string(&exe),
            &path_string(&bios),
            &home,
            &crate::fs::SystemFs,
        );
        assert!(installed.is_ok());
        assert_eq!(installed.to_value()["installed"], true);
        let missing = preflight_emulator(
            "pcsx2",
            &path_string(&exe),
            &path_string(&temp.path().join("missing")),
            &home,
            &crate::fs::SystemFs,
        );
        let value = missing.to_value();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "BIOS_NOT_CONFIGURED");
        assert_eq!(value["code"], "BIOS_NOT_CONFIGURED");
        assert_eq!(value["legacyError"], "bios_nao_configurado");
    }

    #[test]
    fn bios_invalido_por_tamanho_ou_assinatura_nao_conta() {
        let fs = MemoryFs::new();
        fs.add_file(Path::new("/bios/small.bin"), &vec![0u8; 1024]);
        assert!(!plausible_bios("/bios", "duckstation", &fs));
        fs.add_file(Path::new("/bios/ps1.bin"), &vec![0u8; PS1_MIN]);
        assert!(!plausible_bios("/bios", "duckstation", &fs));
        let mut dump = vec![0u8; PS1_MIN];
        dump[10..10 + PS1_SIGNATURE.len()].copy_from_slice(PS1_SIGNATURE);
        fs.add_file(Path::new("/bios/ps1.bin"), &dump);
        assert!(plausible_bios("/bios", "duckstation", &fs));
    }

    #[test]
    fn le_bios_configurado_em_ini_duckstation_e_pcsx2() {
        let temp = TempDir::new("arcadia-bios");
        let home = temp.mkdir("home");
        let duck_bios = temp.mkdir("custom-duck-bios");
        let mut dump = vec![0u8; PS1_MIN];
        dump[32..32 + PS1_SIGNATURE.len()].copy_from_slice(PS1_SIGNATURE);
        std::fs::write(duck_bios.join("bios.bin"), dump).unwrap();
        let duck_ini = temp.write(
            "home/.local/share/duckstation/settings.ini",
            format!("[BIOS]\nSearchDirectory = {}\n", duck_bios.display()).as_bytes(),
        );
        let status = get_emulator_status("duckstation", "", "", &home, &crate::fs::SystemFs);
        assert_eq!(status.to_value()["detectedPath"], path_string(&duck_bios));
        let _ = duck_ini;

        let ps2_bios = temp.mkdir("custom-ps2-bios");
        let mut dump = vec![0u8; PS2_MIN];
        dump[64..69].copy_from_slice(PS2_RESET);
        dump[128..134].copy_from_slice(PS2_ROMVER);
        std::fs::write(ps2_bios.join("bios.bin"), dump).unwrap();
        temp.write(
            "home/.config/PCSX2/inis/PCSX2.ini",
            format!("[Folders]\nBios = {}\n", ps2_bios.display()).as_bytes(),
        );
        let status = get_emulator_status("pcsx2", "", "", &home, &crate::fs::SystemFs);
        assert_eq!(status.to_value()["detectedPath"], path_string(&ps2_bios));
    }

    #[test]
    fn firmware_rpcs3_nao_bloqueia_e_nao_segue_symlink() {
        let temp = TempDir::new("arcadia-bios");
        let home = temp.mkdir("home");
        let external = temp.mkdir("home/.config/rpcs3/dev_flash/sys/external");
        std::fs::write(external.join("firmware.dat"), b"firmware").unwrap();
        let status = get_emulator_status("rpcs3", "", "", &home, &crate::fs::SystemFs);
        let value = status.to_value();
        assert_eq!(value["kind"], "firmware");
        assert_eq!(value["installed"], true);
        assert_eq!(value["required"], false);
        let preflight = preflight_emulator("rpcs3", "", "", &home, &crate::fs::SystemFs);
        assert!(preflight.is_ok());

        let fs = MemoryFs::new();
        fs.add_dir(Path::new("/home/.config/rpcs3/dev_flash/sys"));
        fs.add_file(Path::new("/outside"), b"x");
        fs.add_symlink(
            Path::new("/home/.config/rpcs3/dev_flash/sys/external"),
            Path::new("/outside"),
        );
        let status = get_emulator_status("rpcs3", "", "", Path::new("/home"), &fs);
        assert_eq!(status.to_value()["installed"], false);
    }

    #[test]
    fn preflight_usa_codigos_estruturados_para_entrada_invalida() {
        let result = get_emulator_status("", "", "", Path::new("/home"), &MemoryFs::new());
        let value = result.to_value();
        assert_eq!(value["ok"], false);
        assert_eq!(value["code"], "EMULATOR_UNKNOWN");
        assert_eq!(value["error"], "EMULATOR_UNKNOWN");
    }

    #[test]
    fn status_neutro_para_emuladores_sem_bios() {
        let status = get_emulator_status("dolphin", "", "", Path::new("/home"), &MemoryFs::new());
        let value = status.to_value();
        assert_eq!(value["ok"], true);
        assert_eq!(value["kind"], Value::Null);
        assert_eq!(value["required"], false);
        assert_eq!(value["installed"], true);
        assert_eq!(value["detectedPath"], Value::Null);
        assert!(
            preflight_emulator("dolphin", "", "", Path::new("/home"), &MemoryFs::new()).is_ok()
        );
    }

    #[test]
    fn emulators_status_combina_status_e_processo() {
        use crate::fs::{ManualClock, PlatformKind};
        use crate::registry::{EmulatorRegistry, RegistryOptions};
        use std::sync::Arc;

        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/pcsx2"), b"bin", 0o755, 1);
        fs.add_file(Path::new("/proc/4242/comm"), b"pcsx2\n");
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry =
            EmulatorRegistry::with_probes(options, Arc::new(fs), Arc::new(ManualClock::new(1)));
        let value = emulator_statuses(&registry, Path::new("/home"), Path::new("/proc"));
        assert_eq!(value["ok"], true);
        let statuses = value["statuses"].as_array().unwrap();
        assert_eq!(statuses.len(), 8);
        let pcsx2 = statuses
            .iter()
            .find(|status| status["emulatorId"] == "pcsx2")
            .unwrap();
        assert_eq!(pcsx2["ok"], true);
        assert_eq!(pcsx2["kind"], "bios");
        assert_eq!(pcsx2["installed"], false);
        assert_eq!(pcsx2["running"], true);
        assert_eq!(pcsx2["runningPid"], 4242);
    }
}
