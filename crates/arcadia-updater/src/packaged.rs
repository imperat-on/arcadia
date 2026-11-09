//! Canal empacotado (electron-updater / Tauri updater).
//!
//! Port 1:1 de `app/electron/updater-packaged.js`: detecção de canal por
//! plataforma, máquina de estados (`ocioso` → `disponivel` → `baixando` →
//! `pronto`/`erro`, além de `sem_suporte`), tamanho/progresso, `jaAvisado`,
//! cache `pendente` e a intenção de `quitAndInstall`.
//!
//! Nada aqui baixa ou instala: o `AutoUpdater` é uma trait injetada. Eventos
//! assíncronos do backend entram por [`PackagedUpdater::process_event`] ou pela
//! fila de [`AutoUpdater::take_events`], drenada após cada chamada.

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js;
use crate::manifest::{size_total, UpdateFile};
use crate::version::is_newer_version;

/// Variáveis de ambiente relevantes para detectar o canal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub appimage: Option<String>,
    pub portable_executable_file: Option<String>,
    pub portable_executable_dir: Option<String>,
}

impl Env {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut env = Self::default();
        for (key, value) in pairs {
            let key = key.into();
            let value = value.into();
            match key.as_str() {
                "APPIMAGE" => env.appimage = Some(value),
                "PORTABLE_EXECUTABLE_FILE" => env.portable_executable_file = Some(value),
                "PORTABLE_EXECUTABLE_DIR" => env.portable_executable_dir = Some(value),
                _ => {}
            }
        }
        env
    }

    /// Lê as três variáveis do processo (borda Tauri).
    pub fn from_process() -> Self {
        Self {
            appimage: std::env::var("APPIMAGE").ok(),
            portable_executable_file: std::env::var("PORTABLE_EXECUTABLE_FILE").ok(),
            portable_executable_dir: std::env::var("PORTABLE_EXECUTABLE_DIR").ok(),
        }
    }

    pub fn with_appimage(mut self, value: impl Into<String>) -> Self {
        self.appimage = Some(value.into());
        self
    }

    pub fn with_portable_file(mut self, value: impl Into<String>) -> Self {
        self.portable_executable_file = Some(value.into());
        self
    }

    pub fn with_portable_dir(mut self, value: impl Into<String>) -> Self {
        self.portable_executable_dir = Some(value.into());
        self
    }

    fn present(value: &Option<String>) -> bool {
        value.as_deref().is_some_and(|value| !value.is_empty())
    }
}

/// `process.platform` equivalente.
pub fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    }
}

/// Canal do app empacotado, por plataforma (B4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    #[serde(rename = "fonte")]
    Fonte,
    #[serde(rename = "appimage")]
    AppImage,
    #[serde(rename = "nsis")]
    Nsis,
    #[serde(rename = "portable")]
    Portable,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "sem_suporte")]
    SemSuporte,
}

impl Channel {
    /// `CANAIS_SUPORTADOS = {nsis, appimage}`.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Nsis | Self::AppImage)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fonte => "fonte",
            Self::AppImage => "appimage",
            Self::Nsis => "nsis",
            Self::Portable => "portable",
            Self::Zip => "zip",
            Self::SemSuporte => "sem_suporte",
        }
    }
}

/// `detectarCanal({ app, env, temAppUpdateYml, platform })`.
///
/// - não empacotado → `fonte`;
/// - Linux: `APPIMAGE` define AppImage; sem ela é `sem_suporte`, NUNCA `nsis`;
/// - Windows: env do stub portable; sem `app-update.yml` é `zip`; com ele,
///   `nsis`;
/// - outras plataformas → `sem_suporte`.
pub fn detect_channel(
    is_packaged: bool,
    env: &Env,
    has_app_update_yml: bool,
    platform: &str,
) -> Channel {
    if !is_packaged {
        return Channel::Fonte;
    }
    if platform == "linux" {
        return if Env::present(&env.appimage) {
            Channel::AppImage
        } else {
            Channel::SemSuporte
        };
    }
    if platform == "win32" {
        if Env::present(&env.portable_executable_file) || Env::present(&env.portable_executable_dir)
        {
            return Channel::Portable;
        }
        return if has_app_update_yml {
            Channel::Nsis
        } else {
            Channel::Zip
        };
    }
    Channel::SemSuporte
}

