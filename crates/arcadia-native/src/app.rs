//! Estado da aplicação e arquitetura Elm do iced.
//!
//! `App` guarda somente dados de UI. Trabalho de rede/IPC vira `Task`, e as
//! mensagens voltam para `update` — nenhuma chamada bloqueante roda na thread
//! que desenha.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use arcadia_native_input::NavEvent;
use iced::widget::image::Handle;
use iced::{keyboard, window, Element, Fill, Size, Subscription, Task};
use serde_json::{json, Value};

use crate::backend::{
    Achievement, AchievementsData, Backend, DownloadAction, DownloadItem, DownloadsData,
    FriendsLoad, Game, GameMedia, GameSettingsEnvelope, GameSettingsView, GameStats, LaunchEvent,
    LaunchToggle, NewsItem, PluginItem, ProtonDbInfo, RetroGame, RetroGameDetail,
    RetroOfferDetail, RetroPage, RetroSource, RetroSystemInfo, SettingsConfig, SourceItem,
    StoreDepot, StoreGame, StoreInfo, StoreInstallInfo, StoreLibrary, StorePage, StoreSearch,
    TrailerResult,
};
use crate::big_picture::{
    home_shelf, BigPicture, ConsoleEffect, ConsoleTab, ConsoleZone, ACTION_FAVORITE, ACTION_MORE,
    ACTION_PLAY, ACTION_TRAILER,
};
use crate::covers::CoverCache;
use crate::i18n::{I18n, Lang};
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
    /// Resultado de `game:sysinfo` para o jogo aberto no desktop.
    MediaLoaded {
        game_id: String,
        media: GameMedia,
    },
    /// Conquistas Steam/RetroAchievements do jogo aberto.
    AchievementsLoaded {
        game_id: String,
        result: Result<AchievementsData, String>,
    },
    /// ProtonDB do appid do jogo aberto.
    ProtonDbLoaded {
        game_id: String,
        result: Result<Option<ProtonDbInfo>, String>,
    },
    /// Owners/CCU e reviews do jogo aberto.
    GameStatsLoaded {
        game_id: String,
        result: Result<Option<GameStats>, String>,
    },
    /// Miniatura escolhida na faixa de screenshots.
    SelectScreenshot(usize),
    /// Recarrega `game:sysinfo` e os painéis do jogo atual.
    ReloadGameData,
    /// Resultado de `game:sysinfo` para o painel do Big Picture.
    ConsoleMediaLoaded {
        id: String,
        media: GameMedia,
    },
    /// Resultado de `overrides:set` disparado pelo coração/menu do console.
    ConsoleFavoriteDone {
        id: String,
        result: Result<Vec<Game>, String>,
    },
    /// `news:get` para a aba Notícias do console.
    ConsoleNewsLoaded(Result<Vec<NewsItem>, String>),
    ConsoleNewsSelected(usize),
    ConsoleNewsReload,
    ConsoleMenuAction(usize),
    ConsoleCloseMenu,
    /// Foco programático na busca da Loja (clique ou A no campo).
    ConsoleStoreFocus,
    /// Abre a ficha da loja no console sem sair do Big Picture.
    ConsoleStoreOpenDetail(StoreGame),
    ConsoleStoreCloseDetail,
    /// Tick da Subscription: drena frames e sincroniza o estado do player.
    MediaTick,
    MediaTogglePlayback,
    MediaSeek(f64),
    MediaError(String),
    /// Abre o seletor de trailers do YouTube sobre a ficha do jogo.
    OpenTrailerPicker,
    CloseTrailerPicker,
    TrailerQueryChanged(String),
    TrailerSearchSubmitted,
    TrailerSearchDone {
        query: String,
        result: Result<Vec<TrailerResult>, String>,
    },
    TrailerPathLoaded {
        game_id: String,
        result: Result<Option<String>, String>,
    },
    /// Escolha de um vídeo no seletor; baixa via yt-dlp sem bloquear a UI.
    TrailerDownloadRequested(TrailerResult),
    TrailerDownloadDone {
        game_id: String,
        url: String,
        result: Result<String, String>,
    },
    /// "Abrir trailer baixado" quando `trailer:path` encontrou um arquivo.
    TrailerOpenDownloaded,
    /// "Jogar" na ficha ou no Big Picture: dispara `game:launch`.
    LaunchGame(String),
    /// "Parar": encerra o processo atual via `game:close`.
    StopGame,
    LaunchGameDone {
        id: String,
        result: Result<(), String>,
    },
    StopGameDone(Result<(), String>),
    /// Tick de baixa frequência que drena a fila de eventos do `NativeShell`.
    LaunchEventsTick,
    /// `config:get` silencioso no boot para saber se minimiza ao lançar.
    LaunchConfigLoaded(Result<SettingsConfig, String>),
    /// Abre o diálogo de opções de inicialização do jogo.
    OpenLaunchOptions(String),
    CloseLaunchOptions,
    LaunchOptionsLoaded {
        id: String,
        result: Result<GameSettingsEnvelope, String>,
    },
    LaunchOptionsPrefixChanged(String),
    LaunchOptionsExeChanged(String),
    LaunchOptionsArgsChanged(String),
    LaunchOptionsHudChanged(String),
    LaunchOptionsToggle {
        key: LaunchToggle,
        value: bool,
    },
    LaunchOptionsEnvNameChanged(String),
    LaunchOptionsEnvValueChanged(String),
    LaunchOptionsEnvAdd,
    LaunchOptionsEnvRemove(usize),
    LaunchOptionsModeChanged(Option<String>),
    LaunchOptionsSave,
    LaunchOptionsSaved {
        id: String,
        result: Result<GameSettingsView, String>,
    },
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
    StoreTabChanged(StoreTab),
    StoreQueryChanged(String),
    StoreSearchSubmitted,
    StoreClearSearch,
    StoreSuggestionPicked(StoreGame),
    StoreSearchDone {
        query: String,
        result: Result<StoreSearch, String>,
    },
    StoreSuggestDone {
        query: String,
        result: Result<Vec<StoreGame>, String>,
    },
    StoreBrowseLoaded {
        append: bool,
        result: Result<StorePage, String>,
    },
    StoreLoadMore,
    StoreOpenDetail(StoreGame),
    StoreCloseDetail,
    StoreDetailLoaded {
        appid: String,
        result: Result<StoreInfo, String>,
    },
    StoreInstallInfoLoaded {
        appid: String,
        result: Result<StoreInstallInfo, String>,
    },
    StoreLibrariesLoaded(Result<Vec<StoreLibrary>, String>),
    StoreDepotToggled(String),
    StoreRecommendedDepots,
    StoreClearDepots,
    StoreLibrarySelected(usize),
    StoreInstallRequested,
    StoreInstallDone {
        appid: String,
        result: Result<(), String>,
    },
    RetroQueryChanged(String),
    RetroSystemChanged(RetroSystemInfo),
    RetroSearchSubmitted,
    RetroClearSearch,
    RetroPageLoaded {
        generation: u64,
        offset: u64,
        result: Result<RetroPage, String>,
    },
    RetroPrevPage,
    RetroNextPage,
    RetroOpenDetail(RetroGame),
    RetroCloseDetail,
    RetroDetailLoaded {
        id: String,
        result: Result<RetroGameDetail, String>,
    },
    RetroOfferRequested(String),
    RetroOfferLoaded {
        offer_id: String,
        result: Result<RetroOfferDetail, String>,
    },
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

/// Itens fixos da sidebar. Biblioteca, Home e Lojas têm tela real; Big
/// Picture ainda abre um placeholder explícito.
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

    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            Section::Home => "sidebar.inicio",
            Section::Library => "sidebar.biblioteca",
            Section::Stores => "sidebar.lojas",
            Section::Components => "sidebar.plugins",
            Section::Downloads => "sidebar.downloads",
            Section::Sources => "sidebar.fontes",
            Section::Friends => "amigos.titulo",
            Section::Settings => "settings.title",
            Section::BigPicture => "sidebar.bigpicture",
        };
        i18n.t(key)
    }

    pub fn stub_message<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            Section::Home => "native.section.stub.home",
            Section::Library => "native.section.stub.library",
            Section::Stores => "native.section.stub.stores",
            Section::Components => "native.section.stub.components",
            Section::Downloads => "native.section.stub.downloads",
            Section::Sources => "native.section.stub.sources",
            Section::Friends => "native.section.stub.friends",
            Section::Settings => "native.section.stub.settings",
            Section::BigPicture => "desktop.big_picture_desc",
        };
        i18n.t(key)
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
/// jogo o estado atual pertence. `error` e `warnings` vêm do dispatcher e são
/// exibidos na ficha e no painel do Big Picture — nada fica só no stderr.
#[derive(Debug, Clone, Default)]
pub struct GameLaunchState {
    pub status: GameRunStatus,
    pub game_id: Option<String>,
    pub error: Option<String>,
    /// Avisos de `game:launchWarning` (ex.: Mangohud não instalado).
    pub warnings: Vec<String>,
    /// Jogo aos quais os avisos pertencem; `None` mostra no jogo aberto.
    pub warnings_game_id: Option<String>,
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

    /// Avisos do lançamento para o jogo da ficha (vazio se forem de outro).
    pub fn warnings_for(&self, id: &str) -> &[String] {
        match self.warnings_game_id.as_deref() {
            Some(warning_id) if warning_id != id => &[],
            _ => &self.warnings,
        }
    }

