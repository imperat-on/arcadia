//! Catálogo, perfis, detecção local e montagem de perfis.
//!
//! Port 1:1 de `app/electron/emulator-registry.js` (sem scanner, que vive em
//! [`crate::roms`] e sem resolução de argv, em [`crate::resolve`]). O módulo
//! lê/valida arquivos e monta estruturas; nunca executa binários e não conhece
//! Electron.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::fs::{
    basename, has_symlink_component, is_absolute, is_directory, is_executable, is_regular, js_len,
    js_locale_cmp, js_lower, js_trim, js_truthy, normalize_absolute, normalize_path, path_string,
    resolve_from_cwd, Clock, EmulatorFs, FsKind, PlatformKind, SystemClock, SystemFs,
};
use crate::roms::builtin_rom_extensions;

pub const REGISTRY_VERSION: i64 = 1;
pub const REGISTRY_FILENAME: &str = "emulators.json";
pub const ROMS_FILENAME: &str = "roms.json";
pub const ROMS_VERSION: i64 = 1;
pub const MAX_PROFILES: usize = 64;
pub const MAX_ARGS: usize = 32;
pub const MAX_ARG_LENGTH: usize = 1024;
pub const MAX_ROM_FOLDERS: usize = 32;
pub const MAX_DISCOVERY_ENTRIES: usize = 512;
pub const MAX_APPIMAGE_RESULTS: usize = 64;

pub const LINUX_STANDARD_PATHS: &[&str] = &[
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/local/sbin",
    "/usr/sbin",
    "/sbin",
    "/usr/local/games",
    "/usr/games",
    "/snap/bin",
];

const DEFINITION_ORDER: &[&str] = &[
    "pcsx2",
    "rpcs3",
    "dolphin",
    "ppsspp",
    "duckstation",
    "retroarch",
    "melonds",
    "desmume",
];

// ---------------------------------------------------------------------------
// Normalização e definições
// ---------------------------------------------------------------------------

/// `^[a-z0-9][a-z0-9._-]{1,63}$`, sem os nomes perigosos de protótipo.
pub fn normalize_id(value: &str) -> String {
    let id = js_lower(js_trim(value));
    if matches!(id.as_str(), "__proto__" | "constructor" | "prototype") {
        return String::new();
    }
    let bytes = id.as_bytes();
    if bytes.len() < 2
        || bytes.len() > 64
        || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return String::new();
    }
    id
}

