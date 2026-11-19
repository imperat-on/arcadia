//! Emuladores: catálogo, perfis, resolução de launch, runtime/status, índice de
//! ROMs e configuração RetroAchievements.
//!
//! Port 1:1 dos módulos puros do cliente Electron congelado:
//!
//! - [`registry`] — `app/electron/emulator-registry.js`
//! - [`resolve`] — resolução de argv (`resolveLaunch`, modo Hydra)
//! - [`runtime`] — `emulator-runtime.js` + `emulator-status.js`
//! - [`roms`] — scanner e índice `roms.json`
//! - [`ra_config`] — `retroachievements/emulator-config.js`
//!
//! Nenhum teste executa binários ou varre o home do usuário: todo acesso ao
//! disco passa por [`fs::EmulatorFs`] e todo relógio por [`fs::Clock`]. O
//! crate nunca cria processos; o spawn fica na camada de IPC/launch.

mod crypto;
pub mod fs;
pub mod ra_config;
pub mod registry;
pub mod resolve;
pub mod roms;
pub mod runtime;

#[cfg(test)]
mod test_support;

pub use fs::{
    basename, basename_without_extension, extname, has_symlink_component, is_absolute,
    is_directory, is_executable, is_regular, js_integer, js_len, js_lower, js_number, js_slice,
    js_trim, js_truthy, normalize_absolute, normalize_path, path_string, process_env, Clock,
    EmulatorFs, FsEntry, FsKind, FsMeta, ManualClock, MemoryFs, PlatformKind, SystemClock,
    SystemFs,
};
pub use ra_config::{
    apply_emulator_credentials, configure_duckstation, configure_duckstation_system,
    configure_emulator_credentials, configure_pcsx2, configure_pcsx2_system, configure_ppsspp,
    configure_ppsspp_system, duckstation_config_paths, duckstation_login_key,
    encrypt_duckstation_token, pcsx2_config_paths, ppssp_config_dir, ppssp_ini_path,
    ppssp_secret_path, upsert_ini_section, RaConfigOptions, RaConfigResult, LOGIN_KEY_ROUNDS,
};
pub use registry::{
    atomic_write, build_app_image_directories, build_flatpak_roots, build_linux_search_path,
    builtin_definition, builtin_definitions, create_registry, default_data_dir, detect_definition,
    find_on_path, normalize_args, normalize_command, normalize_definition, normalize_flatpak_id,
    normalize_flatpak_ids, normalize_id, normalize_profile, normalize_rom_directory_markers,
    normalize_rom_extension, normalize_rom_extensions, normalize_rom_folders, resolve_data_dir,
    DetectedExecutable, EmulatorDefinition, EmulatorListItem, EmulatorProfile, EmulatorRegistry,
    ListSource, ProfileInput, RegistryOptions, RomFolder, RomFolderInput, MAX_APPIMAGE_RESULTS,
    MAX_ARGS, MAX_ARG_LENGTH, MAX_DISCOVERY_ENTRIES, MAX_PROFILES, MAX_ROM_FOLDERS,
    REGISTRY_FILENAME, REGISTRY_VERSION, ROMS_FILENAME, ROMS_VERSION,
};
pub use resolve::{
    extra_args_are_valid, hydra_launch_prefix, hydra_launch_suffix, is_core_file,
    is_rom_extension_allowed, is_rom_path_valid, launch_mode_is_valid, resolve_hydra_boot_path,
    resolve_launch, validate_extra_args, HydraBootError, LaunchCommand, ResolveLaunchRequest,
    ResolvedLaunch, DISC_SIDECAR_MISSING, PACKAGE_INSTALL_REQUIRED, PACKAGE_TITLE_INVALID,
    RPCS3_PKG_MAGIC,
};
pub use roms::{
    builtin_rom_extensions, empty_scan_result, indexed_rom_count, is_rom_index_path,
    matched_rom_extension, normalize_stored_rom, rom_extension_allowed, rom_index, rom_key,
    rom_sidecar_rules, save_rom_scan, scan_configured_roms, scan_roms, scan_roms_options,
    EmulatorSidecarRules, RomEntry, RomFolderEntry, RomIndexContext, RomScanContext,
    RomScanOptions, SidecarMapping, StoredRom, BUILTIN_ROM_EXTENSIONS, MAX_SCAN_DEPTH,
    MAX_SCAN_RESULTS, ROM_SIDECAR_RULES,
};
pub use runtime::{
    bios_config_files, emulator_executable_dir, emulator_statuses, emulator_statuses_system,
    get_emulator_status, get_emulator_status_system, get_running_emulator_status,
    get_running_emulator_status_system, normalize_status_absolute, plausible_bios,
    preflight_emulator, preflight_emulator_system, preflight_running_emulator,
    preflight_running_emulator_system, process_names, read_process_name, EmulatorAlreadyRunning,
    EmulatorStatus, EmulatorStatusError, EmulatorStatusResult, PreflightBlocked,
    PreflightEmulatorStatus, RunningEmulatorStatus, RunningPreflight, BIOS_NOT_CONFIGURED,
    EMULATOR_ALREADY_RUNNING, EMULATOR_UNKNOWN, MAX_READ, PREFLIGHT_CODES, PROCESS_NAMES, PS1_MAX,
    PS1_MIN, PS2_MAX, PS2_MIN, RUNNING_CODES,
};

pub fn crate_ready() -> bool {
    true
}
