//! Estado da aplicação e arquitetura Elm do iced.
//!
//! `App` guarda somente dados de UI. Trabalho de rede/IPC vira `Task`, e as
//! mensagens voltam para `update` — nenhuma chamada bloqueante roda na thread
//! que desenha.

use std::collections::BTreeSet;
use std::time::Duration;

use arcadia_native_input::NavEvent;
use iced::widget::image::Handle;
use iced::{keyboard, window, Element, Fill, Size, Subscription, Task};
use serde_json::{json, Value};

use crate::backend::{
    Backend, DownloadAction, DownloadItem, DownloadsData, FriendsLoad, Game, GameMedia,
    LaunchSnapshot, PluginItem, SettingsConfig, SourceItem,
};
use crate::big_picture::{
    BigPicture, ConsoleEffect, ConsoleTab, ACTION_FAVORITE, ACTION_MORE, ACTION_PLAY,
    ACTION_TRAILER,
};
use crate::covers::CoverCache;
use crate::media::{self, MediaPlayer};
use crate::theme;
use crate::views;

#[derive(Debug, Clone)]
pub enum Message {
    LibraryLoaded(Result<Vec<Game>, String>),
    CoverLoaded {
        url: String,
        handle: Option<Handle>,
    },
    Reload,
    SearchChanged(String),
    Navigate(Section),
    OpenGame(String),
    BackToLibrary,
    /// Resultado de `game:sysinfo` para o jogo aberto.
    MediaLoaded(GameMedia),
    /// Tick da Subscription: drena frames e sincroniza o estado do player.
    MediaTick,
    MediaTogglePlayback,
    MediaSeek(f64),
    MediaError(String),
    /// "Jogar" na ficha ou no Big Picture: dispara `game:launch`.
    LaunchGame(String),
    /// "Parar": encerra o processo atual via `game:close`.
    StopGame,
    LaunchGameDone {
        id: String,
        result: Result<(), String>,
    },
    StopGameDone(Result<(), String>),
    /// Polling do runtime de launch enquanto houver jogo abrindo/rodando.
    LaunchPoll,
    /// `config:get` silencioso no boot para saber se minimiza ao lançar.
    LaunchConfigLoaded(Result<SettingsConfig, String>),
    Refresh(Section),
    DownloadsLoaded(Result<DownloadsData, String>),
    DownloadActionRequested {
        item: DownloadItem,
        action: DownloadAction,
    },
    DownloadActionDone(Result<(), String>),
    SettingsLoaded(Result<SettingsConfig, String>),
    SettingsSaved {
        key: String,
        result: Result<SettingsConfig, String>,
    },
    SettingsTabChanged(SettingsTab),
    AccentChanged(String),
    AccentSave,
    LanguageChanged(String),
    SettingToggled {
        key: &'static str,
        value: bool,
    },
    CpuCoresChanged(i64),
    SourcesLoaded(Result<Vec<SourceItem>, String>),
    SourceUrlChanged(String),
    SourceAdd,
    SourceRemove(String),
    SourceSync,
    SourcesActionDone(Result<(), String>),
    FriendsLoaded(Result<FriendsLoad, String>),
    ComponentsLoaded(Result<Vec<PluginItem>, String>),
    SidebarSearchChanged(String),
    ToggleSidebarGames,
    WindowReady(Option<window::Id>),
    WindowDrag,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    WindowEvent(window::Event),
    WindowResized(Size),
    /// Entra no modo console (tela cheia + navegação por foco).
    EnterBigPicture,
    /// Volta ao desktop exatamente na tela em que o modo console foi aberto.
    ExitBigPicture,
    /// Tick de ~60 fps: drena o gamepad via `NavInput::tick`.
    ConsoleTick,
    /// Teclado do modo console (setas, Enter, Esc, Tab).
    ConsoleKey(keyboard::Event),
    /// Clique num card do trilho; o segundo clique no card focado abre o painel.
    ConsoleSelectGame(usize),
    /// Clique numa aba da top bar.
    ConsoleSelectTab(ConsoleTab),
    ConsolePlay,
    ConsoleTrailer,
    ConsoleFavorite,
    ConsoleMore,
    ConsoleCloseOverview,
}

/// Itens fixos da sidebar. Biblioteca, Home e as seções desta onda têm tela
/// real; Lojas e Big Picture ainda abrem um placeholder explícito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Library,
    Stores,
    Components,
    Downloads,
    Sources,
    Friends,
    Settings,
    BigPicture,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Home,
        Section::Library,
        Section::Stores,
        Section::Components,
        Section::Downloads,
        Section::Sources,
        Section::Friends,
        Section::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Home => "Início",
            Section::Library => "Biblioteca",
            Section::Stores => "Lojas",
            Section::Components => "Componentes",
            Section::Downloads => "Downloads",
            Section::Sources => "Fontes",
            Section::Friends => "Amigos",
            Section::Settings => "Configurações",
            Section::BigPicture => "Big Picture",
        }
    }

    pub fn stub_message(self) -> &'static str {
        match self {
            Section::Home => "A Home nativa usa os dados locais da biblioteca.",
            Section::Library => "Biblioteca local carregada via `library:get`.",
            Section::Stores => "A vitrine de lojas ainda não foi portada para o iced.",
            Section::Components => "Plugins e componentes ainda não têm tela nativa.",
            Section::Downloads => "Fila de downloads chega junto com o serviço de download.",
            Section::Sources => "Catálogos e fontes extras entram numa onda própria.",
            Section::Friends => "Amigos e presença dependem da sessão/realtime.",
            Section::Settings => "Configurações nativas serão portadas depois da biblioteca.",
            Section::BigPicture => "O modo console abre em tela cheia pelo botão Big Picture.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Library,
    Game(String),
    Section(Section),
    /// Modo console em tela cheia; o desktop fica guardado em `previous_screen`.
    BigPicture,
}

