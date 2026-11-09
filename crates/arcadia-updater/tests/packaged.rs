//! Testes do canal empacotado — espelham `app/test/updater-packaged.test.js`
//! e cobrem os casos-limite do spec (D6, D7, M3, N7/N8).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::json;

use arcadia_updater::manifest::UpdateFile;
use arcadia_updater::packaged::{
    tamanho_de, ActionResult, AutoUpdater, Channel, CheckForUpdatesResult, Env, ErrorAction,
    PackagedConfig, PackagedState, PackagedUpdater, Phase, UpdateInfo, UpdaterEvent,
};

#[derive(Clone)]
struct BackendData {
    auto_download: bool,
    auto_install_on_app_quit: bool,
    checks: usize,
    downloads: usize,
    installs: usize,
    check_result: Result<CheckForUpdatesResult, String>,
    download_result: Result<(), String>,
    quit_result: Result<(), String>,
    events_on_check: Vec<UpdaterEvent>,
    events_on_download: Vec<UpdaterEvent>,
    events_on_quit: Vec<UpdaterEvent>,
    events: Vec<UpdaterEvent>,
}

impl Default for BackendData {
    fn default() -> Self {
        Self {
            auto_download: true,
            auto_install_on_app_quit: true,
            checks: 0,
            downloads: 0,
            installs: 0,
            check_result: Ok(CheckForUpdatesResult::default()),
            download_result: Ok(()),
            quit_result: Ok(()),
            events_on_check: Vec::new(),
            events_on_download: Vec::new(),
            events_on_quit: Vec::new(),
            events: Vec::new(),
        }
    }
}

struct FakeBackend {
    data: Rc<RefCell<BackendData>>,
}

impl AutoUpdater for FakeBackend {
    fn configure(&mut self, auto_download: bool, auto_install_on_app_quit: bool) {
        let mut data = self.data.borrow_mut();
        data.auto_download = auto_download;
        data.auto_install_on_app_quit = auto_install_on_app_quit;
    }

    fn check_for_updates(&mut self) -> Result<CheckForUpdatesResult, String> {
        let mut data = self.data.borrow_mut();
        data.checks += 1;
        let events = data.events_on_check.clone();
        data.events.extend(events);
        data.check_result.clone()
    }

    fn download_update(&mut self) -> Result<(), String> {
        let mut data = self.data.borrow_mut();
        data.downloads += 1;
        let events = data.events_on_download.clone();
        data.events.extend(events);
        data.download_result.clone()
    }

    fn quit_and_install(&mut self) -> Result<(), String> {
        let mut data = self.data.borrow_mut();
        data.installs += 1;
        let events = data.events_on_quit.clone();
        data.events.extend(events);
        data.quit_result.clone()
    }

    fn take_events(&mut self) -> Vec<UpdaterEvent> {
        std::mem::take(&mut self.data.borrow_mut().events)
    }
}

#[derive(Clone, Default)]
struct Hooks {
    game_running: Rc<Cell<bool>>,
    advised: Rc<RefCell<Option<String>>>,
    saved_advised: Rc<RefCell<Vec<String>>>,
    pending: Rc<RefCell<Option<String>>>,
    saved_pending: Rc<RefCell<Vec<Option<String>>>>,
    changes: Rc<RefCell<Vec<PackagedState>>>,
}

struct Harness {
    updater: PackagedUpdater,
    hooks: Hooks,
    data: Rc<RefCell<BackendData>>,
}

fn harness_with(
    env: Env,
    has_app_update_yml: bool,
    platform: &str,
    version: &str,
    is_packaged: bool,
) -> Harness {
    let data = Rc::new(RefCell::new(BackendData::default()));
    let backend = FakeBackend { data: data.clone() };
    let hooks = Hooks::default();

    let game = hooks.clone();
    let advised = hooks.clone();
    let save_advised = hooks.clone();
    let pending = hooks.clone();
    let save_pending = hooks.clone();
    let on_change = hooks.clone();

    let config = PackagedConfig::new(is_packaged, version)
        .with_env(env)
        .with_app_update_yml(has_app_update_yml)
        .with_platform(platform)
        .with_game_running(move || game.game_running.get())
        .with_advised(move |candidate| advised.advised.borrow().as_deref() == Some(candidate))
        .with_save_advised(move |value| {
            save_advised
                .saved_advised
                .borrow_mut()
                .push(value.to_string());
            *save_advised.advised.borrow_mut() = Some(value.to_string());
        })
        .with_pending(move || pending.pending.borrow().clone())
        .with_save_pending(move |value| {
            save_pending
                .saved_pending
                .borrow_mut()
                .push(value.map(str::to_string));
            *save_pending.pending.borrow_mut() = value.map(str::to_string);
        })
        .with_on_change(move |state| on_change.changes.borrow_mut().push(state.clone()))
        .with_backend(backend);

    Harness {
        updater: PackagedUpdater::new(config),
        hooks,
        data,
    }
}

