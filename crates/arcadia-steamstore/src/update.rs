//! Checagem de atualização de apps Steam instalados, a partir do ACF
//! (`appmanifest_<appid>.acf`) e do buildid mais recente do branch.
//!
//! O cliente Electron congelado não expõe um canal de update de jogo Steam;
//! este módulo segue o mesmo envelope dos outros handlers (`{ ok, ... }`) e
//! documenta os campos novos:
//!
//! ```json
//! { "ok": true, "updates": [ { "appid": "730", "title": "Counter-Strike 2",
//!   "installdir": "Counter-Strike Global Offensive", "buildid": "123",
//!   "latest_buildid": "124", "state_flags": 4, "status": "update_available",
//!   "needs_update": true, "arcadia_download": false,
//!   "size_on_disk": 123, "last_updated": 1700000000 } ],
//!   "total": 1, "pendentes": 1, "atualizados": 0 }
//! ```
//!
//! `state_flags` é o bitmask da Steam; `buildid` compara numericamente quando
//! possível. `download`/`stage`/`validate` em andamento são reportados como
//! `downloading`/`verifying` e não como "pendente" (o update já está agindo).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::depot::AppInfo;
use crate::library::AcfManifest;

/// `StateFlags` da Steam.
pub const STATE_UNINSTALLED: u64 = 1;
pub const STATE_UPDATE_REQUIRED: u64 = 2;
pub const STATE_FULLY_INSTALLED: u64 = 4;
pub const STATE_UPDATE_QUEUED: u64 = 8;
pub const STATE_UPDATE_OPTIONAL: u64 = 16;
pub const STATE_FILES_MISSING: u64 = 32;
pub const STATE_SHARED_ONLY: u64 = 64;
pub const STATE_FILES_CORRUPT: u64 = 128;
pub const STATE_UPDATE_RUNNING: u64 = 256;
pub const STATE_UPDATE_PAUSED: u64 = 512;
pub const STATE_UPDATE_STARTED: u64 = 1024;
pub const STATE_UNINSTALLING: u64 = 2048;
pub const STATE_BACKUP_RUNNING: u64 = 4096;
pub const STATE_RECONFIGURING: u64 = 8192;
pub const STATE_VALIDATING: u64 = 16384;
pub const STATE_ADDING_FILES: u64 = 32768;
pub const STATE_PREALLOCATING: u64 = 65536;
pub const STATE_DOWNLOADING: u64 = 131072;
pub const STATE_STAGING: u64 = 262144;
pub const STATE_COMMITTING: u64 = 524288;
pub const STATE_UPDATE_STOPPING: u64 = 1048576;

/// Estado resumido do app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    Downloading,
    Verifying,
    FilesMissing,
    FilesCorrupt,
    Uninstalling,
    NotInstalled,
    Unknown,
}

impl UpdateStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpToDate => "up_to_date",
            Self::UpdateAvailable => "update_available",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::FilesMissing => "files_missing",
            Self::FilesCorrupt => "files_corrupt",
            Self::Uninstalling => "uninstalling",
            Self::NotInstalled => "not_installed",
            Self::Unknown => "unknown",
        }
    }

    pub fn needs_update(self) -> bool {
        matches!(
            self,
            Self::UpdateAvailable | Self::FilesMissing | Self::FilesCorrupt
        )
    }
}

/// Buildid mais recente de um branch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestBuild {
    pub buildid: String,
    #[serde(default)]
    pub branch: String,
}

impl LatestBuild {
    pub fn new(buildid: impl Into<String>, branch: impl Into<String>) -> Self {
        Self {
            buildid: buildid.into(),
            branch: branch.into(),
        }
    }
}

/// Item do relatório de update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamUpdate {
    pub appid: String,
    pub title: String,
    pub installdir: String,
    pub buildid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_buildid: Option<String>,
    pub state_flags: u64,
    pub status: UpdateStatus,
    pub needs_update: bool,
    pub arcadia_download: bool,
    pub size_on_disk: u64,
    pub last_updated: u64,
}

/// Envelope `{ ok, updates, total, pendentes, atualizados }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateReport {
    pub ok: bool,
    #[serde(default)]
    pub updates: Vec<SteamUpdate>,
    pub total: usize,
    pub pendentes: usize,
    pub atualizados: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl UpdateReport {
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            updates: Vec::new(),
            total: 0,
            pendentes: 0,
            atualizados: 0,
            error: Some(message.into()),
        }
    }
}

/// Opções da checagem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckUpdatesOptions {
    pub branch: String,
    /// Inclui ACFs não instalados (`StateFlags & 4 == 0`).
    pub include_not_installed: bool,
}

impl Default for CheckUpdatesOptions {
    fn default() -> Self {
        Self {
            branch: crate::depot::DEFAULT_BRANCH.to_string(),
            include_not_installed: false,
        }
    }
}

