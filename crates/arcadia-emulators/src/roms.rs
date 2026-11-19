//! Scanner e índice de ROMs (`roms.json`).
//!
//! Port 1:1 de `scanRoms`/`scanConfiguredRoms`/`roms` de
//! `app/electron/emulator-registry.js`. A varredura é somente leitura: usa
//! allowlist de extensões por emulador, ignora symlinks e nunca executa o
//! conteúdo encontrado. O índice persiste em `roms.json` com escrita atômica.
//!
//! O scanner congelado não lê bytes das ROMs (portanto não calcula SHA-1/CRC):
//! a identidade de um item é o caminho absoluto normalizado ([`rom_key`]) mais
//! extensão/kind. Hashes de conteúdo pertencem ao catálogo retrô.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::fs::{
    basename, basename_without_extension, has_symlink_component, is_directory, js_integer,
    js_locale_cmp, js_lower, js_number, js_slice, js_truthy, normalize_absolute, normalize_path,
    path_string, Clock, EmulatorFs, FsKind,
};
use crate::registry::{
    atomic_write, normalize_id, normalize_rom_directory_markers, normalize_rom_extension,
    normalize_rom_folders, RomFolder, ROMS_VERSION,
};

pub const MAX_SCAN_DEPTH: usize = 8;
pub const MAX_SCAN_RESULTS: usize = 4096;

/// Extensões aceitas pelos scanners embutidos (allowlist).
pub const BUILTIN_ROM_EXTENSIONS: &[(&str, &[&str])] = &[
    (
        "pcsx2",
        &[
            ".iso", ".chd", ".cso", ".zso", ".gz", ".nrg", ".cue", ".bin", ".mds", ".mdf", ".m3u",
        ],
    ),
    ("rpcs3", &[".iso", ".pkg", ".elf", ".self"]),
    (
        "dolphin",
        &[
            ".iso",
            ".rvz",
            ".wbfs",
            ".wia",
            ".gcz",
            ".ciso",
            ".nkit.iso",
            ".wad",
        ],
    ),
    ("ppsspp", &[".iso", ".cso", ".chd", ".pbp", ".elf"]),
    (
        "duckstation",
        &[
            ".cue", ".bin", ".iso", ".chd", ".pbp", ".img", ".sub", ".ccd", ".mds", ".mdf", ".ecm",
            ".m3u",
        ],
    ),
    (
        "retroarch",
        &[
            ".zip", ".7z", ".nes", ".unf", ".unif", ".gb", ".gbc", ".gba", ".gbx", ".nds", ".dsi",
            ".smc", ".sfc", ".fig", ".swc", ".n64", ".z64", ".v64", ".md", ".gen", ".smd", ".sms",
            ".gg", ".32x", ".pce", ".cue", ".bin", ".iso", ".chd", ".pbp", ".m3u", ".img", ".wbfs",
        ],
    ),
    ("melonds", &[".nds", ".dsi"]),
    ("desmume", &[".nds", ".dsi"]),
];

/// Extensões de sidecar aceitas para uma extensão primária.
pub type SidecarMapping = (&'static str, &'static [&'static str]);
/// Regras de sidecar de um emulador.
pub type EmulatorSidecarRules = &'static [SidecarMapping];

/// Sidecars agrupados sob a imagem primária (`ROM_SIDECAR_RULES`).
pub const ROM_SIDECAR_RULES: &[(&str, EmulatorSidecarRules)] = &[
    ("pcsx2", &[(".cue", &[".bin"]), (".mds", &[".mdf"])]),
    (
        "duckstation",
        &[
            (".cue", &[".bin"]),
            (".ccd", &[".img", ".sub"]),
            (".mds", &[".mdf"]),
        ],
    ),
];

pub fn builtin_rom_extensions(emulator_id: &str) -> Option<&'static [&'static str]> {
    BUILTIN_ROM_EXTENSIONS
        .iter()
        .find(|(id, _)| *id == emulator_id)
        .map(|(_, extensions)| *extensions)
}

pub fn rom_sidecar_rules(emulator_id: &str) -> EmulatorSidecarRules {
    ROM_SIDECAR_RULES
        .iter()
        .find(|(id, _)| *id == emulator_id)
        .map(|(_, rules)| *rules)
        .unwrap_or(&[])
}

/// Chave estável de identidade/deduplicação: caminho absoluto normalizado.
pub fn rom_key(path: &Path) -> String {
    path_string(&normalize_path(path))
}

/// Item de ROM no resultado do scan e no índice (`EmulatorRomEntry`).
///
/// `mtime_ms` é truncado para milissegundos inteiros; o cliente Electron expõe
/// `Stats#mtimeMs` fracionário, mas nenhum consumidor do contrato usa a fração.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RomEntry {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    pub extension: String,
    pub kind: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub sidecars: Vec<String>,
}

impl RomEntry {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Alias do oráculo para itens persistidos.
pub type StoredRom = RomEntry;

/// Alias para o formato de pasta dos perfis.
pub type RomFolderEntry = RomFolder;

/// Contexto do scanner: probes injetados, definição e pastas do perfil.
pub struct RomScanContext<'a> {
    pub fs: &'a dyn EmulatorFs,
    pub roms_path: &'a Path,
    pub clock: &'a dyn Clock,
    pub definition: &'a crate::registry::EmulatorDefinition,
    pub profile_folders: Vec<RomFolder>,
}

