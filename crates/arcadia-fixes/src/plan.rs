//! Orquestração dos fixes: plano de aplicação e envelopes exatos do IPC.
//!
//! Oráculo: `app/electron/main.js` (handlers `fixes:*`) e
//! `app/electron/fixes.js` (`applyFix`, `getStatus`, `cancelApply`, `isFixed`,
//! `buildLauncherRedirect`, `unfix`, auth Ryuu).
//!
//! Envelopes preservados (nomes de campo camelCase):
//!
//! - `fixes:check` → `{ok, appid, generic, online, crack, authConfigured}`;
//! - `fixes:apply` → `{ok}` ou `{ok:false, error, errorCode?}`;
//! - `fixes:status` → `{status, bytesRead?, totalBytes?, error?, errorCode?}`;
//! - `fixes:cancel` → `{ok:true}`;
//! - `fixes:installed` → `{ok:true, installed}` ou `{ok:false, error}`;
//! - `fixes:unfix` → `{ok:true}` ou `{ok:false, error}`;
//! - `fixes:launcherRedirect` → `{ok:true, redirect}` (redirect pode ser null);
//! - auth Ryuu → `{ok:true, kind}` / `{ok:false, error:"invalid_credential"}` e
//!   status `{configured, kind}`.
//!
//! O engine é síncrono e injeta downloader/extrator/filesystem. A borda Tauri
//! pode executá-lo numa task e emitir progresso; os testes nunca acessam rede
//! nem spawnam processos.

use std::collections::HashSet;
use std::path::{Component, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::catalog::{
    appid_path_component, fix_url_allowed, normalize_appid, FixCandidate, FixCatalog, FixCheck,
    FixCheckItem, FixReleaseInfo, FixType,
};
use crate::download::{
    DownloadError, DownloadEvent, DownloadExecutor, DownloadPlan, ResolvedDownloadSource,
};
use crate::install::{
    is_fixed, is_within, validate_archive_path, ArchiveExtractor, ArchiveLayout, ExtractionRules,
    FixFs, InstallError, InstallPlan, ManifestPlan, PathError, PathRoots, PlannedBackup,
    PlannedDirectory, PlannedRemoval, PlannedWrite, DLL_MANIFEST, LAUNCHER_MANIFEST,
};
use crate::redirect::{RyuuAuthError, RyuuAuthStatus, RyuuAuthStore};

/// Mensagens/códigos do handler IPC.
pub const ERR_MISSING_ARGS: &str = "missing_args";
pub const ERR_URL_REJECTED: &str = "url rejeitada: fonte desconhecida";
pub const ERR_PATH_REJECTED: &str = "installPath rejeitado";
pub const ERR_NO_INSTALL_PATH: &str = "no_install_path";
pub const ERR_INVALID_CREDENTIAL: &str = "invalid_credential";
pub const ERR_RYUU_AUTH: &str =
    "Ryuu authentication is required. Add your session cookie or X-Auth-Key first.";

/// Constantes de status do worker (`fix_downloader.sh`).
pub const STATUS_DOWNLOADING: &str = "downloading";
pub const STATUS_EXTRACTING: &str = "extracting";
pub const STATUS_EXTRACTED: &str = "extracted";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_DONE: &str = "done";

// ─── pedido e plano ─────────────────────────────────────────────────────────

/// Args de `fixes:apply` (`{appid, url, type, installPath}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyRequest {
    pub appid: String,
    #[serde(rename = "type", default)]
    pub fix_type: FixType,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "installPath", default)]
    pub install_path: String,
}

impl ApplyRequest {
    pub fn new(
        appid: impl Into<String>,
        fix_type: FixType,
        url: impl Into<String>,
        install_path: impl Into<String>,
    ) -> Self {
        Self {
            appid: appid.into(),
            fix_type,
            url: url.into(),
            install_path: install_path.into(),
        }
    }
}

/// Erro de preparação/plano. `code` vira `errorCode` no envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixError {
    pub message: String,
    pub code: Option<String>,
}

impl FixError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    pub fn with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code.into()),
        }
    }

    fn missing_args() -> Self {
        Self::new(ERR_MISSING_ARGS)
    }

    fn from_path(error: PathError) -> Self {
        Self::new(error.0)
    }
}

impl std::fmt::Display for FixError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FixError {}

impl From<InstallError> for FixError {
    fn from(error: InstallError) -> Self {
        Self::new(error.0)
    }
}

/// Pré-condição declarativa checada antes da escrita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precondition {
    ArgumentsPresent,
    SourceAllowed,
    InstallPathValid,
    InstallPathExists,
    InstallPathIsDirectory,
    ArchiveEntriesSafe,
    RyuuAuthConfigured,
}

/// Pós-condição esperada de uma aplicação bem-sucedida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Postcondition {
    FilesWritten,
    ManifestsWritten,
    StatePersisted,
    TempCleanupPending,
    LauncherRedirectAvailable,
}

/// Caminhos temporários que `getStatus`/`cancel` limpam.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupPlan {
    pub temp_paths: Vec<PathBuf>,
}

/// Plano completo de uma aplicação, antes de tocar no jogo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyPlan {
    pub appid: String,
    #[serde(rename = "type")]
    pub fix_type: FixType,
    #[serde(rename = "sourceUrl")]
    pub source_url: String,
    pub install_root: PathBuf,
    pub download: DownloadPlan,
    pub extraction: ExtractionRules,
    pub install: InstallPlan,
    pub cleanup: CleanupPlan,
    pub preconditions: Vec<Precondition>,
    pub postconditions: Vec<Postcondition>,
}

struct PreparedRequest {
    appid: String,
    install_root: PathBuf,
    candidate: Option<FixCandidate>,
    source: ResolvedDownloadSource,
}

fn prepare_request(
    catalog: &FixCatalog,
    roots: &PathRoots,
    request: &ApplyRequest,
) -> Result<PreparedRequest, FixError> {
    let appid = normalize_appid(&request.appid);
    if appid.is_empty() || request.url.is_empty() || request.install_path.is_empty() {
        return Err(FixError::missing_args());
    }
    if appid_path_component(&appid).is_none() {
        return Err(FixError::new("invalid appid"));
    }
    if !fix_url_allowed(&request.url) {
        return Err(FixError::new(ERR_URL_REJECTED));
    }
    let install_root = roots
        .validate_install_path(&request.install_path)
        .map_err(FixError::from_path)?;
    let candidate = catalog
        .candidate(&appid, request.fix_type)
        .filter(|candidate| candidate.url == request.url);
    let source = ResolvedDownloadSource::from_url(&request.url)
        .map_err(|_| FixError::new(ERR_URL_REJECTED))?;
    Ok(PreparedRequest {
        appid,
        install_root,
        candidate,
        source,
    })
}

fn release_for(candidate: Option<&FixCandidate>) -> Option<&FixReleaseInfo> {
    candidate
        .map(|candidate| &candidate.release)
        .filter(|release| !release.is_empty())
}

/// Constrói só o plano de download (usado antes da extração).
pub fn build_download_plan(
    catalog: &FixCatalog,
    roots: &PathRoots,
    request: &ApplyRequest,
    auth_header: Option<&str>,
) -> Result<DownloadPlan, FixError> {
    let prepared = prepare_request(catalog, roots, request)?;
    let mut plan = DownloadPlan::new(
        &prepared.appid,
        request.fix_type,
        prepared.source,
        roots,
        release_for(prepared.candidate.as_ref()),
    )
    .map_err(|error| FixError::new(error.message))?;
    if let Some(header) = auth_header {
        plan = plan.with_auth_header(header);
    }
    Ok(plan)
}