/// Fases do estado empacotado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[serde(rename = "ocioso")]
    Ocioso,
    #[serde(rename = "disponivel")]
    Disponivel,
    #[serde(rename = "baixando")]
    Baixando,
    #[serde(rename = "pronto")]
    Pronto,
    #[serde(rename = "erro")]
    Erro,
    #[serde(rename = "sem_suporte")]
    SemSuporte,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ocioso => "ocioso",
            Self::Disponivel => "disponivel",
            Self::Baixando => "baixando",
            Self::Pronto => "pronto",
            Self::Erro => "erro",
            Self::SemSuporte => "sem_suporte",
        }
    }
}

/// Qual ação do usuário gerou o erro (define o retry do diálogo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorAction {
    #[serde(rename = "checar")]
    Checar,
    #[serde(rename = "baixar")]
    Baixar,
    #[serde(rename = "instalar")]
    Instalar,
}

impl ErrorAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checar => "checar",
            Self::Baixar => "baixar",
            Self::Instalar => "instalar",
        }
    }
}

/// Snapshot enviado ao renderer (`update:packaged:changed` / `...:state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagedState {
    pub canal: Channel,
    pub suportado: bool,
    pub versao_atual: String,
    pub versao_nova: Option<String>,
    pub tamanho: Option<u64>,
    pub fase: Phase,
    pub progresso: u8,
    pub erro: Option<String>,
    pub erro_de_fundo: bool,
    pub erro_acao: Option<ErrorAction>,
    pub ja_avisado: bool,
    pub jogo_rodando: bool,
}

/// `info` de uma atualização (o `updateInfo` do electron-updater).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: Vec<UpdateFile>,
}

impl UpdateInfo {
    pub fn new(version: impl Into<String>, files: Vec<UpdateFile>) -> Self {
        Self {
            version: version.into(),
            files,
        }
    }

    /// Conversão tolerante do `updateInfo` vindo do backend.
    pub fn from_value(value: &Value) -> Self {
        let version = js::string_or_empty(value.get("version"));
        let files = value
            .get("files")
            .and_then(Value::as_array)
            .map(|files| files.iter().map(UpdateFile::from_value).collect())
            .unwrap_or_default();
        Self { version, files }
    }
}

/// `tamanhoDe(info)`: soma dos `files[].size` positivos, ou `None`.
pub fn tamanho_de(info: &UpdateInfo) -> Option<u64> {
    size_total(&info.files)
}

/// Resultado de `autoUpdater.checkForUpdates()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckForUpdatesResult {
    pub update_info: Option<UpdateInfo>,
}

impl CheckForUpdatesResult {
    pub fn new(update_info: Option<UpdateInfo>) -> Self {
        Self { update_info }
    }

    pub fn with_info(update_info: UpdateInfo) -> Self {
        Self {
            update_info: Some(update_info),
        }
    }
}

/// Eventos que o electron-updater emite e o módulo consome.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdaterEvent {
    UpdateAvailable(UpdateInfo),
    DownloadProgress { percent: f64 },
    UpdateDownloaded(UpdateInfo),
    UpdateNotAvailable,
    Error(String),
}

/// Backend de update injetado. A implementação Tauri delega ao plugin; os
/// testes usam um fake e nunca tocam a rede.
pub trait AutoUpdater {
    /// `autoDownload = false`, `autoInstallOnAppQuit = false`.
    fn configure(&mut self, auto_download: bool, auto_install_on_app_quit: bool);

    fn check_for_updates(&mut self) -> Result<CheckForUpdatesResult, String>;

    fn download_update(&mut self) -> Result<(), String>;

    fn quit_and_install(&mut self) -> Result<(), String>;

    /// Eventos acumulados desde a última drenagem.
    fn take_events(&mut self) -> Vec<UpdaterEvent> {
        Vec::new()
    }
}

/// Callback que persiste (`Some`) ou limpa (`None`) a versão pendente.
pub type SavePendingCallback = Box<dyn FnMut(Option<&str>)>;