/// Contexto do índice (`emulators:roms:index`).
pub struct RomIndexContext<'a> {
    pub fs: &'a dyn EmulatorFs,
    pub roms_path: &'a Path,
    pub known_definitions: &'a [crate::registry::EmulatorDefinition],
}

/// Opções tipadas; a entrada IPC-exata é [`scan_roms`], que recebe `Value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomScanOptions {
    pub recursive: bool,
    pub max_depth: usize,
    pub max_results: usize,
    pub directory: Option<String>,
    pub folders: Vec<RomFolder>,
}

impl Default for RomScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            max_depth: MAX_SCAN_DEPTH,
            max_results: MAX_SCAN_RESULTS,
            directory: None,
            folders: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Estado roms.json e normalização persistida
// ---------------------------------------------------------------------------

fn read_rom_index(file: &Path, fs: &dyn EmulatorFs) -> Value {
    let empty = || json!({ "version": ROMS_VERSION, "emulators": {} });
    if has_symlink_component(file, fs) {
        return empty();
    }
    let Some(meta) = fs.lstat(file) else {
        return empty();
    };
    if meta.kind != FsKind::File {
        return empty();
    }
    let Some(bytes) = fs.read(file) else {
        return empty();
    };
    let text = String::from_utf8_lossy(&bytes);
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return empty();
    };
    let Some(object) = parsed.as_object() else {
        return empty();
    };
    if object.get("version").and_then(Value::as_f64) != Some(ROMS_VERSION as f64) {
        return empty();
    }
    let Some(Value::Object(emulators)) = object.get("emulators") else {
        return empty();
    };
    json!({ "version": ROMS_VERSION, "emulators": Value::Object(emulators.clone()) })
}

fn number_value(number: f64) -> Value {
    if number.is_finite()
        && number.fract() == 0.0
        && number >= i64::MIN as f64
        && number <= i64::MAX as f64
    {
        return Value::Number(serde_json::Number::from(number as i64));
    }
    serde_json::Number::from_f64(number)
        .map(Value::Number)
        .unwrap_or(Value::Number(serde_json::Number::from(0)))
}

/// `normalizeStoredRom`: normaliza um item vindo do índice/scan.
pub fn normalize_stored_rom(item: &Value) -> Option<RomEntry> {
    let object = item.as_object()?;
    let path_value = object.get("path").and_then(Value::as_str).unwrap_or("");
    let path = normalize_absolute(path_value, "romPath").ok()?;
    let fallback_name = basename(&path);
    let name = match object.get("name") {
        Some(Value::String(name)) => js_slice(name, 512),
        _ => fallback_name.clone(),
    };
    let relative_path = match object.get("relativePath") {
        Some(Value::String(value)) => js_slice(value, 2048),
        _ => fallback_name,
    };
    let extension = match object.get("extension") {
        Some(Value::String(value)) => js_slice(value, 32),
        _ => String::new(),
    };
    let kind = if object.get("kind").and_then(Value::as_str) == Some("directory") {
        "directory".to_string()
    } else {
        "file".to_string()
    };
    let size_bytes = object
        .get("sizeBytes")
        .and_then(js_number)
        .filter(|number| number.is_finite())
        .map(|number| number.max(0.0) as u64)
        .unwrap_or(0);
    let mtime_ms = object
        .get("mtimeMs")
        .and_then(js_number)
        .filter(|number| number.is_finite())
        .map(|number| number.max(0.0))
        .map(|number| {
            if number > i64::MAX as f64 {
                i64::MAX
            } else {
                number as i64
            }
        })
        .unwrap_or(0);
    let sidecars = object
        .get("sidecars")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|value| normalize_absolute(value, "sidecar").ok())
                .map(|path| path_string(&path))
                .take(16)
                .collect()
        })
        .unwrap_or_default();
    Some(RomEntry {
        path: path_string(&path),
        name,
        relative_path,
        extension,
        kind,
        size_bytes,
        mtime_ms,
        sidecars,
    })
}

/// `saveRomScan`: grava o resultado no índice e devolve `persisted`.
pub fn save_rom_scan(ctx: &RomScanContext<'_>, emulator_id: &str, result: &Value) -> bool {
    let mut state = read_rom_index(ctx.roms_path, ctx.fs);
    let roms: Vec<Value> = result
        .get("roms")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(normalize_stored_rom)
                .map(|rom| serde_json::to_value(rom).unwrap_or(Value::Null))
                .take(MAX_SCAN_RESULTS)
                .collect()
        })
        .unwrap_or_default();
    let folders = match result.get("folders").and_then(Value::as_array) {
        Some(folders) if !folders.is_empty() => Value::Array(folders.clone()),
        _ => serde_json::to_value(&ctx.profile_folders).unwrap_or(Value::Null),
    };
    let entry = json!({
        "updatedAt": ctx.clock.now_ms(),
        "directory": result.get("directory").and_then(Value::as_str).unwrap_or(""),
        "folders": folders,
        "truncated": result.get("truncated").map(js_truthy).unwrap_or(false),
        "roms": roms,
    });
    let Some(emulators) = state.get_mut("emulators").and_then(Value::as_object_mut) else {
        return false;
    };
    emulators.insert(emulator_id.to_string(), entry);
    atomic_write(ctx.roms_path, &state, ctx.fs, ctx.clock).is_ok()
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