/// Constrói o plano completo a partir de um layout já extraído.
pub fn build_apply_plan(
    fs: &dyn FixFs,
    catalog: &FixCatalog,
    roots: &PathRoots,
    request: &ApplyRequest,
    layout: &ArchiveLayout,
    auth_header: Option<&str>,
) -> Result<ApplyPlan, FixError> {
    let prepared = prepare_request(catalog, roots, request)?;
    let download = build_download_plan(catalog, roots, request, auth_header)?;
    build_apply_plan_with_download(fs, roots, request, &prepared, layout, download)
}

fn build_apply_plan_with_download(
    fs: &dyn FixFs,
    roots: &PathRoots,
    request: &ApplyRequest,
    prepared: &PreparedRequest,
    layout: &ArchiveLayout,
    download: DownloadPlan,
) -> Result<ApplyPlan, FixError> {
    let install_root = prepared.install_root.clone();
    let appid = prepared.appid.clone();
    let backup_dir = roots.backup_dir(&appid);
    if !is_within(&roots.backup_root, &backup_dir) {
        return Err(FixError::new("fix backup path rejected"));
    }

    let mut writes = Vec::new();
    let mut removals = Vec::new();
    let mut directories = Vec::new();
    let mut file_paths = Vec::new();

    let nested_paths: HashSet<String> = layout
        .nested
        .iter()
        .filter_map(|nested| validate_archive_path(&nested.path).ok())
        .map(|safe| safe.relative)
        .collect();

    for entry in &layout.entries {
        let safe = validate_archive_path(&entry.path)?;
        if entry.directory || safe.directory {
            directories.push(PlannedDirectory {
                path: safe.relative,
                target: install_root.join(&safe.path),
            });
            continue;
        }
        if nested_paths.contains(&safe.relative)
            || crate::install::is_secondary_volume(&safe.relative)
        {
            removals.push(PlannedRemoval {
                path: safe.relative,
                target: install_root.join(&safe.path),
                archive_path: entry.path.clone(),
            });
            continue;
        }
        file_paths.push(safe.relative.clone());
        writes.push(PlannedWrite {
            path: safe.relative,
            target: install_root.join(&safe.path),
            archive_path: entry.path.clone(),
            data: entry.data.clone(),
        });
    }

    for nested in &layout.nested {
        let safe_archive = validate_archive_path(&nested.path)?;
        removals.push(PlannedRemoval {
            path: safe_archive.relative.clone(),
            target: install_root.join(&safe_archive.path),
            archive_path: nested.path.clone(),
        });
        for entry in &nested.entries {
            let safe = validate_archive_path(&entry.path)?;
            if entry.directory || safe.directory {
                directories.push(PlannedDirectory {
                    path: safe.relative,
                    target: install_root.join(&safe.path),
                });
                continue;
            }
            file_paths.push(safe.relative.clone());
            writes.push(PlannedWrite {
                path: safe.relative,
                target: install_root.join(&safe.path),
                archive_path: entry.path.clone(),
                data: entry.data.clone(),
            });
        }
    }

    writes = dedupe_last_by_target(writes, |write| &write.target);
    removals = dedupe_last_by_target(removals, |removal| &removal.target);
    directories = dedupe_last_by_target(directories, |directory| &directory.target);

    for write in &writes {
        if !is_within(&install_root, &write.target) {
            return Err(FixError::new("archive entry path rejected"));
        }
    }
    for removal in &removals {
        if !is_within(&install_root, &removal.target) {
            return Err(FixError::new("archive entry path rejected"));
        }
    }
    for directory in &directories {
        if !is_within(&install_root, &directory.target) {
            return Err(FixError::new("archive entry path rejected"));
        }
    }

    let mut backups = Vec::new();
    for (path, target) in writes
        .iter()
        .map(|write| (&write.path, &write.target))
        .chain(
            removals
                .iter()
                .map(|removal| (&removal.path, &removal.target)),
        )
    {
        if fs.exists(target) && fs.is_file(target) {
            let backup = crate::install::resolve_relative(&backup_dir, path)
                .ok_or_else(|| FixError::new("fix backup path rejected"))?;
            backups.push(PlannedBackup {
                path: path.clone(),
                target: target.clone(),
                backup,
            });
        }
    }
    backups = dedupe_last_by_target(backups, |backup| &backup.target);

    let manifests = ManifestPlan {
        dlls: crate::install::manifest_dlls(file_paths.iter().map(String::as_str)),
        launchers: crate::install::manifest_launchers(file_paths.iter().map(String::as_str)),
        dll_manifest: install_root.join(DLL_MANIFEST),
        launcher_manifest: install_root.join(LAUNCHER_MANIFEST),
    };

    let install = InstallPlan {
        appid: appid.clone(),
        fix_type: request.fix_type,
        source_url: request.url.clone(),
        install_root: install_root.clone(),
        backup_root: roots.backup_root.clone(),
        backup_dir,
        writes,
        removals,
        directories,
        backups,
        manifests,
    };

    let mut preconditions = vec![
        Precondition::ArgumentsPresent,
        Precondition::SourceAllowed,
        Precondition::InstallPathValid,
        Precondition::InstallPathExists,
        Precondition::InstallPathIsDirectory,
        Precondition::ArchiveEntriesSafe,
    ];
    if prepared.source.requires_auth {
        preconditions.push(Precondition::RyuuAuthConfigured);
    }

    let mut postconditions = vec![
        Postcondition::FilesWritten,
        Postcondition::ManifestsWritten,
        Postcondition::StatePersisted,
        Postcondition::TempCleanupPending,
    ];
    if !install.manifests.launchers.is_empty() {
        postconditions.push(Postcondition::LauncherRedirectAvailable);
    }

    Ok(ApplyPlan {
        appid,
        fix_type: request.fix_type,
        source_url: request.url.clone(),
        install_root,
        cleanup: CleanupPlan {
            temp_paths: vec![
                download.state_path.clone(),
                download.destination.clone(),
                download.headers_path.clone(),
            ],
        },
        download,
        extraction: ExtractionRules::default(),
        install,
        preconditions,
        postconditions,
    })
}

fn dedupe_last_by_target<T>(items: Vec<T>, target: impl Fn(&T) -> &PathBuf) -> Vec<T> {
    let mut seen = HashSet::new();
    let mut kept = Vec::with_capacity(items.len());
    for item in items.into_iter().rev() {
        if seen.insert(target(&item).clone()) {
            kept.push(item);
        }
    }
    kept.reverse();
    kept
}

// ─── status (arquivo fix_<appid>_state.json) ────────────────────────────────

/// Envelope de `fixes:status`. `bytesRead`/`totalBytes` só aparecem quando o
/// worker os escreveu; `done` é o default de estado ausente, como no JS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixStatus {
    pub status: String,
    #[serde(rename = "bytesRead", default, skip_serializing_if = "Option::is_none")]
    pub bytes_read: Option<u64>,
    #[serde(
        rename = "totalBytes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "errorCode", default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl Default for FixStatus {
    fn default() -> Self {
        Self::done()
    }
}

impl FixStatus {
    pub fn done() -> Self {
        Self {
            status: STATUS_DONE.to_string(),
            bytes_read: None,
            total_bytes: None,
            error: None,
            error_code: None,
        }
    }