impl Screen {
    pub fn for_section(section: Section) -> Self {
        if section == Section::Library {
            Self::Library
        } else {
            Self::Section(section)
        }
    }

    pub fn back_to_library(&mut self) {
        *self = Self::Library;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryStatus {
    Loading,
    Ready,
    Error(String),
}

#[derive(Debug)]
pub struct Library {
    pub games: Vec<Game>,
    pub status: LibraryStatus,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            status: LibraryStatus::Loading,
        }
    }
}

impl Library {
    pub fn loading() -> Self {
        Self::default()
    }

    pub fn set_loading(&mut self) {
        self.status = LibraryStatus::Loading;
    }

    pub fn ready(&mut self, games: Vec<Game>) {
        self.games = games;
        self.status = LibraryStatus::Ready;
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.status = LibraryStatus::Error(message.into());
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|game| game.id == id)
    }

    /// Lista da grade: esconde ocultos, aplica a busca e põe favoritos primeiro.
    pub fn visible_games(&self, query: &str) -> Vec<&Game> {
        let query = query.trim().to_lowercase();
        let mut games: Vec<&Game> = self
            .games
            .iter()
            .filter(|game| !game.hidden)
            .filter(|game| query.is_empty() || game.title.to_lowercase().contains(query.as_str()))
            .collect();

        games.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        });
        games
    }

    /// Lista da sidebar: mesma ordenação, sem o filtro de busca.
    pub fn visible_all(&self) -> Vec<&Game> {
        self.visible_games("")
    }

    /// Todas as artes necessárias para a biblioteca atual, sem repetição.
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = BTreeSet::new();
        for game in &self.games {
            for url in game.image_urls() {
                urls.insert(url);
            }
        }
        urls.into_iter().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameRunStatus {
    #[default]
    Idle,
    Launching,
    Running,
    Stopping,
}

/// Sessão de jogo vista pela UI nativa.
///
/// O `LaunchRuntime` do IPC só aceita um jogo por vez; `game_id` diz a qual
/// jogo o estado atual pertence. `error` é sempre exibido na ficha e no painel
/// do Big Picture — falha de launch não fica só no stderr.
#[derive(Debug, Clone, Default)]
pub struct GameLaunchState {
    pub status: GameRunStatus,
    pub game_id: Option<String>,
    pub pid: Option<u32>,
    pub error: Option<String>,
    /// Preferência `minimize_on_game_launch`, carregada no boot.
    pub minimize_on_launch: bool,
    /// `true` enquanto fomos nós que minimizamos; restaura ao encerrar.
    pub minimized: bool,
}

impl GameLaunchState {
    pub fn is_launching(&self, id: &str) -> bool {
        self.status == GameRunStatus::Launching && self.game_id.as_deref() == Some(id)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.status == GameRunStatus::Running && self.game_id.as_deref() == Some(id)
    }

    pub fn is_stopping(&self, id: &str) -> bool {
        self.status == GameRunStatus::Stopping && self.game_id.as_deref() == Some(id)
    }

    /// Outro jogo está abrindo/rodando; o runtime só aceita uma sessão.
    pub fn busy_with_other(&self, id: &str) -> bool {
        self.status != GameRunStatus::Idle && self.game_id.as_deref() != Some(id)
    }

    pub fn needs_poll(&self) -> bool {
        self.status != GameRunStatus::Idle
    }