/// `^[A-Za-z0-9][A-Za-z0-9._+@-]{0,127}$` ou caminho absoluto normalizado.
pub fn normalize_command(value: &str) -> String {
    let command = js_trim(value);
    if command.is_empty() || command.contains('\0') {
        return String::new();
    }
    if is_absolute(Path::new(command)) {
        return path_string(&normalize_path(Path::new(command)));
    }
    let bytes = command.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return String::new();
    }
    let valid_first = bytes[0].is_ascii_alphanumeric();
    let valid_rest = bytes[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'@' | b'-')
    });
    if valid_first && valid_rest {
        command.to_string()
    } else {
        String::new()
    }
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$`.
pub fn normalize_flatpak_id(value: &str) -> String {
    if value.contains('\0') {
        return String::new();
    }
    let id = js_trim(value);
    let bytes = id.as_bytes();
    if bytes.len() < 2
        || bytes.len() > 128
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return String::new();
    }
    id.to_string()
}

pub fn normalize_flatpak_ids(value: &Value) -> Vec<String> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    dedupe(
        items
            .iter()
            .filter_map(Value::as_str)
            .map(normalize_flatpak_id)
            .filter(|id| !id.is_empty())
            .collect(),
    )
    .into_iter()
    .take(8)
    .collect()
}

/// `^\.[a-z0-9][a-z0-9+_.-]{0,15}$` (após lowercase e ponto inicial).
pub fn normalize_rom_extension(value: &str) -> String {
    if value.contains('\0') {
        return String::new();
    }
    let raw = js_lower(js_trim(value));
    let extension = if raw.starts_with('.') {
        raw
    } else {
        format!(".{raw}")
    };
    let bytes = extension.as_bytes();
    if bytes.len() < 2
        || bytes.len() > 17
        || bytes[0] != b'.'
        || !bytes[1].is_ascii_lowercase() && !bytes[1].is_ascii_digit()
        || !bytes[2..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'+' | b'_' | b'.' | b'-')
        })
    {
        return String::new();
    }
    extension
}

pub fn normalize_rom_extensions(value: &Value) -> Vec<String> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    dedupe(
        items
            .iter()
            .filter_map(Value::as_str)
            .map(normalize_rom_extension)
            .filter(|extension| !extension.is_empty())
            .collect(),
    )
    .into_iter()
    .take(64)
    .collect()
}

pub fn normalize_rom_directory_markers(value: &Value) -> Vec<String> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    let markers: Vec<String> = items
        .iter()
        .filter_map(Value::as_str)
        .filter(|item| !js_trim(item).is_empty() && !item.contains('\0'))
        .map(|item| js_trim(item).to_string())
        .filter(|item| item != "." && item != ".." && !item.contains('/') && !item.contains('\\'))
        .take(16)
        .collect();
    dedupe(markers)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            result.push(value);
        }
    }
    result
}

/// Um diretório de ROMs configurado em um perfil.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomFolder {
    pub path: String,
    pub recursive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RomFolderInput {
    Path(String),
    Folder {
        path: String,
        #[serde(default)]
        recursive: Option<bool>,
    },
}

/// Entrada tipada para perfis. A entrada IPC-exata é
/// [`EmulatorRegistry::set_profile`], que recebe `serde_json::Value` e preserva
/// as distinções `undefined`/`null` do JS.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileInput {
    pub id: Option<String>,
    pub executable: Option<String>,
    pub command: Option<String>,
    pub core_path: Option<String>,
    pub bios_path: Option<String>,
    pub rom_folders: Option<Vec<RomFolderInput>>,
    pub args: Option<Vec<String>>,
    pub updated_at: Option<Value>,
}

impl ProfileInput {
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// `normalizeRomFolders`: aceita strings ou `{ path, recursive }`, normaliza
/// caminhos absolutos e deduplica preservando a ordem.
pub fn normalize_rom_folders(value: Option<&Value>) -> Result<Vec<RomFolder>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err("rom_folders_invalidas".to_string());
    };
    let mut folders = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let (raw_path, recursive) = match item {
            Value::String(path) => (path.as_str(), true),
            Value::Object(object) => {
                let Some(Value::String(path)) = object.get("path") else {
                    return Err("rom_folder_invalida".to_string());
                };
                let recursive = object.get("recursive") != Some(&Value::Bool(false));
                (path.as_str(), recursive)
            }
            _ => return Err("rom_folder_invalida".to_string()),
        };
        let normalized = normalize_absolute(raw_path, "romFolder")
            .map_err(|_| "rom_folder_invalida".to_string())?;
        let key = path_string(&normalized);
        if !seen.insert(key) {
            continue;
        }
        folders.push(RomFolder {
            path: path_string(&normalized),
            recursive,
        });
        if folders.len() >= MAX_ROM_FOLDERS {
            break;
        }
    }
    Ok(folders)
}

/// Definição de um emulador (built-in ou customizada).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorDefinition {
    pub id: String,
    pub name: String,
    pub systems: Vec<String>,
    pub description: String,
    pub candidates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub flatpak_ids: Option<Vec<String>>,
    pub rom_extensions: Vec<String>,
    pub rom_directory_markers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub requires_core: Option<bool>,
}

impl EmulatorDefinition {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// `normalizeDefinition`: devolve `None` para entradas inválidas, exatamente
/// como o registro usa para ignorar extensões malformadas.
pub fn normalize_definition(input: &Value) -> Option<EmulatorDefinition> {
    let Value::Object(object) = input else {
        return None;
    };
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .map(normalize_id)
        .unwrap_or_default();
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .map(js_trim)
        .unwrap_or("")
        .to_string();
    let raw_description = object
        .get("description")
        .and_then(Value::as_str)
        .map(js_trim)
        .unwrap_or("");
    let description = if !raw_description.is_empty() {
        raw_description.to_string()
    } else if !name.is_empty() {
        format!("{name} (perfil customizado)")
    } else {
        String::new()
    };
    let systems: Vec<String> = object
        .get("systems")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !js_trim(item).is_empty())
                .map(|item| js_trim(item).to_string())
                .take(8)
                .collect()
        })
        .unwrap_or_default();
    let candidates = dedupe(
        object
            .get("candidates")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(normalize_command)
                    .filter(|candidate| !candidate.is_empty())
                    .take(16)
                    .collect()
            })
            .unwrap_or_default(),
    );
    let rom_extensions_value = match object.get("romExtensions") {
        Some(Value::Null) | None => object.get("extensions").unwrap_or(&Value::Null),
        Some(value) => value,
    };
    let rom_extensions = normalize_rom_extensions(rom_extensions_value);
    let rom_directory_markers = object
        .get("romDirectoryMarkers")
        .map(normalize_rom_directory_markers)
        .unwrap_or_default();
    let flatpak_ids = object
        .get("flatpakIds")
        .map(normalize_flatpak_ids)
        .unwrap_or_default();
    if id.is_empty()
        || name.is_empty()
        || js_len(&name) > 120
        || systems.is_empty()
        || description.is_empty()
        || js_len(&description) > 300
    {
        return None;
    }
    Some(EmulatorDefinition {
        id,
        name,
        systems,
        description,
        candidates,
        flatpak_ids: if flatpak_ids.is_empty() {
            None
        } else {
            Some(flatpak_ids)
        },
        rom_extensions,
        rom_directory_markers,
        requires_core: if object.get("requiresCore") == Some(&Value::Bool(true)) {
            Some(true)
        } else {
            None
        },
    })
}

fn make_builtin_definition(
    id: &str,
    name: &str,
    systems: &[&str],
    description: &str,
    candidates: &[&str],
    flatpak_ids: &[&str],
    requires_core: bool,
) -> EmulatorDefinition {
    EmulatorDefinition {
        id: id.to_string(),
        name: name.to_string(),
        systems: systems.iter().map(|system| system.to_string()).collect(),
        description: description.to_string(),
        candidates: candidates
            .iter()
            .map(|candidate| candidate.to_string())
            .collect(),
        flatpak_ids: Some(flatpak_ids.iter().map(|id| id.to_string()).collect()),
        rom_extensions: builtin_rom_extensions(id)
            .unwrap_or(&[])
            .iter()
            .map(|extension| extension.to_string())
            .collect(),
        rom_directory_markers: builtin_rom_directory_markers(id)
            .iter()
            .map(|marker| marker.to_string())
            .collect(),
        requires_core: if requires_core { Some(true) } else { None },
    }
}

fn builtin_rom_directory_markers(id: &str) -> &'static [&'static str] {
    match id {
        "rpcs3" => &["PS3_GAME", "ps3_game"],
        _ => &[],
    }
}

/// Catálogo embutido, na ordem do oráculo.
pub fn builtin_definitions() -> Vec<EmulatorDefinition> {
    DEFINITION_ORDER
        .iter()
        .map(|id| match *id {
            "pcsx2" => make_builtin_definition(
                "pcsx2",
                "PCSX2",
                &["PlayStation 2"],
                "Emulador de PlayStation 2",
                &["pcsx2-qt", "pcsx2", "PCSX2"],
                &["net.pcsx2.PCSX2"],
                false,
            ),
            "rpcs3" => make_builtin_definition(
                "rpcs3",
                "RPCS3",
                &["PlayStation 3"],
                "Emulador de PlayStation 3",
                &["rpcs3"],
                &["net.rpcs3.RPCS3"],
                false,
            ),
            "dolphin" => make_builtin_definition(
                "dolphin",
                "Dolphin",
                &["GameCube", "Wii"],
                "Emulador de GameCube e Wii",
                &["dolphin-emu"],
                &["org.DolphinEmu.dolphin-emu"],
                false,
            ),
            "ppsspp" => make_builtin_definition(
                "ppsspp",
                "PPSSPP",
                &["PlayStation Portable"],
                "Emulador de PSP",
                &["ppsspp", "PPSSPP"],
                &["org.ppsspp.PPSSPP"],
                false,
            ),
            "duckstation" => make_builtin_definition(
                "duckstation",
                "DuckStation",
                &["PlayStation"],
                "Emulador de PlayStation 1",
                &["duckstation-qt", "duckstation"],
                &["org.duckstation.DuckStation"],
                false,
            ),
            "retroarch" => make_builtin_definition(
                "retroarch",
                "RetroArch",
                &["Multi-sistema"],
                "Frontend para cores libretro",
                &["retroarch"],
                &["org.libretro.RetroArch"],
                true,
            ),
            "melonds" => make_builtin_definition(
                "melonds",
                "melonDS",
                &["Nintendo DS"],
                "Emulador de Nintendo DS",
                &["melonDS", "melonds"],
                &["net.kuribo64.melonDS"],
                false,
            ),
            "desmume" => make_builtin_definition(
                "desmume",
                "DeSmuME",
                &["Nintendo DS"],
                "Emulador de Nintendo DS",
                &["desmume"],
                &["org.desmume.DeSmuME"],
                false,
            ),
            _ => unreachable!("id embutido"),
        })
        .collect()
}

/// Definição embutida por id (sem extensões customizadas).
pub fn builtin_definition(id: &str) -> Option<EmulatorDefinition> {
    let id = normalize_id(id);
    builtin_definitions()
        .into_iter()
        .find(|definition| definition.id == id)
}

/// Perfil persistido em `emulators.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorProfile {
    pub id: String,
    pub executable: String,
    pub core_path: String,
    pub bios_path: String,
    pub rom_folders: Vec<RomFolder>,
    pub args: Vec<String>,
    pub updated_at: i64,
}

impl EmulatorProfile {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// `normalizeArgs`.
pub fn normalize_args(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err("args_invalidos".to_string());
    };
    if items.len() > MAX_ARGS {
        return Err("args_invalidos".to_string());
    }
    let mut args = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(arg) = item else {
            return Err("args_invalidos".to_string());
        };
        if arg.contains('\0') || js_len(arg) > MAX_ARG_LENGTH {
            return Err("args_invalidos".to_string());
        }
        args.push(arg.clone());
    }
    Ok(args)
}

/// `normalizeProfile`. `now` substitui `Date.now()`.
pub fn normalize_profile(
    input: &Value,
    definition: &EmulatorDefinition,
    now: i64,
) -> Result<EmulatorProfile, String> {
    let Value::Object(object) = input else {
        return Err("perfil_invalido".to_string());
    };
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .map(normalize_id)
        .unwrap_or_default();
    if id.is_empty() || id != definition.id {
        return Err("emulador_desconhecido".to_string());
    }
    let executable_value = match object.get("executable") {
        Some(value) if js_truthy(value) => value,
        _ => object.get("command").unwrap_or(&Value::Null),
    };
    let executable = executable_value
        .as_str()
        .map(normalize_command)
        .unwrap_or_default();
    if executable.is_empty() {
        return Err("executavel_invalido".to_string());
    }
    let args = normalize_args(object.get("args"))?;
    let core_path = normalize_optional_path(object.get("corePath"), "corePath")?;
    let bios_path = normalize_optional_path(object.get("biosPath"), "biosPath")?;
    let rom_folders = normalize_rom_folders(object.get("romFolders"))?;
    let updated_at = match object.get("updatedAt") {
        None => now,
        Some(value) => crate::fs::js_trunc_or(value, now),
    };
    Ok(EmulatorProfile {
        id,
        executable,
        core_path,
        bios_path,
        rom_folders,
        args,
        updated_at,
    })
}

