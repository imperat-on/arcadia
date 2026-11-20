//! Estado da aplicação e arquitetura Elm do iced.
//!
//! `App` guarda somente dados de UI. Trabalho de rede/IPC vira `Task`, e as
//! mensagens voltam para `update` — nenhuma chamada bloqueante roda na thread
//! que desenha.

use std::collections::BTreeSet;

use iced::widget::image::Handle;
use iced::{window, Element, Fill, Task};
use serde_json::{json, Value};

use crate::backend::{
    Backend, DownloadAction, DownloadItem, DownloadsData, FriendsLoad, Game, PluginItem,
    SettingsConfig, SourceItem,
};
use crate::covers::CoverCache;
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
            Section::BigPicture => "O modo Big Picture será portado junto com o input de gamepad.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Library,
    Game(String),
    Section(Section),
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
    pub downloads: DownloadsState,
    pub settings: SettingsState,
    pub sources: SourcesState,
    pub friends: FriendsState,
    pub components: ComponentsState,
    pub sidebar_search: String,
    pub sidebar_games_open: bool,
    pub window: Option<window::Id>,
}

impl App {
    pub fn boot(backend: Backend) -> (Self, Task<Message>) {
        let app = Self {
            backend,
            screen: Screen::Library,
            library: Library::loading(),
            covers: CoverCache::default(),
            search: String::new(),
            downloads: DownloadsState::default(),
            settings: SettingsState::default(),
            sources: SourcesState::default(),
            friends: FriendsState::default(),
            components: ComponentsState::default(),
            sidebar_search: String::new(),
            sidebar_games_open: true,
            window: None,
        };
        let task = Task::batch([
            app.reload_task(),
            window::latest().map(Message::WindowReady),
        ]);
        (app, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LibraryLoaded(Ok(games)) => {
                eprintln!("[arcadia-native] biblioteca: {} jogo(s)", games.len());
                self.library.ready(games);
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
                self.screen = Screen::for_section(section);
                self.load_section(section, false)
            }
            Message::OpenGame(id) => {
                let urls = self
                    .library
                    .game(&id)
                    .map(Game::image_urls)
                    .unwrap_or_default();
                self.screen = Screen::Game(id);
                self.cover_tasks(urls)
            }
            Message::BackToLibrary => {
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
                Task::none()
            }
            Message::WindowDrag => self.window_task(window::drag),
            Message::WindowMinimize => self.window_task(|id| window::minimize(id, true)),
            Message::WindowToggleMaximize => self.window_task(window::toggle_maximize),
            Message::WindowClose => self.window_task(window::close),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = views::sidebar::view(self);
        let content: Element<'_, Message> = match &self.screen {
            Screen::Library => views::library::view(self),
            Screen::Game(id) => match self.library.game(id) {
                Some(game) => views::game::view(game, &self.covers),
                None => views::missing_game(),
            },
            Screen::Section(Section::Downloads) => views::downloads::view(self),
            Screen::Section(Section::Settings) => views::settings::view(self),
            Screen::Section(Section::Sources) => views::sources::view(self),
            Screen::Section(Section::Friends) => views::friends::view(self),
            Screen::Section(Section::Components) => views::components::view(self),
            Screen::Section(Section::Home) => views::home::view(self),
            Screen::Section(section) => views::section_placeholder(*section),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, title: &str, favorite: bool, hidden: bool) -> Game {
        Game {
            id: id.to_string(),
            title: title.to_string(),
            launcher: "custom".to_string(),
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
}