fn harness() -> Harness {
    harness_with(Env::default(), true, "win32", "1.4.1", true)
}

fn info(version: &str, size: u64) -> UpdateInfo {
    UpdateInfo::new(
        version,
        vec![UpdateFile::new("Arcadia.exe").with_size(size)],
    )
}

fn available(version: &str, size: u64) -> CheckForUpdatesResult {
    CheckForUpdatesResult::with_info(info(version, size))
}

fn state_json(harness: &Harness) -> serde_json::Value {
    serde_json::to_value(harness.updater.state()).unwrap()
}

#[test]
fn channel_fonte_when_not_packaged() {
    let harness = harness_with(Env::default(), true, "win32", "1.4.1", false);
    assert_eq!(harness.updater.channel(), Channel::Fonte);
    assert!(!harness.updater.supported());
    assert_eq!(harness.updater.state().fase, Phase::Ocioso);
    assert_eq!(state_json(&harness)["canal"], json!("fonte"));
}

#[test]
fn channel_appimage_on_linux_with_env() {
    let harness = harness_with(
        Env::default().with_appimage("/tmp/Arcadia.AppImage"),
        false,
        "linux",
        "1.4.1",
        true,
    );
    assert_eq!(harness.updater.channel(), Channel::AppImage);
    assert!(harness.updater.supported());
}

#[test]
fn channel_linux_without_appimage_is_sem_suporte_never_nsis() {
    let harness = harness_with(Env::default(), true, "linux", "1.4.1", true);
    assert_eq!(harness.updater.channel(), Channel::SemSuporte);
    assert!(!harness.updater.supported());
    assert_eq!(harness.updater.state().fase, Phase::SemSuporte);
}

#[test]
fn channel_portable_for_file_or_dir_and_zip_without_yml() {
    for env in [
        Env::default().with_portable_file("C:\\\\Arcadia.exe"),
        Env::default().with_portable_dir("C:\\\\Arcadia"),
    ] {
        let harness = harness_with(env, true, "win32", "1.4.1", true);
        assert_eq!(harness.updater.channel(), Channel::Portable);
        assert!(!harness.updater.supported());
    }

    let harness = harness_with(Env::default(), false, "win32", "1.4.1", true);
    assert_eq!(harness.updater.channel(), Channel::Zip);
    assert!(!harness.updater.supported());

    let harness = harness_with(Env::default(), true, "win32", "1.4.1", true);
    assert_eq!(harness.updater.channel(), Channel::Nsis);
    assert!(harness.updater.supported());
}

#[test]
fn empty_portable_env_does_not_detect_portable() {
    let env = Env {
        portable_executable_file: Some(String::new()),
        portable_executable_dir: Some(String::new()),
        ..Env::default()
    };
    let harness = harness_with(env, true, "win32", "1.4.1", true);
    assert_eq!(harness.updater.channel(), Channel::Nsis);
}

#[test]
fn other_platforms_are_sem_suporte() {
    let harness = harness_with(Env::default(), true, "darwin", "1.4.1", true);
    assert_eq!(harness.updater.channel(), Channel::SemSuporte);
}

#[test]
fn new_configures_auto_download_and_auto_install_off() {
    let harness = harness();
    let data = harness.data.borrow();
    assert!(!data.auto_download);
    assert!(!data.auto_install_on_app_quit);
}

#[test]
fn unsupported_channel_is_not_configured() {
    let harness = harness_with(
        Env::default().with_portable_file("x"),
        true,
        "win32",
        "1.4.1",
        true,
    );
    let data = harness.data.borrow();
    assert!(data.auto_download);
    assert!(data.auto_install_on_app_quit);
}