    pub fn status_only(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            bytes_read: None,
            total_bytes: None,
            error: None,
            error_code: None,
        }
    }

    pub fn downloading(bytes_read: u64, total_bytes: u64) -> Self {
        Self {
            status: STATUS_DOWNLOADING.to_string(),
            bytes_read: Some(bytes_read),
            total_bytes: Some(total_bytes),
            error: None,
            error_code: None,
        }
    }

    pub fn extracting(bytes_read: u64, total_bytes: u64) -> Self {
        Self {
            status: STATUS_EXTRACTING.to_string(),
            bytes_read: Some(bytes_read),
            total_bytes: Some(total_bytes),
            error: None,
            error_code: None,
        }
    }

    pub fn extracted(bytes_read: u64, total_bytes: u64) -> Self {
        Self {
            status: STATUS_EXTRACTED.to_string(),
            bytes_read: Some(bytes_read),
            total_bytes: Some(total_bytes),
            error: None,
            error_code: None,
        }
    }

    pub fn failed(error: impl Into<String>, error_code: Option<String>) -> Self {
        Self {
            status: STATUS_FAILED.to_string(),
            bytes_read: None,
            total_bytes: None,
            error: Some(error.into()),
            error_code,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            STATUS_DONE | STATUS_FAILED | STATUS_EXTRACTED
        )
    }
}

// ─── envelopes ──────────────────────────────────────────────────────────────

/// `{ok, appid?, generic?, online?, crack?, authConfigured?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic: Option<FixCheckItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<FixCheckItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crack: Option<FixCheckItem>,
    #[serde(
        rename = "authConfigured",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auth_configured: Option<bool>,
}

impl CheckResponse {
    pub fn invalid_appid() -> Self {
        Self {
            ok: false,
            appid: None,
            generic: None,
            online: None,
            crack: None,
            auth_configured: None,
        }
    }

    pub fn available(appid: impl Into<String>, check: FixCheck, auth_configured: bool) -> Self {
        Self {
            ok: true,
            appid: Some(appid.into()),
            generic: Some(check.generic),
            online: Some(check.online),
            crack: Some(check.crack),
            auth_configured: Some(auth_configured),
        }
    }
}

/// `{ok}` / `{ok:false, error}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "errorCode", default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl ApplyResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            error_code: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            error_code: None,
        }
    }

    pub fn error_with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            error_code: Some(code.into()),
        }
    }

    pub fn from_error(error: &FixError) -> Self {
        Self {
            ok: false,
            error: Some(error.message.clone()),
            error_code: error.code.clone(),
        }
    }
}

/// `{ok:true}` para cancel/clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}

impl OkResponse {
    pub fn ok() -> Self {
        Self { ok: true }
    }
}

/// `{ok, installed}` ou `{ok:false, error}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl InstalledResponse {
    pub fn ok(installed: bool) -> Self {
        Self {
            ok: true,
            installed: Some(installed),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            installed: None,
            error: Some(message.into()),
        }
    }
}

/// `{ok}` ou `{ok:false, error}` do unfix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnfixResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl UnfixResponse {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
        }
    }
}

/// `{ok:true, redirect}` (redirect pode ser `null`) ou `{ok:false, error}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherRedirectResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl LauncherRedirectResponse {
    pub fn ok(redirect: Option<String>) -> Self {
        Self {
            ok: true,
            redirect: Some(redirect),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            redirect: None,
            error: Some(message.into()),
        }
    }
}

/// `{ok:true, kind}` ou `{ok:false, error:"invalid_credential"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RyuuSetResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RyuuSetResponse {
    pub fn ok(kind: &str) -> Self {
        Self {
            ok: true,
            kind: Some(kind.to_string()),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            kind: None,
            error: Some(message.into()),
        }
    }
}

// ─── engine ─────────────────────────────────────────────────────────────────

/// Operações dos fixes com catálogo, filesystem e raízes injetados.
pub struct FixEngine {
    catalog: RwLock<FixCatalog>,
    fs: Arc<dyn FixFs>,
    roots: PathRoots,
}

impl std::fmt::Debug for FixEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FixEngine")
            .field("roots", &self.roots)
            .finish_non_exhaustive()
    }
}

impl FixEngine {
    pub fn new(catalog: FixCatalog, fs: Arc<dyn FixFs>, roots: PathRoots) -> Self {
        Self {
            catalog: RwLock::new(catalog),
            fs,
            roots,
        }
    }

    pub fn fs(&self) -> &Arc<dyn FixFs> {
        &self.fs
    }

    pub fn roots(&self) -> &PathRoots {
        &self.roots
    }

    pub fn set_catalog(&self, catalog: FixCatalog) {
        *self
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = catalog;
    }

