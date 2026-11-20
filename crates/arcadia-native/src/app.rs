//! Estado da aplicação e arquitetura Elm do iced.
//!
//! `App` guarda somente dados de UI. Trabalho de rede/IPC vira `Task`, e as
//! mensagens voltam para `update` — nenhuma chamada bloqueante roda na thread
//! que desenha.

use std::collections::BTreeSet;
use std::time::Duration;

use iced::widget::image::Handle;
use iced::{Element, Fill, Subscription, Task};

use crate::backend::{Backend, Game, GameMedia};
use crate::covers::CoverCache;
use crate::media::{self, MediaPlayer};
use crate::theme;
use crate::views;

#[derive(Debug, Clone)]
pub enum Message {
    LibraryLoaded(Result<Vec<Game>, String>),
    CoverLoaded { url: String, handle: Option<Handle> },
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
}

/// Itens fixos da sidebar. Apenas Biblioteca tem conteúdo nesta primeira fatia;
/// as demais abrem um placeholder explícito (nunca uma tela vazia sem contexto).
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
            Section::Home => "A Home nativa entra na próxima onda. A Biblioteca já está funcional.",
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

pub struct App {
    pub backend: Backend,
    pub screen: Screen,
    pub library: Library,
    pub covers: CoverCache,
    pub search: String,
    pub media: MediaPlayer,
    /// `None` enquanto o `game:sysinfo` está em voo.
    pub game_media: Option<GameMedia>,
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
        };
        let task = app.reload_task();
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
                self.media.clear();
                self.game_media = None;
                self.screen = Screen::for_section(section);
                Task::none()
            }
            Message::OpenGame(id) => {
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
            Message::BackToLibrary => {
                self.media.clear();
                self.game_media = None;
                self.screen.back_to_library();
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
        }
    }

    /// Timer de ~60 fps que drena os frames do GStreamer.
    ///
    /// Só existe na página do jogo e depois que o pipeline foi criado; fora
    /// isso o app não acorda à toa.
    pub fn subscription(&self) -> Subscription<Message> {
        if matches!(self.screen, Screen::Game(_)) && self.media.is_active() {
            iced::time::every(Duration::from_millis(16)).map(|_| Message::MediaTick)
        } else {
            Subscription::none()
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = views::sidebar::view(self);
        let content: Element<'_, Message> = match &self.screen {
            Screen::Library => views::library::view(self),
            Screen::Game(id) => match self.library.game(id) {
                Some(game) => views::game::view(
                    game,
                    &self.covers,
                    &self.media,
                    self.game_media.as_ref(),
                ),
                None => views::missing_game(),
            },
            Screen::Section(section) => views::section_placeholder(*section),
        };

        iced::widget::row![
            sidebar,
            iced::widget::container(content)
                .width(Fill)
                .height(Fill)
                .style(theme::app_background)
        ]
        .width(Fill)
        .height(Fill)
        .into()
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