fn normalize_optional_path(value: Option<&Value>, field: &str) -> Result<String, String> {
    match value {
        None => Ok(String::new()),
        Some(Value::String(raw)) if raw.is_empty() => Ok(String::new()),
        Some(value) => {
            let Some(raw) = value.as_str() else {
                return Err(format!("{field}_invalido"));
            };
            normalize_absolute(raw, field).map(|path| path_string(&path))
        }
    }
}

// ---------------------------------------------------------------------------
// Descoberta de executáveis
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedExecutable {
    pub executable: String,
    pub args: Vec<String>,
}

/// `findOnPath` com o separador de PATH do host. `env_path` vazio cai no PATH
/// do processo, como o `||` do JS.
pub fn find_on_path(command: &str, fs: &dyn EmulatorFs, env_path: &str) -> String {
    find_on_path_for(command, fs, env_path, PlatformKind::current())
}

pub(crate) fn find_on_path_for(
    command: &str,
    fs: &dyn EmulatorFs,
    env_path: &str,
    platform: PlatformKind,
) -> String {
    let normalized = normalize_command(command);
    if normalized.is_empty() {
        return String::new();
    }
    if is_absolute(Path::new(&normalized)) {
        return if is_executable(Path::new(&normalized), fs, platform) {
            normalized
        } else {
            String::new()
        };
    }
    let path_value = if env_path.is_empty() {
        std::env::var("PATH").unwrap_or_default()
    } else {
        env_path.to_string()
    };
    for directory in path_value.split(PlatformKind::path_delimiter()) {
        if directory.is_empty() {
            continue;
        }
        let candidate = normalize_path(&Path::new(directory).join(&normalized));
        if is_executable(&candidate, fs, platform) {
            return path_string(&candidate);
        }
    }
    String::new()
}

/// `buildLinuxSearchPath`: mantém entradas relativas do PATH e acrescenta os
/// diretórios padrão Linux (e `~/.local/bin`) quando `env_path` é `None`.
pub fn build_linux_search_path(
    env_path: Option<&str>,
    platform: PlatformKind,
    home_dir: &Path,
    include_standard_paths: bool,
) -> String {
    let fallback;
    let value = match env_path {
        Some(value) => value,
        None => {
            fallback = std::env::var("PATH").unwrap_or_default();
            fallback.as_str()
        }
    };
    let mut entries: Vec<String> = value
        .split(PlatformKind::path_delimiter())
        .filter(|entry| !entry.is_empty() && !entry.contains('\0'))
        .map(ToString::to_string)
        .collect();
    if !platform.is_linux() {
        return entries.join(&PlatformKind::path_delimiter().to_string());
    }
    let mut seen: BTreeSet<String> = entries.iter().cloned().collect();
    let mut additions: Vec<PathBuf> = vec![home_dir.join(".local").join("bin")];
    if include_standard_paths {
        additions.extend(LINUX_STANDARD_PATHS.iter().map(PathBuf::from));
    }
    for addition in unique_discovery_paths(&additions) {
        let rendered = path_string(&addition);
        if seen.insert(rendered.clone()) {
            entries.push(rendered);
        }
    }
    entries.join(&PlatformKind::path_delimiter().to_string())
}

fn normalize_discovery_path(value: &Path) -> Option<PathBuf> {
    let raw = path_string(value);
    let trimmed = js_trim(&raw);
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let path = Path::new(trimmed);
    if !is_absolute(path) {
        return None;
    }
    Some(normalize_path(path))
}

fn unique_discovery_paths(values: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let Some(normalized) = normalize_discovery_path(value) else {
            continue;
        };
        let key = path_string(&normalized);
        if seen.insert(key) {
            result.push(normalized);
        }
    }
    result
}