/// Dependências injetáveis do updater empacotado.
pub struct PackagedConfig {
    pub is_packaged: bool,
    pub current_version: String,
    pub env: Env,
    pub has_app_update_yml: bool,
    pub platform: String,
    pub is_game_running: Box<dyn Fn() -> bool>,
    pub previously_advised: Box<dyn Fn(&str) -> bool>,
    pub save_advised: Box<dyn FnMut(&str)>,
    pub pending: Box<dyn Fn() -> Option<String>>,
    pub save_pending: SavePendingCallback,
    pub on_change: Box<dyn FnMut(&PackagedState)>,
    pub backend: Option<Box<dyn AutoUpdater>>,
}

impl Default for PackagedConfig {
    fn default() -> Self {
        Self {
            is_packaged: false,
            current_version: String::new(),
            env: Env::default(),
            has_app_update_yml: false,
            platform: current_platform().to_string(),
            is_game_running: Box::new(|| false),
            previously_advised: Box::new(|_| false),
            save_advised: Box::new(|_| {}),
            pending: Box::new(|| None),
            save_pending: Box::new(|_| {}),
            on_change: Box::new(|_| {}),
            backend: None,
        }
    }
}

impl PackagedConfig {
    pub fn new(is_packaged: bool, current_version: impl Into<String>) -> Self {
        Self {
            is_packaged,
            current_version: current_version.into(),
            ..Self::default()
        }
    }

    pub fn with_env(mut self, env: Env) -> Self {
        self.env = env;
        self
    }

    pub fn with_app_update_yml(mut self, has_app_update_yml: bool) -> Self {
        self.has_app_update_yml = has_app_update_yml;
        self
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = platform.into();
        self
    }

    pub fn with_game_running<F>(mut self, is_running: F) -> Self
    where
        F: Fn() -> bool + 'static,
    {
        self.is_game_running = Box::new(is_running);
        self
    }

    pub fn with_advised<F>(mut self, advised: F) -> Self
    where
        F: Fn(&str) -> bool + 'static,
    {
        self.previously_advised = Box::new(advised);
        self
    }

    pub fn with_save_advised<F>(mut self, save: F) -> Self
    where
        F: FnMut(&str) + 'static,
    {
        self.save_advised = Box::new(save);
        self
    }

    pub fn with_pending<F>(mut self, pending: F) -> Self
    where
        F: Fn() -> Option<String> + 'static,
    {
        self.pending = Box::new(pending);
        self
    }

    pub fn with_save_pending<F>(mut self, save: F) -> Self
    where
        F: FnMut(Option<&str>) + 'static,
    {
        self.save_pending = Box::new(save);
        self
    }

    pub fn with_on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&PackagedState) + 'static,
    {
        self.on_change = Box::new(on_change);
        self
    }

    pub fn with_backend<B>(mut self, backend: B) -> Self
    where
        B: AutoUpdater + 'static,
    {
        self.backend = Some(Box::new(backend));
        self
    }
}

/// Motivo devolvido por `checar()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckReason {
    #[serde(rename = "canal_nao_suportado")]
    CanalNaoSuportado,
    #[serde(rename = "jogo_rodando")]
    JogoRodando,
    #[serde(rename = "erro")]
    Erro,
}

impl CheckReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CanalNaoSuportado => "canal_nao_suportado",
            Self::JogoRodando => "jogo_rodando",
            Self::Erro => "erro",
        }
    }
}

/// Retorno de `checar({ manual })`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub ok: bool,
    pub disponivel: bool,
    pub versao: Option<String>,
    pub tamanho: Option<u64>,
    pub motivo: Option<CheckReason>,
    pub erro: Option<String>,
}

impl CheckResult {
    pub fn canal_nao_suportado() -> Self {
        Self {
            ok: false,
            disponivel: false,
            versao: None,
            tamanho: None,
            motivo: Some(CheckReason::CanalNaoSuportado),
            erro: None,
        }
    }

    pub fn jogo_rodando() -> Self {
        Self {
            ok: true,
            disponivel: false,
            versao: None,
            tamanho: None,
            motivo: Some(CheckReason::JogoRodando),
            erro: None,
        }
    }