/// `state_flags_status`: interpreta o bitmask sem comparar buildid.
pub fn state_flags_status(state_flags: u64) -> UpdateStatus {
    if state_flags == 0 {
        return UpdateStatus::Unknown;
    }
    if state_flags & STATE_UNINSTALLING != 0 {
        return UpdateStatus::Uninstalling;
    }
    if state_flags
        & (STATE_DOWNLOADING
            | STATE_STAGING
            | STATE_COMMITTING
            | STATE_PREALLOCATING
            | STATE_ADDING_FILES
            | STATE_UPDATE_RUNNING
            | STATE_UPDATE_STARTED)
        != 0
    {
        return UpdateStatus::Downloading;
    }
    if state_flags & STATE_VALIDATING != 0 {
        return UpdateStatus::Verifying;
    }
    if state_flags & STATE_FILES_MISSING != 0 {
        return UpdateStatus::FilesMissing;
    }
    if state_flags & STATE_FILES_CORRUPT != 0 {
        return UpdateStatus::FilesCorrupt;
    }
    if state_flags & STATE_UPDATE_PAUSED != 0 {
        return UpdateStatus::UpdateAvailable;
    }
    if state_flags & (STATE_UPDATE_REQUIRED | STATE_UPDATE_QUEUED | STATE_UPDATE_OPTIONAL) != 0 {
        return UpdateStatus::UpdateAvailable;
    }
    if state_flags & STATE_FULLY_INSTALLED == 0 {
        return UpdateStatus::NotInstalled;
    }
    UpdateStatus::UpToDate
}

/// Compara buildids como números quando ambos são numéricos; senão, por
/// desigualdade textual. `latest > installed` (ou instalado vazio).
pub fn buildid_is_newer(installed: &str, latest: &str) -> bool {
    let installed = installed.trim();
    let latest = latest.trim();
    if latest.is_empty() {
        return false;
    }
    if installed.is_empty() {
        return true;
    }
    match (installed.parse::<u64>(), latest.parse::<u64>()) {
        (Ok(installed), Ok(latest)) => latest > installed,
        _ => installed != latest,
    }
}

/// `buildid` do ACF (string; vazio quando ausente).
pub fn buildid_of(manifest: &AcfManifest) -> &str {
    manifest.buildid.as_str()
}

fn status_for(manifest: &AcfManifest, latest: Option<&LatestBuild>) -> UpdateStatus {
    let base = state_flags_status(manifest.state_flags);
    if !matches!(base, UpdateStatus::UpToDate | UpdateStatus::Unknown)
        && base != UpdateStatus::NotInstalled
    {
        return base;
    }
    if manifest.state_flags & STATE_FULLY_INSTALLED == 0 {
        return base;
    }
    let Some(latest) = latest else {
        return UpdateStatus::Unknown;
    };
    if latest.buildid.is_empty() || manifest.buildid.is_empty() {
        return UpdateStatus::Unknown;
    }
    if buildid_is_newer(&manifest.buildid, &latest.buildid) {
        UpdateStatus::UpdateAvailable
    } else {
        UpdateStatus::UpToDate
    }
}

/// Monta um item do relatório.
pub fn build_update(manifest: &AcfManifest, latest: Option<&LatestBuild>) -> SteamUpdate {
    let status = status_for(manifest, latest);
    SteamUpdate {
        appid: manifest.appid.clone(),
        title: manifest.name.clone(),
        installdir: manifest.installdir.clone(),
        buildid: manifest.buildid.clone(),
        latest_buildid: latest.map(|latest| latest.buildid.clone()),
        state_flags: manifest.state_flags,
        status,
        needs_update: status.needs_update(),
        arcadia_download: manifest.arcadia_download,
        size_on_disk: manifest.size_on_disk,
        last_updated: manifest.last_updated,
    }
}

/// Checagem a partir dos ACFs já lidos e do mapa `appid -> buildid`.
pub fn update_report(
    installed: &[AcfManifest],
    latest: &BTreeMap<String, LatestBuild>,
    options: &CheckUpdatesOptions,
) -> UpdateReport {
    let mut updates = Vec::new();
    for manifest in installed {
        if !options.include_not_installed
            && manifest.state_flags & STATE_FULLY_INSTALLED == 0
            && !matches!(
                state_flags_status(manifest.state_flags),
                UpdateStatus::Downloading | UpdateStatus::Verifying | UpdateStatus::UpdateAvailable
            )
        {
            continue;
        }
        updates.push(build_update(manifest, latest.get(&manifest.appid)));
    }
    let pendentes = updates.iter().filter(|update| update.needs_update).count();
    let atualizados = updates
        .iter()
        .filter(|update| update.status == UpdateStatus::UpToDate)
        .count();
    UpdateReport {
        ok: true,
        total: updates.len(),
        updates,
        pendentes,
        atualizados,
        error: None,
    }
}

/// Alias descritivo.
pub fn check_updates(
    installed: &[AcfManifest],
    latest: &BTreeMap<String, LatestBuild>,
    options: &CheckUpdatesOptions,
) -> UpdateReport {
    update_report(installed, latest, options)
}

/// Extrai `buildid` do branch de um [`AppInfo`] para o mapa de `latest`.
pub fn latest_from_app_info(info: &AppInfo, branch: &str) -> Option<LatestBuild> {
    crate::depot::latest_build_from_app_info(info, branch)
        .map(|buildid| LatestBuild::new(buildid, branch))
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