    /// Aplica um evento do `LaunchRuntime` sem consultar o processo.
    fn apply_event(&mut self, event: LaunchEvent) {
        match event {
            LaunchEvent::Running(running) => {
                if running {
                    self.status = GameRunStatus::Running;
                    self.error = None;
                } else if !matches!(
                    self.status,
                    GameRunStatus::Launching | GameRunStatus::Idle
                ) {
                    // Durante "Abrindo…" o `idle` do launchState é a fonte de
                    // verdade; um `game:running(false)` atrasado pode ser de
                    // uma sessão anterior.
                    self.status = GameRunStatus::Idle;
                    self.game_id = None;
                }
            }
            LaunchEvent::Active { running, game_id } => {
                if running {
                    self.status = GameRunStatus::Running;
                    if game_id.is_some() {
                        self.game_id = game_id;
                    }
                    self.error = None;
                } else if self.matches_game(game_id.as_deref())
                    && !matches!(
                        self.status,
                        GameRunStatus::Launching | GameRunStatus::Idle
                    )
                {
                    self.status = GameRunStatus::Idle;
                    self.game_id = None;
                }
            }
            LaunchEvent::Phase { phase, game_id } => match phase {
                crate::backend::LaunchPhase::Starting => {
                    self.status = GameRunStatus::Launching;
                    if game_id.is_some() {
                        self.game_id = game_id;
                    }
                    self.error = None;
                    self.warnings.clear();
                    self.warnings_game_id = None;
                }
                crate::backend::LaunchPhase::Running => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Running;
                        if game_id.is_some() {
                            self.game_id = game_id;
                        }
                        self.error = None;
                    }
                }
                crate::backend::LaunchPhase::Stopping => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Stopping;
                        if game_id.is_some() {
                            self.game_id = game_id;
                        }
                    }
                }
                crate::backend::LaunchPhase::Idle => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Idle;
                        self.game_id = None;
                    }
                }
            },
            LaunchEvent::Warning { game_id, warnings } => {
                self.warnings = warnings;
                self.warnings_game_id = game_id;
            }
            LaunchEvent::Error { game_id, error } => {
                if self.matches_game(game_id.as_deref()) {
                    self.status = GameRunStatus::Idle;
                    self.game_id = game_id;
                    self.error = Some(format!("Falha ao abrir o jogo: {error}"));
                }
            }
        }
    }

    /// Evento sem `gameId` é aceito; com id, só altera a sessão atual.
    fn matches_game(&self, game_id: Option<&str>) -> bool {
        match (game_id, self.game_id.as_deref()) {
            (Some(event_id), Some(current)) => event_id == current,
            _ => true,
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

/// Estado do diálogo "Opções de inicialização".
///
/// `mode` é escolha de sessão (não existe em `game_settings.json`): vale para
/// os próximos `game:launch` do jogo enquanto ele estiver aberto no diálogo.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptionsState {
    pub open: bool,
    pub game_id: String,
    pub game_title: String,
    pub status: LoadStatus,
    pub settings: Option<GameSettingsView>,
    pub default_prefix: String,
    pub mode: Option<String>,
    pub env_name: String,
    pub env_value: String,
    pub saving: bool,
    pub saved: bool,
    /// Há edições no formulário que ainda não foram gravadas.
    pub dirty: bool,
    pub error: Option<String>,
}

impl LaunchOptionsState {
    /// `mode` de `game:launch` para este jogo; `None` = automático.
    ///
    /// Só vale depois que as edições foram salvas: o `LaunchRuntime` lê o
    /// `game_settings.json`, não o rascunho da UI.
    pub fn mode_for(&self, id: &str) -> Option<&str> {
        if self.game_id == id && !self.dirty && !self.saving {
            self.mode.as_deref()
        } else {
            None
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.saved = false;
    }
}

/// Seletor de trailers do YouTube aberto sobre a ficha do jogo.
#[derive(Debug, Clone, Default)]
pub struct TrailerPickerState {
    pub open: bool,
    pub game_id: String,
    pub game_title: String,
    pub query: String,
    pub results: Vec<TrailerResult>,
    pub status: LoadStatus,
    /// `trailer:path` do jogo; `Some(file://…)` quando já existe download.
    pub existing_path: Option<String>,
    /// Vídeo em download (chamada longa do yt-dlp); a UI mostra "Baixando…".
    pub downloading: Option<TrailerResult>,
    pub download_error: Option<String>,
}

/// Aba Notícias do Big Picture: feed `news:get` + erro honesto da consulta.
#[derive(Debug, Clone, Default)]
pub struct NewsState {
    pub items: Vec<NewsItem>,
    pub status: LoadStatus,
    pub error: Option<String>,
}

/// Painel de conquistas da ficha: lista + estado explícito de carga.
#[derive(Debug, Clone, Default)]
pub struct AchievementsPanelState {
    pub status: LoadStatus,
    pub items: Vec<Achievement>,
    /// Ausência explicada (jogo não encontrado no RA, launcher sem suporte).
    pub note: Option<String>,
}

/// Painel ProtonDB da ficha.
#[derive(Debug, Clone, Default)]
pub struct ProtonPanelState {
    pub status: LoadStatus,
    pub info: Option<ProtonDbInfo>,
    /// Ausência explicada (jogo não-Steam, sem relatórios).
    pub note: Option<String>,
}

/// Painel de estatísticas/reviews da ficha.
#[derive(Debug, Clone, Default)]
pub struct StatsPanelState {
    pub status: LoadStatus,
    pub stats: Option<GameStats>,
    /// Ausência explicada (jogo não-Steam, API sem dados).
    pub note: Option<String>,
}

/// Dados dos painéis por jogo; o mapa em [`App`] é o cache da sessão.
#[derive(Debug, Clone, Default)]
pub struct GamePanels {
    pub achievements: AchievementsPanelState,
    pub proton: ProtonPanelState,
    pub stats: StatsPanelState,
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

    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            SettingsTab::General => "settings.general",
            SettingsTab::Appearance => "native.settings.appearance",
            SettingsTab::Downloads => "sidebar.downloads",
            SettingsTab::Playback => "native.settings.playback",
            SettingsTab::Integrations => "settings.integracoes",
            SettingsTab::About => "about.titulo",
        };
        i18n.t(key)
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

/// Aba interna de Lojas: a mesma alternância `steam | retro` da web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoreTab {
    #[default]
    Steam,
    Retro,
}

impl StoreTab {
    pub fn label(self) -> &'static str {
        match self {
            StoreTab::Steam => "Loja",
            StoreTab::Retro => "Retro",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StoreSearchState {
    /// Termo efetivamente enviado ao dispatcher (pode diferir do rascunho).
    pub query: String,
    pub status: LoadStatus,
    pub result: Option<StoreSearch>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreBrowseState {
    pub games: Vec<StoreGame>,
    pub status: LoadStatus,
    pub total: Option<u64>,
    pub has_more: bool,
    pub next_offset: Option<u64>,
    pub loading_more: bool,
    /// Falha de "carregar mais" sem apagar a grade já visível.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoreDetailState {
    pub game: StoreGame,
    pub info: Option<StoreInfo>,
    pub status: LoadStatus,
    pub error: Option<String>,
    /// Manifesto de `store:installInfo` (depots, token, tamanho, installdir).
    pub install: Option<StoreInstallInfo>,
    pub install_status: LoadStatus,
    pub install_error: Option<String>,
    /// Bibliotecas de `store:libraries`, usadas para escolher o `steamDir`.
    pub libraries: Vec<StoreLibrary>,
    pub libraries_status: LoadStatus,
    pub libraries_error: Option<String>,
    pub selected_library: usize,
    pub selected_depots: BTreeSet<String>,
    pub install_busy: bool,
    /// Confirmação exibida depois que `store:install` aceita o pedido.
    pub install_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RetroDetailState {
    /// Jogo semeado pelo card; a ficha completa substitui/completa os campos.
    pub game: RetroGame,
    pub detail: Option<RetroGameDetail>,
    pub status: LoadStatus,
    pub error: Option<String>,
    pub selected_offer: Option<String>,
    pub offer_detail: Option<RetroOfferDetail>,
    pub offer_status: LoadStatus,
    pub offer_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreState {
    pub tab: StoreTab,
    pub query: String,
    pub suggestions: Vec<StoreGame>,
    /// Guarda o termo da última consulta de sugestão para não repetir IPC.
    pub suggested_query: String,
    pub search: Option<StoreSearchState>,
    pub browse: StoreBrowseState,
    pub detail: Option<StoreDetailState>,
}

#[derive(Debug, Clone)]
pub struct RetroState {
    pub query: String,
    /// Termo efetivamente aplicado à grade; paginar não usa o rascunho.
    pub applied_query: String,
    pub system: RetroSystemInfo,
    pub games: Vec<RetroGame>,
    pub sources: Vec<RetroSource>,
    pub status: LoadStatus,
    pub total: Option<u64>,
    pub total_offers: Option<u64>,
    pub offset: u64,
    pub has_more: bool,
    pub error: Option<String>,
    /// Ficha aberta por clique num card; `None` na grade.
    pub detail: Option<RetroDetailState>,
    /// Descarta respostas de uma página antiga quando o usuário pagina rápido.
    pub generation: u64,
}

impl Default for RetroState {
    fn default() -> Self {
        Self {
            query: String::new(),
            applied_query: String::new(),
            system: crate::backend::RETRO_ALL_SYSTEMS,
            games: Vec::new(),
            sources: Vec::new(),
            status: LoadStatus::Idle,
            total: None,
            total_offers: None,
            offset: 0,
            has_more: false,
            error: None,
            detail: None,
            generation: 0,
        }
    }
}

pub const STORE_PAGE_SIZE: u64 = 24;
pub const RETRO_PAGE_SIZE: u64 = 24;

pub struct App {
    pub backend: Backend,
    pub screen: Screen,
    pub library: Library,
    pub covers: CoverCache,
    pub search: String,
    /// Idioma ativo do shell; carregado do `config:get` no boot.
    pub i18n: I18n,
    pub media: MediaPlayer,
    /// `None` enquanto o `game:sysinfo` está em voo.
    pub game_media: Option<GameMedia>,
    /// Índice da screenshot selecionada na galeria da ficha.
    pub selected_screenshot: usize,
    /// Painéis da ficha por jogo (conquistas, ProtonDB, stats/reviews).
    /// O mapa é o cache da sessão: reabrir um jogo não refaz o IPC.
    pub game_panels: HashMap<String, GamePanels>,
    /// Seletor de trailers do YouTube sobre a ficha.
    pub trailer_picker: TrailerPickerState,
    /// O usuário abriu um trailer local; o `game:sysinfo` em voo não pode
    /// trocar o pipeline pelo HLS da Steam.
    pub trailer_override: bool,
    pub downloads: DownloadsState,
    pub settings: SettingsState,
    pub sources: SourcesState,
    pub friends: FriendsState,
    pub components: ComponentsState,
    pub store: StoreState,
    pub retro: RetroState,
    pub sidebar_search: String,
    pub sidebar_games_open: bool,
    pub window: Option<window::Id>,
    /// Tamanho atual da janela; alimenta o scroll centralizado do trilho.
    pub window_size: Size,
    /// Estado do modo console (gamepad, foco, aba, painel).
    pub big_picture: BigPicture,
    /// Feed da aba Notícias do console.
    pub console_news: NewsState,
    /// Jogo cujo `game:sysinfo` alimenta o trailer do painel do console.
    pub console_media_id: Option<String>,
    /// Escrita de favorito em voo; evita dois `overrides:set` concorrentes.
    pub favorite_busy: Option<String>,
    /// Sessão de jogo: abrindo, rodando, parando e última falha.
    pub launch: GameLaunchState,
    /// Diálogo de opções de inicialização (Wine/Proton, env, args).
    pub launch_options: LaunchOptionsState,
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
            i18n: I18n::default(),
            media: MediaPlayer::new(),
            game_media: None,
            selected_screenshot: 0,
            game_panels: HashMap::new(),
            trailer_picker: TrailerPickerState::default(),
            trailer_override: false,
            downloads: DownloadsState::default(),
            settings: SettingsState::default(),
            sources: SourcesState::default(),
            friends: FriendsState::default(),
            components: ComponentsState::default(),
            store: StoreState::default(),
            retro: RetroState::default(),
            sidebar_search: String::new(),
            sidebar_games_open: true,
            window: None,
            window_size: Size::new(1600.0, 900.0),
            big_picture: BigPicture::new(),
            console_news: NewsState::default(),
            console_media_id: None,
            favorite_busy: None,
            launch: GameLaunchState::default(),
            launch_options: LaunchOptionsState::default(),
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
                self.reset_trailer_state();
                if section != Section::Stores {
                    self.store.detail = None;
                    self.retro.detail = None;
                }
                self.screen = Screen::for_section(section);
                self.load_section(section, false)
            }
            Message::OpenGame(id) => self.open_game(id),
            Message::BackToLibrary => {
                self.media.clear();
                self.game_media = None;
                self.reset_trailer_state();
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
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
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
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
                        self.settings.accent_draft = config.accent.clone();
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.settings.config = Some(config);
                        self.settings.status = LoadStatus::Ready;
                        self.settings.save_error = None;
                    }
                    Err(error) => {
                        // O seletor trocou o idioma antes do round-trip; se o
                        // config:set falhou, volta para o valor persistido.
                        if key == "language" {
                            if let Some(config) = &self.settings.config {
                                self.i18n.set_lang(
                                    Lang::from_code(&config.language).unwrap_or_default(),
                                );
                            }
                        }
                        self.settings.save_error = Some(self.i18n.t_args(
                            "native.settings.save_error",
                            &[("key", &key), ("error", &error)],
                        ));
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
                if let Some(lang) = Lang::from_code(&language) {
                    self.i18n.set_lang(lang);
                }
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
            Message::StoreTabChanged(tab) => {
                if self.store.tab == tab {
                    return Task::none();
                }
                self.store.tab = tab;
                self.store.detail = None;
                self.retro.detail = None;
                match tab {
                    StoreTab::Steam if matches!(self.store.browse.status, LoadStatus::Idle) => {
                        self.load_store_browse(false)
                    }
                    StoreTab::Retro if matches!(self.retro.status, LoadStatus::Idle) => {
                        self.load_retro_page(0)
                    }
                    _ => Task::none(),
                }
            }
            Message::StoreQueryChanged(query) => {
                self.store.query = query;
                let trimmed = self.store.query.trim();
                if trimmed.chars().count() < 2 {
                    self.store.suggestions.clear();
                    self.store.suggested_query.clear();
                    return Task::none();
                }
                if self.store.suggested_query == trimmed {
                    return Task::none();
                }
                self.store.suggested_query = trimmed.to_string();
                let request = trimmed.to_string();
                let message_request = request.clone();
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.suggest_store(request).await },
                    move |result| Message::StoreSuggestDone {
                        query: message_request,
                        result,
                    },
                )
            }
            Message::StoreSearchSubmitted => self.submit_store_search(),
            Message::StoreSuggestionPicked(suggestion) => {
                self.store.query = suggestion.title;
                self.store.suggestions.clear();
                self.submit_store_search()
            }
            Message::StoreClearSearch => {
                self.store.query.clear();
                self.store.suggestions.clear();
                self.store.suggested_query.clear();
                self.store.search = None;
                self.sync_console_store();
                Task::none()
            }
            Message::StoreSearchDone { query, result } => {
                let Some(search) = self.store.search.as_mut() else {
                    return Task::none();
                };
                if search.query != query {
                    return Task::none();
                }
                match result {
                    Ok(found) => {
                        let urls: Vec<String> =
                            found.games.iter().flat_map(StoreGame::image_urls).collect();
                        eprintln!(
                            "[arcadia-native] loja: {} resultado(s) para {query:?}",
                            found.games.len()
                        );
                        search.result = Some(found);
                        search.status = LoadStatus::Ready;
                        search.error = None;
                        self.sync_console_store();
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja: {error}");
                        search.result = None;
                        search.status = LoadStatus::Error(error.clone());
                        search.error = Some(error);
                        self.sync_console_store();
                        Task::none()
                    }
                }
            }
            Message::StoreSuggestDone { query, result } => {
                if self.store.query.trim() != query {
                    return Task::none();
                }
                // Sugestão em voo do termo que acabou de virar busca não deve
                // reabrir a lista sobre os resultados.
                if self
                    .store
                    .search
                    .as_ref()
                    .is_some_and(|search| search.query == query)
                {
                    return Task::none();
                }
                match result {
                    Ok(games) => {
                        let urls: Vec<String> =
                            games.iter().flat_map(StoreGame::image_urls).collect();
                        self.store.suggestions = games;
                        self.cover_tasks(urls)
                    }
                    Err(_) => {
                        // Sugestão é auxiliar: uma falha aqui não esconde a busca.
                        self.store.suggestions.clear();
                        Task::none()
                    }
                }
            }
            Message::StoreBrowseLoaded { append, result } => {
                self.store.browse.loading_more = false;
                match result {
                    Ok(page) => {
                        let urls: Vec<String> =
                            page.games.iter().flat_map(StoreGame::image_urls).collect();
                        eprintln!(
                            "[arcadia-native] loja: {} jogo(s) no catálogo (total {:?})",
                            page.games.len(),
                            page.total
                        );
                        if append {
                            for game in page.games {
                                if !self
                                    .store
                                    .browse
                                    .games
                                    .iter()
                                    .any(|current| current.appid == game.appid)
                                {
                                    self.store.browse.games.push(game);
                                }
                            }
                            if let Some(total) = page.total {
                                self.store.browse.total = Some(total);
                            }
                        } else {
                            self.store.browse.games = page.games;
                            self.store.browse.total = page.total;
                        }
                        self.store.browse.has_more = page.has_more;
                        self.store.browse.next_offset = page.next_offset.or_else(|| {
                            page.has_more
                                .then_some(self.store.browse.games.len() as u64)
                        });
                        self.store.browse.status = LoadStatus::Ready;
                        self.store.browse.error = None;
                        self.sync_console_store();
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        if append {
                            self.store.browse.error = Some(error);
                        } else {
                            eprintln!("[arcadia-native] loja: {error}");
                            self.store.browse.status = LoadStatus::Error(error.clone());
                            self.store.browse.error = Some(error);
                            self.store.browse.games.clear();
                            self.store.browse.total = None;
                            self.store.browse.has_more = false;
                            self.store.browse.next_offset = None;
                        }
                        self.sync_console_store();
                        Task::none()
                    }
                }
            }
            Message::StoreLoadMore => self.load_store_more(),
            Message::StoreOpenDetail(game) => self.open_store_detail(game),
            Message::StoreCloseDetail => {
                self.store.detail = None;
                Task::none()
            }
            Message::StoreDetailLoaded { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        let urls = info.image_urls();
                        detail.info = Some(info);
                        detail.status = LoadStatus::Ready;
                        detail.error = None;
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (ficha): {error}");
                        detail.status = LoadStatus::Error(error.clone());
                        detail.error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::StoreInstallInfoLoaded { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        detail.selected_depots = info.recommended_depots();
                        detail.install = Some(info);
                        detail.install_status = LoadStatus::Ready;
                        detail.install_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (installInfo): {error}");
                        detail.install = None;
                        detail.install_status = LoadStatus::Error(error.clone());
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreLibrariesLoaded(result) => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(libraries) => {
                        detail.libraries = libraries;
                        if detail.selected_library >= detail.libraries.len() {
                            detail.selected_library = 0;
                        }
                        detail.libraries_status = LoadStatus::Ready;
                        detail.libraries_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (libraries): {error}");
                        detail.libraries.clear();
                        detail.libraries_status = LoadStatus::Error(error.clone());
                        detail.libraries_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreDepotToggled(depot_id) => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if !detail.selected_depots.remove(&depot_id) {
                        detail.selected_depots.insert(depot_id);
                    }
                    detail.install_message = None;
                }
                Task::none()
            }
            Message::StoreRecommendedDepots => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if let Some(info) = detail.install.as_ref() {
                        detail.selected_depots = info.recommended_depots();
                        detail.install_message = None;
                    }
                }
                Task::none()
            }
            Message::StoreClearDepots => {
                if let Some(detail) = self.store.detail.as_mut() {
                    detail.selected_depots.clear();
                    detail.install_message = None;
                }
                Task::none()
            }
            Message::StoreLibrarySelected(index) => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if index < detail.libraries.len() {
                        detail.selected_library = index;
                        detail.install_message = None;
                    }
                }
                Task::none()
            }
            Message::StoreInstallRequested => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.install_busy {
                    return Task::none();
                }
                let Some(info) = detail.install.as_ref() else {
                    detail.install_error = Some(
                        "O manifesto de store:installInfo ainda não foi carregado.".to_string(),
                    );
                    return Task::none();
                };
                let Some(library) = detail.libraries.get(detail.selected_library) else {
                    detail.install_error = Some(
                        "Nenhuma biblioteca Steam encontrada para o store:install.".to_string(),
                    );
                    return Task::none();
                };
                let depots: Vec<StoreDepot> = info
                    .depots
                    .iter()
                    .filter(|depot| detail.selected_depots.contains(&depot.depot_id))
                    .cloned()
                    .collect();
                if depots.is_empty() {
                    detail.install_error = Some("Selecione ao menos um depot.".to_string());
                    return Task::none();
                }

                let payload = crate::backend::store_install_payload(
                    &detail.game,
                    info,
                    &library.steam_dir,
                    &depots,
                );
                let appid = detail.game.appid.clone();
                detail.install_busy = true;
                detail.install_error = None;
                detail.install_message = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.install_store_game(payload).await },
                    move |result| Message::StoreInstallDone { appid, result },
                )
            }
            Message::StoreInstallDone { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                detail.install_busy = false;
                match result {
                    Ok(()) => {
                        eprintln!(
                            "[arcadia-native] loja: {} enfileirado em store:install",
                            detail.game.title
                        );
                        detail.install_error = None;
                        detail.install_message = Some(format!(
                            "“{}” entrou na fila de downloads. Acompanhe em Downloads.",
                            detail.game.title
                        ));
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (install): {error}");
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::RetroQueryChanged(query) => {
                self.retro.query = query;
                Task::none()
            }
            Message::RetroSystemChanged(system) => {
                if self.retro.system == system {
                    return Task::none();
                }
                self.retro.system = system;
                self.load_retro_page(0)
            }
            Message::RetroSearchSubmitted => self.submit_retro_search(),
            Message::RetroClearSearch => {
                self.retro.query.clear();
                self.retro.applied_query.clear();
                self.load_retro_page(0)
            }
            Message::RetroPageLoaded {
                generation,
                offset,
                result,
            } => {
                if generation != self.retro.generation {
                    return Task::none();
                }
                match result {
                    Ok(page) => {
                        let urls: Vec<String> =
                            page.games.iter().flat_map(RetroGame::image_urls).collect();
                        eprintln!(
                            "[arcadia-native] retro: {} jogo(s), offset {}",
                            page.games.len(),
                            page.offset
                        );
                        self.retro.offset = offset;
                        self.retro.games = page.games;
                        self.retro.sources = page.sources;
                        self.retro.total = page.total;
                        self.retro.total_offers = page.total_offers;
                        self.retro.has_more = page.has_more;
                        self.retro.status = LoadStatus::Ready;
                        self.retro.error = None;
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro: {error}");
                        self.retro.status = LoadStatus::Error(error.clone());
                        self.retro.error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::RetroPrevPage => {
                if self.retro.offset == 0 || self.retro.status == LoadStatus::Loading {
                    return Task::none();
                }
                let offset = self.retro.offset.saturating_sub(RETRO_PAGE_SIZE);
                self.load_retro_page(offset)
            }
            Message::RetroNextPage => {
                if !self.retro.has_more || self.retro.status == LoadStatus::Loading {
                    return Task::none();
                }
                let offset = self.retro.offset + RETRO_PAGE_SIZE;
                self.load_retro_page(offset)
            }
            Message::RetroOpenDetail(game) => {
                let urls = game.image_urls();
                let detail_task = self.retro_game_task(&game);
                self.store.detail = None;
                self.retro.detail = Some(RetroDetailState {
                    game,
                    detail: None,
                    status: LoadStatus::Loading,
                    error: None,
                    selected_offer: None,
                    offer_detail: None,
                    offer_status: LoadStatus::Idle,
                    offer_error: None,
                });
                Task::batch([self.cover_tasks(urls), detail_task])
            }
            Message::RetroCloseDetail => {
                self.retro.detail = None;
                Task::none()
            }
            Message::RetroDetailLoaded { id, result } => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.id != id {
                    return Task::none();
                }
                let urls = match result {
                    Ok(full) => {
                        let urls = full.image_urls();
                        detail.game = full.game.clone();
                        detail.detail = Some(full);
                        detail.status = LoadStatus::Ready;
                        detail.error = None;
                        urls
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro (ficha): {error}");
                        detail.status = LoadStatus::Error(error.clone());
                        detail.error = Some(error);
                        Vec::new()
                    }
                };
                self.cover_tasks(urls)
            }
            Message::RetroOfferRequested(offer_id) => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.offer_status == LoadStatus::Loading
                    && detail.selected_offer.as_deref() == Some(offer_id.as_str())
                {
                    return Task::none();
                }
                detail.selected_offer = Some(offer_id.clone());
                detail.offer_detail = None;
                detail.offer_status = LoadStatus::Loading;
                detail.offer_error = None;
                let backend = self.backend.clone();
                let request_id = offer_id.clone();
                Task::perform(
                    async move { backend.load_retro_offer(request_id).await },
                    move |result| Message::RetroOfferLoaded { offer_id, result },
                )
            }
            Message::RetroOfferLoaded { offer_id, result } => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.selected_offer.as_deref() != Some(offer_id.as_str()) {
                    return Task::none();
                }
                match result {
                    Ok(offer) => {
                        detail.offer_detail = Some(offer);
                        detail.offer_status = LoadStatus::Ready;
                        detail.offer_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro (oferta): {error}");
                        detail.offer_detail = None;
                        detail.offer_status = LoadStatus::Error(error.clone());
                        detail.offer_error = Some(error);
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
                Task::batch([
                    self.console_sync_media(),
                    self.ensure_console_news(),
                ])
            }
            Message::ConsoleMenuAction(index) => {
                let games = console_shelf_for(&self.library, self.big_picture.tab());
                let effect = self.big_picture.press_menu(index, &games);
                self.console_effect(effect)
            }
            Message::ConsoleCloseMenu => {
                self.big_picture.close_menu();
                Task::none()
            }
            Message::ConsoleStoreFocus => {
                if self.big_picture.tab() != ConsoleTab::Stores {
                    return Task::none();
                }
                self.big_picture.begin_store_editing();
                self.console_store_focus_task(true)
            }
            Message::ConsoleStoreOpenDetail(game) => {
                if !self.big_picture.open_store_detail() {
                    return Task::none();
                }
                self.open_store_detail(game)
            }
            Message::ConsoleStoreCloseDetail => {
                self.big_picture.close_store_detail();
                self.store.detail = None;
                Task::none()
            }
            Message::ConsoleMediaLoaded { id, media } => {
                if !matches!(self.screen, Screen::BigPicture)
                    || !self.big_picture.overview_open()
                    || self.console_media_id.as_deref() != Some(id.as_str())
                {
                    return Task::none();
                }
                eprintln!(
                    "[arcadia-native] trailer (console): {} movie(s){}",
                    media.movies.len(),
                    media
                        .notice
                        .as_deref()
                        .map(|notice| format!(" — {notice}"))
                        .unwrap_or_default()
                );
                self.game_media = Some(media);
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
            Message::ConsoleFavoriteDone { id, result } => {
                self.favorite_busy = None;
                match result {
                    Ok(games) => {
                        self.library.ready(games);
                        self.big_picture
                            .sync_games(self.library.visible_all().len());
                        self.focus_console_game(&id);
                        let (title, favorite) = self
                            .library
                            .game(&id)
                            .map(|game| (game.title.clone(), game.favorite))
                            .unwrap_or_else(|| (id.clone(), false));
                        self.big_picture.set_notice(if favorite {
                            format!("\"{title}\" adicionado aos favoritos.")
                        } else {
                            format!("\"{title}\" removido dos favoritos.")
                        });
                        let urls = self.library.image_urls();
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] favorito: {error}");
                        let title = self
                            .library
                            .game(&id)
                            .map(|game| game.title.clone())
                            .unwrap_or(id);
                        self.big_picture
                            .set_notice(format!("Falha ao favoritar \"{title}\": {error}"));
                        Task::none()
                    }
                }
            }
            Message::ConsoleNewsLoaded(result) => {
                match result {
                    Ok(items) => {
                        eprintln!("[arcadia-native] notícias: {} item(ns)", items.len());
                        self.big_picture.sync_news(items.len());
                        self.console_news.items = items;
                        self.console_news.status = LoadStatus::Ready;
                        self.console_news.error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] notícias: {error}");
                        self.big_picture.sync_news(0);
                        self.console_news.items.clear();
                        self.console_news.status = LoadStatus::Error(error.clone());
                        self.console_news.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::ConsoleNewsSelected(index) => {
                self.big_picture.select_news(index);
                Task::none()
            }
            Message::ConsoleNewsReload => self.load_console_news(),

            Message::MediaLoaded { game_id, media: game_media } => {
                // Resposta do trailer do desktop não pode abrir mídia por cima
                // do painel do console; o console usa `ConsoleMediaLoaded`.
                if matches!(self.screen, Screen::BigPicture) {
                    return Task::none();
                }
                // Clique rápido em outro jogo: a resposta antiga não pode
                // trocar screenshots/descrição da ficha atual.
                if self.current_game().map(|game| game.id.as_str()) != Some(game_id.as_str()) {
                    return Task::none();
                }
                eprintln!(
                    "[arcadia-native] trailer: {} movie(s), {} screenshot(s){}",
                    game_media.movies.len(),
                    game_media.screenshots.len(),
                    game_media
                        .notice
                        .as_deref()
                        .map(|notice| format!(" — {notice}"))
                        .unwrap_or_default()
                );
                self.game_media = Some(game_media);
                self.selected_screenshot = 0;

                let mut urls = Vec::new();
                if let Some(media) = self.game_media.as_ref() {
                    for screenshot in &media.screenshots {
                        if let Some(url) = screenshot.thumb_or_full() {
                            urls.push(url.to_string());
                        }
                    }
                    if let Some(full) = media
                        .screenshots
                        .first()
                        .and_then(|screenshot| screenshot.full_or_thumb())
                    {
                        urls.push(full.to_string());
                    }
                }
                let images = self.cover_tasks(urls);

                // Um trailer local escolhido pelo usuário tem prioridade sobre
                // um `game:sysinfo` que ainda estava em voo.
                if self.trailer_override {
                    return images;
                }
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
                let playback = match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                };
                Task::batch([images, playback])
            }
            Message::AchievementsLoaded { game_id, result } => {
                let mut icons = Vec::new();
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(data) => {
                            eprintln!(
                                "[arcadia-native] conquistas: {} item(ns) para {game_id}",
                                data.items.len()
                            );
                            panels.achievements.items = data.items;
                            panels.achievements.note = data.note;
                            panels.achievements.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] conquistas: {error}");
                            panels.achievements.items.clear();
                            panels.achievements.note = None;
                            panels.achievements.status = LoadStatus::Error(error);
                        }
                    }
                    icons = panels
                        .achievements
                        .items
                        .iter()
                        .filter_map(|item| item.icon.clone())
                        .collect();
                }
                self.cover_tasks(icons)
            }
            Message::ProtonDbLoaded { game_id, result } => {
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(info) => {
                            panels.proton.info = info;
                            panels.proton.note = None;
                            panels.proton.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] protondb: {error}");
                            panels.proton.info = None;
                            panels.proton.status = LoadStatus::Error(error);
                        }
                    }
                }
                Task::none()
            }
            Message::GameStatsLoaded { game_id, result } => {
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(stats) => {
                            panels.stats.stats = stats;
                            panels.stats.note = None;
                            panels.stats.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] stats: {error}");
                            panels.stats.stats = None;
                            panels.stats.status = LoadStatus::Error(error);
                        }
                    }
                }
                Task::none()
            }
            Message::SelectScreenshot(index) => {
                self.selected_screenshot = index;
                let url = self
                    .game_media
                    .as_ref()
                    .and_then(|media| media.screenshots.get(index))
                    .and_then(|screenshot| screenshot.full_or_thumb())
                    .map(str::to_string);
                match url {
                    Some(url) => self.cover_tasks(vec![url]),
                    None => Task::none(),
                }
            }
            Message::ReloadGameData => {
                let Some(game) = self.current_game().cloned() else {
                    return Task::none();
                };
                self.game_panels.remove(&game.id);
                self.media.clear();
                self.game_media = None;
                self.selected_screenshot = 0;
                self.reset_trailer_state();
                let media = self.game_media_task(&game);
                let panels = self.game_panels_task(&game);
                Task::batch([media, panels])
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
            Message::OpenTrailerPicker => self.open_trailer_picker(),
            Message::CloseTrailerPicker => {
                self.trailer_picker.open = false;
                Task::none()
            }
            Message::TrailerQueryChanged(query) => {
                self.trailer_picker.query = query;
                Task::none()
            }
            Message::TrailerSearchSubmitted => self.submit_trailer_search(),
            Message::TrailerSearchDone { query, result } => {
                if !self.trailer_picker.open
                    || self.trailer_picker.downloading.is_some()
                    || self.trailer_picker.query.trim() != query
                {
                    return Task::none();
                }

                match result {
                    Ok(results) => {
                        let thumbnails: Vec<String> = results
                            .iter()
                            .filter_map(|result| result.thumbnail.clone())
                            .collect();
                        eprintln!(
                            "[arcadia-native] trailer: {} vídeo(s) para {query:?}",
                            results.len()
                        );
                        self.trailer_picker.results = results;
                        self.trailer_picker.status = LoadStatus::Ready;
                        self.cover_tasks(thumbnails)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] trailer: {error}");
                        self.trailer_picker.results.clear();
                        self.trailer_picker.status = LoadStatus::Error(error);
                        Task::none()
                    }
                }
            }
            Message::TrailerPathLoaded { game_id, result } => {
                if !self.trailer_picker.open || self.trailer_picker.game_id != game_id {
                    return Task::none();
                }
                match result {
                    Ok(path) => self.trailer_picker.existing_path = path,
                    Err(error) => eprintln!("[arcadia-native] trailer local: {error}"),
                }
                Task::none()
            }
            Message::TrailerDownloadRequested(video) => {
                if !self.trailer_picker.open || self.trailer_picker.downloading.is_some() {
                    return Task::none();
                }
                let id = self.trailer_picker.game_id.clone();
                eprintln!(
                    "[arcadia-native] trailer: baixando {:?} ({})",
                    video.title, video.url
                );
                self.trailer_picker.downloading = Some(video.clone());
                self.trailer_picker.download_error = None;
                let backend = self.backend.clone();
                let message_game_id = id.clone();
                let message_url = video.url.clone();
                Task::perform(
                    async move { backend.download_trailer(id, video.url).await },
                    move |result| Message::TrailerDownloadDone {
                        game_id: message_game_id,
                        url: message_url,
                        result,
                    },
                )
            }
            Message::TrailerDownloadDone {
                game_id,
                url,
                result,
            } => {
                let still_current = self.trailer_picker.open
                    && self.trailer_picker.game_id == game_id
                    && self
                        .trailer_picker
                        .downloading
                        .as_ref()
                        .is_some_and(|video| video.url == url);
                if !still_current {
                    // O usuário fechou/trocou de jogo durante o download: o
                    // arquivo fica no disco para "Abrir trailer baixado".
                    eprintln!(
                        "[arcadia-native] trailer: download de {url} terminou fora do seletor"
                    );
                    return Task::none();
                }

                self.trailer_picker.downloading = None;
                match result {
                    Ok(path) => {
                        eprintln!("[arcadia-native] trailer baixado: {path}");
                        self.trailer_picker.open = false;
                        self.open_local_trailer(path)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] trailer: {error}");
                        self.trailer_picker.download_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::TrailerOpenDownloaded => {
                let Some(path) = self.trailer_picker.existing_path.clone() else {
                    return Task::none();
                };
                self.trailer_picker.open = false;
                self.open_local_trailer(path)
            }
            Message::LaunchGame(id) => self.request_launch(id),
            Message::StopGame => self.request_stop(),
            Message::LaunchGameDone { id, result } => self.finish_launch(id, result),
            Message::StopGameDone(result) => self.finish_stop(result),
            Message::LaunchEventsTick => self.drain_launch_events(),
            Message::OpenLaunchOptions(id) => self.open_launch_options(id),
            Message::CloseLaunchOptions => {
                self.launch_options.open = false;
                Task::none()
            }
            Message::LaunchOptionsLoaded { id, result } => {
                if !self.launch_options.open || self.launch_options.game_id != id {
                    return Task::none();
                }
                match result {
                    Ok(envelope) => {
                        // `envelope.default_prefix` é o default das ferramentas
                        // Wine da UI web; o launch nativo tem fallback próprio
                        // (já gravado no estado ao abrir o diálogo).
                        if self.launch_options.mode.as_deref() == Some("exe")
                            && envelope.settings.exe_path.trim().is_empty()
                        {
                            self.launch_options.mode = None;
                        }
                        self.launch_options.settings = Some(envelope.settings);
                        self.launch_options.status = LoadStatus::Ready;
                        self.launch_options.saved = false;
                        self.launch_options.dirty = false;
                        self.launch_options.error = None;
                    }
                    Err(error) => {
                        self.launch_options.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::LaunchOptionsPrefixChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.prefix_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsExeChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    if value.trim().is_empty() && self.launch_options.mode.as_deref() == Some("exe")
                    {
                        self.launch_options.mode = None;
                    }
                    settings.exe_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsArgsChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.game_args = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsHudChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.dxvk_hud = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsToggle { key, value } => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.set_toggle(key, value);
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsEnvNameChanged(value) => {
                self.launch_options.env_name = value;
                Task::none()
            }
            Message::LaunchOptionsEnvValueChanged(value) => {
                self.launch_options.env_value = value;
                Task::none()
            }
            Message::LaunchOptionsEnvAdd => {
                self.add_launch_env_var();
                Task::none()
            }
            Message::LaunchOptionsEnvRemove(index) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    if index < settings.env_vars.len() {
                        settings.env_vars.remove(index);
                        self.launch_options.mark_dirty();
                    }
                }
                Task::none()
            }
            Message::LaunchOptionsModeChanged(mode) => {
                if let Some(value) = mode.as_deref() {
                    if !matches!(value, "steam" | "exe") {
                        return Task::none();
                    }
                    if value == "exe"
                        && self
                            .launch_options
                            .settings
                            .as_ref()
                            .map_or(true, |settings| settings.exe_path.trim().is_empty())
                    {
                        self.launch_options.error =
                            Some("Informe o executável antes de escolher o modo exe.".to_string());
                        return Task::none();
                    }
                }
                self.launch_options.mode = mode;
                self.launch_options.error = None;
                Task::none()
            }
            Message::LaunchOptionsSave => self.save_launch_options(),
            Message::LaunchOptionsSaved { id, result } => {
                if self.launch_options.game_id != id {
                    return Task::none();
                }
                self.launch_options.saving = false;
                match result {
                    Ok(settings) => {
                        // Edições feitas enquanto o save estava em voo não
                        // podem ser sobrescritas pela resposta do servidor.
                        if !self.launch_options.dirty {
                            self.launch_options.settings = Some(settings);
                            self.launch_options.saved = true;
                        }
                        self.launch_options.status = LoadStatus::Ready;
                        self.launch_options.error = None;
                    }
                    Err(error) => {
                        self.launch_options.saved = false;
                        self.launch_options.dirty = true;
                        self.launch_options.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::LaunchConfigLoaded(result) => {
                match result {
                    Ok(config) => {
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.big_picture.set_motion(!config.no_anim);
                    }
                    Err(error) => {
                        // Preferência opcional: sem config, não minimiza e o
                        // resto do app segue normalmente (pt-BR de fallback).
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
        // O pipeline de trailer do desktop para ao entrar; o painel do console
        // abre o trailer do jogo selecionado sob demanda.
        self.media.clear();
        self.game_media = None;
        self.reset_trailer_state();
        self.console_media_id = None;
        self.favorite_busy = None;
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
        self.media.clear();
        self.game_media = None;
        self.console_media_id = None;
        self.favorite_busy = None;
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
            self.reset_trailer_state();
            tasks.push(self.cover_tasks(urls));
            if let Some(game) = game {
                tasks.push(self.game_media_task(&game));
            }
        }

        Task::batch(tasks)
    }

    /// Fecha o seletor e devolve o trailer padrão (`game:sysinfo`) ao estado
    /// inicial sempre que a ficha deixa de ser a tela atual.
    fn reset_trailer_state(&mut self) {
        self.trailer_picker = TrailerPickerState::default();
        self.trailer_override = false;
    }

    fn current_game(&self) -> Option<&Game> {
        match &self.screen {
            Screen::Game(id) => self.library.game(id),
            _ => None,
        }
    }

    /// Abre o seletor com busca automática pelo título do jogo e consulta em
    /// paralelo se já existe trailer baixado (`trailer:path`).
    fn open_trailer_picker(&mut self) -> Task<Message> {
        let Some(game) = self.current_game().cloned() else {
            return Task::none();
        };

        self.trailer_picker = TrailerPickerState {
            open: true,
            game_id: game.id.clone(),
            game_title: game.title.clone(),
            query: game.title.clone(),
            status: LoadStatus::Loading,
            ..TrailerPickerState::default()
        };

        let backend = self.backend.clone();
        let path_id = game.id.clone();
        let message_game_id = path_id.clone();
        let path_task = Task::perform(
            async move { backend.trailer_path(path_id).await },
            move |result| Message::TrailerPathLoaded {
                game_id: message_game_id,
                result,
            },
        );

        let backend = self.backend.clone();
        let search_query = game.title;
        let message_query = search_query.clone();
        let search_task = Task::perform(
            async move { backend.search_trailers(search_query).await },
            move |result| Message::TrailerSearchDone {
                query: message_query,
                result,
            },
        );

        Task::batch([path_task, search_task])
    }

    fn submit_trailer_search(&mut self) -> Task<Message> {
        if !self.trailer_picker.open || self.trailer_picker.downloading.is_some() {
            return Task::none();
        }

        let query = self.trailer_picker.query.trim().to_string();
        if query.is_empty() {
            self.trailer_picker.results.clear();
            self.trailer_picker.status = LoadStatus::Error(
                "Informe o nome do jogo ou do vídeo para buscar.".to_string(),
            );
            return Task::none();
        }

        self.trailer_picker.query = query.clone();
        self.trailer_picker.results.clear();
        self.trailer_picker.status = LoadStatus::Loading;
        self.trailer_picker.download_error = None;

        let backend = self.backend.clone();
        let message_query = query.clone();
        Task::perform(
            async move { backend.search_trailers(query).await },
            move |result| Message::TrailerSearchDone {
                query: message_query,
                result,
            },
        )
    }

    /// Troca o HLS da Steam pelo arquivo baixado escolhido explicitamente.
    fn open_local_trailer(&mut self, path: String) -> Task<Message> {
        self.trailer_override = true;
        match self.media.open_local(path) {
            Ok(()) => Task::none(),
            Err(error) => Task::done(Message::MediaError(error)),
        }
    }

    /// Abre a ficha do jogo no desktop (mesmo caminho do clique na biblioteca).
    fn open_game(&mut self, id: String) -> Task<Message> {
        let game = self.library.game(&id).cloned();
        let urls = game.as_ref().map(Game::image_urls).unwrap_or_default();
        self.screen = Screen::Game(id);
        // Troca de jogo nunca reaproveita o pipeline anterior.
        self.media.clear();
        self.game_media = None;
        self.selected_screenshot = 0;
        self.reset_trailer_state();
        let covers = self.cover_tasks(urls);
        let media = game
            .as_ref()
            .map(|game| self.game_media_task(game))
            .unwrap_or_else(Task::none);
        let panels = game
            .as_ref()
            .map(|game| self.game_panels_task(game))
            .unwrap_or_else(Task::none);
        Task::batch([covers, media, panels])
    }

    /// Drena a fila do `NativeShell` e aplica os eventos de lançamento.
    ///
    /// A Subscription chama isto a cada ~150 ms; não há consulta ao processo.
    fn drain_launch_events(&mut self) -> Task<Message> {
        let events = self.backend.drain_launch_events();
        if events.is_empty() {
            return Task::none();
        }

        let was_running = self.launch.status == GameRunStatus::Running;
        for event in events {
            self.launch.apply_event(event);
        }

        if was_running && self.launch.status == GameRunStatus::Idle {
            self.restore_window_task()
        } else {
            Task::none()
        }
    }

    /// Abre o diálogo e carrega `gamesettings:get`.
    fn open_launch_options(&mut self, id: String) -> Task<Message> {
        let Some(game) = self.library.game(&id) else {
            return Task::none();
        };
        let title = game.title.clone();
        let default_prefix = self.backend.native_default_prefix(&id);
        // Reabrir a mesma ficha preserva o modo já escolhido, desde que não
        // haja rascunho pendente (nesse caso o modo não seria aplicável).
        let previous_mode =
            if self.launch_options.game_id == id && !self.launch_options.dirty {
                self.launch_options.mode.clone()
            } else {
                None
            };
        self.launch_options = LaunchOptionsState {
            open: true,
            game_id: id.clone(),
            game_title: title,
            status: LoadStatus::Loading,
            settings: None,
            default_prefix,
            mode: previous_mode,
            env_name: String::new(),
            env_value: String::new(),
            saving: false,
            saved: false,
            dirty: false,
            error: None,
        };

        let backend = self.backend.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.load_game_settings(id).await },
            move |result| Message::LaunchOptionsLoaded {
                id: message_id,
                result,
            },
        )
    }

    fn add_launch_env_var(&mut self) {
        let name = self.launch_options.env_name.trim().to_string();
        if name.is_empty() {
            self.launch_options.error = Some("Informe o nome da variável.".to_string());
            return;
        }
        let Some(settings) = self.launch_options.settings.as_mut() else {
            return;
        };
        let value = self.launch_options.env_value.clone();
        settings.env_vars.retain(|variable| variable.name != name);
        settings.env_vars.push(crate::backend::EnvVarSetting { name, value });
        self.launch_options.env_name.clear();
        self.launch_options.env_value.clear();
        self.launch_options.error = None;
        self.launch_options.mark_dirty();
    }

    /// Salva o patch via `gamesettings:set`; o próximo `game:launch` lê o
    /// arquivo no `RuntimeDeps` do IPC, sem repetir as opções no payload.
    fn save_launch_options(&mut self) -> Task<Message> {
        if self.launch_options.saving {
            return Task::none();
        }
        let Some(settings) = self.launch_options.settings.as_ref() else {
            return Task::none();
        };
        if self.launch_options.mode.as_deref() == Some("exe") && settings.exe_path.trim().is_empty()
        {
            self.launch_options.error =
                Some("Informe o executável antes de salvar o modo exe.".to_string());
            return Task::none();
        }

        let id = self.launch_options.game_id.clone();
        let patch = settings.to_patch();
        self.launch_options.saving = true;
        self.launch_options.saved = false;
        // O patch cobre o estado atual; se algo for editado enquanto o save
        // estiver em voo, `mark_dirty` volta a ligar o flag.
        self.launch_options.dirty = false;
        self.launch_options.error = None;

        let backend = self.backend.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.save_game_settings(id, patch).await },
            move |result| Message::LaunchOptionsSaved {
                id: message_id,
                result,
            },
        )
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
        self.launch.error = None;
        self.launch.warnings.clear();
        self.launch.warnings_game_id = None;
        let mode = self.launch_options.mode_for(&id).map(str::to_string);

        let backend = self.backend.clone();
        Task::perform(
            async move { backend.launch_game(&game, mode.as_deref()).await },
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
        if self.big_picture.overview_open() && self.media.is_active() {
            self.media.tick();
        }
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

        let keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        } = &event
        else {
            return Task::none();
        };

        use keyboard::key::Named;

        if matches!(key, keyboard::Key::Named(Named::F5)) {
            self.library.set_loading();
            return self.reload_task();
        }

        // Tab/Shift+Tab percorrem a ordem de foco; L1/R1 (PageUp/PageDown no
        // teclado) continuam trocando de aba.
        if matches!(key, keyboard::Key::Named(Named::Tab)) {
            return self.console_focus_step(!modifiers.shift());
        }

        if self.big_picture.tab() == ConsoleTab::Stores {
            if self.big_picture.store_editing() {
                return match key {
                    keyboard::Key::Named(Named::Escape) => self.console_nav(NavEvent::Back),
                    keyboard::Key::Named(Named::ArrowDown) => self.console_nav(NavEvent::Down),
                    // Enter/Space são consumidos pelo `text_input` (on_submit
                    // e edição do valor); não duplicar a busca aqui.
                    _ => Task::none(),
                };
            }

            if self.big_picture.zone() == ConsoleZone::StoreSearch && !modifiers.control() {
                if let Some(text) = text {
                    let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !typed.is_empty() && !modifiers.alt() {
                        // O campo ainda não tinha foco: o primeiro caractere é
                        // aplicado direto e o `text_input` recebe o foco para o
                        // restante da digitação.
                        self.store.query.push_str(&typed);
                        self.big_picture.begin_store_editing();
                        let query = self.store.query.clone();
                        let suggestions = self.update(Message::StoreQueryChanged(query));
                        let focus = self.console_store_focus_task(true);
                        return Task::batch([suggestions, focus]);
                    }
                }
            }
        }

        match console_nav_from_key(&event) {
            Some(nav) => self.console_nav(nav),
            None => Task::none(),
        }
    }

    fn console_focus_step(&mut self, forward: bool) -> Task<Message> {
        let was_editing = self.big_picture.store_editing();
        self.big_picture.focus_step(forward);
        let focus = if was_editing != self.big_picture.store_editing() {
            self.console_store_focus_task(self.big_picture.store_editing())
        } else {
            Task::none()
        };
        Task::batch([focus, self.ensure_console_store()])
    }

    fn console_nav(&mut self, event: NavEvent) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let before = self.big_picture.active_index();
        let was_editing = self.big_picture.store_editing();
        let effect = self.big_picture.handle(event, &games);
        let moved = before != self.big_picture.active_index();
        let editing_changed = was_editing != self.big_picture.store_editing();
        let scroll = if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        let focus = if editing_changed {
            self.console_store_focus_task(self.big_picture.store_editing())
        } else {
            Task::none()
        };
        Task::batch([
            scroll,
            focus,
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
            self.ensure_console_store(),
        ])
    }

    fn console_select_game(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let (moved, effect) = self.big_picture.press_game(index, &games);
        let scroll = if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        Task::batch([
            scroll,
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
        ])
    }

    fn console_select_tab(&mut self, tab: ConsoleTab) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let was_editing = self.big_picture.store_editing();
        self.big_picture.select_tab(tab, &games);
        let focus = if was_editing && !self.big_picture.store_editing() {
            self.console_store_focus_task(false)
        } else {
            Task::none()
        };
        Task::batch([
            focus,
            self.console_rail_scroll(),
            self.console_sync_media(),
            self.ensure_console_news(),
            self.ensure_console_store(),
        ])
    }

    fn console_action(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let effect = self.big_picture.press_action(index, &games);
        Task::batch([
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
        ])
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
            ConsoleEffect::OpenGameDetail(id) => self.open_game_from_console(id),
            ConsoleEffect::ToggleFavorite(id) => self.toggle_console_favorite(id),
            ConsoleEffect::Trailer(id) => self.console_trailer(id),
            ConsoleEffect::Notice(text) => {
                self.big_picture.set_notice(text);
                Task::none()
            }
            ConsoleEffect::OpenStoreDetail(index) => {
                let game = self.console_store_games().get(index).copied().cloned();
                match game {
                    Some(game) if self.big_picture.open_store_detail() => {
                        self.open_store_detail(game)
                    }
                    _ => Task::none(),
                }
            }
            ConsoleEffect::CloseStoreDetail => {
                self.big_picture.close_store_detail();
                self.store.detail = None;
                Task::none()
            }
            ConsoleEffect::StoreLoadMore => self.load_store_more(),
            ConsoleEffect::StoreSearch => {
                let task = self.submit_store_search();
                let sync = self.ensure_console_store();
                Task::batch([task, sync])
            }
        }
    }

    /// Mantém o trailer do painel alinhado ao jogo selecionado. Ao fechar o
    /// painel (ou trocar de aba) o pipeline é descartado; ao abrir/trocar de
    /// jogo, um novo `game:sysinfo` é disparado sem bloquear a UI.
    fn console_sync_media(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let selected = if self.big_picture.overview_open() {
            let games = console_shelf_for(&self.library, self.big_picture.tab());
            self.big_picture
                .selected(&games)
                .map(|game| (game.id.clone(), game.title.clone()))
        } else {
            None
        };

        let Some((id, title)) = selected else {
            self.clear_console_media();
            return Task::none();
        };

        if self.console_media_id.as_deref() == Some(id.as_str()) {
            return Task::none();
        }

        self.media.clear();
        self.game_media = None;
        self.console_media_id = Some(id.clone());
        let backend = self.backend.clone();
        let request_id = id.clone();
        Task::perform(
            async move { backend.load_game_media(request_id, title).await },
            move |media| Message::ConsoleMediaLoaded { id, media },
        )
    }

    fn clear_console_media(&mut self) {
        if self.console_media_id.take().is_some() {
            self.media.clear();
            self.game_media = None;
        }
    }

    fn ensure_console_news(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture)
            || self.big_picture.tab() != ConsoleTab::News
        {
            return Task::none();
        }
        if matches!(self.console_news.status, LoadStatus::Idle) {
            self.load_console_news()
        } else {
            Task::none()
        }
    }

    fn load_console_news(&mut self) -> Task<Message> {
        if self.console_news.status == LoadStatus::Loading {
            return Task::none();
        }
        self.console_news.status = LoadStatus::Loading;
        self.console_news.error = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_news().await },
            Message::ConsoleNewsLoaded,
        )
    }

    fn toggle_console_favorite(&mut self, id: String) -> Task<Message> {
        if self.favorite_busy.is_some() {
            self.big_picture
                .set_notice("Aguarde a gravação do favorito atual.".to_string());
            return Task::none();
        }

        let Some(game) = self.library.game(&id).cloned() else {
            self.big_picture
                .set_notice("O jogo saiu da biblioteca antes de favoritar.".to_string());
            return Task::none();
        };
        let favorite = !game.favorite;
        self.favorite_busy = Some(id.clone());
        // O painel continua mostrando o estado antigo até o dispatcher
        // responder com a biblioteca atualizada.
        self.big_picture.set_notice(if favorite {
            format!("Adicionando \"{}\" aos favoritos…", game.title)
        } else {
            format!("Removendo \"{}\" dos favoritos…", game.title)
        });

        let backend = self.backend.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.set_favorite(id, favorite).await },
            move |result| Message::ConsoleFavoriteDone {
                id: message_id,
                result,
            },
        )
    }

    fn console_trailer(&mut self, id: String) -> Task<Message> {
        if self.console_media_id.as_deref() != Some(id.as_str()) {
            return self.console_sync_media();
        }

        if self.media.can_control() {
            return match self.media.toggle() {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            };
        }

        let sources = self
            .game_media
            .as_ref()
            .and_then(|media| media.movies.first())
            .map(|movie| media::TrailerSources {
                hls: movie.hls.clone(),
                mp4: movie.mp4.clone(),
                webm: movie.webm.clone(),
            });
        if let Some(sources) = sources {
            return match self.media.open(&sources) {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            };
        }

        if self.game_media.is_some() {
            let notice = self
                .game_media
                .as_ref()
                .and_then(|media| media.notice.clone())
                .unwrap_or_else(|| "Este jogo não tem trailer disponível.".to_string());
            self.big_picture.set_notice(notice);
            Task::none()
        } else {
            self.big_picture
                .set_notice("O trailer ainda está carregando…".to_string());
            Task::none()
        }
    }

    /// Abre a ficha desktop saindo do console sem restaurar o trailer da tela
    /// anterior (o `open_game` já dispara a mídia do jogo escolhido).
    fn open_game_from_console(&mut self, id: String) -> Task<Message> {
        let mut tasks = Vec::new();
        if matches!(self.screen, Screen::BigPicture) {
            self.previous_screen = None;
            self.big_picture.leave();
            self.media.clear();
            self.game_media = None;
            self.console_media_id = None;
            if let Some(window) = self.window {
                tasks.push(window::set_mode(window, window::Mode::Windowed));
            }
        }
        tasks.push(self.open_game(id));
        Task::batch(tasks)
    }

    fn focus_console_game(&mut self, id: &str) {
        let games = console_shelf_for(&self.library, self.big_picture.tab());
        if let Some(index) = games.iter().position(|game| game.id == id) {
            self.big_picture.focus_rail(index);
        }
    }

    fn console_rail_scroll(&self) -> Task<Message> {
        views::big_picture::rail_scroll_task(
            self.big_picture.active_index(),
            self.window_size.width,
        )
    }

    /// Timer de ~60 fps que drena os frames do GStreamer e o gamepad do
    /// console, mais o tick de ~150 ms que drena os eventos de lançamento.
    ///
    /// O canal do `NativeShell` é a fonte de verdade da sessão de jogo: não há
    /// mais consulta periódica ao `LaunchRuntime`. O tick de eventos existe em
    /// qualquer tela (inclusive no Big Picture) e não acorda nada além de uma
    /// fila em memória quando está vazia.
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

        subscriptions.push(
            iced::time::every(Duration::from_millis(150)).map(|_| Message::LaunchEventsTick),
        );

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
                    self.game_panels.get(id),
                    self.selected_screenshot,
                    &self.launch,
                    &self.trailer_picker,
                    &self.launch_options,
                ),
                None => views::missing_game(&self.i18n),
            },
            Screen::Section(Section::Downloads) => views::downloads::view(self),
            Screen::Section(Section::Settings) => views::settings::view(self),
            Screen::Section(Section::Sources) => views::sources::view(self),
            Screen::Section(Section::Friends) => views::friends::view(self),
            Screen::Section(Section::Components) => views::components::view(self),
            Screen::Section(Section::Home) => views::home::view(self),
            Screen::Section(Section::Stores) => views::store::view(self),
            Screen::Section(section) => views::section_placeholder(*section, &self.i18n),
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
            Section::Stores => match self.store.tab {
                StoreTab::Steam => {
                    if force && self.store.search.is_some() {
                        self.submit_store_search()
                    } else {
                        let should_load =
                            force || matches!(self.store.browse.status, LoadStatus::Idle);
                        if should_load {
                            self.load_store_browse(force)
                        } else {
                            Task::none()
                        }
                    }
                }
                StoreTab::Retro => {
                    let should_load = force || matches!(self.retro.status, LoadStatus::Idle);
                    if should_load {
                        self.load_retro_page(0)
                    } else {
                        Task::none()
                    }
                }
            },
            _ => Task::none(),
        }
    }

    /// Dispara a busca com o texto atual; chamado pelo Enter, pelo botão e
    /// pelo clique numa sugestão.
    fn submit_store_search(&mut self) -> Task<Message> {
        let query = self.store.query.trim().to_string();
        if query.is_empty() {
            self.store.search = None;
            self.store.suggestions.clear();
            self.sync_console_store();
            return Task::none();
        }
        self.store.suggestions.clear();
        self.store.search = Some(StoreSearchState {
            query: query.clone(),
            status: LoadStatus::Loading,
            result: None,
            error: None,
        });
        self.sync_console_store();
        let backend = self.backend.clone();
        let message_query = query.clone();
        Task::perform(
            async move { backend.search_store(query).await },
            move |result| Message::StoreSearchDone {
                query: message_query,
                result,
            },
        )
    }

    fn load_store_browse(&mut self, force: bool) -> Task<Message> {
        if !force && self.store.browse.status == LoadStatus::Loading {
            return Task::none();
        }
        self.store.browse.status = LoadStatus::Loading;
        self.store.browse.error = None;
        self.store.browse.loading_more = false;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_store_recent(0, STORE_PAGE_SIZE).await },
            |result| Message::StoreBrowseLoaded {
                append: false,
                result,
            },
        )
    }

    fn load_store_more(&mut self) -> Task<Message> {
        let browse = &self.store.browse;
        if browse.loading_more || !browse.has_more {
            return Task::none();
        }
        let offset = browse.next_offset.unwrap_or(browse.games.len() as u64);
        self.store.browse.loading_more = true;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_store_recent(offset, STORE_PAGE_SIZE).await },
            |result| Message::StoreBrowseLoaded {
                append: true,
                result,
            },
        )
    }

    fn open_store_detail(&mut self, game: StoreGame) -> Task<Message> {
        let urls = game.image_urls();
        let info_task = self.store_info_task(&game);
        let install_task = self.store_install_info_task(&game);
        let libraries_task = self.store_libraries_task();
        self.retro.detail = None;
        self.store.detail = Some(StoreDetailState {
            game,
            info: None,
            status: LoadStatus::Loading,
            error: None,
            install: None,
            install_status: LoadStatus::Loading,
            install_error: None,
            libraries: Vec::new(),
            libraries_status: LoadStatus::Loading,
            libraries_error: None,
            selected_library: 0,
            selected_depots: BTreeSet::new(),
            install_busy: false,
            install_message: None,
        });
        Task::batch([
            self.cover_tasks(urls),
            info_task,
            install_task,
            libraries_task,
        ])
    }

    /// Itens que a Loja do console desenha: resultado da busca quando existe,
    /// senão o catálogo `store:recent` já acumulado.
    fn console_store_games(&self) -> Vec<&StoreGame> {
        if let Some(search) = self.store.search.as_ref() {
            search
                .result
                .as_ref()
                .map(|result| result.games.iter().collect())
                .unwrap_or_default()
        } else {
            self.store.browse.games.iter().collect()
        }
    }

    /// Mantém a grade/foco do console alinhados ao estado real da loja.
    fn sync_console_store(&mut self) {
        let count = self.console_store_games().len();
        let has_more = self.store.search.is_none() && self.store.browse.has_more;
        self.big_picture.sync_store(count, has_more);
    }

    /// Garante o catálogo Steam quando a aba Loja do console está ativa. A
    /// aba Retro do desktop não é portada para o Big Picture nesta onda.
    fn ensure_console_store(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture)
            || self.big_picture.tab() != ConsoleTab::Stores
        {
            return Task::none();
        }

        if self.store.tab != StoreTab::Steam {
            self.store.tab = StoreTab::Steam;
        }
        self.sync_console_store();

        if self.store.search.is_none() && matches!(self.store.browse.status, LoadStatus::Idle) {
            self.load_store_browse(false)
        } else {
            Task::none()
        }
    }

    /// Foco programático no campo de busca; `false` devolve o foco ao painel.
    fn console_store_focus_task(&self, focused: bool) -> Task<Message> {
        let id = iced::widget::Id::new(views::big_picture::STORE_SEARCH_ID);
        if focused {
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::focus(id),
            )
        } else {
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus(),
            )
        }
    }

    fn load_retro_page(&mut self, offset: u64) -> Task<Message> {
        self.retro.status = LoadStatus::Loading;
        self.retro.error = None;
        self.retro.generation = self.retro.generation.wrapping_add(1);
        let generation = self.retro.generation;
        let query = self.retro.applied_query.clone();
        let system = (!self.retro.system.id.is_empty()).then(|| self.retro.system.id.to_string());
        let backend = self.backend.clone();
        Task::perform(
            async move {
                backend
                    .load_retro_page(query, system, offset, RETRO_PAGE_SIZE)
                    .await
            },
            move |result| Message::RetroPageLoaded {
                generation,
                offset,
                result,
            },
        )
    }

    fn submit_retro_search(&mut self) -> Task<Message> {
        self.retro.applied_query = self.retro.query.trim().to_string();
        self.load_retro_page(0)
    }

    fn store_info_task(&self, game: &StoreGame) -> Task<Message> {
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        let message_appid = appid.clone();
        let title = game.title.clone();
        Task::perform(
            async move { backend.load_store_info(appid, title).await },
            move |result| Message::StoreDetailLoaded {
                appid: message_appid,
                result,
            },
        )
    }

    fn store_install_info_task(&self, game: &StoreGame) -> Task<Message> {
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        let message_appid = appid.clone();
        Task::perform(
            async move { backend.load_store_install_info(appid).await },
            move |result| Message::StoreInstallInfoLoaded {
                appid: message_appid,
                result,
            },
        )
    }

    fn store_libraries_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_store_libraries().await },
            Message::StoreLibrariesLoaded,
        )
    }

    fn retro_game_task(&self, game: &RetroGame) -> Task<Message> {
        let backend = self.backend.clone();
        let id = game.id.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.load_retro_game(id).await },
            move |result| Message::RetroDetailLoaded {
                id: message_id,
                result,
            },
        )
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
        let message_id = game.id.clone();
        Task::perform(
            async move { backend.load_game_media(id, title).await },
            move |media| Message::MediaLoaded {
                game_id: message_id,
                media,
            },
        )
    }

    /// Dispara, em paralelo, os painéis da ficha para o jogo. O mapa
    /// `game_panels` é o cache: se o jogo já foi aberto nesta sessão, nada é
    /// refeito. Jogos fora da Steam/Retro entram como `Ready` com uma nota
    /// explícita, não como painel vazio.
    fn game_panels_task(&mut self, game: &Game) -> Task<Message> {
        if self.game_panels.contains_key(&game.id) {
            return Task::none();
        }

        let mut panels = GamePanels::default();
        let mut tasks = Vec::new();

        if let Some(appid) = game.steam_appid().map(str::to_string) {
            panels.achievements.status = LoadStatus::Loading;
            panels.proton.status = LoadStatus::Loading;
            panels.stats.status = LoadStatus::Loading;

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let achievements_appid = appid.clone();
            tasks.push(Task::perform(
                async move {
                    backend
                        .load_steam_achievements(achievements_appid)
                        .await
                },
                move |result| Message::AchievementsLoaded { game_id, result },
            ));

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let proton_appid = appid.clone();
            tasks.push(Task::perform(
                async move { backend.load_protondb(proton_appid).await },
                move |result| Message::ProtonDbLoaded { game_id, result },
            ));

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            tasks.push(Task::perform(
                async move { backend.load_game_stats(appid).await },
                move |result| Message::GameStatsLoaded { game_id, result },
            ));
        } else if let (true, Some(system)) = (game.is_retro(), game.retro_system()) {
            panels.achievements.status = LoadStatus::Loading;
            panels.proton.status = LoadStatus::Ready;
            panels.proton.note =
                Some("ProtonDB cobre jogos da Steam; não se aplica a este título retro.".to_string());
            panels.stats.status = LoadStatus::Ready;
            panels.stats.note =
                Some("Estatísticas da loja Steam não se aplicam a jogos retro.".to_string());

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let title = game.title.clone();
            let system = system.to_string();
            tasks.push(Task::perform(
                async move {
                    backend
                        .load_retro_achievements(title, system)
                        .await
                },
                move |result| Message::AchievementsLoaded { game_id, result },
            ));
        } else {
            let launcher = if game.launcher.trim().is_empty() {
                "desconhecido"
            } else {
                game.launcher.as_str()
            };
            panels.achievements.status = LoadStatus::Ready;
            panels.achievements.note = Some(format!(
                "Conquistas não estão disponíveis para o launcher \"{launcher}\" nesta build."
            ));
            panels.proton.status = LoadStatus::Ready;
            panels.proton.note =
                Some("ProtonDB só tem relatórios para jogos com appid da Steam.".to_string());
            panels.stats.status = LoadStatus::Ready;
            panels.stats.note =
                Some("Estatísticas e reviews da loja Steam não se aplicam a este jogo.".to_string());
        }

        self.game_panels.insert(game.id.clone(), panels);
        Task::batch(tasks)
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
fn console_shelf_for<'a>(library: &'a Library, tab: ConsoleTab) -> Vec<&'a Game> {
    let games = library.visible_all();
    if tab == ConsoleTab::Home {
        home_shelf(&games).0
    } else {
        games
    }
}

