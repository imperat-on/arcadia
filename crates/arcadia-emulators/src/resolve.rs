//! Resolução de launch: monta argv do emulador sem shell e sem executar nada.
//!
//! Port 1:1 de `resolveLaunch` de `app/electron/emulator-registry.js`,
//! incluindo `launchMode: "hydra"` (prefixo/sufixo determinísticos, troca de
//! `.mds` por `.mdf` e EBOOT instalado de pacote `.pkg`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::fs::{
    basename, extname, is_directory, is_regular, js_trim, js_truthy, normalize_absolute,
    path_string,
};
use crate::registry::{
    find_on_path_for, normalize_args, normalize_id, normalize_rom_directory_markers,
    EmulatorDefinition, EmulatorRegistry,
};

/// Assinatura big-endian de um pacote PS3 (`PKG`).
pub const RPCS3_PKG_MAGIC: u32 = 0x7f50_4b47;
pub const DISC_SIDECAR_MISSING: &str = "DISC_SIDECAR_MISSING";
pub const PACKAGE_TITLE_INVALID: &str = "PACKAGE_TITLE_INVALID";
pub const PACKAGE_INSTALL_REQUIRED: &str = "PACKAGE_INSTALL_REQUIRED";

const RPCS3_PKG_HEADER: usize = 0x80;
const RPCS3_PKG_MIN_READ: usize = 0x54;

/// Prefixo Hydra por emulador (`HYDRA_LAUNCH_PREFIX`).
pub fn hydra_launch_prefix(emulator_id: &str) -> &'static [&'static str] {
    match emulator_id {
        "duckstation" | "pcsx2" => &["-batch", "-fullscreen", "--"],
        "rpcs3" => &["--no-gui"],
        "retroarch" => &[],
        _ => &[],
    }
}

/// Sufixo Hydra por emulador (`HYDRA_LAUNCH_SUFFIX`).
pub fn hydra_launch_suffix(emulator_id: &str) -> &'static [&'static str] {
    match emulator_id {
        "retroarch" => &["-f"],
        _ => &[],
    }
}

/// `isCoreFile`: extensão libretro/dll/dylib em arquivo regular.
pub fn is_core_file(file: &Path, fs: &dyn crate::fs::EmulatorFs) -> bool {
    if !is_regular(file, fs) {
        return false;
    }
    let lower = crate::fs::js_lower(&basename(file));
    lower.ends_with(".so")
        || lower.contains(".so.")
        || lower.ends_with(".dll")
        || lower.ends_with(".dylib")
}