    pub fn em_dia() -> Self {
        Self {
            ok: true,
            disponivel: false,
            versao: None,
            tamanho: None,
            motivo: None,
            erro: None,
        }
    }

    pub fn disponivel(version: String, size: Option<u64>) -> Self {
        Self {
            ok: true,
            disponivel: true,
            versao: Some(version),
            tamanho: size,
            motivo: None,
            erro: None,
        }
    }

    pub fn erro(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            disponivel: false,
            versao: None,
            tamanho: None,
            motivo: Some(CheckReason::Erro),
            erro: Some(message.into()),
        }
    }

    pub fn atualizado() -> Self {
        Self::em_dia()
    }
}

impl Serialize for CheckResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("ok", &self.ok)?;
        map.serialize_entry("disponivel", &self.disponivel)?;
        if let Some(erro) = &self.erro {
            map.serialize_entry("erro", erro)?;
        }
        if self.disponivel {
            map.serialize_entry("versao", &self.versao)?;
            map.serialize_entry("tamanho", &self.tamanho)?;
        } else if self.ok && self.motivo.is_none() && self.erro.is_none() {
            // `{ok:true, disponivel:false, versao:null}` do oráculo.
            map.serialize_entry("versao", &Option::<String>::None)?;
        }
        if let Some(motivo) = self.motivo {
            map.serialize_entry("motivo", &motivo)?;
        }
        map.end()
    }
}

/// Retorno de `baixar()`/`instalar()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erro: Option<String>,
}

