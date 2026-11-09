//! Fixes por jogo: catálogo, plano de aplicação, instalação/rollback,
//! redirect do launcher e credencial do Ryuu.
//!
//! Port 1:1 de `app/electron/fixes.js` (e dos shapes de `hosters.js`/
//! `catalog.js`). O núcleo é puro: filesystem, downloader e extrator são traits
//! injetados; rede/processos ficam nos executores de produção. Os envelopes
//! `fixes:check/apply/status/cancel/installed/
//! unfix/launcherRedirect/setRyuuAuth/ryuuAuthStatus/clearRyuuAuth` preservam
//! os nomes de campo consumidos pelo IPC e pela `FixesPanel`.
//!
//! - [`catalog`]: índices luatools/Ryuu, `pickEntry`, badges, URLs/magnets,
//!   tamanho/hash de release e gating por auth;
//! - [`plan`]: plano de aplicação + [`plan::FixEngine`] com os envelopes;
//! - [`install`]: manifests `.slssteam_fix_*`, backups, rollback, idempotência
//!   e rejeição de path traversal;
//! - [`redirect`]: `"<abs>" %command%` e auth Ryuu (X-Auth-Key/session);
//! - [`download`]: classificação direta/hoster/magnet e plano do worker;
//! - [`exec_http`]: executor HTTP de produção com transporte injetável;
//! - [`exec_archive`]: extratores ZIP e 7z/unzip com runner injetável;
//! - [`exec`]: bundle [`FixExecutors`] usado pelo `fixes:apply` da borda.
//!
//! Nenhum teste acessa a rede externa ou spawna processos: o executor HTTP é
//! exercitado contra um servidor TCP local e os extratores usam runners falsos.

pub mod catalog;
pub mod download;
pub mod exec;
pub mod exec_archive;
pub mod exec_http;
pub mod install;
pub mod plan;
pub mod redirect;

pub use catalog::{
    appid_path_component, cache_is_fresh, fix_url_allowed, fix_url_type, normalize_appid,
    parse_size, sanitize_url, url_encode, valid_fixes_index, valid_ryuu_index, CatalogError,
    FileHash, FixCandidate, FixCatalog, FixCheck, FixCheckItem, FixEntry, FixGate, FixReleaseInfo,
    FixType, FixesIndex, RyuuGame, FIXES_INDEX_CACHE_FILE, FIXES_INDEX_URL, FIX_URL_BASES,
    GENERIC_BASE, ONLINE_BASE, REFRESH_TTL_SECS, RYUU_BASE, RYUU_CATALOG_URL,
    RYUU_INDEX_CACHE_FILE,
};
pub use download::{
    classify_download_source, is_known_hoster, DownloadError, DownloadEvent, DownloadExecutor,
    DownloadFetcher, DownloadPlan, DownloadedArchive, FetchRequest, FetchResponse,
    IdentitySourceResolver, MagnetLink, ResolvedDownloadSource, SourceKind, SourceResolver,
    CONNECT_TIMEOUT, DEFAULT_USER_AGENT, ERROR_AUTHENTICATION, ERROR_BAD_URL, ERROR_STALLED,
    FIX_TMP_PREFIX, SPEED_LIMIT_KBPS, SPEED_TIME_SECS,
};
pub use exec::FixExecutors;
pub use exec_archive::{
    find_seven_zip, find_unzip, ArchiveTool, ProcessOutcome, ProcessRunner, ProcessSpec,
    SevenZipArchiveExtractor, SystemProcessRunner, ZipArchiveExtractor, ARCHIVE_DEPTH_EXCEEDED,
    ARCHIVE_ENTRY_UNSUPPORTED, ARCHIVE_OPEN_FAILED, ARCHIVE_SYMLINK_REJECTED, EXTRACT_TMP_PREFIX,
    MAX_NESTED_DEPTH, NESTED_ARCHIVE_UNSUPPORTED,
};
pub use exec_http::{
    CancelHandle, HttpDownloadExecutor, HttpRequest, HttpStream, HttpTransport, ReqwestTransport,
    ERROR_CANCELLED, HTTP_CHUNK_SIZE,
};
pub use install::{
    apply, dll_basename, dll_manifest_path, is_archive_file, is_fixed, is_launcher,
    is_secondary_volume, is_within, launcher_manifest_path, manifest_dlls, manifest_launchers,
    read_install_state, read_manifest, resolve_relative, sort_unique_case_insensitive, state_path,
    unfix, validate_archive_path, write_manifest, ArchiveEntry, ArchiveExtractor, ArchiveLayout,
    ExtractionRules, FixFs, FsError, FsResult, InstallError, InstallOutcome, InstallPlan,
    InstallState, ManagedFile, ManagedKind, ManifestPlan, NestedArchive, PathError, PathRoots,
    PlannedBackup, PlannedDirectory, PlannedRemoval, PlannedWrite, SafeArchivePath,
    StaticArchiveExtractor, StdFixFs, UnfixOutcome, DLL_MANIFEST, INSTALL_STATE_FILE,
    INSTALL_STATE_VERSION, LAUNCHER_MANIFEST, MAX_ARCHIVE_PATH_LEN,
};
pub use plan::{
    build_apply_plan, build_download_plan, single_root_roots, ApplyPlan, ApplyRequest,
    ApplyResponse, CheckResponse, CleanupPlan, FixEngine, FixError, FixStatus, InstalledResponse,
    LauncherRedirectResponse, OkResponse, Postcondition, Precondition, RyuuSetResponse,
    UnfixResponse, ERR_INVALID_CREDENTIAL, ERR_MISSING_ARGS, ERR_NO_INSTALL_PATH,
    ERR_PATH_REJECTED, ERR_RYUU_AUTH, ERR_URL_REJECTED, STATUS_DONE, STATUS_DOWNLOADING,
    STATUS_EXTRACTED, STATUS_EXTRACTING, STATUS_FAILED,
};
pub use redirect::{
    build_launcher_redirect, mask_secret, normalize_ryuu_auth, MaskedRyuuAuthStatus, RyuuAuth,
    RyuuAuthError, RyuuAuthKind, RyuuAuthStatus, RyuuAuthStore, RYUU_AUTH_FILE,
    RYUU_AUTH_FILE_LEGACY, RYUU_AUTH_MAX_LEN,
};

/// Mantido do stub original para consumidores que já verificavam o crate.
pub fn crate_ready() -> bool {
    true
}