/// `isRomExtensionAllowed`: allowlist do catálogo; lista vazia aceita tudo.
pub fn is_rom_extension_allowed(name: &str, definition: &EmulatorDefinition) -> bool {
    let allowed: Vec<String> = definition
        .rom_extensions
        .iter()
        .filter_map(|extension| {
            let normalized = crate::registry::normalize_rom_extension(extension);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect();
    if allowed.is_empty() {
        return true;
    }
    let lower = crate::fs::js_lower(name);
    allowed.iter().any(|extension| lower.ends_with(extension))
}

/// `isRomPathValid`: arquivo com extensão permitida ou diretório marcador
/// (`PS3_GAME`, `ps3_game`).
pub fn is_rom_path_valid(
    file: &Path,
    definition: &EmulatorDefinition,
    fs: &dyn crate::fs::EmulatorFs,
) -> bool {
    if is_regular(file, fs) {
        return is_rom_extension_allowed(&basename(file), definition);
    }
    if !is_directory(file, fs) {
        return false;
    }
    let markers = normalize_rom_directory_markers(&Value::Array(
        definition
            .rom_directory_markers
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    ));
    markers.contains(&basename(file))
}

/// Erro estruturado de `resolveHydraBootPath`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydraBootError {
    pub error: String,
    pub code: String,
    pub expected_path: Option<String>,
    pub title_id: Option<String>,
}

impl HydraBootError {
    fn new(error: &str, code: &str) -> Self {
        Self {
            error: error.to_string(),
            code: code.to_string(),
            expected_path: None,
            title_id: None,
        }
    }
}

fn find_title_id(header: &[u8]) -> Option<String> {
    if header.len() < 9 {
        return None;
    }
    for window in header.windows(9) {
        let uppercase = window[..4].iter().all(u8::is_ascii_uppercase);
        let digits = window[4..].iter().all(u8::is_ascii_digit);
        if uppercase && digits {
            return Some(String::from_utf8_lossy(window).into_owned());
        }
    }
    None
}

/// `resolveHydraBootPath`: `.mds` -> `.mdf` para PCSX2/DuckStation e `.pkg` ->
/// `EBOOT.BIN` instalado para RPCS3.
pub fn resolve_hydra_boot_path(
    emulator_id: &str,
    rom_path: &Path,
    fs: &dyn crate::fs::EmulatorFs,
    executable_path: &str,
    home_dir: &Path,
) -> Result<PathBuf, HydraBootError> {
    let rom_text = path_string(rom_path);
    let extension = crate::fs::js_lower(&extname(&rom_text));
    if (emulator_id == "pcsx2" || emulator_id == "duckstation") && extension == ".mds" {
        let stem = &rom_text[..rom_text.len().saturating_sub(4)];
        let candidates = [format!("{stem}.mdf"), format!("{stem}.MDF")];
        if let Some(mdf) = candidates
            .iter()
            .map(PathBuf::from)
            .find(|candidate| is_regular(candidate, fs))
        {
            return Ok(mdf);
        }
        let mut error = HydraBootError::new("disc_sidecar_ausente", DISC_SIDECAR_MISSING);
        error.expected_path = Some(format!("{stem}.mdf"));
        return Err(error);
    }
    if emulator_id != "rpcs3" || extension != ".pkg" {
        return Ok(rom_path.to_path_buf());
    }
    let header = fs.read_prefix(rom_path, RPCS3_PKG_HEADER);
    let Some(header) = header else {
        return Err(HydraBootError::new(
            "pkg_titulo_invalido",
            PACKAGE_TITLE_INVALID,
        ));
    };
    if header.len() < RPCS3_PKG_MIN_READ
        || u32::from_be_bytes([header[0], header[1], header[2], header[3]]) != RPCS3_PKG_MAGIC
    {
        return Err(HydraBootError::new(
            "pkg_titulo_invalido",
            PACKAGE_TITLE_INVALID,
        ));
    }
    let Some(title_id) = find_title_id(&header) else {
        return Err(HydraBootError::new(
            "pkg_titulo_invalido",
            PACKAGE_TITLE_INVALID,
        ));
    };
    let title_id = title_id.to_uppercase();
    let mut roots = vec![
        home_dir.join(".config").join("rpcs3"),
        home_dir
            .join(".var")
            .join("app")
            .join("net.rpcs3.RPCS3")
            .join("config")
            .join("rpcs3"),
    ];
    if let Ok(executable) = normalize_absolute(executable_path, "executablePath") {
        if let Some(parent) = executable.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for root in roots {
        if !seen.insert(path_string(&root)) {
            continue;
        }
        let eboot = root
            .join("dev_hdd0")
            .join("game")
            .join(&title_id)
            .join("USRDIR")
            .join("EBOOT.BIN");
        if is_regular(&eboot, fs) {
            return Ok(eboot);
        }
    }
    let mut error = HydraBootError::new("pkg_instalacao_necessaria", PACKAGE_INSTALL_REQUIRED);
    error.title_id = Some(title_id);
    Err(error)
}

/// Pedido tipado de `resolveLaunch`. A entrada IPC-exata é
/// [`EmulatorRegistry::resolve_launch`] (`serde_json::Value`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveLaunchRequest {
    pub emulator_id: Option<String>,
    pub rom_path: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub core_path: Option<String>,
    pub launch_mode: Option<String>,
}

impl ResolveLaunchRequest {
    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();
        if let Some(value) = &self.emulator_id {
            map.insert("emulatorId".into(), Value::String(value.clone()));
        }
        if let Some(value) = &self.rom_path {
            map.insert("romPath".into(), Value::String(value.clone()));
        }
        if let Some(value) = &self.extra_args {
            map.insert(
                "extraArgs".into(),
                Value::Array(value.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(value) = &self.core_path {
            map.insert("corePath".into(), Value::String(value.clone()));
        }
        if let Some(value) = &self.launch_mode {
            map.insert("launchMode".into(), Value::String(value.clone()));
        }
        Value::Object(map)
    }
}

/// Resultado de `resolveLaunch`, com os mesmos nomes JSON do cliente.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunch {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emulator_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_id: Option<String>,
}

impl ResolvedLaunch {
    pub fn success(emulator_id: &str, system: &str, cmd: Vec<String>) -> Self {
        Self {
            ok: true,
            emulator_id: Some(emulator_id.to_string()),
            system: Some(system.to_string()),
            cmd: Some(cmd),
            ..Self::default()
        }
    }

    pub fn error(error: &str) -> Self {
        Self {
            ok: false,
            error: Some(error.to_string()),
            ..Self::default()
        }
    }

    fn from_boot_error(error: HydraBootError) -> Self {
        Self {
            ok: false,
            error: Some(error.error),
            code: Some(error.code),
            expected_path: error.expected_path,
            title_id: error.title_id,
            ..Self::default()
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Comando pronto para a camada de launch. O resolver do Electron devolve
    /// só `cmd`; `env`/`cwd` ficam vazios aqui e são preenchidos pelo wrapper
    /// de processo (mesmo contrato do `BuiltCommand` de `arcadia-launch`).
    pub fn command(&self) -> Option<LaunchCommand> {
        self.cmd.as_ref().map(|cmd| LaunchCommand {
            cmd: cmd.clone(),
            env: BTreeMap::new(),
            cwd: String::new(),
        })
    }
}

/// `{ cmd, env, cwd }` para consumo direto pelo spawn.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCommand {
    pub cmd: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
}

fn launch_mode(request: &Value) -> Result<bool, String> {
    match request.get("launchMode") {
        None => Ok(false),
        Some(Value::String(mode)) if mode == "default" => Ok(false),
        Some(Value::String(mode)) if mode == "hydra" => Ok(true),
        Some(_) => Err("launch_mode_invalido".to_string()),
    }
}

fn request_string(request: &Value, key: &str) -> String {
    request
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Resolve argv com o registry. Devolve erro estruturado em vez de lançar.
pub fn resolve_launch(registry: &EmulatorRegistry, request: &Value) -> ResolvedLaunch {
    let hydra_mode = match launch_mode(request) {
        Ok(mode) => mode,
        Err(error) => return ResolvedLaunch::error(&error),
    };
    let id = normalize_id(&request_string(request, "emulatorId"));
    let Some(definition) = registry.definition(&id).cloned() else {
        return ResolvedLaunch::error("emulador_desconhecido");
    };
    let rom_value = request_string(request, "romPath");
    let rom = match normalize_absolute(&rom_value, "romPath") {
        Ok(rom) if is_rom_path_valid(&rom, &definition, registry.fs()) => rom,
        _ => return ResolvedLaunch::error("rom_invalida"),
    };

    let configured = registry.get_profile(&id);
    let configured_executable = configured
        .as_ref()
        .map(|profile| {
            find_on_path_for(
                &profile.executable,
                registry.fs(),
                registry.search_path(),
                registry.platform(),
            )
        })
        .unwrap_or_default();
    if configured.is_some() && configured_executable.is_empty() {
        return ResolvedLaunch::error("executavel_configurado_invalido");
    }
    let detected = match &configured {
        Some(profile) => Some((configured_executable.clone(), profile.args.clone())),
        None => crate::registry::detect_definition_for(
            &definition,
            registry.fs(),
            registry.search_path(),
            registry.platform(),
            registry.app_image_dirs(),
            registry.flatpak_roots(),
        )
        .map(|detected| (detected.executable, detected.args)),
    };
    let Some((executable, detected_args)) = detected else {
        return ResolvedLaunch::error("emulador_nao_encontrado");
    };
    if executable.is_empty() {
        return ResolvedLaunch::error("emulador_nao_encontrado");
    }

    let extra_args = match request.get("extraArgs") {
        None => Ok(Vec::new()),
        Some(value) => normalize_args(Some(value)),
    };
    let extra_args = match extra_args {
        Ok(args) => args,
        Err(error) => return ResolvedLaunch::error(&error),
    };
    let base_args = match &configured {
        Some(profile) => profile.args.clone(),
        None => detected_args,
    };

    let selected_core = match request.get("corePath") {
        Some(value) if js_truthy(value) => match value.as_str() {
            Some(core) => core.to_string(),
            None => return ResolvedLaunch::error("corePath_invalido"),
        },
        _ => configured
            .as_ref()
            .map(|profile| profile.core_path.clone())
            .unwrap_or_default(),
    };

    let mut command: Vec<String> = vec![executable.clone()];
    command.extend(base_args);
    if hydra_mode {
        command.extend(hydra_launch_prefix(&id).iter().map(ToString::to_string));
    }
    if definition.requires_core == Some(true) {
        let core = match normalize_absolute(&selected_core, "corePath") {
            Ok(core) if is_core_file(&core, registry.fs()) => core,
            _ => return ResolvedLaunch::error("retroarch_core_invalido"),
        };
        command.push("-L".to_string());
        command.push(path_string(&core));
    }
    let boot = if hydra_mode {
        match resolve_hydra_boot_path(&id, &rom, registry.fs(), &executable, registry.home_dir()) {
            Ok(path) => path,
            Err(error) => return ResolvedLaunch::from_boot_error(error),
        }
    } else {
        rom
    };
    command.push(path_string(&boot));
    command.extend(extra_args);
    if hydra_mode {
        command.extend(hydra_launch_suffix(&id).iter().map(ToString::to_string));
    }
    let system = definition.systems.first().cloned().unwrap_or_default();
    ResolvedLaunch::success(&id, &system, command)
}

/// Normaliza `extraArgs` de um payload JSON sem montar o registry (helper de
/// validação exposto para a camada IPC).
pub fn validate_extra_args(value: Option<&Value>) -> Result<Vec<String>, String> {
    normalize_args(value)
}

/// `true` quando o payload traz um `launchMode` diferente de `default`/`hydra`.
pub fn launch_mode_is_valid(request: &Value) -> bool {
    launch_mode(request).is_ok()
}

/// Compara um `extraArgs` com o formato aceito pelo oráculo.
pub fn extra_args_are_valid(request: &Value) -> bool {
    match request.get("extraArgs") {
        None => true,
        Some(value) => normalize_args(Some(value)).is_ok(),
    }
}

impl EmulatorRegistry {
    /// `resolveLaunch(payload)` — envelope exato do IPC.
    pub fn resolve_launch(&self, request: &Value) -> Value {
        resolve_launch(self, request).to_value()
    }

    /// `resolveLaunch` a partir de uma entrada tipada.
    pub fn resolve_launch_typed(&self, request: &ResolveLaunchRequest) -> ResolvedLaunch {
        resolve_launch(self, &request.to_value())
    }
}

/// `path.extname` exposto para consumidores que montam o mesmo payload.
pub fn rom_extension(path: &str) -> String {
    extname(path)
}

/// Helper de trim idêntico ao `normalizeId` para campos de id de perfil.
pub fn normalized_emulator_id(value: &str) -> String {
    normalize_id(js_trim(value))
}

/// Constrói apenas o JSON de sucesso, útil em snapshots.
pub fn success_json(emulator_id: &str, system: &str, cmd: &[String]) -> Value {
    json!({
        "ok": true,
        "emulatorId": emulator_id,
        "system": system,
        "cmd": cmd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::PlatformKind;
    use crate::registry::RegistryOptions;
    use crate::test_support::{chmod_exec, TempDir};

    fn registry_for(temp: &TempDir, bin: &Path) -> EmulatorRegistry {
        let mut options = RegistryOptions::for_data_dir(temp.path().join("state"));
        options.env_path = Some(path_string(bin));
        options.home_dir = temp.path().join("home");
        options.platform = PlatformKind::current();
        EmulatorRegistry::new(options)
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let temp = TempDir::new("arcadia-resolve");
        let bin = temp.mkdir("bin");
        let exe = temp.write("bin/pcsx2-qt", b"#!/bin/sh\n");
        chmod_exec(&exe);
        (temp, bin, exe)
    }

    #[test]
    fn monta_argv_sem_shell() {
        let (temp, bin, exe) = fixture();
        let rom = temp.write("games/demo.iso", b"rom bytes");
        let registry = registry_for(&temp, &bin);
        let result = registry.resolve_launch(&json!({
            "emulatorId": "pcsx2",
            "romPath": path_string(&rom),
            "extraArgs": ["--fullscreen"],
        }));
        assert_eq!(result["ok"], true);
        assert_eq!(result["emulatorId"], "pcsx2");
        assert_eq!(result["system"], "PlayStation 2");
        assert_eq!(
            result["cmd"],
            json!([path_string(&exe), path_string(&rom), "--fullscreen"])
        );
        assert!(!result["cmd"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg.as_str().unwrap_or("").contains("&&")));
    }

    #[test]
    fn rejeita_emulador_rom_core_e_symlink() {
        let (temp, bin, _exe) = fixture();
        let rom = temp.write("games/demo.iso", b"rom");
        let bad_extension = temp.write("games/demo.txt", b"txt");
        let registry = registry_for(&temp, &bin);
        assert_eq!(
            registry
                .resolve_launch(&json!({ "emulatorId": "unknown", "romPath": path_string(&rom) }))
                ["error"],
            "emulador_desconhecido"
        );
        assert_eq!(
            registry
                .resolve_launch(&json!({ "emulatorId": "toString", "romPath": path_string(&rom) }))
                ["error"],
            "emulador_desconhecido"
        );
        assert_eq!(
            registry.resolve_launch(&json!({ "emulatorId": "pcsx2", "romPath": path_string(&temp.path().join("missing.iso")) }))["error"],
            "rom_invalida"
        );
        assert_eq!(
            registry.resolve_launch(&json!({ "emulatorId": "pcsx2", "romPath": "../demo.iso" }))
                ["error"],
            "rom_invalida"
        );
        assert_eq!(
            registry.resolve_launch(
                &json!({ "emulatorId": "pcsx2", "romPath": path_string(&bad_extension) })
            )["error"],
            "rom_invalida"
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": format!("{}/games/../games/demo.iso", temp.path().display()),
            }))["error"],
            "rom_invalida"
        );
    }

    #[test]
    fn retroarch_exige_core_regular() {
        let temp = TempDir::new("arcadia-resolve");
        let bin = temp.mkdir("bin");
        let retro = temp.write("bin/retroarch", b"bin");
        chmod_exec(&retro);
        let core = temp.write("cores/pcsx2_libretro.so", b"core");
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        assert_eq!(
            registry.resolve_launch(
                &json!({ "emulatorId": "retroarch", "romPath": path_string(&rom) })
            )["error"],
            "retroarch_core_invalido"
        );
        let result = registry.resolve_launch(&json!({
            "emulatorId": "retroarch",
            "romPath": path_string(&rom),
            "corePath": path_string(&core),
        }));
        assert_eq!(
            result["cmd"],
            json!([
                path_string(&retro),
                "-L",
                path_string(&core),
                path_string(&rom)
            ])
        );
    }

    #[test]
    fn modo_hydra_gera_argv_por_emulador() {
        let (temp, bin, exe) = fixture();
        let rpcs3 = temp.write("bin/rpcs3", b"bin");
        let retro = temp.write("bin/retroarch", b"bin");
        chmod_exec(&rpcs3);
        chmod_exec(&retro);
        let core = temp.write("cores/core.so", b"core");
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "launchMode": "hydra",
            }))["cmd"],
            json!([
                path_string(&exe),
                "-batch",
                "-fullscreen",
                "--",
                path_string(&rom)
            ])
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "rpcs3",
                "romPath": path_string(&rom),
                "launchMode": "hydra",
            }))["cmd"],
            json!([path_string(&rpcs3), "--no-gui", path_string(&rom)])
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "retroarch",
                "romPath": path_string(&rom),
                "corePath": path_string(&core),
                "launchMode": "hydra",
            }))["cmd"],
            json!([
                path_string(&retro),
                "-L",
                path_string(&core),
                path_string(&rom),
                "-f"
            ])
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "launchMode": "sh",
            }))["error"],
            "launch_mode_invalido"
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "launchMode": null,
            }))["error"],
            "launch_mode_invalido"
        );
    }

    #[test]
    fn hydra_troca_mds_por_mdf_e_bloqueia_sidecar_ausente() {
        let (temp, bin, _) = fixture();
        let mds = temp.write("games/disc.mds", b"descriptor");
        let mdf = temp.write("games/disc.mdf", b"image");
        let registry = registry_for(&temp, &bin);
        let result = registry.resolve_launch(&json!({
            "emulatorId": "pcsx2",
            "romPath": path_string(&mds),
            "launchMode": "hydra",
        }));
        assert_eq!(
            result["cmd"].as_array().unwrap().last().unwrap(),
            &Value::String(path_string(&mdf))
        );

        let missing = temp.write("games/missing.mds", b"descriptor");
        let blocked = registry.resolve_launch(&json!({
            "emulatorId": "pcsx2",
            "romPath": path_string(&missing),
            "launchMode": "hydra",
        }));
        assert_eq!(blocked["error"], "disc_sidecar_ausente");
        assert_eq!(blocked["code"], "DISC_SIDECAR_MISSING");
        assert_eq!(blocked["expectedPath"], temp.abs("games/missing.mdf"));
    }

    #[test]
    fn hydra_pkg_usa_eboot_instalado() {
        let (temp, bin, _) = fixture();
        let rpcs3 = temp.write("bin/rpcs3", b"bin");
        chmod_exec(&rpcs3);
        let pkg = temp.write("games/INSTALL.pkg", b"");
        let mut header = vec![0u8; 0x80];
        header[..4].copy_from_slice(&RPCS3_PKG_MAGIC.to_be_bytes());
        let title = b"UP0001-NPAB12345_00-GAME000000000000";
        header[0x30..0x30 + title.len()].copy_from_slice(title);
        std::fs::write(&pkg, &header).unwrap();
        let registry = registry_for(&temp, &bin);
        let missing = registry.resolve_launch(&json!({
            "emulatorId": "rpcs3",
            "romPath": path_string(&pkg),
            "launchMode": "hydra",
        }));
        assert_eq!(missing["error"], "pkg_instalacao_necessaria");
        assert_eq!(missing["code"], "PACKAGE_INSTALL_REQUIRED");
        assert_eq!(missing["titleId"], "NPAB12345");

        let eboot = temp.write(
            "home/.config/rpcs3/dev_hdd0/game/NPAB12345/USRDIR/EBOOT.BIN",
            b"eboot",
        );
        let result = registry.resolve_launch(&json!({
            "emulatorId": "rpcs3",
            "romPath": path_string(&pkg),
            "launchMode": "hydra",
        }));
        assert_eq!(
            result["cmd"],
            json!([path_string(&rpcs3), "--no-gui", path_string(&eboot)])
        );
    }

    #[test]
    fn pkg_invalido_retorna_codigo_estruturado() {
        let (temp, bin, _) = fixture();
        let rpcs3 = temp.write("bin/rpcs3", b"bin");
        chmod_exec(&rpcs3);
        let pkg = temp.write("games/bad.pkg", b"not a pkg");
        let registry = registry_for(&temp, &bin);
        let result = registry.resolve_launch(&json!({
            "emulatorId": "rpcs3",
            "romPath": path_string(&pkg),
            "launchMode": "hydra",
        }));
        assert_eq!(result["error"], "pkg_titulo_invalido");
        assert_eq!(result["code"], "PACKAGE_TITLE_INVALID");
    }

    #[test]
    fn diretorio_marcador_ps3_e_aceito() {
        let (temp, bin, _) = fixture();
        let rpcs3 = temp.write("bin/rpcs3", b"bin");
        chmod_exec(&rpcs3);
        let marker = temp.mkdir("games/Title/PS3_GAME");
        let registry = registry_for(&temp, &bin);
        let result = registry.resolve_launch(&json!({
            "emulatorId": "rpcs3",
            "romPath": path_string(&marker),
        }));
        assert_eq!(result["ok"], true);
        assert_eq!(
            result["cmd"],
            json!([path_string(&rpcs3), path_string(&marker)])
        );
    }

    #[test]
    fn comando_tipado_preenche_env_e_cwd_vazios() {
        let (temp, bin, exe) = fixture();
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        let resolved = registry.resolve_launch_typed(&ResolveLaunchRequest {
            emulator_id: Some("pcsx2".into()),
            rom_path: Some(path_string(&rom)),
            ..ResolveLaunchRequest::default()
        });
        let command = resolved.command().expect("command");
        assert_eq!(command.cmd, vec![path_string(&exe), path_string(&rom)]);
        assert!(command.env.is_empty());
        assert_eq!(command.cwd, "");
    }

    #[cfg(unix)]
    #[test]
    fn rejeita_rom_em_symlink_e_diretorio_em_symlink() {
        let (temp, bin, _exe) = fixture();
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        let link = temp.path().join("link.iso");
        std::os::unix::fs::symlink(&rom, &link).unwrap();
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&link),
            }))["error"],
            "rom_invalida"
        );
        let linked_dir = temp.path().join("linked-games");
        std::os::unix::fs::symlink(temp.path().join("games"), &linked_dir).unwrap();
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&linked_dir.join("demo.iso")),
            }))["error"],
            "rom_invalida"
        );
    }

    #[test]
    fn usa_args_e_core_do_perfil_quando_payload_nao_traz() {
        let temp = TempDir::new("arcadia-resolve");
        let bin = temp.mkdir("bin");
        let retro = temp.write("bin/retroarch", b"bin");
        chmod_exec(&retro);
        let core = temp.write("cores/core.so", b"core");
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        assert_eq!(
            registry.set_profile(&json!({
                "id": "retroarch",
                "executable": path_string(&retro),
                "corePath": path_string(&core),
                "args": ["--verbose"],
            }))["ok"],
            true
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "retroarch",
                "romPath": path_string(&rom),
            }))["cmd"],
            json!([
                path_string(&retro),
                "--verbose",
                "-L",
                path_string(&core),
                path_string(&rom)
            ])
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "retroarch",
                "romPath": path_string(&rom),
                "corePath": "relative/core.so",
            }))["error"],
            "retroarch_core_invalido"
        );
    }

    #[test]
    fn erros_de_args_e_emulador_ausente_seguem_o_oraculo() {
        let (temp, bin, _exe) = fixture();
        let rom = temp.write("games/demo.iso", b"rom");
        let registry = registry_for(&temp, &bin);
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "extraArgs": null,
            }))["error"],
            "args_invalidos"
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "extraArgs": ["a".repeat(1025)],
            }))["error"],
            "args_invalidos"
        );
        let empty = TempDir::new("arcadia-resolve");
        let registry = registry_for(&empty, &empty.mkdir("bin"));
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
            }))["error"],
            "emulador_nao_encontrado"
        );
        assert_eq!(
            registry.resolve_launch(&json!({
                "emulatorId": "pcsx2",
                "romPath": path_string(&rom),
                "launchMode": "",
            }))["error"],
            "launch_mode_invalido"
        );
    }
}