impl ActionResult {
    pub fn ok() -> Self {
        Self {
            ok: true,
            erro: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            erro: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JaAvisadoResult {
    pub ok: bool,
}

/// Máquina de estados do updater empacotado.
pub struct PackagedUpdater {
    state: PackagedState,
    is_game_running: Box<dyn Fn() -> bool>,
    previously_advised: Box<dyn Fn(&str) -> bool>,
    save_advised: Box<dyn FnMut(&str)>,
    pending: Box<dyn Fn() -> Option<String>>,
    save_pending: SavePendingCallback,
    on_change: Box<dyn FnMut(&PackagedState)>,
    backend: Option<Box<dyn AutoUpdater>>,
    user_action: Option<ErrorAction>,
}

impl PackagedUpdater {
    pub fn new(config: PackagedConfig) -> Self {
        let PackagedConfig {
            is_packaged,
            current_version,
            env,
            has_app_update_yml,
            platform,
            is_game_running,
            previously_advised,
            save_advised,
            pending,
            save_pending,
            on_change,
            backend,
        } = config;

        let canal = detect_channel(is_packaged, &env, has_app_update_yml, &platform);
        let suportado = canal.is_supported();
        let fase = if suportado || canal == Channel::Fonte {
            Phase::Ocioso
        } else {
            Phase::SemSuporte
        };
        let state = PackagedState {
            canal,
            suportado,
            versao_atual: current_version,
            versao_nova: None,
            tamanho: None,
            fase,
            progresso: 0,
            erro: None,
            erro_de_fundo: false,
            erro_acao: None,
            ja_avisado: false,
            jogo_rodando: false,
        };

        let mut updater = Self {
            state,
            is_game_running,
            previously_advised,
            save_advised,
            pending,
            save_pending,
            on_change,
            backend,
            user_action: None,
        };
        if suportado {
            if let Some(backend) = updater.backend.as_mut() {
                backend.configure(false, false);
            }
        }
        updater
    }

    pub fn channel(&self) -> Channel {
        self.state.canal
    }

    pub fn supported(&self) -> bool {
        self.state.suportado
    }

    pub fn current_version(&self) -> &str {
        &self.state.versao_atual
    }

    /// Snapshot com `jogoRodando` avaliado no momento.
    pub fn state(&self) -> PackagedState {
        let mut snapshot = self.state.clone();
        snapshot.jogo_rodando = (self.is_game_running)();
        snapshot
    }

    fn mutate(&mut self, patch: impl FnOnce(&mut PackagedState)) {
        patch(&mut self.state);
        let mut snapshot = self.state.clone();
        snapshot.jogo_rodando = (self.is_game_running)();
        (self.on_change)(&snapshot);
    }

    fn drain_events(&mut self) {
        let events = match self.backend.as_mut() {
            Some(backend) => backend.take_events(),
            None => Vec::new(),
        };
        for event in events {
            self.process_event(event);
        }
    }

    /// Aplica um evento do backend fora de uma chamada (evento assíncrono).
    pub fn process_event(&mut self, event: UpdaterEvent) {
        match event {
            UpdaterEvent::UpdateAvailable(info) => {
                let size = tamanho_de(&info);
                let version = info.version;
                let advised = (self.previously_advised)(&version);
                self.mutate(|state| {
                    state.fase = Phase::Disponivel;
                    state.versao_nova = Some(version);
                    state.tamanho = size;
                    state.progresso = 0;
                    state.erro = None;
                    state.erro_de_fundo = false;
                    state.erro_acao = None;
                    state.ja_avisado = advised;
                });
            }
            UpdaterEvent::DownloadProgress { percent } => {
                let progress = normalize_percent(percent);
                self.mutate(|state| {
                    state.fase = Phase::Baixando;
                    state.progresso = progress;
                    state.erro = None;
                    state.erro_acao = None;
                });
            }
            UpdaterEvent::UpdateDownloaded(info) => {
                let version = if info.version.is_empty() {
                    self.state.versao_nova.clone().unwrap_or_default()
                } else {
                    info.version
                };
                if !version.is_empty() {
                    (self.save_pending)(Some(&version));
                }
                let next_version = if version.is_empty() {
                    self.state.versao_nova.clone()
                } else {
                    Some(version)
                };
                self.mutate(|state| {
                    state.fase = Phase::Pronto;
                    state.versao_nova = next_version;
                    state.progresso = 100;
                    state.erro = None;
                    state.erro_de_fundo = false;
                    state.erro_acao = None;
                });
            }
            UpdaterEvent::UpdateNotAvailable => {
                (self.save_pending)(None);
                self.mutate(|state| {
                    state.fase = Phase::Ocioso;
                    state.versao_nova = None;
                    state.tamanho = None;
                    state.progresso = 0;
                    state.erro = None;
                    state.erro_de_fundo = false;
                    state.erro_acao = None;
                    state.ja_avisado = false;
                });
            }
            UpdaterEvent::Error(error) => {
                let message = error_message(&error);
                let background = self.user_action.is_none();
                let action = self.user_action;
                self.mutate(|state| {
                    state.fase = Phase::Erro;
                    state.erro = Some(message);
                    state.erro_de_fundo = background;
                    state.erro_acao = action;
                });
            }
        }
    }

    /// `checar({ manual })` — nunca baixa sozinho.
    pub fn check(&mut self, manual: bool) -> CheckResult {
        let result = self.check_inner(manual);
        self.user_action = None;
        result
    }

    fn check_inner(&mut self, manual: bool) -> CheckResult {
        if !self.state.suportado {
            self.mutate(|state| state.fase = Phase::SemSuporte);
            return CheckResult::canal_nao_suportado();
        }

        // Ciclo automático não interrompe partida; reavalia no próximo ciclo.
        if !manual && (self.is_game_running)() {
            self.mutate(|state| state.jogo_rodando = true);
            return CheckResult::jogo_rodando();
        }

        self.user_action = manual.then_some(ErrorAction::Checar);
        let outcome = match self.backend.as_mut() {
            Some(backend) => backend.check_for_updates(),
            None => Err("auto_updater_indisponivel".to_string()),
        };
        self.drain_events();

        let result = match outcome {
            Ok(result) => result,
            Err(error) => {
                let message = error_message(&error);
                let background = !manual;
                let action = manual.then_some(ErrorAction::Checar);
                self.mutate(|state| {
                    state.fase = Phase::Erro;
                    state.erro = Some(message.clone());
                    state.erro_de_fundo = background;
                    state.erro_acao = action;
                });
                return CheckResult::erro(message);
            }
        };

        let info = result.update_info;
        let version = info
            .as_ref()
            .map(|info| info.version.clone())
            .unwrap_or_default();
        let available = is_newer_version(&version, &self.state.versao_atual);

        if !available {
            self.mutate(|state| {
                state.fase = Phase::Ocioso;
                state.versao_nova = None;
                state.tamanho = None;
                state.progresso = 0;
                state.erro = None;
                state.erro_de_fundo = false;
                state.erro_acao = None;
                state.ja_avisado = false;
            });
            return CheckResult::em_dia();
        }

        let size = info.as_ref().and_then(tamanho_de);

        // D6: já baixado numa sessão anterior. O backend revalida o cache e
        // reemite `update-downloaded`; se sumiu, baixa a mesma versão aceita.
        if (self.pending)().as_deref() == Some(version.as_str()) {
            self.mutate(|state| {
                state.fase = Phase::Baixando;
                state.progresso = 0;
                state.erro = None;
                state.erro_de_fundo = false;
                state.erro_acao = None;
            });
            let outcome = match self.backend.as_mut() {
                Some(backend) => backend.download_update(),
                None => Err("auto_updater_indisponivel".to_string()),
            };
            self.drain_events();
            if let Err(error) = outcome {
                let message = error_message(&error);
                let action = manual.then_some(ErrorAction::Checar);
                self.mutate(|state| {
                    state.fase = Phase::Erro;
                    state.erro = Some(message.clone());
                    state.erro_de_fundo = !manual;
                    state.erro_acao = action;
                });
                return CheckResult::erro(message);
            }
            if self.state.fase != Phase::Pronto {
                self.mutate(|state| {
                    state.fase = Phase::Pronto;
                    state.progresso = 100;
                });
            }
            return CheckResult::disponivel(version, size);
        }

        if manual {
            // Checagem manual mostra mesmo com "já avisei".
            let next = version.clone();
            self.mutate(|state| {
                state.fase = Phase::Disponivel;
                state.versao_nova = Some(next);
                state.tamanho = size;
                state.erro = None;
                state.erro_acao = None;
                state.ja_avisado = false;
            });
        } else if self.state.fase != Phase::Disponivel
            || self.state.versao_nova.as_deref() != Some(version.as_str())
        {
            let advised = (self.previously_advised)(&version);
            let next = version.clone();
            self.mutate(|state| {
                state.fase = Phase::Disponivel;
                state.versao_nova = Some(next);
                state.tamanho = size;
                state.erro = None;
                state.erro_acao = None;
                state.ja_avisado = advised;
            });
        }

        CheckResult::disponivel(version, size)
    }

    /// `baixar()` — só é chamado depois do "Baixar" do usuário.
    pub fn download(&mut self) -> ActionResult {
        let result = self.download_inner();
        self.user_action = None;
        result
    }

    fn download_inner(&mut self) -> ActionResult {
        if !self.state.suportado {
            return ActionResult::error("canal_nao_suportado");
        }
        if self.state.versao_nova.as_deref().is_none_or(str::is_empty) {
            return ActionResult::error("sem_versao");
        }

        self.user_action = Some(ErrorAction::Baixar);
        self.mutate(|state| {
            state.fase = Phase::Baixando;
            state.progresso = 0;
            state.erro = None;
            state.erro_de_fundo = false;
            state.erro_acao = None;
        });

        let outcome = match self.backend.as_mut() {
            Some(backend) => backend.download_update(),
            None => Err("auto_updater_indisponivel".to_string()),
        };
        self.drain_events();

        match outcome {
            Err(error) => {
                let message = error_message(&error);
                self.mutate(|state| {
                    state.fase = Phase::Erro;
                    state.erro = Some(message.clone());
                    state.erro_de_fundo = false;
                    state.erro_acao = Some(ErrorAction::Baixar);
                });
                ActionResult::error(message)
            }
            Ok(()) => {
                if self.state.fase != Phase::Pronto {
                    self.mutate(|state| {
                        state.fase = Phase::Pronto;
                        state.progresso = 100;
                    });
                }
                ActionResult::ok()
            }
        }
    }

    /// `instalar()` — chama `quitAndInstall` só depois de `pronto`.
    pub fn install(&mut self) -> ActionResult {
        if !self.state.suportado {
            return ActionResult::error("canal_nao_suportado");
        }
        if self.state.fase != Phase::Pronto {
            return ActionResult::error("sem_download");
        }

        // O oráculo não marca ação de usuário aqui: um evento `error` emitido
        // durante o quit é de fundo; já a exceção do `quitAndInstall` vira ação
        // "instalar" no catch abaixo.
        let outcome = match self.backend.as_mut() {
            Some(backend) => backend.quit_and_install(),
            None => Err("auto_updater_indisponivel".to_string()),
        };
        self.drain_events();

        match outcome {
            Ok(()) => ActionResult::ok(),
            Err(error) => {
                let message = error_message(&error);
                self.mutate(|state| {
                    state.fase = Phase::Erro;
                    state.erro = Some(message.clone());
                    state.erro_de_fundo = false;
                    state.erro_acao = Some(ErrorAction::Instalar);
                });
                ActionResult::error(message)
            }
        }
    }

    /// `marcarJaAvisado(versao)`: persiste e atualiza o snapshot quando a
    /// versão é a oferecida no momento.
    pub fn mark_advised(&mut self, version: Option<&str>) -> JaAvisadoResult {
        let value = version
            .filter(|version| !version.is_empty())
            .map(str::to_string)
            .or_else(|| self.state.versao_nova.clone())
            .unwrap_or_default();
        if value.is_empty() {
            return JaAvisadoResult { ok: false };
        }
        (self.save_advised)(&value);
        if self.state.versao_nova.as_deref() == Some(value.as_str()) {
            self.mutate(|state| state.ja_avisado = true);
        }
        JaAvisadoResult { ok: true }
    }

    // Aliases com os nomes do oráculo.
    pub fn canal(&self) -> Channel {
        self.channel()
    }

    pub fn suportado(&self) -> bool {
        self.supported()
    }

    pub fn estado(&self) -> PackagedState {
        self.state()
    }

    pub fn checar(&mut self, manual: bool) -> CheckResult {
        self.check(manual)
    }

    pub fn baixar(&mut self) -> ActionResult {
        self.download()
    }

    pub fn instalar(&mut self) -> ActionResult {
        self.install()
    }

    pub fn marcar_ja_avisado(&mut self, version: Option<&str>) -> JaAvisadoResult {
        self.mark_advised(version)
    }
}

fn error_message(error: &str) -> String {
    if error.is_empty() {
        "erro".to_string()
    } else {
        error.to_string()
    }
}

/// `Math.max(0, Math.min(100, Math.round(Number(percent) || 0)))`.
pub fn normalize_percent(percent: f64) -> u8 {
    let percent = if percent.is_nan() || percent == 0.0 {
        0.0
    } else {
        percent
    };
    let rounded = percent.round();
    if rounded <= 0.0 {
        0
    } else if rounded >= 100.0 {
        100
    } else {
        rounded as u8
    }
}

/// Aliases em português dos tipos do oráculo.
pub type Canal = Channel;
pub type Fase = Phase;
pub type ErroAcao = ErrorAction;
pub type EstadoEmpacotado = PackagedState;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_wire_names() {
        assert_eq!(
            serde_json::to_value(Channel::SemSuporte).unwrap(),
            serde_json::json!("sem_suporte")
        );
        assert_eq!(
            serde_json::to_value(Channel::AppImage).unwrap(),
            serde_json::json!("appimage")
        );
        assert_eq!(
            serde_json::to_value(Phase::Pronto).unwrap(),
            serde_json::json!("pronto")
        );
        assert_eq!(
            serde_json::to_value(ErrorAction::Instalar).unwrap(),
            serde_json::json!("instalar")
        );
    }

    #[test]
    fn percent_matches_math_round_and_clamp() {
        assert_eq!(normalize_percent(42.6), 43);
        assert_eq!(normalize_percent(42.4), 42);
        assert_eq!(normalize_percent(-5.0), 0);
        assert_eq!(normalize_percent(150.0), 100);
        assert_eq!(normalize_percent(f64::NAN), 0);
        assert_eq!(normalize_percent(f64::INFINITY), 100);
        assert_eq!(normalize_percent(f64::NEG_INFINITY), 0);
    }
}