fn relative_path(base: &Path, file: &Path) -> String {
    file.strip_prefix(base)
        .map(path_string)
        .unwrap_or_else(|_| path_string(file))
}

fn directory_size(directory: &Path, fs: &dyn EmulatorFs) -> u64 {
    let mut total: u64 = 0;
    let mut pending: VecDeque<PathBuf> = VecDeque::from([directory.to_path_buf()]);
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut seen_entries = 0usize;
    while let Some(current) = pending.pop_front() {
        if seen_entries >= 100_000 {
            break;
        }
        let key = rom_key(&current);
        if !visited.insert(key) || has_symlink_component(&current, fs) {
            continue;
        }
        let Some(children) = fs.read_dir(&current) else {
            continue;
        };
        for child in children {
            seen_entries += 1;
            if seen_entries > 100_000 {
                break;
            }
            if child.name.is_empty() || child.is_symlink() {
                continue;
            }
            let nested = current.join(&child.name);
            if child.kind == FsKind::Directory {
                pending.push_back(nested);
            } else if child.kind == FsKind::File && !has_symlink_component(&nested, fs) {
                if let Some(meta) = fs.lstat(&nested) {
                    total += meta.size;
                }
            }
        }
    }
    total
}

struct Candidate {
    path: String,
    name: String,
    relative_path: String,
    extension: String,
    size_bytes: u64,
    mtime_ms: i64,
    directory: String,
    is_directory_marker: bool,
}

fn allowed_extensions(definition: &crate::registry::EmulatorDefinition) -> Vec<String> {
    let mut allowed: Vec<String> = Vec::new();
    for extension in &definition.rom_extensions {
        let normalized = normalize_rom_extension(extension);
        if !normalized.is_empty() && !allowed.contains(&normalized) {
            allowed.push(normalized);
        }
    }
    allowed
}

fn selected_extension(name: &str, allowed: &[String]) -> Option<String> {
    let lower = js_lower(name);
    let mut sorted = allowed.to_vec();
    sorted.sort_by_key(|extension| std::cmp::Reverse(extension.len()));
    sorted
        .into_iter()
        .find(|extension| lower.ends_with(extension))
}

fn parse_scan_controls(payload: &Value) -> Result<(bool, usize, usize), String> {
    let recursive = match payload.get("recursive") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("opcoes_scan_invalidas".to_string()),
    };
    let max_depth = match payload.get("maxDepth") {
        None => MAX_SCAN_DEPTH,
        Some(value) => {
            let value = js_integer(value).ok_or_else(|| "opcoes_scan_invalidas".to_string())?;
            if !(0..=MAX_SCAN_DEPTH as i64).contains(&value) {
                return Err("opcoes_scan_invalidas".to_string());
            }
            value as usize
        }
    };
    let max_results = match payload.get("maxResults") {
        None => MAX_SCAN_RESULTS,
        Some(value) => {
            let value = js_integer(value).ok_or_else(|| "opcoes_scan_invalidas".to_string())?;
            if value < 1 || value > MAX_SCAN_RESULTS as i64 {
                return Err("opcoes_scan_invalidas".to_string());
            }
            value as usize
        }
    };
    Ok((recursive, max_depth, max_results))
}

fn selected_directory(payload: &Value) -> Option<&Value> {
    ["directory", "rootPath", "folderPath"]
        .iter()
        .find_map(|key| payload.get(*key).filter(|value| js_truthy(value)))
}