    pub fn catalog_snapshot(&self) -> FixCatalog {
        self.catalog
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn catalog_ref(&self) -> std::sync::RwLockReadGuard<'_, FixCatalog> {
        self.catalog
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn auth_store(&self) -> RyuuAuthStore {
        RyuuAuthStore::new(self.fs.clone(), self.roots.data_root.clone())
    }

    fn roots_ready(&self) -> bool {
        let sane = |path: &std::path::Path| {
            path.is_absolute()
                && !path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
        };
        sane(&self.roots.backup_root)
            && sane(&self.roots.data_root)
            && self.roots.game_roots.iter().all(|root| sane(root))
            && self.roots.prefix_roots.iter().all(|root| sane(root))
    }

    // ── check ─────────────────────────────────────────────────────────────

    /// `fixes:check`.
    pub fn check(&self, appid: &str) -> CheckResponse {
        let appid = normalize_appid(appid);
        if appid.is_empty() {
            return CheckResponse::invalid_appid();
        }
        let auth = if self.roots_ready() {
            self.auth_store().status()
        } else {
            RyuuAuthStatus::empty()
        };
        let check = self.catalog_ref().check(&appid);
        CheckResponse::available(appid, check, auth.configured)
    }

    // ── status/cancel ─────────────────────────────────────────────────────

    fn temp_path(&self, appid: &str, suffix: &str) -> Option<PathBuf> {
        let appid = appid_path_component(appid)?;
        if !self.roots.data_root.is_absolute() {
            return None;
        }
        let path = self
            .roots
            .data_tmp_dir()
            .join(format!("fix_{appid}{suffix}"));
        is_within(&self.roots.data_root, &path).then_some(path)
    }

    fn state_path(&self, appid: &str) -> Option<PathBuf> {
        self.temp_path(appid, "_state.json")
    }

    fn cleanup_tmp(&self, appid: &str) {
        for path in [
            self.state_path(appid),
            self.temp_path(appid, ".zip"),
            self.temp_path(appid, "_headers.txt"),
        ]
        .into_iter()
        .flatten()
        {
            if self.fs.exists(&path) {
                let _ = self.fs.remove_file(&path);
            }
        }
    }

    fn write_status(&self, appid: &str, status: &FixStatus) {
        let Some(path) = self.state_path(appid) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = self.fs.create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(status) {
            let _ = self.fs.write(&path, &bytes);
        }
    }

    /// Lê o estado com a mesma semântica de `getStatus`:
    /// ausente → done; `extracted` → done + limpeza; `failed` → devolve e
    /// limpa; JSON inválido → `{status:"downloading"}`.
    pub fn status(&self, appid: &str) -> FixStatus {
        let appid = normalize_appid(appid);
        let Some(path) = self.state_path(&appid) else {
            return FixStatus::done();
        };
        if !self.fs.exists(&path) {
            return FixStatus::done();
        }
        let Ok(bytes) = self.fs.read(&path) else {
            return FixStatus::done();
        };
        let Ok(status) = serde_json::from_slice::<FixStatus>(&bytes) else {
            return FixStatus::status_only(STATUS_DOWNLOADING);
        };
        match status.status.as_str() {
            STATUS_EXTRACTED => {
                self.cleanup_tmp(&appid);
                FixStatus::done()
            }
            STATUS_FAILED => {
                self.cleanup_tmp(&appid);
                status
            }
            _ => status,
        }
    }

    /// `fixes:cancel`: limpa só os temporários (não desfaz um fix aplicado).
    pub fn cancel(&self, appid: &str) -> OkResponse {
        let appid = normalize_appid(appid);
        if appid_path_component(&appid).is_some() {
            self.cleanup_tmp(&appid);
        }
        OkResponse::ok()
    }

    // ── apply ─────────────────────────────────────────────────────────────

    fn auth_header_for(
        &self,
        source: &ResolvedDownloadSource,
    ) -> Result<Option<String>, ApplyResponse> {
        if !source.requires_auth {
            return Ok(None);
        }
        let auth = self.auth_store().read();
        match auth.and_then(|auth| auth.header_line()) {
            Some(header) => Ok(Some(header)),
            None => Err(ApplyResponse::error_with_code(
                ERR_RYUU_AUTH,
                "authentication",
            )),
        }
    }

    fn fail(&self, appid: &str, error: &FixError) -> ApplyResponse {
        self.write_status(
            appid,
            &FixStatus::failed(&error.message, error.code.clone()),
        );
        ApplyResponse::from_error(error)
    }

    fn fail_download(&self, appid: &str, error: &DownloadError) -> ApplyResponse {
        self.write_status(
            appid,
            &FixStatus::failed(&error.message, error.code.clone()),
        );
        ApplyResponse {
            ok: false,
            error: Some(error.message.clone()),
            error_code: error.code.clone(),
        }
    }

    /// `fs.writeFileSync(headerFile, line, {mode:0o600})` do `applyFix`.
    fn write_auth_header(&self, plan: &DownloadPlan) -> bool {
        let Some(header) = &plan.auth_header else {
            return true;
        };
        if let Some(parent) = plan.headers_path.parent() {
            if self.fs.create_dir_all(parent).is_err() {
                return false;
            }
        }
        if self
            .fs
            .write(&plan.headers_path, header.as_bytes())
            .is_err()
        {
            return false;
        }
        let _ = self.fs.set_private(&plan.headers_path);
        true
    }

    /// `fixes:apply` completo: download injetado + extração injetada +
    /// instalação no filesystem injetado.
    pub fn apply(
        &self,
        request: &ApplyRequest,
        downloader: &dyn DownloadExecutor,
        extractor: &dyn ArchiveExtractor,
    ) -> ApplyResponse {
        if !self.roots_ready() {
            return ApplyResponse::error("invalid internal roots");
        }
        let catalog = self.catalog_snapshot();
        let prepared = match prepare_request(&catalog, &self.roots, request) {
            Ok(prepared) => prepared,
            Err(error) => return ApplyResponse::from_error(&error),
        };
        let auth_header = match self.auth_header_for(&prepared.source) {
            Ok(header) => header,
            Err(response) => return response,
        };
        let download = match DownloadPlan::new(
            &prepared.appid,
            request.fix_type,
            prepared.source.clone(),
            &self.roots,
            release_for(prepared.candidate.as_ref()),
        ) {
            Ok(plan) => plan,
            Err(error) => return ApplyResponse::error(error.message),
        };
        let download = match auth_header {
            Some(header) => download.with_auth_header(header),
            None => download,
        };

        if !self.write_auth_header(&download) {
            return ApplyResponse::error_with_code(ERR_RYUU_AUTH, "authentication");
        }

        self.write_status(&prepared.appid, &FixStatus::downloading(0, 0));

        let appid = prepared.appid.clone();
        let archive = match downloader.download(&download, &mut |event| match event {
            DownloadEvent::Downloading {
                bytes_read,
                total_bytes,
            } => self.write_status(&appid, &FixStatus::downloading(bytes_read, total_bytes)),
            DownloadEvent::Extracting => self.write_status(&appid, &FixStatus::extracting(0, 0)),
            DownloadEvent::Failed { error, error_code } => {
                self.write_status(&appid, &FixStatus::failed(error, error_code))
            }
            DownloadEvent::Done => {}
        }) {
            Ok(archive) => archive,
            Err(error) => return self.fail_download(&prepared.appid, &error),
        };
        if !is_within(&self.roots.data_root, &archive.path) {
            return self.fail(&prepared.appid, &FixError::new("download path rejected"));
        }
        if let Err(error) = archive.verify(&download) {
            return self.fail_download(&prepared.appid, &error);
        }

        self.write_status(
            &prepared.appid,
            &FixStatus::extracting(archive.bytes, archive.bytes),
        );
        let layout = match extractor.extract(&archive.path) {
            Ok(layout) => layout,
            Err(error) => {
                return self.fail(&prepared.appid, &FixError::from(error));
            }
        };
        let plan = match build_apply_plan_with_download(
            self.fs.as_ref(),
            &self.roots,
            request,
            &prepared,
            &layout,
            download,
        ) {
            Ok(plan) => plan,
            Err(error) => return self.fail(&prepared.appid, &error),
        };
        match crate::install::apply(self.fs.as_ref(), &plan.install) {
            Ok(_) => {
                self.write_status(
                    &prepared.appid,
                    &FixStatus::extracted(archive.bytes, archive.bytes),
                );
                ApplyResponse::ok()
            }
            Err(error) => self.fail(&prepared.appid, &FixError::from(error)),
        }
    }

    /// Aplica um layout já extraído (sem download). Usado pelos testes e pela
    /// borda quando o worker já entregou o arquivo.
    pub fn apply_archive(&self, request: &ApplyRequest, layout: &ArchiveLayout) -> ApplyResponse {
        if !self.roots_ready() {
            return ApplyResponse::error("invalid internal roots");
        }
        let catalog = self.catalog_snapshot();
        let prepared = match prepare_request(&catalog, &self.roots, request) {
            Ok(prepared) => prepared,
            Err(error) => return ApplyResponse::from_error(&error),
        };
        let auth_header = match self.auth_header_for(&prepared.source) {
            Ok(header) => header,
            Err(response) => return response,
        };
        self.write_status(&prepared.appid, &FixStatus::extracting(0, 0));
        let plan = match build_apply_plan(
            self.fs.as_ref(),
            &catalog,
            &self.roots,
            request,
            layout,
            auth_header.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(error) => return self.fail(&prepared.appid, &error),
        };
        match crate::install::apply(self.fs.as_ref(), &plan.install) {
            Ok(_) => {
                self.write_status(&prepared.appid, &FixStatus::extracted(0, 0));
                ApplyResponse::ok()
            }
            Err(error) => self.fail(&prepared.appid, &FixError::from(error)),
        }
    }

    // ── installed/unfix/redirect ──────────────────────────────────────────

    fn install_root_for(&self, install_path: Option<&str>) -> Result<PathBuf, PathError> {
        let Some(path) = install_path.filter(|path| !path.is_empty()) else {
            return Err(PathError(ERR_PATH_REJECTED.into()));
        };
        self.roots.validate_install_path(path)
    }

    /// `fixes:installed`; `appid` é aceito por contrato, mas o teste do JS é
    /// somente a existência do manifest na instalação.
    pub fn installed(&self, _appid: &str, install_path: Option<&str>) -> InstalledResponse {
        let install_path = match install_path.filter(|path| !path.is_empty()) {
            Some(path) => path,
            None => return InstalledResponse::ok(false),
        };
        let root = match self.roots.validate_install_path(install_path) {
            Ok(root) => root,
            Err(error) => return InstalledResponse::error(error.0),
        };
        InstalledResponse::ok(is_fixed(self.fs.as_ref(), &root))
    }

    /// `fixes:unfix`.
    pub fn unfix(&self, appid: &str, install_path: Option<&str>) -> UnfixResponse {
        if !self.roots_ready() {
            return UnfixResponse::error("invalid internal roots");
        }
        let root = match self.install_root_for(install_path) {
            Ok(root) => root,
            Err(error) => return UnfixResponse::error(error.0),
        };
        let appid = normalize_appid(appid);
        let hint = appid_path_component(&appid);
        match crate::install::unfix(
            self.fs.as_ref(),
            &root,
            &self.roots.backup_root,
            hint.as_deref(),
        ) {
            Ok(_) => UnfixResponse::ok(),
            Err(error) => UnfixResponse::error(error.0),
        }
    }

    /// `fixes:launcherRedirect`.
    pub fn launcher_redirect(&self, install_path: Option<&str>) -> LauncherRedirectResponse {
        let root = match self.install_root_for(install_path) {
            Ok(root) => root,
            Err(error) => return LauncherRedirectResponse::error(error.0),
        };
        LauncherRedirectResponse::ok(crate::redirect::build_launcher_redirect(
            self.fs.as_ref(),
            &root,
        ))
    }

    // ── auth Ryuu ─────────────────────────────────────────────────────────

    /// `fixes:setRyuuAuth`.
    pub fn set_ryuu_auth(&self, raw: &str) -> RyuuSetResponse {
        if !self.roots_ready() {
            return RyuuSetResponse::error("invalid internal roots");
        }
        match self.auth_store().set(raw) {
            Ok(auth) => RyuuSetResponse::ok(auth.kind.as_str()),
            Err(RyuuAuthError::InvalidCredential) => RyuuSetResponse::error(ERR_INVALID_CREDENTIAL),
            Err(RyuuAuthError::Storage(error)) => RyuuSetResponse::error(error.message),
        }
    }

    /// `fixes:ryuuAuthStatus`.
    pub fn ryuu_auth_status(&self) -> RyuuAuthStatus {
        if !self.roots_ready() {
            return RyuuAuthStatus::empty();
        }
        self.auth_store().status()
    }

    /// Diagnóstico mascarado (não é canal da UI, mas nunca vaza a key).
    pub fn ryuu_auth_masked_status(&self) -> crate::redirect::MaskedRyuuAuthStatus {
        if !self.roots_ready() {
            return crate::redirect::MaskedRyuuAuthStatus {
                configured: false,
                kind: None,
                masked: None,
            };
        }
        self.auth_store().masked_status()
    }

    /// `fixes:clearRyuuAuth`.
    pub fn clear_ryuu_auth(&self) -> OkResponse {
        if self.roots_ready() {
            let _ = self.auth_store().clear();
        }
        OkResponse::ok()
    }

    /// Plano sem tocar em disco (diagnóstico/testes do contrato).
    pub fn plan(
        &self,
        request: &ApplyRequest,
        layout: &ArchiveLayout,
    ) -> Result<ApplyPlan, FixError> {
        let catalog = self.catalog_snapshot();
        let prepared = prepare_request(&catalog, &self.roots, request)?;
        let auth_header = if prepared.source.requires_auth && self.roots_ready() {
            self.auth_store().read().and_then(|auth| auth.header_line())
        } else {
            None
        };
        build_apply_plan(
            self.fs.as_ref(),
            &catalog,
            &self.roots,
            request,
            layout,
            auth_header.as_deref(),
        )
    }
}

/// Conveniência: roots com uma raiz de jogo e diretórios derivados.
pub fn single_root_roots(
    game_root: impl Into<PathBuf>,
    data_root: impl Into<PathBuf>,
) -> PathRoots {
    let game_root = game_root.into();
    let data_root = data_root.into();
    PathRoots::new(
        vec![game_root],
        Vec::new(),
        data_root.join("fix_backups"),
        data_root,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FixCatalog;
    use crate::download::{DownloadEvent, DownloadPlan, DownloadedArchive};
    use crate::install::{
        dll_manifest_path, launcher_manifest_path, read_manifest, ArchiveEntry, StdFixFs,
        INSTALL_STATE_FILE,
    };
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;

    fn roots(dir: &Path, game: &Path) -> PathRoots {
        PathRoots::new(
            vec![game.to_path_buf()],
            Vec::new(),
            dir.join("backups"),
            dir.join("data"),
        )
    }

    fn setup(
        fixes: serde_json::Value,
        ryuu: serde_json::Value,
        temp: &tempfile::TempDir,
    ) -> (FixEngine, PathBuf) {
        let game = temp.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        let catalog = FixCatalog::from_values(Some(&fixes), Some(&ryuu));
        let engine = FixEngine::new(catalog, Arc::new(StdFixFs), roots(temp.path(), &game));
        (engine, game)
    }

    fn generic_layout() -> ArchiveLayout {
        ArchiveLayout::new(vec![
            ArchiveEntry::file("root.dll", b"new".to_vec()),
            ArchiveEntry::file("launcher.exe", b"exe".to_vec()),
        ])
    }

    fn request(appid: &str, url: &str, install: &Path) -> ApplyRequest {
        ApplyRequest::new(
            appid,
            FixType::Generic,
            url,
            install.to_string_lossy().to_string(),
        )
    }

    #[test]
    fn check_envelope_matches_fixes_panel_contract() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(
            json!({"genericFixes": [730], "onlineFixes": [730]}),
            json!({"fixes": {"730": [{"file": "bypass.zip", "badge": "bypass"}]}}),
            &temp,
        );
        let value = serde_json::to_value(engine.check("steam:730")).unwrap();
        assert_eq!(
            value,
            json!({
                "ok": true,
                "appid": "730",
                "generic": {
                    "available": true,
                    "status": 200,
                    "url": "https://files.luatools.work/GameBypasses/730.zip"
                },
                "online": {
                    "available": true,
                    "status": 200,
                    "url": "https://files.luatools.work/OnlineFix1/730.zip"
                },
                "crack": {
                    "available": true,
                    "status": 200,
                    "url": "https://generator.ryuu.lol/fixes/bypass.zip",
                    "file": "bypass.zip",
                    "badge": "bypass",
                    "requiresAuth": true
                },
                "authConfigured": false
            })
        );
    }

    #[test]
    fn check_empty_appid_is_only_ok_false() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        assert_eq!(
            serde_json::to_value(engine.check("steam:")).unwrap(),
            json!({"ok": false})
        );
    }

