//! Updater do Arcadia — os dois canais do cliente Electron.
//!
//! Oráculo congelado (`app/electron`):
//! - [`git_channel`] espelha `updater.js` (quem roda da fonte);
//! - [`packaged`] espelha `updater-packaged.js` (AppImage/NSIS via
//!   electron-updater);
//! - [`version`] preserva a comparação `parseInt`/`||` do JS;
//! - [`manifest`] valida `latest.yml`, `latest-linux.yml` e `app-update.yml`.
//!
//! O crate não executa comandos, não acessa a rede e não instala nada: git,
//! filesystem, HTTP, eventos do updater e o reinício entram por traits
//! injetadas. A borda Tauri (e o `arcadia-ipc`) faz o wiring.

mod js;

pub mod git_channel;
pub mod manifest;
pub mod packaged;
pub mod version;

pub use git_channel::{
    ApplyResult, CommandOutput, CommandSpec, CommitInfo, FsProbe, GitChannel, GitRunner, HttpProbe,
    HttpRequest, HttpResponse, NoopRestart, RestartProbe, SystemFs, SystemGitRunner, UpdateCheck,
    UpdatePhase, UpdateProgress, UpdateReason, UpdateState, BRANCH, COMPARE_TIMEOUT_MS, DEPS_FILE,
    GITHUB_API, REPO,
};
pub use manifest::{
    app_update_envelope, latest_envelope, parse_app_update, parse_latest, parse_manifest,
    size_total, AppUpdateConfig, LatestManifest, Manifest, ManifestError, UpdateFile, APP_UPDATE,
    LATEST_LINUX, LATEST_WINDOWS,
};
pub use packaged::{
    current_platform, detect_channel, normalize_percent, tamanho_de, ActionResult, AutoUpdater,
    Canal, Channel, CheckForUpdatesResult, CheckReason, CheckResult, Env, ErroAcao, ErrorAction,
    EstadoEmpacotado, Fase, JaAvisadoResult, PackagedConfig, PackagedState, PackagedUpdater, Phase,
    SavePendingCallback, UpdateInfo, UpdaterEvent,
};
pub use version::{compare_versions, is_newer_version, parse_semver, version_parts};

/// Aliases em português dos helpers de versão.
pub use version::{comparar_versao, versao_mais_nova};

/// Mantido do stub original para consumidores que já verificavam o crate.
pub fn crate_ready() -> bool {
    true
}