    /// Aplica o polling sem atropelar um `game:launch`/`game:close` em voo.
    fn apply_snapshot(&mut self, snapshot: LaunchSnapshot) {
        if matches!(
            self.status,
            GameRunStatus::Launching | GameRunStatus::Stopping
        ) {
            return;
        }

        if snapshot.running {
            self.status = GameRunStatus::Running;
            if let Some(id) = snapshot.game_id {
                self.game_id = Some(id);
            }
            self.pid = snapshot.pid;
            return;
        }

        if self.status == GameRunStatus::Running {
            self.status = GameRunStatus::Idle;
            self.game_id = None;
            self.pid = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error(String),
}

/// Aba interna das Configurações, equivalente à sub-navegação da UI web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Appearance,
    Downloads,
    Playback,
    Integrations,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 6] = [
        SettingsTab::General,
        SettingsTab::Appearance,
        SettingsTab::Downloads,
        SettingsTab::Playback,
        SettingsTab::Integrations,
        SettingsTab::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "Geral",
            SettingsTab::Appearance => "Aparência",
            SettingsTab::Downloads => "Downloads",
            SettingsTab::Playback => "Trailers e mídia",
            SettingsTab::Integrations => "Integrações",
            SettingsTab::About => "Sobre",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DownloadsState {
    pub data: DownloadsData,
    pub status: LoadStatus,
    pub busy_id: Option<String>,
    pub action_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub config: Option<SettingsConfig>,
    pub status: LoadStatus,
    pub tab: SettingsTab,
    pub accent_draft: String,
    pub save_error: Option<String>,
    pub saving: Option<String>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            config: None,
            status: LoadStatus::Idle,
            tab: SettingsTab::General,
            accent_draft: String::new(),
            save_error: None,
            saving: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourcesState {
    pub items: Vec<SourceItem>,
    pub status: LoadStatus,
    pub url_draft: String,
    pub busy: bool,
    pub action_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FriendsState {
    pub data: Option<FriendsLoad>,
    pub status: LoadStatus,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentsState {
    pub plugins: Vec<PluginItem>,
    pub status: LoadStatus,
}

pub struct App {
    pub backend: Backend,
    pub screen: Screen,
    pub library: Library,
    pub covers: CoverCache,
    pub search: String,
    pub media: MediaPlayer,
    /// `None` enquanto o `game:sysinfo` está em voo.
    pub game_media: Option<GameMedia>,
    pub downloads: DownloadsState,
    pub settings: SettingsState,
    pub sources: SourcesState,
    pub friends: FriendsState,
    pub components: ComponentsState,
    pub sidebar_search: String,
    pub sidebar_games_open: bool,
    pub window: Option<window::Id>,
    /// Tamanho atual da janela; alimenta o scroll centralizado do trilho.
    pub window_size: Size,
    /// Estado do modo console (gamepad, foco, aba, painel).
    pub big_picture: BigPicture,
    /// Sessão de jogo: abrindo, rodando, parando e última falha.
    pub launch: GameLaunchState,
    /// Tela desktop a restaurar quando o console for fechado.
    pub previous_screen: Option<Screen>,
}

impl App {
    pub fn boot(backend: Backend) -> (Self, Task<Message>) {
        let app = Self {
            backend,
            screen: Screen::Library,
            library: Library::loading(),
            covers: CoverCache::default(),
            search: String::new(),
            media: MediaPlayer::new(),
            game_media: None,
            downloads: DownloadsState::default(),
            settings: SettingsState::default(),
            sources: SourcesState::default(),
            friends: FriendsState::default(),
            components: ComponentsState::default(),
            sidebar_search: String::new(),
            sidebar_games_open: true,
            window: None,
            window_size: Size::new(1600.0, 900.0),
            big_picture: BigPicture::new(),
            launch: GameLaunchState::default(),
            previous_screen: None,
        };
        let launch_config = {
            let backend = app.backend.clone();
            Task::perform(
                async move { backend.load_settings().await },
                Message::LaunchConfigLoaded,
            )
        };
        let task = Task::batch([
            app.reload_task(),
            window::latest().map(Message::WindowReady),
            launch_config,
        ]);
        (app, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LibraryLoaded(Ok(games)) => {
                eprintln!("[arcadia-native] biblioteca: {} jogo(s)", games.len());
                self.library.ready(games);
                self.big_picture
                    .sync_games(self.library.visible_all().len());
                let urls = self.library.image_urls();
                self.cover_tasks(urls)
            }
            Message::LibraryLoaded(Err(error)) => {
                eprintln!("[arcadia-native] falha ao carregar biblioteca: {error}");
                self.library.error(error);
                Task::none()
            }
            Message::CoverLoaded { url, handle } => {
                match handle {
                    Some(handle) => self.covers.insert(url, handle),
                    None => self.covers.fail(&url),
                }
                Task::none()
            }
            Message::Reload => {
                self.library.set_loading();
                self.reload_task()
            }
            Message::SearchChanged(search) => {
                self.search = search;
                Task::none()
            }
            Message::Navigate(section) => {
                if section == Section::BigPicture {
                    return self.enter_big_picture();
                }
                self.media.clear();
                self.game_media = None;
                self.screen = Screen::for_section(section);
                self.load_section(section, false)
            }
            Message::OpenGame(id) => self.open_game(id),
            Message::BackToLibrary => {
                self.media.clear();
                self.game_media = None;
                self.screen.back_to_library();
                Task::none()
            }
            Message::Refresh(section) => self.load_section(section, true),
            Message::DownloadsLoaded(result) => {
                match result {
                    Ok(data) => {
                        self.downloads.data = data;
                        self.downloads.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] downloads: {error}");
                        self.downloads.status = LoadStatus::Error(error);
                    }
                }
                self.downloads.busy_id = None;
                Task::none()
            }
            Message::DownloadActionRequested { item, action } => {
                if self.downloads.busy_id.is_some() {
                    return Task::none();
                }
                self.downloads.busy_id = Some(item.id.clone());
                self.downloads.action_error = None;
                let backend = self.backend.clone();
                let id = item.id;
                let is_torrent = item.is_torrent;
                Task::perform(
                    async move { backend.run_download_action(id, is_torrent, action).await },
                    Message::DownloadActionDone,
                )
            }
            Message::DownloadActionDone(result) => {
                self.downloads.busy_id = None;
                match result {
                    Ok(()) => {
                        self.downloads.status = LoadStatus::Loading;
                        let backend = self.backend.clone();
                        Task::perform(
                            async move { backend.load_downloads().await },
                            Message::DownloadsLoaded,
                        )
                    }
                    Err(error) => {
                        self.downloads.action_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::SettingsLoaded(result) => {
                match result {
                    Ok(config) => {
                        self.settings.accent_draft = config.accent.clone();
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.settings.config = Some(config);
                        self.settings.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] configurações: {error}");
                        self.settings.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::SettingsSaved { key, result } => {
                self.settings.saving = None;
                match result {
                    Ok(config) => {
                        self.settings.accent_draft = config.accent.clone();
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.settings.config = Some(config);
                        self.settings.status = LoadStatus::Ready;
                        self.settings.save_error = None;
                    }
                    Err(error) => {
                        self.settings.save_error = Some(format!("Falha ao salvar {key}: {error}"));
                    }
                }
                Task::none()
            }
            Message::SettingsTabChanged(tab) => {
                self.settings.tab = tab;
                Task::none()
            }
            Message::AccentChanged(accent) => {
                self.settings.accent_draft = accent;
                Task::none()
            }
            Message::AccentSave => {
                let accent = self.settings.accent_draft.trim().to_string();
                self.save_setting("accent", Value::String(accent))
            }
            Message::LanguageChanged(language) => {
                self.save_setting("language", Value::String(language))
            }
            Message::SettingToggled { key, value } => self.save_setting(key, Value::Bool(value)),
            Message::CpuCoresChanged(delta) => {
                let current = self
                    .settings
                    .config
                    .as_ref()
                    .map(|config| config.download_cpu_cores)
                    .unwrap_or(0) as i64;
                let next = (current + delta).clamp(0, 64);
                if next == current {
                    return Task::none();
                }
                self.save_setting("download_cpu_cores", json!(next))
            }
            Message::SourcesLoaded(result) => {
                self.sources.busy = false;
                match result {
                    Ok(items) => {
                        self.sources.items = items;
                        self.sources.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] fontes: {error}");
                        self.sources.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::SourceUrlChanged(url) => {
                self.sources.url_draft = url;
                Task::none()
            }
            Message::SourceAdd => {
                if self.sources.busy {
                    return Task::none();
                }
                let url = self.sources.url_draft.trim().to_string();
                if url.is_empty() {
                    self.sources.action_error = Some("Informe a URL do catálogo.".to_string());
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.add_source(url).await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourceRemove(id) => {
                if self.sources.busy {
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.remove_source(id).await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourceSync => {
                if self.sources.busy {
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.sync_sources().await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourcesActionDone(result) => {
                self.sources.busy = false;
                match result {
                    Ok(()) => {
                        self.sources.url_draft.clear();
                        self.sources.status = LoadStatus::Loading;
                        let backend = self.backend.clone();
                        Task::perform(
                            async move { backend.load_sources().await },
                            Message::SourcesLoaded,
                        )
                    }
                    Err(error) => {
                        self.sources.action_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::FriendsLoaded(result) => {
                match result {
                    Ok(data) => {
                        self.friends.data = Some(data);
                        self.friends.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] amigos: {error}");
                        self.friends.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::ComponentsLoaded(result) => {
                match result {
                    Ok(plugins) => {
                        self.components.plugins = plugins;
                        self.components.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] componentes: {error}");
                        self.components.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::SidebarSearchChanged(search) => {
                self.sidebar_search = search;
                Task::none()
            }
            Message::ToggleSidebarGames => {
                self.sidebar_games_open = !self.sidebar_games_open;
                Task::none()
            }
            Message::WindowReady(id) => {
                self.window = id;
                match id {
                    Some(id) if matches!(self.screen, Screen::BigPicture) => {
                        window::set_mode(id, window::Mode::Fullscreen)
                    }
                    _ => Task::none(),
                }
            }
            Message::WindowDrag => self.window_task(window::drag),
            Message::WindowMinimize => self.window_task(|id| window::minimize(id, true)),
            Message::WindowToggleMaximize => self.window_task(window::toggle_maximize),
            Message::WindowClose => self.window_task(window::close),
            Message::WindowEvent(event) => self.window_event(event),
            Message::WindowResized(size) => {
                self.window_size = size;
                if matches!(self.screen, Screen::BigPicture) {
                    self.console_rail_scroll()
                } else {
                    Task::none()
                }
            }
            Message::EnterBigPicture => self.enter_big_picture(),
            Message::ExitBigPicture => self.exit_big_picture(),
            Message::ConsoleTick => self.console_tick(),
            Message::ConsoleKey(event) => self.console_key(event),
            Message::ConsoleSelectGame(index) => self.console_select_game(index),
            Message::ConsoleSelectTab(tab) => self.console_select_tab(tab),
            Message::ConsolePlay => self.console_action(ACTION_PLAY),
            Message::ConsoleTrailer => self.console_action(ACTION_TRAILER),
            Message::ConsoleFavorite => self.console_action(ACTION_FAVORITE),
            Message::ConsoleMore => self.console_action(ACTION_MORE),
            Message::ConsoleCloseOverview => {
                self.big_picture.close_overview();
                Task::none()
            }
            Message::MediaLoaded(game_media) => {
                eprintln!(
                    "[arcadia-native] trailer: {} movie(s){}",
                    game_media.movies.len(),
                    game_media
                        .notice
                        .as_deref()
                        .map(|notice| format!(" — {notice}"))
                        .unwrap_or_default()
                );
                self.game_media = Some(game_media);
                // Prioridade hls → mp4 → webm fica no wrapper de mídia.
                let sources = self
                    .game_media
                    .as_ref()
                    .and_then(|game_media| game_media.movies.first())
                    .map(|movie| media::TrailerSources {
                        hls: movie.hls.clone(),
                        mp4: movie.mp4.clone(),
                        webm: movie.webm.clone(),
                    });
                match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                }
            }
            Message::MediaTick => {
                self.media.tick();
                Task::none()
            }
            Message::MediaTogglePlayback => match self.media.toggle() {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            },
            Message::MediaSeek(seconds) => match self.media.seek(seconds) {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            },
            Message::MediaError(error) => {
                eprintln!("[arcadia-native] trailer: {error}");
                self.media.report_error(error);
                Task::none()
            }
            Message::LaunchGame(id) => self.request_launch(id),
            Message::StopGame => self.request_stop(),
            Message::LaunchGameDone { id, result } => self.finish_launch(id, result),
            Message::StopGameDone(result) => self.finish_stop(result),
            Message::LaunchPoll => {
                let was_running = self.launch.status == GameRunStatus::Running;
                self.launch.apply_snapshot(self.backend.launch_state());
                if was_running && self.launch.status == GameRunStatus::Idle {
                    self.restore_window_task()
                } else {
                    Task::none()
                }
            }
            Message::LaunchConfigLoaded(result) => {
                match result {
                    Ok(config) => {
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                    }
                    Err(error) => {
                        // Preferência opcional: sem config, não minimiza e o
                        // resto do app segue normalmente.
                        eprintln!("[arcadia-native] minimize_on_game_launch indisponível: {error}");
                    }
                }
                Task::none()
            }
        }
    }

    /// Entra no Big Picture guardando a tela desktop para restaurar ao sair.
    fn enter_big_picture(&mut self) -> Task<Message> {
        if matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        self.previous_screen = Some(self.screen.clone());
        // O pipeline de trailer para ao entrar: o console não toca áudio nesta
        // onda e não deve deixar um processo rodando atrás da tela cheia.
        self.media.clear();
        self.game_media = None;
        self.screen = Screen::BigPicture;

        let count = self.library.visible_all().len();
        self.big_picture.enter(count);
        eprintln!("[arcadia-native] Big Picture: {count} jogo(s)");

        let urls = self.library.image_urls();
        let mut tasks = vec![self.cover_tasks(urls)];
        if let Some(window) = self.window {
            tasks.push(window::set_mode(window, window::Mode::Fullscreen));
            tasks.push(window::size(window).map(Message::WindowResized));
        }
        Task::batch(tasks)
    }

    /// Volta ao desktop na tela em que o console foi aberto.
    fn exit_big_picture(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let previous = self.previous_screen.take().unwrap_or(Screen::Library);
        self.big_picture.leave();
        self.screen = previous;
        eprintln!("[arcadia-native] Big Picture encerrado");

        let mut tasks = Vec::new();
        if let Some(window) = self.window {
            tasks.push(window::set_mode(window, window::Mode::Windowed));
        }

        // A ficha restaurada precisa do trailer de novo: ao entrar no console
        // o pipeline foi descartado junto com o `game_media`.
        if let Screen::Game(id) = self.screen.clone() {
            let game = self.library.game(&id).cloned();
            let urls = game.as_ref().map(Game::image_urls).unwrap_or_default();
            self.media.clear();
            self.game_media = None;
            tasks.push(self.cover_tasks(urls));
            if let Some(game) = game {
                tasks.push(self.game_media_task(&game));
            }
        }

        Task::batch(tasks)
    }

    /// Abre a ficha do jogo no desktop (mesmo caminho do clique na biblioteca).
    fn open_game(&mut self, id: String) -> Task<Message> {
        let game = self.library.game(&id).cloned();
        let urls = game.as_ref().map(Game::image_urls).unwrap_or_default();
        self.screen = Screen::Game(id);
        // Troca de jogo nunca reaproveita o pipeline anterior.
        self.media.clear();
        self.game_media = None;
        let covers = self.cover_tasks(urls);
        let media = game
            .map(|game| self.game_media_task(&game))
            .unwrap_or_else(Task::none);
        Task::batch([covers, media])
    }

    /// "Jogar" na ficha ou no Big Picture: dispara `game:launch`.
    ///
    /// O runtime só aceita uma sessão; um segundo jogo é recusado na UI com
    /// mensagem explícita antes de chegar ao backend. `minimize_on_game_launch`
    /// é aplicado só depois do `ok:true`, para uma falha continuar visível.
    fn request_launch(&mut self, id: String) -> Task<Message> {
        if matches!(
            self.launch.status,
            GameRunStatus::Launching | GameRunStatus::Stopping
        ) {
            return Task::none();
        }

        if self.launch.status == GameRunStatus::Running
            && self.launch.game_id.as_deref() == Some(id.as_str())
        {
            return Task::none();
        }

        if self.launch.busy_with_other(&id) {
            let running = self.launch.game_id.clone().unwrap_or_default();
            let title = self
                .library
                .game(&running)
                .map(|game| game.title.clone())
                .unwrap_or(running);
            self.launch.error = Some(format!("Feche \"{title}\" antes de abrir outro jogo."));
            return Task::none();
        }

        let Some(game) = self.library.game(&id).cloned() else {
            return Task::none();
        };
        if !game.installed {
            self.launch.error = Some(format!("\"{}\" não está instalado.", game.title));
            return Task::none();
        }

        self.launch.status = GameRunStatus::Launching;
        self.launch.game_id = Some(id.clone());
        self.launch.pid = None;
        self.launch.error = None;

        let backend = self.backend.clone();
        Task::perform(
            async move { backend.launch_game(&game, None).await },
            move |result| Message::LaunchGameDone { id, result },
        )
    }

    /// "Parar": `game:close` encerra o processo e a árvore dele.
    fn request_stop(&mut self) -> Task<Message> {
        if self.launch.status != GameRunStatus::Running {
            return Task::none();
        }

        self.launch.status = GameRunStatus::Stopping;
        self.launch.error = None;
        let backend = self.backend.clone();
        Task::perform(async move { backend.close_game().await }, Message::StopGameDone)
    }

    fn finish_launch(&mut self, id: String, result: Result<(), String>) -> Task<Message> {
        // Uma resposta atrasada não pode reabrir uma sessão que já mudou.
        if !self.launch.is_launching(&id) {
            return Task::none();
        }

        match result {
            Ok(()) => {
                eprintln!("[arcadia-native] jogo iniciado: {id}");
                self.launch.status = GameRunStatus::Running;
                self.launch.game_id = Some(id);
                self.launch.pid = None;
                self.launch.error = None;
                if self.launch.minimize_on_launch && self.window.is_some() {
                    self.launch.minimized = true;
                    self.window_task(|window| window::minimize(window, true))
                } else {
                    Task::none()
                }
            }
            Err(error) => {
                eprintln!("[arcadia-native] falha ao abrir jogo: {error}");
                self.launch.status = GameRunStatus::Idle;
                self.launch.game_id = None;
                self.launch.pid = None;
                self.launch.error = Some(format!("Falha ao abrir o jogo: {error}"));
                Task::none()
            }
        }
    }

    fn finish_stop(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => {
                eprintln!("[arcadia-native] jogo encerrado");
                self.launch.status = GameRunStatus::Idle;
                self.launch.game_id = None;
                self.launch.pid = None;
                self.launch.error = None;
                self.restore_window_task()
            }
            Err(error) => {
                eprintln!("[arcadia-native] falha ao parar jogo: {error}");
                self.launch.status = GameRunStatus::Running;
                self.launch.error = Some(format!("Falha ao parar o jogo: {error}"));
                Task::none()
            }
        }
    }

    /// Desfaz só a minimização feita pelo launch; não mexe se o usuário
    /// minimizou a janela por conta própria.
    fn restore_window_task(&mut self) -> Task<Message> {
        if !self.launch.minimized {
            return Task::none();
        }
        self.launch.minimized = false;
        self.window_task(|window| window::minimize(window, false))
    }

    fn window_event(&mut self, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Resized(size) => {
                self.window_size = size;
                if matches!(self.screen, Screen::BigPicture) {
                    self.console_rail_scroll()
                } else {
                    Task::none()
                }
            }
            window::Event::Unfocused => {
                // Direção segurada não deve continuar repetindo ao voltar.
                self.big_picture.reset_input();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// ~60 fps: drena os eventos do gamepad e aplica um por um.
    fn console_tick(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        self.big_picture.refresh_clock();
        let events = self.big_picture.tick(Duration::from_millis(16));
        Task::batch(
            events
                .into_iter()
                .map(|event| self.console_nav(event))
                .collect::<Vec<_>>(),
        )
    }

    fn console_key(&mut self, event: keyboard::Event) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        if let keyboard::Event::KeyPressed { key, .. } = &event {
            if matches!(key, keyboard::Key::Named(keyboard::key::Named::F5)) {
                self.library.set_loading();
                return self.reload_task();
            }
        }

        match console_nav_from_key(&event) {
            Some(nav) => self.console_nav(nav),
            None => Task::none(),
        }
    }

    fn console_nav(&mut self, event: NavEvent) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = self.library.visible_all();
        let before = self.big_picture.rail_index();
        let effect = self.big_picture.handle(event, &games);
        let moved = before != self.big_picture.rail_index();
        let scroll = if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        Task::batch([scroll, self.console_effect(effect)])
    }

    fn console_select_game(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = self.library.visible_all();
        let (moved, effect) = self.big_picture.press_game(index, &games);
        let scroll = if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        Task::batch([scroll, self.console_effect(effect)])
    }

    fn console_select_tab(&mut self, tab: ConsoleTab) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = self.library.visible_all();
        self.big_picture.select_tab(tab, &games);
        self.console_rail_scroll()
    }

    fn console_action(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = self.library.visible_all();
        let effect = self.big_picture.press_action(index, &games);
        self.console_effect(effect)
    }

    fn console_effect(&mut self, effect: ConsoleEffect) -> Task<Message> {
        match effect {
            ConsoleEffect::None => Task::none(),
            ConsoleEffect::Exit => self.exit_big_picture(),
            ConsoleEffect::LaunchGame(id) => {
                if self.launch.is_running(&id) {
                    self.request_stop()
                } else {
                    self.request_launch(id)
                }
            }
            ConsoleEffect::Notice(text) => {
                self.big_picture.set_notice(text);
                Task::none()
            }
        }
    }

    fn console_rail_scroll(&self) -> Task<Message> {
        views::big_picture::rail_scroll_task(self.big_picture.rail_index(), self.window_size.width)
    }

    /// Timer de ~60 fps que drena os frames do GStreamer, o gamepad do console
    /// e o polling da sessão de jogo.
    ///
    /// O polling só existe enquanto há jogo abrindo/rodando; fora isso o app
    /// não acorda à toa. Os eventos `game:running` do IPC ainda não têm canal
    /// no shell nativo, então `LaunchRuntime::state_json` é a fonte de verdade.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();

        if matches!(self.screen, Screen::BigPicture) {
            subscriptions.push(
                iced::time::every(Duration::from_millis(16)).map(|_| Message::ConsoleTick),
            );
            subscriptions.push(iced::keyboard::listen().map(Message::ConsoleKey));
            subscriptions.push(window::events().filter_map(|(_, event)| match event {
                window::Event::Resized(_) | window::Event::Unfocused => {
                    Some(Message::WindowEvent(event))
                }
                _ => None,
            }));
        } else if matches!(self.screen, Screen::Game(_)) && self.media.is_active() {
            subscriptions.push(
                iced::time::every(Duration::from_millis(16)).map(|_| Message::MediaTick),
            );
        }

        if self.launch.needs_poll() {
            subscriptions.push(
                iced::time::every(Duration::from_millis(1500)).map(|_| Message::LaunchPoll),
            );
        }

        Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        if matches!(self.screen, Screen::BigPicture) {
            return views::big_picture::view(self);
        }

        let sidebar = views::sidebar::view(self);
        let content: Element<'_, Message> = match &self.screen {
            Screen::Library => views::library::view(self),
            Screen::Game(id) => match self.library.game(id) {
                Some(game) => views::game::view(
                    game,
                    &self.covers,
                    &self.media,
                    self.game_media.as_ref(),
                    &self.launch,
                ),
                None => views::missing_game(),
            },
            Screen::Section(Section::Downloads) => views::downloads::view(self),
            Screen::Section(Section::Settings) => views::settings::view(self),
            Screen::Section(Section::Sources) => views::sources::view(self),
            Screen::Section(Section::Friends) => views::friends::view(self),
            Screen::Section(Section::Components) => views::components::view(self),
            Screen::Section(Section::Home) => views::home::view(self),
            Screen::Section(section) => views::section_placeholder(*section),
            Screen::BigPicture => views::big_picture::view(self),
        };

        iced::widget::column![
            views::chrome::top_bar(),
            iced::widget::row![
                sidebar,
                iced::widget::container(content)
                    .width(Fill)
                    .height(Fill)
                    .style(theme::app_background)
            ]
            .width(Fill)
            .height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Carrega a seção sob demanda. `force` é o botão "Atualizar"; sem ele, o
    /// cache em memória evita refazer IPC a cada troca de aba.
    fn load_section(&mut self, section: Section, force: bool) -> Task<Message> {
        match section {
            Section::Downloads if force || matches!(self.downloads.status, LoadStatus::Idle) => {
                self.downloads.status = LoadStatus::Loading;
                self.downloads.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_downloads().await },
                    Message::DownloadsLoaded,
                )
            }
            Section::Settings if force || matches!(self.settings.status, LoadStatus::Idle) => {
                self.settings.status = LoadStatus::Loading;
                self.settings.save_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_settings().await },
                    Message::SettingsLoaded,
                )
            }
            Section::Sources if force || matches!(self.sources.status, LoadStatus::Idle) => {
                self.sources.status = LoadStatus::Loading;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_sources().await },
                    Message::SourcesLoaded,
                )
            }
            Section::Friends if force || matches!(self.friends.status, LoadStatus::Idle) => {
                self.friends.status = LoadStatus::Loading;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_friends(force).await },
                    Message::FriendsLoaded,
                )
            }
            Section::Components if force || matches!(self.components.status, LoadStatus::Idle) => {
                self.components.status = LoadStatus::Loading;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_plugins().await },
                    Message::ComponentsLoaded,
                )
            }
            _ => Task::none(),
        }
    }

    /// Envelopa um patch `config:set` em uma única chave e recarrega o estado
    /// tipado com a resposta redigida do dispatcher.
    fn save_setting(&mut self, key: &'static str, value: Value) -> Task<Message> {
        if self.settings.saving.is_some() {
            return Task::none();
        }
        self.settings.saving = Some(key.to_string());
        self.settings.save_error = None;
        let backend = self.backend.clone();
        Task::perform(
            async move {
                let mut patch = serde_json::Map::new();
                patch.insert(key.to_string(), value);
                backend.save_config(Value::Object(patch)).await
            },
            move |result| Message::SettingsSaved {
                key: key.to_string(),
                result,
            },
        )
    }

    fn window_task<F>(&self, action: F) -> Task<Message>
    where
        F: FnOnce(window::Id) -> Task<Message>,
    {
        self.window.map(action).unwrap_or_else(Task::none)
    }

    fn reload_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_library().await },
            Message::LibraryLoaded,
        )
    }

    fn game_media_task(&self, game: &Game) -> Task<Message> {
        let backend = self.backend.clone();
        let id = game.id.clone();
        let title = game.title.clone();
        Task::perform(
            async move { backend.load_game_media(id, title).await },
            Message::MediaLoaded,
        )
    }

    fn cover_tasks(&mut self, urls: Vec<String>) -> Task<Message> {
        let mut tasks = Vec::new();

        for url in urls {
            if !self.covers.request(&url) {
                continue;
            }

            let backend = self.backend.clone();
            let fetch_url = url.clone();
            tasks.push(Task::perform(
                async move { backend.fetch_cover(fetch_url).await },
                move |handle| Message::CoverLoaded { url, handle },
            ));
        }

        Task::batch(tasks)
    }
}

/// Teclado do console: mesmas intenções do gamepad (`NavEvent`).
fn console_nav_from_key(event: &keyboard::Event) -> Option<NavEvent> {
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };

    use keyboard::key::Named;

    match key {
        keyboard::Key::Named(Named::ArrowUp) => Some(NavEvent::Up),
        keyboard::Key::Named(Named::ArrowDown) => Some(NavEvent::Down),
        keyboard::Key::Named(Named::ArrowLeft) => Some(NavEvent::Left),
        keyboard::Key::Named(Named::ArrowRight) => Some(NavEvent::Right),
        keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
            Some(NavEvent::Confirm)
        }
        keyboard::Key::Named(Named::Escape) => Some(NavEvent::Back),
        keyboard::Key::Named(Named::Tab) => Some(if modifiers.shift() {
            NavEvent::PageLeft
        } else {
            NavEvent::PageRight
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, title: &str, favorite: bool, hidden: bool) -> Game {
        Game {
            id: id.to_string(),
            title: title.to_string(),
            launcher: "custom".to_string(),
            launch_cmd: Vec::new(),
            cover_url: None,
            hero_url: None,
            description: None,
            developer: None,
            publisher: None,
            genre: None,
            year: None,
            playtime_minutes: 0,
            installed: true,
            hidden,
            favorite,
            categories: Vec::new(),
        }
    }

    #[test]
    fn visible_games_hide_and_filter_then_sort_favorites_first() {
        let mut library = Library::loading();
        library.ready(vec![
            game("1", "Zelda", false, false),
            game("2", "Alpha", false, false),
            game("3", "Beta", true, false),
            game("4", "Oculto", true, true),
        ]);

        let titles: Vec<&str> = library
            .visible_games("")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Beta", "Alpha", "Zelda"]);

        let filtered: Vec<&str> = library
            .visible_games("ALP")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(filtered, vec!["Alpha"]);

        assert!(library.game("4").is_some());
        assert_eq!(library.visible_all().len(), 3);
    }

    #[test]
    fn section_navigation_routes_library_and_placeholders() {
        assert_eq!(Screen::for_section(Section::Library), Screen::Library);
        assert_eq!(
            Screen::for_section(Section::Downloads),
            Screen::Section(Section::Downloads)
        );

        let mut screen = Screen::Game("steam:10".to_string());
        screen.back_to_library();
        assert_eq!(screen, Screen::Library);
    }

    #[test]
    fn image_urls_dedupe_cover_and_hero() {
        let mut library = Library::loading();
        let mut first = game("1", "A", false, false);
        first.cover_url = Some("https://example.test/a.jpg".to_string());
        first.hero_url = Some("https://example.test/a.jpg".to_string());
        let mut second = game("2", "B", false, false);
        second.cover_url = Some("https://example.test/b.jpg".to_string());
        library.ready(vec![first, second]);

        assert_eq!(
            library.image_urls(),
            vec![
                "https://example.test/a.jpg".to_string(),
                "https://example.test/b.jpg".to_string()
            ]
        );
    }

    #[test]
    fn launch_state_is_per_game_and_survives_polling_while_launching() {
        let mut launch = GameLaunchState {
            status: GameRunStatus::Launching,
            game_id: Some("steam:10".to_string()),
            ..GameLaunchState::default()
        };
        assert!(launch.is_launching("steam:10"));
        assert!(!launch.is_running("steam:10"));
        assert!(launch.needs_poll());

        // O processo ainda não existe durante o `game:launch`: um snapshot
        // vazio não pode cancelar o estado "Abrindo…".
        launch.apply_snapshot(LaunchSnapshot::default());
        assert!(launch.is_launching("steam:10"));

        launch.status = GameRunStatus::Running;
        launch.pid = Some(42);
        launch.apply_snapshot(LaunchSnapshot {
            running: true,
            game_id: Some("steam:10".to_string()),
            pid: Some(42),
        });
        assert!(launch.is_running("steam:10"));
        assert_eq!(launch.pid, Some(42));

        // Processo terminou sozinho: o polling devolve a UI para Idle.
        launch.apply_snapshot(LaunchSnapshot::default());
        assert_eq!(launch.status, GameRunStatus::Idle);
        assert!(launch.game_id.is_none());
        assert!(!launch.needs_poll());
    }

    #[test]
    fn another_game_cannot_launch_over_a_running_session() {
        let launch = GameLaunchState {
            status: GameRunStatus::Running,
            game_id: Some("steam:10".to_string()),
            ..GameLaunchState::default()
        };
        assert!(launch.busy_with_other("custom:1"));
        assert!(!launch.busy_with_other("steam:10"));
    }
}