fn scan_directory(
    ctx: &RomScanContext<'_>,
    directory_value: &Value,
    recursive: bool,
    max_depth: usize,
    max_results: usize,
) -> Value {
    let Some(directory_text) = directory_value.as_str() else {
        return json!({ "ok": false, "error": "diretorio_rom_invalido" });
    };
    let normalized = match normalize_absolute(directory_text, "directory") {
        Ok(path) if is_directory(&path, ctx.fs) => path,
        _ => return json!({ "ok": false, "error": "diretorio_rom_invalido" }),
    };
    let definition = ctx.definition;
    let allowed = allowed_extensions(definition);
    let markers = normalize_rom_directory_markers(&Value::Array(
        definition
            .rom_directory_markers
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    ));
    let candidate_limit = MAX_SCAN_RESULTS.min((max_results + 32).max(max_results * 4));
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::from([(normalized.clone(), 0)]);
    let mut seen_directories: BTreeSet<String> = BTreeSet::new();
    let mut capped = false;

    while !queue.is_empty() && candidates.len() < candidate_limit {
        let Some((current, depth)) = queue.pop_front() else {
            break;
        };
        let current_key = rom_key(&current);
        if !seen_directories.insert(current_key) {
            continue;
        }
        let Some(mut entries) = ctx.fs.read_dir(&current) else {
            continue;
        };
        entries.sort_by(|left, right| js_locale_cmp(&left.name, &right.name));
        for entry in entries {
            if candidates.len() >= candidate_limit {
                capped = true;
                break;
            }
            if entry.name.is_empty() || entry.name == "." || entry.name == ".." {
                continue;
            }
            let file = current.join(&entry.name);
            if entry.is_symlink() || has_symlink_component(&file, ctx.fs) {
                continue;
            }
            let Some(meta) = ctx.fs.lstat(&file) else {
                continue;
            };
            if meta.kind == FsKind::Directory {
                if markers.contains(&entry.name) {
                    let size_bytes = directory_size(&file, ctx.fs);
                    candidates.push(Candidate {
                        path: rom_key(&file),
                        name: entry.name.clone(),
                        relative_path: relative_path(&normalized, &file),
                        extension: String::new(),
                        size_bytes,
                        mtime_ms: meta.mtime_ms,
                        directory: path_string(&current),
                        is_directory_marker: true,
                    });
                    continue;
                }
                if recursive && depth < max_depth {
                    queue.push_back((file, depth + 1));
                }
                continue;
            }
            if meta.kind != FsKind::File {
                continue;
            }
            let Some(extension) = selected_extension(&entry.name, &allowed) else {
                continue;
            };
            candidates.push(Candidate {
                path: rom_key(&file),
                name: entry.name.clone(),
                relative_path: relative_path(&normalized, &file),
                extension,
                size_bytes: meta.size,
                mtime_ms: meta.mtime_ms,
                directory: path_string(&current),
                is_directory_marker: false,
            });
        }
    }
    if candidates.len() >= candidate_limit {
        capped = true;
    }

    let rules = rom_sidecar_rules(&definition.id);
    let mut suppressed: BTreeSet<String> = BTreeSet::new();
    let mut sidecars: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (primary_index, primary) in candidates.iter().enumerate() {
        let primary_extension = js_lower(&crate::fs::extname(&primary.name));
        let Some(sidecar_extensions) = rules
            .iter()
            .find(|(extension, _)| *extension == primary_extension)
            .map(|(_, extensions)| *extensions)
        else {
            continue;
        };
        let primary_stem = js_lower(&basename_without_extension(
            &primary.name,
            &crate::fs::extname(&primary.name),
        ));
        for (sidecar_index, sidecar) in candidates.iter().enumerate() {
            if sidecar_index == primary_index {
                continue;
            }
            if sidecar.directory != primary.directory {
                continue;
            }
            let sidecar_extension = js_lower(&crate::fs::extname(&sidecar.name));
            let sidecar_stem = js_lower(&basename_without_extension(
                &sidecar.name,
                &crate::fs::extname(&sidecar.name),
            ));
            if sidecar_stem != primary_stem
                || !sidecar_extensions.contains(&sidecar_extension.as_str())
            {
                continue;
            }
            suppressed.insert(sidecar.path.clone());
            sidecars
                .entry(primary.path.clone())
                .or_default()
                .push(sidecar_index);
        }
    }

    let playlist_directories: BTreeSet<String> = candidates
        .iter()
        .filter(|candidate| candidate.extension == ".m3u")
        .map(|candidate| candidate.directory.clone())
        .collect();
    let mut roms: Vec<RomEntry> = candidates
        .iter()
        .filter(|candidate| {
            if suppressed.contains(&candidate.path) {
                return false;
            }
            !(playlist_directories.contains(&candidate.directory)
                && candidate.extension != ".m3u"
                && !candidate.is_directory_marker)
        })
        .map(|candidate| {
            let attached = sidecars.get(&candidate.path).cloned().unwrap_or_default();
            let mut sidecar_paths: Vec<String> = attached
                .iter()
                .map(|index| candidates[*index].path.clone())
                .collect();
            sidecar_paths.sort();
            RomEntry {
                path: candidate.path.clone(),
                name: candidate.name.clone(),
                relative_path: candidate.relative_path.clone(),
                extension: candidate.extension.clone(),
                kind: if candidate.is_directory_marker {
                    "directory".to_string()
                } else {
                    "file".to_string()
                },
                size_bytes: candidate.size_bytes
                    + attached
                        .iter()
                        .map(|index| candidates[*index].size_bytes)
                        .sum::<u64>(),
                mtime_ms: candidate.mtime_ms,
                sidecars: sidecar_paths,
            }
        })
        .collect();
    roms.sort_by(|left, right| js_locale_cmp(&left.path, &right.path));
    let truncated = capped || roms.len() > max_results;
    roms.truncate(max_results);

    let mut result = Map::new();
    result.insert("ok".into(), Value::Bool(true));
    result.insert("emulatorId".into(), Value::String(definition.id.clone()));
    result.insert("directory".into(), Value::String(path_string(&normalized)));
    result.insert("recursive".into(), Value::Bool(recursive));
    result.insert("maxDepth".into(), json!(max_depth));
    result.insert(
        "romExtensions".into(),
        Value::Array(allowed.iter().cloned().map(Value::String).collect()),
    );
    result.insert(
        "romDirectoryMarkers".into(),
        Value::Array(markers.iter().cloned().map(Value::String).collect()),
    );
    result.insert("truncated".into(), Value::Bool(truncated));
    result.insert(
        "roms".into(),
        serde_json::to_value(&roms).unwrap_or(Value::Array(Vec::new())),
    );
    let persisted = save_rom_scan(ctx, &definition.id, &Value::Object(result.clone()));
    result.insert("persisted".into(), Value::Bool(persisted));
    Value::Object(result)
}

fn configured_folders(ctx: &RomScanContext<'_>) -> Vec<RomFolder> {
    ctx.profile_folders.clone()
}