fn console_nav_from_key(event: &keyboard::Event) -> Option<NavEvent> {
    let keyboard::Event::KeyPressed { key, .. } = event else {
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
        keyboard::Key::Named(Named::PageUp) => Some(NavEvent::PageLeft),
        keyboard::Key::Named(Named::PageDown) => Some(NavEvent::PageRight),
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
    fn section_e_abas_de_configuracoes_seguem_o_idioma() {
        let pt = I18n::new(Lang::PtBr);
        let en = I18n::new(Lang::EnUs);
        let es = I18n::new(Lang::EsEs);

        assert_eq!(Section::Library.label(&pt), "Biblioteca");
        assert_eq!(Section::Library.label(&en), "Library");
        assert_eq!(Section::Settings.label(&en), "Settings");
        assert_eq!(Section::Home.label(&es), "Inicio");
        assert_eq!(SettingsTab::General.label(&en), "General Settings");
        assert_eq!(SettingsTab::Appearance.label(&es), "Apariencia");
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
    fn launch_events_drive_the_status_without_polling() {
        let mut launch = GameLaunchState::default();

        let starting = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "starting", "gameId": "steam:10", "token": 1 }),
        )
        .expect("starting");
        launch.apply_event(starting);
        assert!(launch.is_launching("steam:10"));
        assert!(!launch.is_running("steam:10"));

        let warning = crate::backend::parse_launch_event(
            "game:launchWarning",
            &json!({ "gameId": "steam:10", "warnings": ["gamemoderun ausente"] }),
        )
        .expect("warning");
        launch.apply_event(warning);
        assert_eq!(launch.warnings_for("steam:10"), ["gamemoderun ausente"]);
        assert!(launch.warnings_for("steam:11").is_empty());

        let running = crate::backend::parse_launch_event("game:running", &json!(true))
            .expect("running");
        launch.apply_event(running);
        assert!(launch.is_running("steam:10"));

        // `game:running(false)` é a transição natural de fim do monitor.
        let stopped = crate::backend::parse_launch_event("game:running", &json!(false))
            .expect("stopped");
        launch.apply_event(stopped);
        assert_eq!(launch.status, GameRunStatus::Idle);
        assert!(launch.game_id.is_none());

        // Os avisos continuam disponíveis para o jogo depois do fim.
        assert_eq!(launch.warnings_for("steam:10"), ["gamemoderun ausente"]);
    }

    #[test]
    fn active_and_idle_events_carry_the_game_id() {
        let mut launch = GameLaunchState::default();
        let active = crate::backend::parse_launch_event(
            "game:active",
            &json!({ "rodando": true, "gameId": "steam:10" }),
        )
        .expect("active");
        launch.apply_event(active);
        assert!(launch.is_running("steam:10"));

        let idle = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "idle", "gameId": "steam:10", "token": 1 }),
        )
        .expect("idle");
        launch.apply_event(idle);
        assert_eq!(launch.status, GameRunStatus::Idle);
        assert!(launch.game_id.is_none());
    }

    #[test]
    fn stale_events_from_a_previous_session_do_not_cancel_a_new_launch() {
        let mut launch = GameLaunchState::default();

        let starting = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "starting", "gameId": "steam:11" }),
        )
        .expect("starting novo");
        launch.apply_event(starting);
        assert!(launch.is_launching("steam:11"));

        // Monitor do jogo anterior termina depois do novo `starting`.
        let stale_idle = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "idle", "gameId": "steam:10" }),
        )
        .expect("idle antigo");
        launch.apply_event(stale_idle);
        assert!(launch.is_launching("steam:11"));

        let stale_running_off =
            crate::backend::parse_launch_event("game:running", &json!(false))
                .expect("running false antigo");
        launch.apply_event(stale_running_off);
        assert!(launch.is_launching("steam:11"));
    }

    #[test]
    fn launch_error_event_returns_to_idle_with_a_message() {
        let mut launch = GameLaunchState {
            status: GameRunStatus::Launching,
            game_id: Some("steam:10".to_string()),
            ..GameLaunchState::default()
        };
        let error = crate::backend::parse_launch_event(
            "game:launchError",
            &json!({ "gameId": "steam:10", "error": "No such file or directory" }),
        )
        .expect("erro");
        launch.apply_event(error);
        assert_eq!(launch.status, GameRunStatus::Idle);
        let message = launch.error.as_deref().expect("mensagem");
        assert!(message.contains("No such file"), "{message}");
    }

    #[test]
    fn launch_options_mode_is_scoped_to_the_open_game_and_requires_saved_edits() {
        let state = LaunchOptionsState {
            game_id: "steam:10".to_string(),
            mode: Some("exe".to_string()),
            ..LaunchOptionsState::default()
        };
        assert_eq!(state.mode_for("steam:10"), Some("exe"));
        assert_eq!(state.mode_for("steam:11"), None);

        // Rascunho não salvo não vai no `game:launch`: o backend lê o disco.
        let dirty = LaunchOptionsState {
            dirty: true,
            ..state
        };
        assert_eq!(dirty.mode_for("steam:10"), None);
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
