//! Pipeline de launch — resolução de comando, Proton/Wine, gamescope, sessão
//! e rastreio de processo. Port dos módulos de launch do Electron.
//!
//! The crate is deliberately free of Electron and of process spawning: every
//! machine-dependent decision is reached through an injected probe.  See the
//! module docs for the exact JS counterpart.

pub mod clock;
pub mod gamepad;
pub mod gamescope;
pub mod log;
pub mod platform;
pub mod process;
pub mod resolver;
pub mod session;
pub mod steam_path;
pub mod umu;

pub use clock::{Clock, ManualClock, SystemClock};
pub use gamepad::{
    build_gamepad_wrapper_command, detect_gamepad_event, is_gamepad_uevent, parse_uevent,
    python_cmd, unpack_asar_path, wrapper_path, GamepadFs, SystemGamepadFs, SONY_PRODUCTS,
    SONY_VENDOR, WRAPPER_RELATIVE, XBOX_VENDOR,
};
pub use gamescope::{
    build_external_gamescope_command, can_use_systemd_session, create_process_session,
    create_systemd_unit_name, gamescope_supports_keep_alive, is_systemd_unit_name,
    parse_systemd_show, read_cgroup_pids, safe_integer, systemd_stop_args, valid_window_mode,
    GamescopeError, GamescopeLaunch, GamescopeSettings, ProcessSession, SystemdSession,
    MAX_DIMENSION, MAX_FRAMERATE_LIMIT, MAX_REFRESH,
};
pub use log::{
    format_timestamp, safe_name as safe_log_name, LaunchLog, LaunchLogError, LaunchLogHandle,
    MAX_LOG_BYTES,
};
pub use platform::{strip_exe, strip_lower_exe, Platform};
pub use process::{
    identities_match, is_process_alive, is_process_running, is_steam_game_running,
    is_steam_running, plan_kill_tree, plan_owned_signal, plan_steam_stop, steam_app_id,
    steam_stop_uri, KillSignal, KillTreePlan, ProcessIdentity, ProcessInfo, ProcessLookup,
    ProcessProbe, StaticProcessProbe, SteamStopPlan, SystemProcessProbe,
};
pub use resolver::{
    build_custom_launch, build_exe_launch, plan_from_request, plan_launch, resolve_launch_request,
    split_args, steam_game_id, steam_launch_kind, steam_silent, validate_launch_binaries,
    BuiltCommand, EmulatorLaunchResult, EnvVar, ExeLaunch, ExeLaunchRequest, GameEntry,
    GameSettings, GraphicsLibsRequest, LaunchError, LaunchPayload, LaunchPlan, LaunchRequest,
    LaunchResolverDeps, LaunchSource, PlanEnvironment, ProtonInstall, ResolvedLaunch,
    SteamLaunchKind, SteamOverride, Wrapper, ERR_ALREADY_RUNNING, ERR_EXE_NOT_FOUND,
    ERR_GAME_NOT_FOUND, ERR_LEGACY_REJECTED, ERR_NO_COMMAND,
};
pub use session::{
    is_steam_install_uri, restore_window_focus, should_track_game_session, FocusCallback,
    FocusChange, FocusSession, GamePresenceProbe, GameSession, LaunchCallback, LaunchMeta,
    LaunchSession, LaunchSnapshot, LaunchState, LaunchToken, LaunchTransition, Presence,
    SessionEnded, SessionEvent, StaticGameProbe, StopOutcome, StopResult, StopState, TickOutcome,
    TrackedGame, WindowFocusTarget, EMPTY_CONFIRMATIONS, MAX_START_MS,
};
#[cfg(windows)]
pub use steam_path::WindowsRegistryProbe;
pub use steam_path::{
    find_steam_dir_with, find_steam_exe_with, looks_like_steam_root, parse_reg_query_output,
    registry_roots, steam_candidates, MapRegistryProbe, NullRegistryProbe, PathFs, RegistryProbe,
    SteamPathResolver, SystemPathFs, DEFAULT_WINDOWS_STEAM, DISK_LETTERS, REGISTRY_QUERIES,
    TTL_CACHE_MS,
};
pub use umu::{
    ensure_umu_launcher, external_candidates, extract_umu_from_tar, find_umu_launcher,
    is_managed_umu_valid, is_managed_umu_valid_with, managed_umu_path, parse_tar_size, sha256_hex,
    umu_run_command, verify_umu_archive, verify_umu_archive_with, EnsureUmu, SystemUmuFs, UmuFs,
    UmuSpec, MAX_ARCHIVE_BYTES, UMU_ARCHIVE_SHA256, UMU_ARCHIVE_URL, UMU_EXECUTABLE_SHA256,
    UMU_VERSION,
};

pub fn crate_ready() -> bool {
    true
}