fn scan_configured(
    ctx: &RomScanContext<'_>,
    recursive: bool,
    max_depth: usize,
    max_results: usize,
) -> Value {
    let folders = configured_folders(ctx);
    let mut merged: BTreeMap<String, RomEntry> = BTreeMap::new();
    let mut truncated = false;
    let mut rom_extensions: Vec<String> = Vec::new();
    for folder in &folders {
        let payload = json!({
            "emulatorId": ctx.definition.id,
            "directory": folder.path,
            "recursive": folder.recursive && recursive,
            "maxDepth": max_depth,
            "maxResults": max_results,
        });
        let result = scan_roms(ctx, &payload);
        if result.get("ok") != Some(&Value::Bool(true)) {
            continue;
        }
        if let Some(extensions) = result.get("romExtensions").and_then(Value::as_array) {
            rom_extensions = extensions
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect();
        }
        if result.get("truncated").and_then(Value::as_bool) == Some(true) {
            truncated = true;
        }
        if let Some(roms) = result.get("roms").and_then(Value::as_array) {
            for rom in roms {
                if let Some(entry) = normalize_stored_rom(rom) {
                    merged.entry(entry.path.clone()).or_insert(entry);
                }
            }
        }
        if merged.len() >= max_results {
            truncated = true;
            break;
        }
    }
    let mut roms: Vec<RomEntry> = merged.into_values().collect();
    roms.sort_by(|left, right| js_locale_cmp(&left.path, &right.path));
    roms.truncate(max_results);

    let markers = normalize_rom_directory_markers(&Value::Array(
        ctx.definition
            .rom_directory_markers
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    ));
    let mut result = Map::new();
    result.insert("ok".into(), Value::Bool(true));
    result.insert(
        "emulatorId".into(),
        Value::String(ctx.definition.id.clone()),
    );
    result.insert("recursive".into(), Value::Bool(recursive));
    result.insert("maxDepth".into(), json!(max_depth));
    result.insert(
        "romExtensions".into(),
        Value::Array(rom_extensions.into_iter().map(Value::String).collect()),
    );
    result.insert(
        "romDirectoryMarkers".into(),
        Value::Array(markers.into_iter().map(Value::String).collect()),
    );
    result.insert("truncated".into(), Value::Bool(truncated));
    result.insert(
        "folders".into(),
        serde_json::to_value(&folders).unwrap_or(Value::Array(Vec::new())),
    );
    result.insert(
        "roms".into(),
        serde_json::to_value(&roms).unwrap_or(Value::Array(Vec::new())),
    );
    let persisted = save_rom_scan(ctx, &ctx.definition.id, &Value::Object(result.clone()));
    result.insert("persisted".into(), Value::Bool(persisted));
    Value::Object(result)
}

/// `scanRoms(payload)` — diretório explícito ou pastas configuradas.
pub fn scan_roms(ctx: &RomScanContext<'_>, payload: &Value) -> Value {
    let id = payload
        .get("emulatorId")
        .and_then(Value::as_str)
        .map(normalize_id)
        .unwrap_or_default();
    if id.is_empty() || id != ctx.definition.id {
        return json!({ "ok": false, "error": "emulador_desconhecido" });
    }
    let (recursive, max_depth, max_results) = match parse_scan_controls(payload) {
        Ok(values) => values,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let Some(directory) = selected_directory(payload) else {
        return scan_configured(ctx, recursive, max_depth, max_results);
    };
    scan_directory(ctx, directory, recursive, max_depth, max_results)
}

/// Variante tipada (valores já validados pela camada chamadora).
pub fn scan_roms_options(ctx: &RomScanContext<'_>, options: &RomScanOptions) -> Value {
    let mut payload = json!({
        "emulatorId": ctx.definition.id,
        "recursive": options.recursive,
        "maxDepth": options.max_depth,
        "maxResults": options.max_results,
    });
    if let Some(directory) = &options.directory {
        payload["directory"] = Value::String(directory.clone());
    }
    scan_roms(ctx, &payload)
}

/// `scanConfiguredRoms(payload)`.
pub fn scan_configured_roms(ctx: &RomScanContext<'_>, payload: &Value) -> Value {
    let (recursive, max_depth, max_results) = match parse_scan_controls(payload) {
        Ok(values) => values,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    scan_configured(ctx, recursive, max_depth, max_results)
}

/// `roms()`: índice filtrado pelo catálogo conhecido.
pub fn rom_index(ctx: &RomIndexContext<'_>) -> Value {
    let state = read_rom_index(ctx.roms_path, ctx.fs);
    let mut result = Map::new();
    let Some(emulators) = state.get("emulators").and_then(Value::as_object) else {
        return Value::Object(result);
    };
    for (id, entry) in emulators {
        if !ctx
            .known_definitions
            .iter()
            .any(|definition| definition.id == *id)
        {
            continue;
        }
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let folders = normalize_rom_folders(entry.get("folders")).unwrap_or_default();
        let updated_at = entry
            .get("updatedAt")
            .and_then(js_number)
            .filter(|number| number.is_finite())
            .map(number_value)
            .unwrap_or_else(|| Value::Number(serde_json::Number::from(0)));
        let directory = entry
            .get("directory")
            .and_then(Value::as_str)
            .and_then(|value| normalize_absolute(value, "directory").ok())
            .map(|path| path_string(&path))
            .unwrap_or_default();
        let truncated = entry.get("truncated").map(js_truthy).unwrap_or(false);
        let roms: Vec<Value> = entry
            .get("roms")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(normalize_stored_rom)
                    .map(|rom| serde_json::to_value(rom).unwrap_or(Value::Null))
                    .take(MAX_SCAN_RESULTS)
                    .collect()
            })
            .unwrap_or_default();
        let mut item = Map::new();
        item.insert("updatedAt".into(), updated_at);
        item.insert("directory".into(), Value::String(directory));
        item.insert(
            "folders".into(),
            serde_json::to_value(&folders).unwrap_or(Value::Array(Vec::new())),
        );
        item.insert("truncated".into(), Value::Bool(truncated));
        item.insert("roms".into(), Value::Array(roms));
        result.insert(id.clone(), Value::Object(item));
    }
    Value::Object(result)
}