#[test]
fn check_does_not_download_extracts_size_and_goes_available() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 104_857_600));

    let result = harness.updater.check(false);
    assert!(result.ok);
    assert!(result.disponivel);
    assert_eq!(result.versao.as_deref(), Some("9.9.9"));
    assert_eq!(result.tamanho, Some(104_857_600));
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({
            "ok": true,
            "disponivel": true,
            "versao": "9.9.9",
            "tamanho": 104857600
        })
    );

    let data = harness.data.borrow();
    assert_eq!(data.checks, 1);
    assert_eq!(
        data.downloads, 0,
        "não pode baixar antes do usuário aceitar"
    );

    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Disponivel);
    assert_eq!(state.versao_nova.as_deref(), Some("9.9.9"));
    assert_eq!(state.tamanho, Some(104_857_600));
    assert!(!state.ja_avisado);
}

#[test]
fn check_available_without_size_serializes_null() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(CheckForUpdatesResult::with_info(UpdateInfo::new(
        "9.9.9",
        vec![],
    )));
    let result = harness.updater.check(false);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({
            "ok": true,
            "disponivel": true,
            "versao": "9.9.9",
            "tamanho": null
        })
    );
    assert_eq!(harness.updater.state().tamanho, None);
}

#[test]
fn check_not_available_goes_idle_and_serializes_version_null() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("1.4.1", 10));
    let result = harness.updater.check(false);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": true, "disponivel": false, "versao": null })
    );
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Ocioso);
    assert_eq!(state.versao_nova, None);
    assert_eq!(state.tamanho, None);
    assert_eq!(state.progresso, 0);
    assert!(!state.ja_avisado);
}

#[test]
fn check_empty_version_is_not_available() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result =
        Ok(CheckForUpdatesResult::with_info(UpdateInfo::default()));
    let result = harness.updater.check(false);
    assert!(!result.disponivel);
    assert_eq!(harness.updater.state().fase, Phase::Ocioso);
}

#[test]
fn check_unsupported_does_not_call_backend() {
    let mut harness = harness_with(
        Env::default().with_portable_file("x"),
        true,
        "win32",
        "1.4.1",
        true,
    );
    let result = harness.updater.check(false);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({
            "ok": false,
            "disponivel": false,
            "motivo": "canal_nao_suportado"
        })
    );
    assert_eq!(harness.updater.state().fase, Phase::SemSuporte);
    assert_eq!(harness.data.borrow().checks, 0);
}

#[test]
fn game_running_defers_automatic_check_but_manual_ignores() {
    let mut harness = harness();
    harness.hooks.game_running.set(true);
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));

    let result = harness.updater.check(false);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": true, "disponivel": false, "motivo": "jogo_rodando" })
    );
    assert_eq!(harness.data.borrow().checks, 0);
    assert!(harness.updater.state().jogo_rodando);

    let result = harness.updater.check(true);
    assert!(result.disponivel);
    assert_eq!(harness.data.borrow().checks, 1);
}

#[test]
fn version_comparison_is_numeric_in_the_check() {
    let mut harness = harness_with(Env::default(), true, "win32", "1.4.9", true);
    harness.data.borrow_mut().check_result = Ok(available("1.4.10", 10));
    assert!(harness.updater.check(false).disponivel);

    let mut harness = harness_with(Env::default(), true, "win32", "1.4.9", true);
    harness.data.borrow_mut().check_result = Ok(available("1.4.9", 10));
    assert!(!harness.updater.check(false).disponivel);

    let mut harness = harness_with(Env::default(), true, "win32", "1.4.9", true);
    harness.data.borrow_mut().check_result = Ok(available("v1.4.10", 10));
    assert!(harness.updater.check(false).disponivel);
}

#[test]
fn update_available_event_updates_state_and_advised_flag() {
    let mut harness = harness();
    *harness.hooks.advised.borrow_mut() = Some("9.9.9".into());
    harness
        .updater
        .process_event(UpdaterEvent::UpdateAvailable(info("9.9.9", 777)));

    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Disponivel);
    assert_eq!(state.versao_nova.as_deref(), Some("9.9.9"));
    assert_eq!(state.tamanho, Some(777));
    assert_eq!(state.progresso, 0);
    assert!(state.ja_avisado);
}

#[test]
fn download_progress_rounds_and_clamps() {
    let mut harness = harness();
    harness
        .updater
        .process_event(UpdaterEvent::DownloadProgress { percent: 42.6 });
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Baixando);
    assert_eq!(state.progresso, 43);

    for (percent, expected) in [(-5.0, 0), (150.0, 100), (f64::NAN, 0)] {
        harness
            .updater
            .process_event(UpdaterEvent::DownloadProgress { percent });
        assert_eq!(harness.updater.state().progresso, expected);
    }
}