/// `buildAppImageDirectories` (somente Linux).
pub fn build_app_image_directories(
    platform: PlatformKind,
    home_dir: &Path,
    app_image_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    if !platform.is_linux() {
        return Vec::new();
    }
    let mut values: Vec<PathBuf> = app_image_dirs.to_vec();
    values.extend([
        home_dir.join("Applications"),
        home_dir.join("AppImages"),
        home_dir.join("appimages"),
        home_dir.join("Downloads"),
        home_dir.join(".local").join("bin"),
        PathBuf::from("/opt"),
        PathBuf::from("/opt/Applications"),
        PathBuf::from("/opt/AppImages"),
        PathBuf::from("/opt/appimages"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    unique_discovery_paths(&values)
}

/// `buildFlatpakRoots` (somente Linux).
pub fn build_flatpak_roots(
    platform: PlatformKind,
    home_dir: &Path,
    env: &BTreeMap<String, String>,
    flatpak_roots: &[PathBuf],
) -> Vec<PathBuf> {
    if !platform.is_linux() {
        return Vec::new();
    }
    let mut values: Vec<PathBuf> = flatpak_roots.to_vec();
    if let Some(xdg_data_home) = env.get("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        values.push(PathBuf::from(xdg_data_home).join("flatpak"));
    }
    values.extend([
        home_dir.join(".local").join("share").join("flatpak"),
        PathBuf::from("/var/lib/flatpak"),
        PathBuf::from("/usr/local/share/flatpak"),
    ]);
    unique_discovery_paths(&values)
}

fn read_directory_names(directory: &Path, fs: &dyn EmulatorFs) -> Vec<String> {
    if !is_directory(directory, fs) {
        return Vec::new();
    }
    let entries = match fs.read_dir(directory) {
        Some(entries) => entries,
        None => return Vec::new(),
    };
    let mut names: Vec<String> = entries
        .into_iter()
        .map(|entry| entry.name)
        .filter(|name| !name.is_empty() && !name.contains('\0'))
        .collect();
    names.sort_by(|left, right| js_locale_cmp(left, right));
    names.truncate(MAX_DISCOVERY_ENTRIES);
    names
}

fn compact_discovery_name(value: &str) -> String {
    js_lower(value)
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn is_appimage_for_definition(name: &str, definition: &EmulatorDefinition) -> bool {
    let lower = js_lower(name);
    let Some(stem) = lower.strip_suffix(".appimage") else {
        return false;
    };
    let mut aliases: Vec<String> = Vec::new();
    for candidate in std::iter::once(&definition.id).chain(definition.candidates.iter()) {
        let alias = compact_discovery_name(&basename(Path::new(candidate)));
        if !alias.is_empty() {
            aliases.push(alias);
        }
    }
    let compact = compact_discovery_name(stem);
    aliases.iter().any(|alias| compact.contains(alias))
}

fn find_app_image(
    definition: &EmulatorDefinition,
    fs: &dyn EmulatorFs,
    platform: PlatformKind,
    directories: &[PathBuf],
) -> String {
    let mut count = 0usize;
    for directory in directories {
        for name in read_directory_names(directory, fs) {
            count += 1;
            if count > MAX_APPIMAGE_RESULTS * 8 {
                return String::new();
            }
            if !is_appimage_for_definition(&name, definition) {
                continue;
            }
            let candidate = directory.join(name);
            if is_executable(&candidate, fs, platform) {
                return path_string(&candidate);
            }
        }
    }
    String::new()
}

fn is_flatpak_export(file: &Path, fs: &dyn EmulatorFs) -> bool {
    let Some(parent) = file.parent() else {
        return false;
    };
    if has_symlink_component(parent, fs) {
        return false;
    }
    fs.lstat(file)
        .map(|meta| matches!(meta.kind, FsKind::File | FsKind::Symlink))
        .unwrap_or(false)
}

fn flatpak_app_present(app_id: &str, fs: &dyn EmulatorFs, roots: &[PathBuf]) -> bool {
    for root in roots {
        let app_paths = [root.join("app").join(app_id), root.join(app_id)];
        if app_paths.iter().any(|path| is_directory(path, fs)) {
            return true;
        }
        let desktop = root
            .join("exports")
            .join("share")
            .join("applications")
            .join(format!("{app_id}.desktop"));
        if is_flatpak_export(&desktop, fs) {
            return true;
        }
        let wrapper = root.join("exports").join("bin").join(app_id);
        if is_flatpak_export(&wrapper, fs) {
            return true;
        }
    }
    false
}

fn find_flatpak(
    definition: &EmulatorDefinition,
    fs: &dyn EmulatorFs,
    platform: PlatformKind,
    env_path: &str,
    roots: &[PathBuf],
) -> Option<DetectedExecutable> {
    let ids = definition.flatpak_ids.clone().unwrap_or_default();
    if ids.is_empty() {
        return None;
    }
    let executable = find_on_path_for("flatpak", fs, env_path, platform);
    if executable.is_empty() {
        return None;
    }
    for app_id in ids {
        if flatpak_app_present(&app_id, fs, roots) {
            return Some(DetectedExecutable {
                executable,
                args: vec!["run".to_string(), app_id],
            });
        }
    }
    None
}

/// `detectDefinition`: candidatos do PATH, AppImage e Flatpak, nessa ordem.
pub fn detect_definition(
    definition: &EmulatorDefinition,
    fs: &dyn EmulatorFs,
    env_path: &str,
    app_image_dirs: &[PathBuf],
    flatpak_roots: &[PathBuf],
) -> Option<DetectedExecutable> {
    detect_definition_for(
        definition,
        fs,
        env_path,
        PlatformKind::current(),
        app_image_dirs,
        flatpak_roots,
    )
}

pub(crate) fn detect_definition_for(
    definition: &EmulatorDefinition,
    fs: &dyn EmulatorFs,
    env_path: &str,
    platform: PlatformKind,
    app_image_dirs: &[PathBuf],
    flatpak_roots: &[PathBuf],
) -> Option<DetectedExecutable> {
    for candidate in &definition.candidates {
        let executable = find_on_path_for(candidate, fs, env_path, platform);
        if !executable.is_empty() {
            return Some(DetectedExecutable {
                executable,
                args: Vec::new(),
            });
        }
    }
    let app_image = find_app_image(definition, fs, platform, app_image_dirs);
    if !app_image.is_empty() {
        return Some(DetectedExecutable {
            executable: app_image,
            args: Vec::new(),
        });
    }
    find_flatpak(definition, fs, platform, env_path, flatpak_roots)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListSource {
    Builtin,
    Detected,
    Configured,
}

/// Item de `emulators:list` / `emulators:detect`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorListItem {
    #[serde(flatten)]
    pub definition: EmulatorDefinition,
    pub executable: String,
    pub available: bool,
    pub source: ListSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<EmulatorProfile>,
}

impl EmulatorListItem {
    pub fn id(&self) -> &str {
        &self.definition.id
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

impl std::ops::Deref for EmulatorListItem {
    type Target = EmulatorDefinition;

    fn deref(&self) -> &Self::Target {
        &self.definition
    }
}

// ---------------------------------------------------------------------------
// Estado persistido e escrita atômica
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct RegistryState {
    profiles: Map<String, Value>,
}

fn read_state(file: &Path, fs: &dyn EmulatorFs) -> RegistryState {
    if has_symlink_component(file, fs) {
        return RegistryState::default();
    }
    let Some(meta) = fs.lstat(file) else {
        return RegistryState::default();
    };
    if meta.kind != FsKind::File {
        return RegistryState::default();
    }
    let Some(bytes) = fs.read(file) else {
        return RegistryState::default();
    };
    let text = String::from_utf8_lossy(&bytes);
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return RegistryState::default();
    };
    let Some(object) = parsed.as_object() else {
        return RegistryState::default();
    };
    if object.get("version").and_then(Value::as_f64) != Some(REGISTRY_VERSION as f64) {
        return RegistryState::default();
    }
    let profiles = match object.get("profiles") {
        Some(Value::Object(profiles)) => profiles.clone(),
        _ => Map::new(),
    };
    RegistryState { profiles }
}

fn temp_path(file: &Path, clock: &dyn Clock) -> PathBuf {
    PathBuf::from(format!(
        "{}.{}.{}.tmp",
        path_string(file),
        std::process::id(),
        clock.now_ms()
    ))
}

/// `atomicWrite`: temporário com `wx`, rename e `rm` forçado no final.
pub fn atomic_write(
    file: &Path,
    value: &Value,
    fs: &dyn EmulatorFs,
    clock: &dyn Clock,
) -> Result<(), String> {
    let Some(directory) = file.parent() else {
        return Err("diretorio_emuladores_invalido".to_string());
    };
    if has_symlink_component(directory, fs) {
        return Err("diretorio_emuladores_symlink".to_string());
    }
    fs.mkdir_all(directory, 0o700)
        .map_err(|_| "diretorio_emuladores_invalido".to_string())?;
    match fs.lstat(directory) {
        Some(meta) if meta.kind == FsKind::Directory => {}
        _ => return Err("diretorio_emuladores_invalido".to_string()),
    }
    if !is_regular(file, fs) {
        if let Some(meta) = fs.lstat(file) {
            if meta.kind == FsKind::Symlink {
                return Err("arquivo_emuladores_symlink".to_string());
            }
        }
    }
    let temporary = temp_path(file, clock);
    let mut data = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    data.push('\n');
    let result = (|| {
        fs.write_new(&temporary, data.as_bytes(), 0o600)
            .map_err(|error| error.to_string())?;
        if fs.lstat(&temporary).map(|meta| meta.kind) == Some(FsKind::Symlink) {
            return Err("temporario_emuladores_symlink".to_string());
        }
        fs.rename(&temporary, file)
            .map_err(|error| error.to_string())
    })();
    let _ = fs.remove_file(&temporary);
    result
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Opções do registry. `None` em `env_path` equivale a `undefined` no JS:
/// usa `env.PATH` e inclui os diretórios padrão Linux.
#[derive(Clone, Debug)]
pub struct RegistryOptions {
    pub data_dir: PathBuf,
    pub profiles_path: Option<PathBuf>,
    pub env_path: Option<String>,
    pub home_dir: PathBuf,
    pub platform: PlatformKind,
    pub env: BTreeMap<String, String>,
    pub app_image_dirs: Vec<PathBuf>,
    pub flatpak_roots: Vec<PathBuf>,
    pub extra_definitions: Vec<Value>,
    pub definitions: Option<Vec<Value>>,
}

impl Default for RegistryOptions {
    fn default() -> Self {
        let platform = PlatformKind::current();
        let env: BTreeMap<String, String> = crate::fs::process_env();
        let home_dir = default_home_dir(&env);
        Self {
            data_dir: default_data_dir(platform, &env, &home_dir),
            profiles_path: None,
            env_path: None,
            home_dir,
            platform,
            env,
            app_image_dirs: Vec::new(),
            flatpak_roots: Vec::new(),
            extra_definitions: Vec::new(),
            definitions: None,
        }
    }
}

impl RegistryOptions {
    pub fn for_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }
}

fn default_home_dir(env: &BTreeMap<String, String>) -> PathBuf {
    if cfg!(windows) {
        env.get("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                env.get("HOMEDRIVE")
                    .zip(env.get("HOMEPATH"))
                    .map(|(drive, path)| PathBuf::from(format!("{drive}{path}")))
            })
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        env.get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// `getDefaultDataDir` de `runtime-paths.js`.
pub fn default_data_dir(
    platform: PlatformKind,
    env: &BTreeMap<String, String>,
    home_dir: &Path,
) -> PathBuf {
    if platform.is_windows() {
        let base = env
            .get("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join("AppData").join("Local"));
        base.join("arcadia")
    } else {
        home_dir.join(".local").join("share").join("arcadia")
    }
}

/// `resolveDataDir`: `ARCADIA_DATA_DIR`, ou o padrão, sempre absoluto.
pub fn resolve_data_dir(
    env: &BTreeMap<String, String>,
    platform: PlatformKind,
    home_dir: &Path,
) -> PathBuf {
    let raw = env
        .get("ARCADIA_DATA_DIR")
        .map(|value| js_trim(value))
        .unwrap_or("");
    if raw.is_empty() {
        resolve_from_cwd(&default_data_dir(platform, env, home_dir))
    } else {
        resolve_from_cwd(Path::new(raw))
    }
}

/// Registry de emuladores. Uma instância guarda o catálogo e os probes.
pub struct EmulatorRegistry {
    data_dir: PathBuf,
    state_path: PathBuf,
    roms_path: PathBuf,
    fs: Arc<dyn EmulatorFs>,
    clock: Arc<dyn Clock>,
    search_path: String,
    home_dir: PathBuf,
    platform: PlatformKind,
    app_image_dirs: Vec<PathBuf>,
    flatpak_roots: Vec<PathBuf>,
    catalog: Vec<EmulatorDefinition>,
}

impl EmulatorRegistry {
    pub fn new(options: RegistryOptions) -> Self {
        Self::with_probes(options, Arc::new(SystemFs), Arc::new(SystemClock))
    }

    pub fn with_probes(
        options: RegistryOptions,
        fs: Arc<dyn EmulatorFs>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let include_standard = options.env_path.is_none();
        let effective_env_path = options.env_path.clone().or_else(|| {
            options
                .env
                .get("PATH")
                .cloned()
                .or_else(|| std::env::var("PATH").ok())
        });
        let search_path = build_linux_search_path(
            effective_env_path.as_deref(),
            options.platform,
            &options.home_dir,
            include_standard,
        );
        let app_image_dirs = build_app_image_directories(
            options.platform,
            &options.home_dir,
            &options.app_image_dirs,
        );
        let flatpak_roots = build_flatpak_roots(
            options.platform,
            &options.home_dir,
            &options.env,
            &options.flatpak_roots,
        );
        let root = resolve_from_cwd(&options.data_dir);
        let state_path = match &options.profiles_path {
            Some(path) => resolve_from_cwd(path),
            None => root.join(REGISTRY_FILENAME),
        };
        let roms_path = root.join(ROMS_FILENAME);

        let mut catalog = builtin_definitions();
        let configured = match &options.definitions {
            Some(definitions) => definitions.as_slice(),
            None => options.extra_definitions.as_slice(),
        };
        for input in configured {
            let Some(normalized) = normalize_definition(input) else {
                continue;
            };
            if let Some(index) = catalog.iter().position(|item| item.id == normalized.id) {
                catalog[index] = normalized;
            } else {
                catalog.push(normalized);
            }
        }

        Self {
            data_dir: root,
            state_path,
            roms_path,
            fs,
            clock,
            search_path,
            home_dir: options.home_dir,
            platform: options.platform,
            app_image_dirs,
            flatpak_roots,
            catalog,
        }
    }

    pub fn fs(&self) -> &dyn EmulatorFs {
        self.fs.as_ref()
    }

    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    pub fn roms_path(&self) -> &Path {
        &self.roms_path
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    pub fn platform(&self) -> PlatformKind {
        self.platform
    }

    pub fn search_path(&self) -> &str {
        &self.search_path
    }

    pub fn app_image_dirs(&self) -> &[PathBuf] {
        &self.app_image_dirs
    }

    pub fn flatpak_roots(&self) -> &[PathBuf] {
        &self.flatpak_roots
    }

    /// `paths()` do oráculo.
    pub fn paths(&self) -> Value {
        json!({ "statePath": path_string(&self.state_path) })
    }

    /// `romPaths()` do oráculo.
    pub fn rom_paths(&self) -> Value {
        json!({ "romsPath": path_string(&self.roms_path) })
    }

    pub fn definitions(&self) -> Vec<EmulatorDefinition> {
        self.catalog.clone()
    }

    pub fn definition(&self, id: &str) -> Option<&EmulatorDefinition> {
        let id = normalize_id(id);
        self.catalog.iter().find(|item| item.id == id)
    }

    fn read_profiles(&self) -> Vec<(String, EmulatorProfile)> {
        let state = read_state(&self.state_path, self.fs.as_ref());
        let mut profiles = Vec::new();
        for definition in &self.catalog {
            let Some(candidate) = state.profiles.get(&definition.id) else {
                continue;
            };
            let mut object = match candidate {
                Value::Object(object) => object.clone(),
                _ => Map::new(),
            };
            object.insert("id".to_string(), Value::String(definition.id.clone()));
            let wrapped = Value::Object(object);
            if let Ok(profile) = normalize_profile(&wrapped, definition, self.clock.now_ms()) {
                profiles.push((definition.id.clone(), profile));
            }
        }
        profiles
    }

    /// Perfis válidos, na ordem do catálogo.
    pub fn profiles(&self) -> Vec<EmulatorProfile> {
        self.read_profiles()
            .into_iter()
            .map(|(_, profile)| profile)
            .collect()
    }

    /// `profiles()` em objeto JSON (`emulators:profiles`).
    pub fn profiles_value(&self) -> Value {
        let mut map = Map::new();
        for (id, profile) in self.read_profiles() {
            map.insert(id, serde_json::to_value(profile).unwrap_or(Value::Null));
        }
        Value::Object(map)
    }

    pub fn get_profile(&self, id: &str) -> Option<EmulatorProfile> {
        let id = normalize_id(id);
        self.read_profiles()
            .into_iter()
            .find(|(profile_id, _)| *profile_id == id)
            .map(|(_, profile)| profile)
    }

    fn save_profiles(&self, profiles: &[(String, EmulatorProfile)]) -> Result<(), String> {
        let mut map = Map::new();
        for (id, profile) in profiles {
            map.insert(
                id.clone(),
                serde_json::to_value(profile).map_err(|error| error.to_string())?,
            );
        }
        let state = json!({ "version": REGISTRY_VERSION, "profiles": Value::Object(map) });
        atomic_write(
            &self.state_path,
            &state,
            self.fs.as_ref(),
            self.clock.as_ref(),
        )
    }

    /// `setProfile(payload)` — envelope exato do IPC.
    pub fn set_profile(&self, input: &Value) -> Value {
        if !input.is_object() {
            return json!({ "ok": false, "error": "perfil_invalido" });
        }
        let id = input
            .get("id")
            .and_then(Value::as_str)
            .map(normalize_id)
            .unwrap_or_default();
        let Some(definition) = self.catalog.iter().find(|item| item.id == id).cloned() else {
            return json!({ "ok": false, "error": "emulador_desconhecido" });
        };
        let profile = match normalize_profile(input, &definition, self.clock.now_ms()) {
            Ok(profile) => profile,
            Err(error) => return json!({ "ok": false, "error": error }),
        };
        let mut profiles = self.read_profiles();
        if !profiles.iter().any(|(profile_id, _)| *profile_id == id)
            && profiles.len() >= MAX_PROFILES
        {
            return json!({ "ok": false, "error": "perfis_emuladores_limite" });
        }
        if let Some(entry) = profiles
            .iter_mut()
            .find(|(profile_id, _)| *profile_id == id)
        {
            entry.1 = profile.clone();
        } else {
            profiles.push((id, profile.clone()));
        }
        match self.save_profiles(&profiles) {
            Ok(()) => json!({
                "ok": true,
                "profile": serde_json::to_value(profile).unwrap_or(Value::Null),
            }),
            Err(_) => json!({ "ok": false, "error": "emuladores_nao_gravavel" }),
        }
    }

    /// `setProfile` a partir de uma entrada tipada.
    pub fn set_profile_input(&self, input: &ProfileInput) -> Value {
        self.set_profile(&input.to_value())
    }

    /// `removeProfile(id)`.
    pub fn remove_profile(&self, id: &str) -> Value {
        let id = normalize_id(id);
        if !self.catalog.iter().any(|item| item.id == id) {
            return json!({ "ok": false, "error": "emulador_desconhecido" });
        }
        let mut profiles = self.read_profiles();
        let Some(index) = profiles
            .iter()
            .position(|(profile_id, _)| *profile_id == id)
        else {
            return json!({ "ok": true, "removed": false });
        };
        profiles.remove(index);
        match self.save_profiles(&profiles) {
            Ok(()) => json!({ "ok": true, "removed": true }),
            Err(_) => json!({ "ok": false, "error": "emuladores_nao_gravavel" }),
        }
    }

    fn detect_one(
        &self,
        definition: &EmulatorDefinition,
        profiles: &[(String, EmulatorProfile)],
    ) -> EmulatorListItem {
        let configured = profiles
            .iter()
            .find(|(id, _)| *id == definition.id)
            .map(|(_, profile)| profile);
        let configured_executable = configured
            .map(|profile| {
                find_on_path_for(
                    &profile.executable,
                    self.fs.as_ref(),
                    &self.search_path,
                    self.platform,
                )
            })
            .unwrap_or_default();
        let detected = match configured {
            Some(_) => {
                if configured_executable.is_empty() {
                    None
                } else {
                    Some(DetectedExecutable {
                        executable: configured_executable.clone(),
                        args: configured
                            .map(|profile| profile.args.clone())
                            .unwrap_or_default(),
                    })
                }
            }
            None => detect_definition_for(
                definition,
                self.fs.as_ref(),
                &self.search_path,
                self.platform,
                &self.app_image_dirs,
                &self.flatpak_roots,
            ),
        };
        let source = if configured.is_some() {
            ListSource::Configured
        } else if detected.is_some() {
            ListSource::Detected
        } else {
            ListSource::Builtin
        };
        EmulatorListItem {
            definition: definition.clone(),
            executable: detected
                .as_ref()
                .map(|item| item.executable.clone())
                .or_else(|| configured.map(|profile| profile.executable.clone()))
                .unwrap_or_default(),
            available: detected.is_some(),
            source,
            detected_args: if configured.is_none() {
                detected.as_ref().and_then(|item| {
                    if item.args.is_empty() {
                        None
                    } else {
                        Some(item.args.clone())
                    }
                })
            } else {
                None
            },
            profile: configured.cloned(),
        }
    }

    /// `list()`: catálogo + disponibilidade/perfil.
    pub fn list(&self) -> Vec<EmulatorListItem> {
        let profiles = self.read_profiles();
        self.catalog
            .iter()
            .map(|definition| self.detect_one(definition, &profiles))
            .collect()
    }

    /// `list()` em JSON.
    pub fn list_value(&self) -> Value {
        serde_json::to_value(self.list()).unwrap_or(Value::Array(Vec::new()))
    }

    /// `detect()`: `{ ok: true, emulators: [...] }`.
    pub fn detect(&self) -> Value {
        json!({ "ok": true, "emulators": self.list_value() })
    }

    /// `emulators:list`: `{ ok: true, emulators: [...] }`.
    pub fn list_envelope(&self) -> Value {
        json!({ "ok": true, "emulators": self.list_value() })
    }

    /// `emulators:profiles`: `{ ok: true, profiles: { ... } }`.
    pub fn profiles_envelope(&self) -> Value {
        json!({ "ok": true, "profiles": self.profiles_value() })
    }

    pub(crate) fn catalog_ref(&self) -> &[EmulatorDefinition] {
        &self.catalog
    }

    pub(crate) fn profile_folders(&self, id: &str) -> Vec<RomFolder> {
        self.get_profile(id)
            .map(|profile| profile.rom_folders)
            .unwrap_or_default()
    }

    /// `scanRoms(payload)` (`emulators:roms`).
    pub fn scan_roms(&self, payload: &Value) -> Value {
        let id = payload
            .get("emulatorId")
            .and_then(Value::as_str)
            .map(normalize_id)
            .unwrap_or_default();
        let Some(definition) = self.definition(&id).cloned() else {
            return json!({ "ok": false, "error": "emulador_desconhecido" });
        };
        let context = crate::roms::RomScanContext {
            fs: self.fs.as_ref(),
            roms_path: &self.roms_path,
            clock: self.clock.as_ref(),
            definition: &definition,
            profile_folders: self.profile_folders(&id),
        };
        crate::roms::scan_roms(&context, payload)
    }

    /// `scanConfiguredRoms(payload)`.
    pub fn scan_configured_roms(&self, payload: &Value) -> Value {
        let id = payload
            .get("emulatorId")
            .and_then(Value::as_str)
            .map(normalize_id)
            .unwrap_or_default();
        let Some(definition) = self.definition(&id).cloned() else {
            return json!({ "ok": false, "error": "emulador_desconhecido" });
        };
        let context = crate::roms::RomScanContext {
            fs: self.fs.as_ref(),
            roms_path: &self.roms_path,
            clock: self.clock.as_ref(),
            definition: &definition,
            profile_folders: self.profile_folders(&id),
        };
        crate::roms::scan_configured_roms(&context, payload)
    }

    /// `roms()`: índice persistido filtrado pelo catálogo (`emulators:roms:index`).
    pub fn roms(&self) -> Value {
        let context = crate::roms::RomIndexContext {
            fs: self.fs.as_ref(),
            roms_path: &self.roms_path,
            known_definitions: self.catalog_ref(),
        };
        crate::roms::rom_index(&context)
    }

    /// `roms()` embrulhado no envelope do IPC.
    pub fn roms_value(&self) -> Value {
        json!({ "ok": true, "emulators": self.roms() })
    }
}

/// Atalho para `EmulatorRegistry::new`.
pub fn create_registry(options: RegistryOptions) -> EmulatorRegistry {
    EmulatorRegistry::new(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemoryFs;
    use crate::test_support::{chmod_exec, TempDir};
    use std::sync::Arc;

    fn def(id: &str) -> EmulatorDefinition {
        builtin_definitions()
            .into_iter()
            .find(|definition| definition.id == id)
            .expect("builtin")
    }

    #[test]
    fn catalogo_embutido_cobre_os_oito_emuladores() {
        let definitions = builtin_definitions();
        assert_eq!(definitions.len(), 8);
        for id in [
            "pcsx2",
            "rpcs3",
            "dolphin",
            "ppsspp",
            "duckstation",
            "retroarch",
            "melonds",
            "desmume",
        ] {
            let definition = definitions.iter().find(|item| item.id == id).expect(id);
            assert!(!definition.systems.is_empty(), "{id}");
            assert!(!definition.rom_extensions.is_empty(), "{id}");
        }
        let retroarch = def("retroarch");
        assert_eq!(retroarch.requires_core, Some(true));
        assert_eq!(retroarch.systems, vec!["Multi-sistema"]);
        assert_eq!(
            def("rpcs3").rom_directory_markers,
            vec!["PS3_GAME", "ps3_game"]
        );
        assert_eq!(def("dolphin").candidates, vec!["dolphin-emu"]);
        assert_eq!(
            def("pcsx2").flatpak_ids,
            Some(vec!["net.pcsx2.PCSX2".to_string()])
        );
    }

    #[test]
    fn ids_args_e_path_sao_normalizados_sem_shell() {
        assert_eq!(normalize_id(" PCSX2 "), "pcsx2");
        assert_eq!(normalize_id("../bad"), "");
        assert_eq!(normalize_id("__proto__"), "");
        assert_eq!(normalize_id("a"), "");
        assert_eq!(normalize_command("myemu"), "myemu");
        assert_eq!(normalize_command("myemu;rm -rf /"), "");
        assert_eq!(normalize_command("./emu"), "");
        assert!(normalize_args(None).unwrap().is_empty());
        assert_eq!(
            normalize_args(Some(&json!(["--fullscreen", "arquivo com espaço"]))).unwrap(),
            vec!["--fullscreen", "arquivo com espaço"]
        );
        assert!(normalize_args(Some(&json!(["a\0b"]))).is_err());
        assert!(normalize_args(Some(&Value::Null)).is_err());
    }

    #[test]
    fn definicao_customizada_normaliza_extensoes_e_marcadores() {
        let definition = normalize_definition(&json!({
            "id": "myemu",
            "name": "Meu Emulador",
            "description": "Emulador local",
            "systems": ["Sistema próprio"],
            "candidates": ["myemu", "myemu;rm -rf /"],
            "romExtensions": ["foo", ".foo", "bad/ext", "*"],
            "romDirectoryMarkers": ["DISC", "../bad", "nested/name"],
        }))
        .expect("definition");
        assert_eq!(definition.candidates, vec!["myemu"]);
        assert_eq!(definition.rom_extensions, vec![".foo"]);
        assert_eq!(definition.rom_directory_markers, vec!["DISC"]);
        assert_eq!(definition.flatpak_ids, None);
    }

    #[test]
    fn perfil_persiste_atomicamente_e_lista_disponibilidade() {
        let temp = TempDir::new("arcadia-emulators");
        let bin = temp.mkdir("bin");
        let exe = temp.write("bin/pcsx2-qt", b"#!/bin/sh\n");
        chmod_exec(&exe);
        let rom = temp.write("games/demo.iso", b"rom");
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(path_string(&bin));
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);

        let saved = registry.set_profile(&json!({
            "id": "pcsx2",
            "executable": path_string(&exe),
            "biosPath": path_string(&temp.path().join("bios")),
            "romFolders": [{ "path": path_string(&temp.path().join("roms")), "recursive": false }],
            "args": ["--fullscreen"],
        }));
        assert_eq!(saved["ok"], true);
        assert_eq!(saved["profile"]["args"], json!(["--fullscreen"]));
        assert!(temp.path().join("emulators.json").exists());
        assert!(temp
            .read_dir_names()
            .iter()
            .all(|name| !name.contains(".tmp")));

        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .expect("pcsx2");
        assert!(item.available);
        assert_eq!(item.source, ListSource::Configured);
        assert_eq!(item.profile.as_ref().unwrap().bios_path, temp.abs("bios"));
        assert!(!item.profile.as_ref().unwrap().rom_folders[0].recursive);

        let state: Value = serde_json::from_str(
            &std::fs::read_to_string(temp.path().join("emulators.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state["version"], 1);
        assert_eq!(state["profiles"]["pcsx2"]["executable"], path_string(&exe));

        let resolved = registry.get_profile("PCSX2").expect("profile");
        assert_eq!(resolved.args, vec!["--fullscreen"]);
        assert_eq!(registry.remove_profile("pcsx2")["removed"], true);
        assert_eq!(registry.remove_profile("pcsx2")["removed"], false);
        assert!(registry.get_profile("pcsx2").is_none());
        let _ = rom;
    }

    #[test]
    fn perfil_configurado_invalido_nao_cai_em_outro_candidato() {
        let temp = TempDir::new("arcadia-emulators");
        let bin = temp.mkdir("bin");
        temp.write("bin/pcsx2-qt", b"bin");
        chmod_exec(&temp.path().join("bin").join("pcsx2-qt"));
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(path_string(&bin));
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);
        let missing = temp.abs("bin/missing");
        assert_eq!(
            registry.set_profile(&json!({
                "id": "pcsx2",
                "executable": missing,
            }))["ok"],
            true
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(!item.available);
        assert_eq!(item.source, ListSource::Configured);
        assert_eq!(item.executable, missing);
    }

    #[test]
    fn detecta_linux_fora_do_path_e_em_appimage() {
        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/home/.local/bin/pcsx2-qt"), b"binary", 0o755, 1);
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/missing-bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(item.available, "standard path detection");
        assert_eq!(
            item.executable,
            path_string(&normalize_path(Path::new("/home/.local/bin/pcsx2-qt")))
        );
        assert_eq!(item.source, ListSource::Detected);

        let fs = MemoryFs::new();
        fs.add_file_mode(
            Path::new("/home/AppImages/PCSX2-v2.4;safe.AppImage"),
            b"appimage",
            0o755,
            1,
        );
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/missing-bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let detected = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(detected.available);
        assert_eq!(
            detected.executable,
            path_string(&normalize_path(Path::new(
                "/home/AppImages/PCSX2-v2.4;safe.AppImage"
            )))
        );
    }

    #[test]
    fn detecta_flatpak_sem_executar_wrapper() {
        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/flatpak"), b"flatpak", 0o755, 1);
        fs.add_file_mode(
            Path::new("/flatpak/exports/bin/net.pcsx2.PCSX2"),
            b"wrapper",
            0o755,
            1,
        );
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        options.flatpak_roots = vec![PathBuf::from("/flatpak")];
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(item.available);
        assert_eq!(
            item.executable,
            path_string(&normalize_path(Path::new("/bin/flatpak")))
        );
        assert_eq!(
            item.detected_args,
            Some(vec!["run".to_string(), "net.pcsx2.PCSX2".to_string()])
        );
        assert_ne!(
            item.executable,
            path_string(&normalize_path(Path::new(
                "/flatpak/exports/bin/net.pcsx2.PCSX2"
            )))
        );
    }

    #[test]
    fn dolphin_ignora_file_manager_do_kde() {
        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/dolphin"), b"kde", 0o755, 1);
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        assert!(
            !registry
                .list()
                .into_iter()
                .find(|item| item.id() == "dolphin")
                .unwrap()
                .available
        );

        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/dolphin"), b"kde", 0o755, 1);
        fs.add_file_mode(Path::new("/bin/dolphin-emu"), b"emu", 0o755, 1);
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "dolphin")
            .unwrap();
        assert!(item.available);
        assert_eq!(
            item.executable,
            path_string(&normalize_path(Path::new("/bin/dolphin-emu")))
        );
    }

    #[cfg(unix)]
    #[test]
    fn estado_symlink_nao_e_seguido() {
        let temp = TempDir::new("arcadia-emulators");
        let outside = temp.write("outside.json", b"outside");
        let state = temp.path().join("emulators.json");
        std::os::unix::fs::symlink(&outside, &state).unwrap();
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(String::new());
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);
        let result = registry.set_profile(&json!({ "id": "pcsx2", "executable": "pcsx2" }));
        assert_eq!(result["ok"], false);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    }

    #[test]
    fn detecta_executavel_linux_em_symlink_e_nome_uppercase() {
        let fs = MemoryFs::new();
        fs.add_file_mode(Path::new("/bin/PCSX2.real"), b"binary", 0o755, 1);
        fs.add_symlink(Path::new("/bin/PCSX2"), Path::new("PCSX2.real"));
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(item.available);
        assert_eq!(
            item.executable,
            path_string(&normalize_path(Path::new("/bin/PCSX2")))
        );
    }

    #[test]
    fn detecta_appimage_em_home_appimages_minusculo() {
        let fs = MemoryFs::new();
        fs.add_file_mode(
            Path::new("/home/appimages/pcsx2.appimage"),
            b"app",
            0o755,
            1,
        );
        let mut options = RegistryOptions::for_data_dir("/state");
        options.env_path = Some("/missing-bin".into());
        options.home_dir = PathBuf::from("/home");
        options.platform = PlatformKind::Linux;
        let registry = EmulatorRegistry::with_probes(
            options,
            Arc::new(fs),
            Arc::new(crate::fs::ManualClock::new(1)),
        );
        let item = registry
            .list()
            .into_iter()
            .find(|item| item.id() == "pcsx2")
            .unwrap();
        assert!(item.available);
        assert_eq!(item.source, ListSource::Detected);
        assert_eq!(
            item.executable,
            path_string(&normalize_path(Path::new("/home/appimages/pcsx2.appimage")))
        );
    }

    #[test]
    fn limite_de_perfis_usa_cinquenta_e_quatro() {
        let temp = TempDir::new("arcadia-emulators");
        let extras: Vec<Value> = (0..65)
            .map(|index| {
                json!({
                    "id": format!("emu{index:02}"),
                    "name": format!("Emu {index}"),
                    "systems": ["Sistema"],
                    "candidates": ["emu"],
                })
            })
            .collect();
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(temp.abs("bin"));
        options.home_dir = temp.path().join("home");
        options.extra_definitions = extras;
        let registry = EmulatorRegistry::new(options);
        for index in 0..64 {
            let result = registry.set_profile(&json!({
                "id": format!("emu{index:02}"),
                "executable": "emu",
            }));
            assert_eq!(result["ok"], true, "perfil {index}");
        }
        let blocked = registry.set_profile(&json!({ "id": "emu64", "executable": "emu" }));
        assert_eq!(blocked["ok"], false);
        assert_eq!(blocked["error"], "perfis_emuladores_limite");
        assert_eq!(registry.profiles().len(), 64);
    }

    #[test]
    fn estado_invalido_e_ignorado_e_updated_at_e_truncado() {
        let temp = TempDir::new("arcadia-emulators");
        std::fs::write(
            temp.path().join("emulators.json"),
            r#"{"version":2,"profiles":{"pcsx2":{"executable":"pcsx2"}}}"#,
        )
        .unwrap();
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(temp.abs("bin"));
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);
        assert!(registry.get_profile("pcsx2").is_none());

        std::fs::write(
            temp.path().join("emulators.json"),
            r#"{"version":1,"profiles":{"pcsx2":{"executable":"pcsx2","args":"bad"}}}"#,
        )
        .unwrap();
        assert!(registry.get_profile("pcsx2").is_none());

        std::fs::write(
            temp.path().join("emulators.json"),
            r#"{"version":1,"profiles":{"pcsx2":{"id":"wrong","executable":"PCSX2","args":[],"corePath":"","biosPath":"","romFolders":[],"updatedAt":"42.9"}}}"#,
        )
        .unwrap();
        let profile = registry.get_profile("pcsx2").expect("profile");
        assert_eq!(profile.id, "pcsx2");
        assert_eq!(profile.updated_at, 42);
    }

    #[test]
    fn detect_envolve_o_catalogo_e_erros_de_perfil() {
        let temp = TempDir::new("arcadia-emulators");
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(temp.abs("bin"));
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);
        let detect = registry.detect();
        assert_eq!(detect["ok"], true);
        assert_eq!(detect["emulators"].as_array().unwrap().len(), 8);
        assert_eq!(registry.list_envelope()["ok"], true);
        assert_eq!(registry.profiles_envelope()["ok"], true);
        assert_eq!(
            registry.set_profile(&json!({ "id": "unknown", "executable": "x" }))["error"],
            "emulador_desconhecido"
        );
        assert_eq!(
            registry.set_profile(&Value::Null)["error"],
            "perfil_invalido"
        );
        assert_eq!(
            registry.remove_profile("unknown")["error"],
            "emulador_desconhecido"
        );
        assert_eq!(
            registry.set_profile(&json!({ "id": "pcsx2", "executable": "pcsx2;rm -rf /" }))
                ["error"],
            "executavel_invalido"
        );
        assert_eq!(
            registry.set_profile(&json!({
                "id": "pcsx2",
                "executable": "pcsx2",
                "corePath": null,
            }))["error"],
            "corePath_invalido"
        );
        assert_eq!(
            registry.set_profile(&json!({
                "id": "pcsx2",
                "executable": "pcsx2",
                "romFolders": null,
            }))["error"],
            "rom_folders_invalidas"
        );
    }

    #[test]
    fn paths_respeita_profiles_path_e_data_dir_padrao() {
        let temp = TempDir::new("arcadia-emulators");
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.profiles_path = Some(temp.path().join("custom/emulators.json"));
        options.env_path = Some(temp.abs("bin"));
        options.home_dir = temp.path().join("home");
        let registry = EmulatorRegistry::new(options);
        assert_eq!(
            registry.paths()["statePath"],
            temp.abs("custom/emulators.json")
        );
        assert_eq!(registry.rom_paths()["romsPath"], temp.abs("roms.json"));

        let mut env = BTreeMap::new();
        env.insert(
            "LOCALAPPDATA".to_string(),
            "C:\\Users\\x\\AppData\\Local".to_string(),
        );
        // O separador de `PathBuf::join` é o do host, não o da plataforma
        // simulada: comparar com a string Windows literal quebra no Linux.
        // Montar o esperado com `join` preserva a intenção (base + "arcadia")
        // e roda em qualquer host.
        assert_eq!(
            path_string(&normalize_path(&default_data_dir(
                PlatformKind::Windows,
                &env,
                Path::new("C:\\Users\\x")
            ))),
            path_string(&normalize_path(
                &PathBuf::from("C:\\Users\\x\\AppData\\Local").join("arcadia")
            ))
        );
        assert_eq!(
            path_string(&normalize_path(&default_data_dir(
                PlatformKind::Linux,
                &env,
                Path::new("/home/x")
            ))),
            path_string(&normalize_path(Path::new("/home/x/.local/share/arcadia")))
        );
        env.insert("ARCADIA_DATA_DIR".to_string(), "   ".to_string());
        assert!(resolve_data_dir(&env, PlatformKind::Linux, Path::new("/home/x")).has_root());
        env.insert("ARCADIA_DATA_DIR".to_string(), "/custom/state".to_string());
        assert_eq!(
            resolve_data_dir(&env, PlatformKind::Linux, Path::new("/home/x")),
            PathBuf::from("/custom/state")
        );
    }
}