/// Extensão efetiva de um nome, normalizada pelo allowlist do catálogo.
pub fn matched_rom_extension(
    name: &str,
    definition: &crate::registry::EmulatorDefinition,
) -> Option<String> {
    selected_extension(name, &allowed_extensions(definition))
}

/// Informa se um nome é aceito pelo allowlist (ou se o catálogo aceita tudo).
pub fn rom_extension_allowed(name: &str, definition: &crate::registry::EmulatorDefinition) -> bool {
    let allowed = allowed_extensions(definition);
    allowed.is_empty() || selected_extension(name, &allowed).is_some()
}

/// `true` se o caminho é absoluto e pode ser usado como chave de índex.
pub fn is_rom_index_path(value: &str) -> bool {
    normalize_absolute(value, "romPath").is_ok()
}

/// Contagem de ROMs do índice (helper de UI/teste).
pub fn indexed_rom_count(value: &Value) -> usize {
    value
        .as_object()
        .map(|emulators| {
            emulators
                .values()
                .filter_map(|entry| entry.get("roms").and_then(Value::as_array))
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0)
}

/// Representa um scan vazio (útil para chamadas sem pastas configuradas).
pub fn empty_scan_result(emulator_id: &str) -> Value {
    json!({
        "ok": true,
        "emulatorId": emulator_id,
        "recursive": true,
        "maxDepth": MAX_SCAN_DEPTH,
        "romExtensions": [],
        "romDirectoryMarkers": [],
        "truncated": false,
        "folders": [],
        "roms": [],
        "persisted": false,
    })
}

/// Normaliza `js_trim` de extensões para consumidores que montam o payload.
pub fn normalized_extension(value: &str) -> String {
    normalize_rom_extension(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{ManualClock, MemoryFs, SystemFs};
    use crate::registry::{builtin_definitions, EmulatorRegistry, RegistryOptions};
    use crate::test_support::TempDir;
    use std::sync::Arc;

    fn registry(temp: &TempDir, extras: Vec<Value>) -> EmulatorRegistry {
        let mut options = RegistryOptions::for_data_dir(temp.path());
        options.env_path = Some(temp.abs("bin"));
        options.home_dir = temp.path().join("home");
        options.extra_definitions = extras;
        EmulatorRegistry::with_probes(
            options,
            Arc::new(SystemFs),
            Arc::new(ManualClock::new(1_700_000_000_000)),
        )
    }

    fn relative_paths(result: &Value) -> Vec<String> {
        result["roms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rom| {
                rom["relativePath"]
                    .as_str()
                    .unwrap()
                    .split(['/', '\\'])
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect()
    }

    #[test]
    fn scan_usa_allowlist_recursao_e_agrupa_sidecars() {
        let temp = TempDir::new("arcadia-roms");
        temp.mkdir("roms/nested");
        temp.write("roms/Game.cue", b"cue");
        let bin = temp.write("roms/Game.bin", b"bin");
        temp.write("roms/nested/Other.ISO", b"iso");
        temp.write("roms/readme.txt", b"ignore");
        let registry = registry(&temp, Vec::new());
        let result = registry.scan_roms(&json!({
            "emulatorId": "pcsx2",
            "directory": temp.abs("roms"),
        }));
        assert_eq!(result["ok"], true);
        assert_eq!(
            relative_paths(&result),
            vec!["Game.cue", "nested/Other.ISO"]
        );
        assert_eq!(result["roms"][0]["sidecars"], json!([path_string(&bin)]));
        assert_eq!(result["roms"][0]["sizeBytes"], 6);
        assert_eq!(
            result["romExtensions"],
            json!([
                ".iso", ".chd", ".cso", ".zso", ".gz", ".nrg", ".cue", ".bin", ".mds", ".mdf",
                ".m3u"
            ])
        );
        assert_eq!(result["persisted"], true);
        let index = registry.roms();
        assert_eq!(index["pcsx2"]["roms"][0]["path"], temp.abs("roms/Game.cue"));
    }

    #[test]
    fn scan_limita_profundidade_resultados_e_extensoes_customizadas() {
        let temp = TempDir::new("arcadia-roms");
        temp.mkdir("roms/one/two");
        temp.write("roms/a.foo", b"a");
        temp.write("roms/b.FOO", b"b");
        temp.write("roms/one/two/deep.foo", b"deep");
        let registry = registry(
            &temp,
            vec![json!({
                "id": "customscan",
                "name": "Scanner",
                "systems": ["Sistema"],
                "candidates": ["scanner"],
                "romExtensions": ["foo", ".foo", "bad/ext", "*"],
                "romDirectoryMarkers": ["DISC", "../bad", "nested/name"],
            })],
        );
        assert_eq!(
            registry
                .definitions()
                .into_iter()
                .find(|definition| definition.id == "customscan")
                .unwrap()
                .rom_extensions,
            vec![".foo"]
        );
        let shallow = registry.scan_roms(&json!({
            "emulatorId": "customscan",
            "directory": temp.abs("roms"),
            "maxDepth": 0,
        }));
        assert_eq!(relative_paths(&shallow), vec!["a.foo", "b.FOO"]);
        let limited = registry.scan_roms(&json!({
            "emulatorId": "customscan",
            "directory": temp.abs("roms"),
            "maxResults": 1,
        }));
        assert_eq!(limited["roms"].as_array().unwrap().len(), 1);
        assert_eq!(limited["truncated"], true);
        assert_eq!(
            registry.scan_roms(&json!({
                "emulatorId": "customscan",
                "directory": temp.abs("roms"),
                "maxDepth": 99,
            }))["error"],
            "opcoes_scan_invalidas"
        );
        assert_eq!(
            registry.scan_roms(&json!({
                "emulatorId": "customscan",
                "directory": temp.abs("missing"),
            }))["error"],
            "diretorio_rom_invalido"
        );
        assert_eq!(
            registry.scan_roms(&json!({
                "emulatorId": "customscan",
                "directory": temp.abs("roms"),
                "recursive": "yes",
            }))["error"],
            "opcoes_scan_invalidas"
        );
        assert_eq!(
            registry.scan_roms(&json!({
                "emulatorId": "unknown",
                "directory": temp.abs("roms"),
            }))["error"],
            "emulador_desconhecido"
        );
    }

    #[test]
    fn scan_reconhece_ps3_game_e_prioriza_playlists_m3u() {
        let temp = TempDir::new("arcadia-roms");
        temp.mkdir("roms/Title/PS3_GAME");
        temp.write("roms/Title/PS3_GAME/PARAM.SFO", b"metadata");
        temp.write("roms/Title/EBOOT.BIN", b"internal");
        temp.mkdir("roms/multi");
        temp.write("roms/multi/game.m3u", b"disc1.iso\ndisc2.iso");
        temp.write("roms/multi/disc1.iso", b"disc");
        temp.write("roms/multi/disc2.iso", b"disc");
        let registry = registry(&temp, Vec::new());
        let result = registry.scan_roms(&json!({
            "emulatorId": "rpcs3",
            "directory": temp.abs("roms"),
        }));
        let marker = temp.abs("roms/Title/PS3_GAME");
        assert!(result["roms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rom| rom["path"] == marker.as_str() && rom["kind"] == "directory"));
        assert!(!result["roms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rom| rom["name"] == "EBOOT.BIN"));
        let playlist = registry.scan_roms(&json!({
            "emulatorId": "pcsx2",
            "directory": temp.abs("roms/multi"),
        }));
        assert_eq!(relative_paths(&playlist), vec!["game.m3u"]);
    }

    #[test]
    fn scan_sem_diretorio_usa_pastas_persistidas_e_deduplica() {
        let temp = TempDir::new("arcadia-roms");
        temp.mkdir("roms");
        let rom = temp.write("roms/game.iso", b"iso");
        let registry = registry(&temp, Vec::new());
        let folder = temp.abs("roms");
        assert_eq!(
            registry.set_profile(&json!({
                "id": "pcsx2",
                "executable": "pcsx2",
                "romFolders": [folder, folder],
            }))["ok"],
            true
        );
        let invalid = format!("{}/../outside", temp.path().display());
        assert_eq!(
            registry.set_profile(&json!({
                "id": "pcsx2",
                "executable": "pcsx2",
                "romFolders": [invalid],
            }))["error"],
            "rom_folder_invalida"
        );
        let result = registry.scan_roms(&json!({ "emulatorId": "pcsx2", "maxResults": 32 }));
        assert_eq!(result["ok"], true);
        assert_eq!(
            result["folders"],
            json!([{ "path": folder, "recursive": true }])
        );
        assert_eq!(result["persisted"], true);
        assert_eq!(result["roms"][0]["path"], path_string(&rom));
        assert_eq!(
            registry.roms()["pcsx2"]["roms"][0]["path"],
            path_string(&rom)
        );
        assert_eq!(registry.roms_value()["ok"], true);
    }

    #[test]
    fn scan_agrupa_mdf_sob_mds() {
        let temp = TempDir::new("arcadia-roms");
        let mds = temp.write("roms/disc.mds", b"descriptor");
        let mdf = temp.write("roms/disc.mdf", b"image");
        let registry = registry(&temp, Vec::new());
        let result = registry.scan_roms(&json!({
            "emulatorId": "pcsx2",
            "directory": temp.abs("roms"),
        }));
        assert_eq!(result["roms"].as_array().unwrap().len(), 1);
        assert_eq!(result["roms"][0]["path"], path_string(&mds));
        assert_eq!(result["roms"][0]["sidecars"], json!([path_string(&mdf)]));
    }

    #[test]
    fn scan_aceita_root_path_folder_path_e_ignora_symlink() {
        let definition = builtin_definitions()
            .into_iter()
            .find(|definition| definition.id == "pcsx2")
            .unwrap();
        let fs = MemoryFs::new();
        fs.add_dir(Path::new("/roms"));
        fs.add_file(Path::new("/roms/a.iso"), b"iso");
        fs.add_file(Path::new("/outside/b.iso"), b"iso");
        fs.add_symlink(Path::new("/roms/link.iso"), Path::new("/roms/a.iso"));
        fs.add_symlink(Path::new("/roms/linked-dir"), Path::new("/outside"));
        let clock = ManualClock::new(5);
        let context = RomScanContext {
            fs: &fs,
            roms_path: Path::new("/state/roms.json"),
            clock: &clock,
            definition: &definition,
            profile_folders: Vec::new(),
        };
        let root = scan_roms(
            &context,
            &json!({ "emulatorId": "pcsx2", "rootPath": "/roms" }),
        );
        assert_eq!(root["ok"], true);
        assert_eq!(root["roms"].as_array().unwrap().len(), 1);
        assert_eq!(
            root["roms"][0]["path"],
            path_string(&normalize_path(Path::new("/roms/a.iso")))
        );
        let folder = scan_roms(
            &context,
            &json!({ "emulatorId": "pcsx2", "folderPath": "/roms" }),
        );
        assert_eq!(folder["roms"].as_array().unwrap().len(), 1);
        let configured = scan_roms(&context, &json!({ "emulatorId": "pcsx2" }));
        assert_eq!(configured["ok"], true);
        assert_eq!(configured["roms"], json!([]));
        assert_eq!(configured["folders"], json!([]));
    }

    #[test]
    fn roms_index_filtra_catalogo_e_normaliza_estado() {
        let temp = TempDir::new("arcadia-roms");
        let rom = temp.write("roms/game.iso", b"iso");
        let state = json!({
            "version": 1,
            "emulators": {
                "pcsx2": {
                    "updatedAt": "123",
                    "directory": temp.abs("roms"),
                    "folders": "bad",
                    "truncated": "yes",
                    "roms": [
                        {
                            "path": path_string(&rom),
                            "name": "game.iso",
                            "relativePath": "game.iso",
                            "extension": ".iso",
                            "kind": "file",
                            "sizeBytes": 3,
                            "mtimeMs": 10,
                            "sidecars": [],
                        },
                        { "path": "../bad", "name": "bad" }
                    ],
                },
                "unknown": {
                    "updatedAt": 1,
                    "directory": "",
                    "folders": [],
                    "truncated": false,
                    "roms": [],
                },
            },
        });
        std::fs::write(
            temp.path().join("roms.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();
        let registry = registry(&temp, Vec::new());
        let index = registry.roms();
        assert!(index.get("unknown").is_none());
        assert_eq!(index["pcsx2"]["updatedAt"], 123);
        assert_eq!(index["pcsx2"]["directory"], temp.abs("roms"));
        assert_eq!(index["pcsx2"]["folders"], json!([]));
        assert_eq!(index["pcsx2"]["truncated"], true);
        assert_eq!(index["pcsx2"]["roms"].as_array().unwrap().len(), 1);
        assert_eq!(index["pcsx2"]["roms"][0]["path"], path_string(&rom));
    }

    #[test]
    fn normalize_stored_rom_aplica_limites_e_defaults() {
        let temp = TempDir::new("arcadia-roms");
        let rom = temp.write("roms/game.iso", b"iso");
        let long_name = "n".repeat(600);
        let long_relative = "r".repeat(2100);
        let stored = normalize_stored_rom(&json!({
            "path": path_string(&rom),
            "name": long_name,
            "relativePath": long_relative,
            "extension": "e".repeat(40),
            "kind": "directory",
            "sizeBytes": -5,
            "mtimeMs": "42",
            "sidecars": [path_string(&rom), "../bad", 7],
        }))
        .unwrap();
        assert_eq!(stored.name.len(), 512);
        assert_eq!(stored.relative_path.len(), 2048);
        assert_eq!(stored.extension.len(), 32);
        assert_eq!(stored.kind, "directory");
        assert_eq!(stored.size_bytes, 0);
        assert_eq!(stored.mtime_ms, 42);
        assert_eq!(stored.sidecars, vec![path_string(&rom)]);
        let fallback = normalize_stored_rom(&json!({ "path": path_string(&rom) })).unwrap();
        assert_eq!(fallback.name, "game.iso");
        assert_eq!(fallback.relative_path, "game.iso");
        assert_eq!(fallback.extension, "");
        assert_eq!(fallback.kind, "file");
        assert!(normalize_stored_rom(&json!({ "path": "../bad" })).is_none());
        assert!(normalize_stored_rom(&Value::Null).is_none());
    }

    #[test]
    fn estado_roms_com_versao_errada_e_symlink_e_ignorado() {
        let temp = TempDir::new("arcadia-roms");
        std::fs::write(
            temp.path().join("roms.json"),
            r#"{"version":2,"emulators":{"pcsx2":{"roms":[]}}}"#,
        )
        .unwrap();
        let registry = registry(&temp, Vec::new());
        assert_eq!(registry.roms(), json!({}));
        assert_eq!(
            rom_key(Path::new("/a/./b")),
            path_string(&normalize_path(Path::new("/a/b")))
        );
        assert!(is_rom_index_path(&temp.abs("roms")));
        assert!(!is_rom_index_path("../bad"));
        assert_eq!(indexed_rom_count(&json!({ "x": { "roms": [1, 2] } })), 2);
    }
}