    #[test]
    fn status_defaults_to_done_and_parse_failure_is_downloading() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        assert_eq!(engine.status("1"), FixStatus::done());
        assert_eq!(
            serde_json::to_value(engine.status("1")).unwrap(),
            json!({"status": "done"})
        );
        let state = temp.path().join("data/tmp/fix_1_state.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        std::fs::write(&state, b"{not json").unwrap();
        assert_eq!(
            serde_json::to_value(engine.status("1")).unwrap(),
            json!({"status": "downloading"})
        );
    }

    #[test]
    fn status_extracted_becomes_done_and_cleans_temp() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        let state = temp.path().join("data/tmp/fix_1_state.json");
        let zip = temp.path().join("data/tmp/fix_1.zip");
        let headers = temp.path().join("data/tmp/fix_1_headers.txt");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        for path in [&state, &zip, &headers] {
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::write(
            &state,
            serde_json::to_vec(&FixStatus::extracted(10, 20)).unwrap(),
        )
        .unwrap();
        assert_eq!(engine.status("1"), FixStatus::done());
        assert!(!state.exists());
        assert!(!zip.exists());
        assert!(!headers.exists());
    }

    #[test]
    fn status_failed_is_returned_once_then_cleaned() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        let state = temp.path().join("data/tmp/fix_1_state.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        std::fs::write(
            &state,
            serde_json::to_vec(&FixStatus::failed("boom", Some("badurl".into()))).unwrap(),
        )
        .unwrap();
        let first = engine.status("1");
        assert_eq!(first.status, STATUS_FAILED);
        assert_eq!(first.error.as_deref(), Some("boom"));
        assert_eq!(first.error_code.as_deref(), Some("badurl"));
        assert_eq!(engine.status("1"), FixStatus::done());
    }