#[test]
fn download_only_runs_when_called_and_reaches_ready() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    assert_eq!(harness.data.borrow().downloads, 0);

    harness.data.borrow_mut().events_on_download = vec![
        UpdaterEvent::DownloadProgress { percent: 42.6 },
        UpdaterEvent::UpdateDownloaded(info("9.9.9", 100)),
    ];

    let result = harness.updater.download();
    assert_eq!(result, ActionResult::ok());
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": true })
    );
    assert_eq!(harness.data.borrow().downloads, 1);

    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Pronto);
    assert_eq!(state.progresso, 100);
    assert!(harness
        .hooks
        .changes
        .borrow()
        .iter()
        .any(|state| state.fase == Phase::Baixando && state.progresso == 43));
    assert_eq!(
        harness.hooks.saved_pending.borrow().as_slice(),
        &[Some("9.9.9".to_string())]
    );
}

#[test]
fn download_without_offered_version_fails_with_sem_versao() {
    let mut harness = harness();
    let result = harness.updater.download();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": false, "erro": "sem_versao" })
    );
    assert_eq!(harness.data.borrow().downloads, 0);
}

#[test]
fn download_unsupported_channel_fails() {
    let mut harness = harness_with(
        Env::default().with_portable_file("x"),
        true,
        "win32",
        "1.4.1",
        true,
    );
    assert_eq!(
        serde_json::to_value(harness.updater.download()).unwrap(),
        json!({ "ok": false, "erro": "canal_nao_suportado" })
    );
}

#[test]
fn download_success_without_event_still_marks_ready() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    assert_eq!(harness.updater.download(), ActionResult::ok());
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Pronto);
    assert_eq!(state.progresso, 100);
}

#[test]
fn update_downloaded_persists_pending_and_uses_state_version_fallback() {
    let mut harness = harness();
    harness
        .updater
        .process_event(UpdaterEvent::UpdateAvailable(info("9.9.9", 100)));
    harness
        .updater
        .process_event(UpdaterEvent::UpdateDownloaded(UpdateInfo::default()));

    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Pronto);
    assert_eq!(state.versao_nova.as_deref(), Some("9.9.9"));
    assert_eq!(state.progresso, 100);
    assert_eq!(
        harness.hooks.saved_pending.borrow().as_slice(),
        &[Some("9.9.9".to_string())]
    );
}

#[test]
fn update_not_available_clears_pending_and_advised() {
    let mut harness = harness();
    *harness.hooks.pending.borrow_mut() = Some("9.9.9".into());
    *harness.hooks.advised.borrow_mut() = Some("9.9.9".into());
    harness
        .updater
        .process_event(UpdaterEvent::UpdateNotAvailable);

    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Ocioso);
    assert_eq!(state.versao_nova, None);
    assert_eq!(state.tamanho, None);
    assert_eq!(state.progresso, 0);
    assert!(!state.ja_avisado);
    assert_eq!(harness.hooks.saved_pending.borrow().as_slice(), &[None]);
    assert!(harness.hooks.pending.borrow().is_none());
}

#[test]
fn update_not_available_event_during_check_clears_pending_once() {
    let mut harness = harness();
    harness.data.borrow_mut().events_on_check = vec![UpdaterEvent::UpdateNotAvailable];
    harness.data.borrow_mut().check_result = Ok(available("1.4.1", 10));

    let result = harness.updater.check(false);
    assert!(!result.disponivel);
    assert_eq!(harness.updater.state().fase, Phase::Ocioso);
    assert_eq!(harness.hooks.saved_pending.borrow().as_slice(), &[None]);
}

#[test]
fn automatic_error_is_background_and_manual_error_offers_retry() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Err("sem rede".into());

    let result = harness.updater.check(false);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": false, "disponivel": false, "erro": "sem rede", "motivo": "erro" })
    );
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert_eq!(state.erro.as_deref(), Some("sem rede"));
    assert!(state.erro_de_fundo);
    assert_eq!(state.erro_acao, None);

    let result = harness.updater.check(true);
    assert!(!result.ok);
    let state = harness.updater.state();
    assert!(!state.erro_de_fundo);
    assert_eq!(state.erro_acao, Some(ErrorAction::Checar));
}

#[test]
fn error_event_outside_action_is_background() {
    let mut harness = harness();
    harness
        .updater
        .process_event(UpdaterEvent::Error("boom".into()));
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert_eq!(state.erro.as_deref(), Some("boom"));
    assert!(state.erro_de_fundo);
    assert_eq!(state.erro_acao, None);
}