    #[test]
    fn cancel_cleans_temp_but_keeps_installed_files() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        let state = temp.path().join("data/tmp/fix_1_state.json");
        let zip = temp.path().join("data/tmp/fix_1.zip");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        std::fs::write(&state, b"{}").unwrap();
        std::fs::write(&zip, b"x").unwrap();
        std::fs::write(game.join("kept.dll"), b"x").unwrap();
        assert_eq!(
            serde_json::to_value(engine.cancel("1")).unwrap(),
            json!({"ok": true})
        );
        assert!(!state.exists());
        assert!(!zip.exists());
        assert!(game.join("kept.dll").exists());
    }

    #[test]
    fn apply_archive_succeeds_and_installed_reports_true() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [730]}), json!({}), &temp);
        let url = "https://files.luatools.work/GameBypasses/730.zip";
        let response = engine.apply_archive(&request("steam:730", url, &game), &generic_layout());
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"ok": true})
        );
        assert!(game.join("root.dll").exists());
        assert!(is_fixed(engine.fs().as_ref(), &game));
        assert_eq!(
            serde_json::to_value(engine.installed("730", Some(&game.to_string_lossy()))).unwrap(),
            json!({"ok": true, "installed": true})
        );
        // O polling converte extracted em done e limpa o estado temporário.
        assert_eq!(engine.status("730"), FixStatus::done());
        assert!(!temp.path().join("data/tmp/fix_730_state.json").exists());
    }

    #[test]
    fn installed_missing_or_invalid_path_envelopes() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        assert_eq!(
            serde_json::to_value(engine.installed("1", None)).unwrap(),
            json!({"ok": true, "installed": false})
        );
        assert_eq!(
            serde_json::to_value(engine.installed("1", Some("relative"))).unwrap(),
            json!({"ok": false, "error": "installPath rejeitado"})
        );
        let outside = temp.path().join("outside");
        assert_eq!(
            serde_json::to_value(engine.installed("1", Some(&outside.to_string_lossy()))).unwrap(),
            json!({"ok": false, "error": "installPath fora das raízes de jogo/prefixo"})
        );
        assert!(!game.join("root.dll").exists());
    }

    #[test]
    fn apply_rejects_missing_args_url_and_install_path() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let layout = generic_layout();
        let cases = [
            (
                ApplyRequest::new(
                    "",
                    FixType::Generic,
                    "https://x",
                    game.to_string_lossy().to_string(),
                ),
                ERR_MISSING_ARGS,
            ),
            (
                ApplyRequest::new(
                    "1",
                    FixType::Generic,
                    "",
                    game.to_string_lossy().to_string(),
                ),
                ERR_MISSING_ARGS,
            ),
            (
                ApplyRequest::new(
                    "1",
                    FixType::Generic,
                    "https://files.luatools.work/GameBypasses/1.zip",
                    "",
                ),
                ERR_MISSING_ARGS,
            ),
            (
                ApplyRequest::new(
                    "1",
                    FixType::Generic,
                    "https://evil.test/1.zip",
                    game.to_string_lossy().to_string(),
                ),
                ERR_URL_REJECTED,
            ),
            (
                ApplyRequest::new(
                    "1",
                    FixType::Generic,
                    "https://files.luatools.work/GameBypasses/1.zip",
                    "relative",
                ),
                ERR_PATH_REJECTED,
            ),
        ];
        for (request, expected) in cases {
            let response = engine.apply_archive(&request, &layout);
            assert!(!response.ok);
            assert_eq!(response.error.as_deref(), Some(expected));
            assert!(!game.join("root.dll").exists());
        }
    }

    #[test]
    fn nested_archive_paths_are_validated_too() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        let layout = ArchiveLayout::new(vec![ArchiveEntry::file("pack.zip", b"pack".to_vec())])
            .with_nested(vec![crate::install::NestedArchive::new(
                "../pack.zip",
                vec![ArchiveEntry::file("ok.dll", b"x".to_vec())],
            )]);
        let error = engine.plan(&request, &layout).unwrap_err();
        assert_eq!(error.message, "archive entry path rejected");

        let layout = ArchiveLayout::new(vec![ArchiveEntry::file("pack.zip", b"pack".to_vec())])
            .with_nested(vec![crate::install::NestedArchive::new(
                "pack.zip",
                vec![ArchiveEntry::file("../evil.dll", b"x".to_vec())],
            )]);
        let error = engine.plan(&request, &layout).unwrap_err();
        assert_eq!(error.message, "archive entry path rejected");
    }

    #[test]
    fn apply_rejects_paths_outside_game_roots() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let request = request(
            "1",
            "https://files.luatools.work/GameBypasses/1.zip",
            &outside,
        );
        let response = engine.apply_archive(&request, &generic_layout());
        assert!(!response.ok);
        assert_eq!(
            response.error.as_deref(),
            Some("installPath fora das raízes de jogo/prefixo")
        );
    }

    #[test]
    fn archive_traversal_is_rejected_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        for bad in [
            ArchiveEntry::file("../evil.dll", b"x".to_vec()),
            ArchiveEntry::file("C:\\evil.dll", b"x".to_vec()),
            ArchiveEntry::file("/evil.dll", b"x".to_vec()),
            ArchiveEntry::file("sub/../../evil.dll", b"x".to_vec()),
            ArchiveEntry::file(".slssteam_fix_dlls", b"x".to_vec()),
        ] {
            let response = engine.apply_archive(&request, &ArchiveLayout::new(vec![bad]));
            assert!(!response.ok);
            assert_eq!(
                response.error.as_deref(),
                Some("archive entry path rejected")
            );
            assert!(!temp.path().join("evil.dll").exists());
            assert!(!game.join("evil.dll").exists());
        }
    }

    #[test]
    fn ryuu_apply_requires_auth_and_never_writes_before_it() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(
            json!({"genericFixes": []}),
            json!({"fixes": {"730": [{"file": "bypass.zip", "badge": "bypass"}]}}),
            &temp,
        );
        let url = "https://generator.ryuu.lol/fixes/bypass.zip";
        let mut request = request("730", url, &game);
        request.fix_type = FixType::Crack;
        let response = engine.apply_archive(&request, &generic_layout());
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({
                "ok": false,
                "error": ERR_RYUU_AUTH,
                "errorCode": "authentication"
            })
        );
        assert!(!game.join("root.dll").exists());
        assert!(!temp.path().join("data/tmp/fix_730_state.json").exists());

        let set = engine.set_ryuu_auth("X-Auth-Key: secret-key");
        assert_eq!(
            serde_json::to_value(set).unwrap(),
            json!({"ok": true, "kind": "key"})
        );
        assert_eq!(
            serde_json::to_value(engine.ryuu_auth_status()).unwrap(),
            json!({"configured": true, "kind": "key"})
        );
        let response = engine.apply_archive(&request, &generic_layout());
        assert!(response.ok);
        assert!(game.join("root.dll").exists());
    }

    #[test]
    fn apply_uses_catalog_release_metadata_for_size_and_hash() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(
            json!({"genericFixes": []}),
            json!({"fixes": {"730": [{"file": "bypass.zip", "badge": "bypass", "size": 3, "sha256": "abcd"}]}}),
            &temp,
        );
        let request = ApplyRequest::new(
            "730",
            FixType::Crack,
            "https://generator.ryuu.lol/fixes/bypass.zip",
            game.to_string_lossy().to_string(),
        );
        engine.set_ryuu_auth("key");
        let plan = engine.plan(&request, &generic_layout()).unwrap();
        assert_eq!(plan.download.expected_size, Some(3));
        assert_eq!(plan.download.expected_sha256.as_deref(), Some("abcd"));
        assert_eq!(
            plan.download.auth_header.as_deref(),
            Some("X-Auth-Key: key\n")
        );
        assert_eq!(plan.source_url, request.url);
        assert!(plan
            .preconditions
            .contains(&Precondition::RyuuAuthConfigured));
        assert!(plan
            .postconditions
            .contains(&Postcondition::LauncherRedirectAvailable));
    }

    #[test]
    fn apply_plan_archive_layout_expands_nested_and_skips_secondary_volumes() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let layout = ArchiveLayout::new(vec![
            ArchiveEntry::file("main.dll", b"m".to_vec()),
            ArchiveEntry::file("pack.zip", b"pack".to_vec()),
            ArchiveEntry::file("data.part02.rar", b"vol".to_vec()),
            ArchiveEntry::directory("empty/"),
        ])
        .with_nested(vec![crate::install::NestedArchive::new(
            "pack.zip",
            vec![ArchiveEntry::file("inner/hook.dll", b"h".to_vec())],
        )]);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        let plan = engine.plan(&request, &layout).unwrap();
        let targets: Vec<String> = plan
            .install
            .writes
            .iter()
            .map(|write| write.path.clone())
            .collect();
        assert_eq!(targets, vec!["main.dll", "inner/hook.dll"]);
        assert_eq!(plan.install.removals.len(), 2);
        assert_eq!(plan.install.manifests.dlls, vec!["hook.dll", "main.dll"]);
        assert_eq!(
            plan.cleanup.temp_paths,
            vec![
                temp.path().join("data/tmp/fix_1_state.json"),
                temp.path().join("data/tmp/fix_1.zip"),
                temp.path().join("data/tmp/fix_1_headers.txt")
            ]
        );
    }

    #[test]
    fn duplicated_entries_keep_the_last_one() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let layout = ArchiveLayout::new(vec![
            ArchiveEntry::file("same.dll", b"first".to_vec()),
            ArchiveEntry::file("same.dll", b"second".to_vec()),
        ]);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        let plan = engine.plan(&request, &layout).unwrap();
        assert_eq!(plan.install.writes.len(), 1);
        assert_eq!(plan.install.writes[0].data, b"second");
    }

    #[test]
    fn launcher_redirect_envelope_after_apply() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        engine.apply_archive(&request, &generic_layout());
        let value =
            serde_json::to_value(engine.launcher_redirect(Some(&game.to_string_lossy()))).unwrap();
        let expected = format!(
            "\"{}\" %command%",
            game.join("launcher.exe").to_string_lossy()
        );
        assert_eq!(value, json!({"ok": true, "redirect": expected}));
        assert_eq!(
            serde_json::to_value(engine.launcher_redirect(Some("relative"))).unwrap(),
            json!({"ok": false, "error": "installPath rejeitado"})
        );
    }

    #[test]
    fn launcher_redirect_without_manifest_is_ok_null() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        assert_eq!(
            serde_json::to_value(engine.launcher_redirect(Some(&game.to_string_lossy()))).unwrap(),
            json!({"ok": true, "redirect": null})
        );
    }

    #[test]
    fn status_envelopes_include_worker_byte_counts() {
        assert_eq!(
            serde_json::to_value(FixStatus::downloading(1, 2)).unwrap(),
            json!({"status": "downloading", "bytesRead": 1, "totalBytes": 2})
        );
        assert_eq!(
            serde_json::to_value(FixStatus::extracting(3, 4)).unwrap(),
            json!({"status": "extracting", "bytesRead": 3, "totalBytes": 4})
        );
        assert_eq!(
            serde_json::to_value(FixStatus::failed("x", None)).unwrap(),
            json!({"status": "failed", "error": "x"})
        );
    }

    #[test]
    fn dry_plan_records_backups_for_existing_targets() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        std::fs::write(game.join("root.dll"), b"original").unwrap();
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        let plan = engine.plan(&request, &generic_layout()).unwrap();
        assert_eq!(plan.install.backups.len(), 1);
        assert_eq!(plan.install.backups[0].path, "root.dll");
        assert_eq!(plan.install.backups[0].target, game.join("root.dll"));
        assert_eq!(
            plan.install.backups[0].backup,
            temp.path().join("backups/1/root.dll")
        );
    }

    #[test]
    fn check_reports_auth_configured_after_set() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(
            json!({"genericFixes": []}),
            json!({"fixes": {"730": [{"file": "bypass.zip"}]}}),
            &temp,
        );
        assert_eq!(engine.check("730").auth_configured, Some(false));
        engine.set_ryuu_auth("X-Auth-Key: abc");
        assert_eq!(engine.check("730").auth_configured, Some(true));
        assert_eq!(
            engine.check("730").crack.unwrap().badge.as_deref(),
            Some("")
        );
    }

    #[test]
    fn unfix_envelopes_and_idempotency() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        engine.apply_archive(&request, &generic_layout());
        assert!(is_fixed(engine.fs().as_ref(), &game));
        assert_eq!(
            serde_json::to_value(engine.unfix("1", None)).unwrap(),
            json!({"ok": false, "error": "installPath rejeitado"})
        );
        assert_eq!(
            serde_json::to_value(engine.unfix("1", Some(&game.to_string_lossy()))).unwrap(),
            json!({"ok": true})
        );
        assert!(!is_fixed(engine.fs().as_ref(), &game));
        assert!(!game.join("root.dll").exists());
        assert_eq!(
            serde_json::to_value(engine.unfix("1", Some(&game.to_string_lossy()))).unwrap(),
            json!({"ok": true})
        );
    }

    #[test]
    fn apply_twice_via_engine_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        assert!(engine.apply_archive(&request, &generic_layout()).ok);
        let before = std::fs::read(game.join("root.dll")).unwrap();
        assert!(engine.apply_archive(&request, &generic_layout()).ok);
        assert_eq!(std::fs::read(game.join("root.dll")).unwrap(), before);
    }

    #[test]
    fn dry_plan_does_not_write_and_requires_safe_layout() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        let plan = engine.plan(&request, &generic_layout()).unwrap();
        assert_eq!(plan.install_root, game);
        assert!(!game.join("root.dll").exists());
        let error = engine
            .plan(
                &request,
                &ArchiveLayout::new(vec![ArchiveEntry::file("../x.dll", b"x".to_vec())]),
            )
            .unwrap_err();
        assert_eq!(error.message, "archive entry path rejected");
    }

    #[test]
    fn auth_envelopes_and_masked_status_do_not_leak_secret() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, _game) = setup(json!({"genericFixes": []}), json!({}), &temp);
        assert_eq!(
            serde_json::to_value(engine.set_ryuu_auth("Authorization: Bearer x")).unwrap(),
            json!({"ok": false, "error": "invalid_credential"})
        );
        assert_eq!(
            serde_json::to_value(engine.ryuu_auth_status()).unwrap(),
            json!({"configured": false, "kind": null})
        );
        engine.set_ryuu_auth("my-super-secret");
        let status = serde_json::to_value(engine.ryuu_auth_status()).unwrap();
        assert_eq!(status, json!({"configured": true, "kind": "key"}));
        assert!(!status.to_string().contains("my-super-secret"));
        let masked = serde_json::to_value(engine.ryuu_auth_masked_status()).unwrap();
        assert_eq!(
            masked,
            json!({"configured": true, "kind": "key", "masked": "my-•••et"})
        );
        assert!(!masked.to_string().contains("my-super-secret"));
        assert_eq!(
            serde_json::to_value(engine.clear_ryuu_auth()).unwrap(),
            json!({"ok": true})
        );
        assert_eq!(
            serde_json::to_value(engine.ryuu_auth_status()).unwrap(),
            json!({"configured": false, "kind": null})
        );
    }

    #[test]
    fn download_failure_writes_failed_status_and_returns_code() {
        struct FailingDownloader;
        impl DownloadExecutor for FailingDownloader {
            fn download(
                &self,
                _plan: &DownloadPlan,
                _progress: &mut dyn FnMut(DownloadEvent),
            ) -> Result<DownloadedArchive, DownloadError> {
                Err(DownloadError::stalled())
            }
        }
        struct EmptyExtractor;
        impl ArchiveExtractor for EmptyExtractor {
            fn extract(&self, _archive: &Path) -> Result<ArchiveLayout, InstallError> {
                Ok(ArchiveLayout::default())
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [7]}), json!({}), &temp);
        let request = request("7", "https://files.luatools.work/GameBypasses/7.zip", &game);
        let response = engine.apply(&request, &FailingDownloader, &EmptyExtractor);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({
                "ok": false,
                "error": "Download stalled. Try again, or pick another source.",
                "errorCode": "stalled"
            })
        );
        let status = engine.status("7");
        assert_eq!(status.status, STATUS_FAILED);
        assert_eq!(status.error_code.as_deref(), Some("stalled"));
        assert_eq!(engine.status("7"), FixStatus::done());
    }

    #[test]
    fn successful_download_path_emits_progress_and_installs() {
        struct FakeDownloader;
        impl DownloadExecutor for FakeDownloader {
            fn download(
                &self,
                plan: &DownloadPlan,
                progress: &mut dyn FnMut(DownloadEvent),
            ) -> Result<DownloadedArchive, DownloadError> {
                progress(DownloadEvent::downloading(5, 10));
                progress(DownloadEvent::downloading(10, 10));
                progress(DownloadEvent::Extracting);
                Ok(DownloadedArchive::new(plan.destination.clone(), 10, None))
            }
        }
        struct StaticLayout(ArchiveLayout);
        impl ArchiveExtractor for StaticLayout {
            fn extract(&self, _archive: &Path) -> Result<ArchiveLayout, InstallError> {
                Ok(self.0.clone())
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [7]}), json!({}), &temp);
        let request = request("7", "https://files.luatools.work/GameBypasses/7.zip", &game);
        let response = engine.apply(&request, &FakeDownloader, &StaticLayout(generic_layout()));
        assert!(response.ok);
        assert!(is_fixed(engine.fs().as_ref(), &game));
        assert_eq!(engine.status("7"), FixStatus::done());
    }

    #[test]
    fn apply_writes_ryuu_auth_header_before_download() {
        struct HeaderCheckingDownloader;
        impl DownloadExecutor for HeaderCheckingDownloader {
            fn download(
                &self,
                plan: &DownloadPlan,
                _progress: &mut dyn FnMut(DownloadEvent),
            ) -> Result<DownloadedArchive, DownloadError> {
                let header = std::fs::read_to_string(&plan.headers_path)
                    .map_err(|_| DownloadError::authentication())?;
                assert_eq!(header, "X-Auth-Key: secret\n");
                Ok(DownloadedArchive::new(plan.destination.clone(), 0, None))
            }
        }
        struct StaticLayout(ArchiveLayout);
        impl ArchiveExtractor for StaticLayout {
            fn extract(&self, _archive: &Path) -> Result<ArchiveLayout, InstallError> {
                Ok(self.0.clone())
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(
            json!({"genericFixes": []}),
            json!({"fixes": {"730": [{"file": "bypass.zip", "badge": "bypass"}]}}),
            &temp,
        );
        engine.set_ryuu_auth("secret");
        let request = ApplyRequest::new(
            "730",
            FixType::Crack,
            "https://generator.ryuu.lol/fixes/bypass.zip",
            game.to_string_lossy().to_string(),
        );
        let response = engine.apply(
            &request,
            &HeaderCheckingDownloader,
            &StaticLayout(generic_layout()),
        );
        assert!(response.ok, "{response:?}");
        assert!(game.join("root.dll").exists());
        // O header temporário só some quando a UI faz o polling final.
        assert!(temp.path().join("data/tmp/fix_730_headers.txt").exists());
        assert_eq!(engine.status("730"), FixStatus::done());
        assert!(!temp.path().join("data/tmp/fix_730_headers.txt").exists());
    }

    #[test]
    fn download_returning_path_outside_data_root_is_rejected() {
        struct OutsideDownloader(std::path::PathBuf);
        impl DownloadExecutor for OutsideDownloader {
            fn download(
                &self,
                _plan: &DownloadPlan,
                _progress: &mut dyn FnMut(DownloadEvent),
            ) -> Result<DownloadedArchive, DownloadError> {
                Ok(DownloadedArchive::new(self.0.clone(), 0, None))
            }
        }
        struct EmptyExtractor;
        impl ArchiveExtractor for EmptyExtractor {
            fn extract(&self, _archive: &Path) -> Result<ArchiveLayout, InstallError> {
                Ok(ArchiveLayout::default())
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [7]}), json!({}), &temp);
        let request = request("7", "https://files.luatools.work/GameBypasses/7.zip", &game);
        let outside = temp.path().join("outside.zip");
        let response = engine.apply(&request, &OutsideDownloader(outside), &EmptyExtractor);
        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("download path rejected"));
        assert_eq!(engine.status("7").status, STATUS_FAILED);
    }

    #[test]
    fn apply_archive_writes_exact_manifest_lines() {
        let temp = tempfile::tempdir().unwrap();
        let (engine, game) = setup(json!({"genericFixes": [1]}), json!({}), &temp);
        let layout = ArchiveLayout::new(vec![
            ArchiveEntry::file("a/one.DLL", b"1".to_vec()),
            ArchiveEntry::file("two.dll", b"2".to_vec()),
            ArchiveEntry::file("bin/launcher_x.exe", b"3".to_vec()),
        ]);
        let request = request("1", "https://files.luatools.work/GameBypasses/1.zip", &game);
        engine.apply_archive(&request, &layout);
        assert_eq!(
            read_manifest(engine.fs().as_ref(), &dll_manifest_path(&game)),
            vec!["one.DLL", "two.dll"]
        );
        assert_eq!(
            read_manifest(engine.fs().as_ref(), &launcher_manifest_path(&game)),
            vec!["bin/launcher_x.exe"]
        );
        assert!(game.join(".arcadia_fix_state.json").exists());
        assert_eq!(INSTALL_STATE_FILE, ".arcadia_fix_state.json");
    }
}