#[test]
fn empty_error_message_falls_back_to_erro() {
    let mut harness = harness();
    harness
        .updater
        .process_event(UpdaterEvent::Error(String::new()));
    assert_eq!(harness.updater.state().erro.as_deref(), Some("erro"));
}

#[test]
fn download_failure_is_action_error_with_retry_baixar() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    harness.data.borrow_mut().download_result = Err("caiu a rede".into());

    let result = harness.updater.download();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": false, "erro": "caiu a rede" })
    );
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert_eq!(state.erro_acao, Some(ErrorAction::Baixar));
    assert!(!state.erro_de_fundo);
}

#[test]
fn ja_avisei_is_read_and_manual_check_reopens() {
    let mut harness = harness();
    *harness.hooks.advised.borrow_mut() = Some("9.9.9".into());
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));

    harness.updater.check(false);
    assert!(harness.updater.state().ja_avisado);

    let result = harness.updater.mark_advised(Some("9.9.9"));
    assert_eq!(
        result,
        arcadia_updater::packaged::JaAvisadoResult { ok: true }
    );
    assert_eq!(
        harness.hooks.saved_advised.borrow().as_slice(),
        &["9.9.9".to_string()]
    );

    harness.updater.check(true);
    assert!(
        !harness.updater.state().ja_avisado,
        "checagem manual mostra mesmo com 'já avisei'"
    );
}

#[test]
fn mark_advised_falls_back_to_offered_version_and_rejects_empty() {
    let mut harness = harness();
    assert_eq!(
        harness.updater.mark_advised(None),
        arcadia_updater::packaged::JaAvisadoResult { ok: false }
    );
    assert!(harness.hooks.saved_advised.borrow().is_empty());

    harness
        .updater
        .process_event(UpdaterEvent::UpdateAvailable(info("9.9.9", 100)));
    assert_eq!(
        harness.updater.mark_advised(None),
        arcadia_updater::packaged::JaAvisadoResult { ok: true }
    );
    assert!(harness.updater.state().ja_avisado);
    assert_eq!(
        harness.hooks.saved_advised.borrow().as_slice(),
        &["9.9.9".to_string()]
    );

    // Versão diferente da oferecida persiste, mas não marca o snapshot atual.
    harness.updater.mark_advised(Some("10.0.0"));
    assert_eq!(
        harness.hooks.saved_advised.borrow().as_slice(),
        &["9.9.9".to_string(), "10.0.0".to_string()]
    );
}

#[test]
fn pending_boot_reuses_cache_and_reaches_ready() {
    let mut harness = harness();
    *harness.hooks.pending.borrow_mut() = Some("9.9.9".into());
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.data.borrow_mut().events_on_download =
        vec![UpdaterEvent::UpdateDownloaded(info("9.9.9", 100))];

    let result = harness.updater.check(false);
    assert!(result.disponivel);
    assert_eq!(harness.data.borrow().downloads, 1);
    assert_eq!(harness.updater.state().fase, Phase::Pronto);
    assert_eq!(harness.updater.state().progresso, 100);
}

#[test]
fn pending_cache_redownloads_when_download_update_succeeds_without_event() {
    let mut harness = harness();
    *harness.hooks.pending.borrow_mut() = Some("9.9.9".into());
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));

    let result = harness.updater.check(false);
    assert!(result.disponivel);
    assert_eq!(harness.data.borrow().downloads, 1);
    assert_eq!(harness.updater.state().fase, Phase::Pronto);
    assert_eq!(harness.updater.state().progresso, 100);
}

#[test]
fn pending_download_failure_in_manual_check_is_retry_checar() {
    let mut harness = harness();
    *harness.hooks.pending.borrow_mut() = Some("9.9.9".into());
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.data.borrow_mut().download_result = Err("cache sumiu".into());

    let result = harness.updater.check(true);
    assert!(!result.ok);
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert_eq!(state.erro_acao, Some(ErrorAction::Checar));
    assert!(!state.erro_de_fundo);
}

#[test]
fn pending_download_failure_in_auto_check_is_background() {
    let mut harness = harness();
    *harness.hooks.pending.borrow_mut() = Some("9.9.9".into());
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.data.borrow_mut().download_result = Err("cache sumiu".into());

    harness.updater.check(false);
    let state = harness.updater.state();
    assert_eq!(state.erro_acao, None);
    assert!(state.erro_de_fundo);
}

#[test]
fn install_requires_ready_and_calls_quit_and_install() {
    let mut harness = harness();
    assert_eq!(
        serde_json::to_value(harness.updater.install()).unwrap(),
        json!({ "ok": false, "erro": "sem_download" })
    );
    assert_eq!(harness.data.borrow().installs, 0);

    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    harness.updater.download();
    assert_eq!(harness.updater.state().fase, Phase::Pronto);

    assert_eq!(
        serde_json::to_value(harness.updater.install()).unwrap(),
        json!({ "ok": true })
    );
    assert_eq!(harness.data.borrow().installs, 1);
}

#[test]
fn install_unsupported_channel_fails() {
    let mut harness = harness_with(
        Env::default().with_portable_file("x"),
        true,
        "win32",
        "1.4.1",
        true,
    );
    assert_eq!(
        serde_json::to_value(harness.updater.install()).unwrap(),
        json!({ "ok": false, "erro": "canal_nao_suportado" })
    );
}

#[test]
fn quit_and_install_failure_is_action_error_instalar() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    harness.updater.download();
    harness.data.borrow_mut().quit_result = Err("sem permissao de escrita".into());

    let result = harness.updater.install();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "ok": false, "erro": "sem permissao de escrita" })
    );
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert_eq!(state.erro_acao, Some(ErrorAction::Instalar));
    assert!(!state.erro_de_fundo);
}

#[test]
fn error_event_during_quit_is_background_but_install_returns_ok() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    harness.updater.download();
    harness.data.borrow_mut().events_on_quit = vec![UpdaterEvent::Error("evento".into())];

    assert_eq!(harness.updater.install(), ActionResult::ok());
    let state = harness.updater.state();
    assert_eq!(state.fase, Phase::Erro);
    assert!(state.erro_de_fundo);
    assert_eq!(state.erro_acao, None);
}

#[test]
fn manual_check_always_republishes_but_auto_same_version_does_not() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    let after_first = harness.hooks.changes.borrow().len();

    harness.updater.check(false);
    assert_eq!(
        harness.hooks.changes.borrow().len(),
        after_first,
        "mesma versão disponível não republica no ciclo automático"
    );

    harness.updater.check(true);
    assert!(harness.hooks.changes.borrow().len() > after_first);
}

#[test]
fn state_snapshot_has_exact_envelope_and_dynamic_game_flag() {
    let harness = harness();
    assert_eq!(
        state_json(&harness),
        json!({
            "canal": "nsis",
            "suportado": true,
            "versaoAtual": "1.4.1",
            "versaoNova": null,
            "tamanho": null,
            "fase": "ocioso",
            "progresso": 0,
            "erro": null,
            "erroDeFundo": false,
            "erroAcao": null,
            "jaAvisado": false,
            "jogoRodando": false
        })
    );

    harness.hooks.game_running.set(true);
    assert!(harness.updater.state().jogo_rodando);
    assert!(harness
        .hooks
        .changes
        .borrow()
        .iter()
        .all(|state| !state.jogo_rodando));
}

#[test]
fn state_after_ready_serializes_full_contract() {
    let mut harness = harness();
    harness.data.borrow_mut().check_result = Ok(available("9.9.9", 100));
    harness.updater.check(false);
    harness.updater.download();

    assert_eq!(
        state_json(&harness),
        json!({
            "canal": "nsis",
            "suportado": true,
            "versaoAtual": "1.4.1",
            "versaoNova": "9.9.9",
            "tamanho": 100,
            "fase": "pronto",
            "progresso": 100,
            "erro": null,
            "erroDeFundo": false,
            "erroAcao": null,
            "jaAvisado": false,
            "jogoRodando": false
        })
    );
}

#[test]
fn tamanho_de_sums_files_and_ignores_missing_sizes() {
    let info = UpdateInfo::new(
        "9.9.9",
        vec![
            UpdateFile::new("a").with_size(100),
            UpdateFile::new("b"),
            UpdateFile::new("c").with_size(23),
        ],
    );
    assert_eq!(tamanho_de(&info), Some(123));
    assert_eq!(tamanho_de(&UpdateInfo::default()), None);
}

#[test]
fn update_info_from_value_is_tolerant() {
    let info = UpdateInfo::from_value(&json!({
        "version": 9.9,
        "files": [
            {"url": "a", "size": "100", "blockMapSize": 7},
            {"url": "b", "size": -3}
        ]
    }));
    assert_eq!(info.version, "9.9");
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.files[0].size, Some(100));
    assert_eq!(info.files[0].block_map_size, Some(7));
    assert_eq!(info.files[1].size, None);

    let info = UpdateInfo::from_value(&json!(null));
    assert_eq!(info, UpdateInfo::default());
}
