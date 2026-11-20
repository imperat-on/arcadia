//! Estado do modo console (Big Picture).
//!
//! O renderer iced não vê botões físicos: [`BigPicture::handle`] consome
//! [`NavEvent`] do crate `arcadia-native-input` e devolve efeitos de alto nível
//! (sair, abrir a ficha do jogo, avisar stub). O foco usa [`FocusGrid`] — o
//! trilho é uma grade 1×N, as abas e as ações do painel também —, então limites
//! e bordas são exatamente os testados no crate de input.

use std::time::Duration;

use arcadia_native_input::{EdgeBehavior, FocusGrid, NavEvent, NavInput};

use crate::backend::Game;

/// Quantidade de ações do painel do jogo.
pub const ACTION_COUNT: usize = 4;
/// Jogar (ou parar, quando o jogo está em execução).
pub const ACTION_PLAY: usize = 0;
/// Trailer: reproduz/pausa o pipeline aberto ao abrir o painel.
pub const ACTION_TRAILER: usize = 1;
/// Favorito: grava `favorite` via `overrides:set`.
pub const ACTION_FAVORITE: usize = 2;
/// "•••": menu de contexto do jogo.
pub const ACTION_MORE: usize = 3;

/// Itens do menu "•••". A ordem é a mesma desenhada pela view.
pub const MENU_COUNT: usize = 3;
pub const MENU_OPEN_DETAIL: usize = 0;
pub const MENU_FAVORITE: usize = 1;
pub const MENU_TRAILER: usize = 2;

/// Abas da top bar do console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleTab {
    Home,
    Library,
    Stores,
    News,
}

impl ConsoleTab {
    pub const ALL: [ConsoleTab; 4] = [
        ConsoleTab::Home,
        ConsoleTab::Library,
        ConsoleTab::Stores,
        ConsoleTab::News,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConsoleTab::Home => "Início",
            ConsoleTab::Library => "Biblioteca",
            ConsoleTab::Stores => "Loja",
            ConsoleTab::News => "Notícias",
        }
    }

    /// Só a Biblioteca tem conteúdo console nesta onda; as outras abas
    /// mostram um estado explícito em vez de uma tela vazia sem explicação.
    pub const fn is_library(self) -> bool {
        matches!(self, ConsoleTab::Library)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

/// Onde o foco está agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleZone {
    Tabs,
    Rail,
    Overview,
}

/// Efeito de alto nível pedido pela navegação. O `App` decide como executar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleEffect {
    None,
    /// Sai do modo console e volta ao desktop (janela em `Windowed`).
    Exit,
    /// Inicia o jogo selecionado de verdade via `game:launch`.
    LaunchGame(String),
    /// Abre a ficha desktop do jogo (mesmo caminho de `OpenGame`).
    OpenGameDetail(String),
    /// Alterna o favorito via `overrides:set`.
    ToggleFavorite(String),
    /// Abre/alterna o trailer do jogo no player GStreamer.
    Trailer(String),
    /// Aviso curto no rodapé do painel.
    Notice(String),
}

/// Estado do modo console: foco, aba ativa, painel e o leitor de gamepad.
pub struct BigPicture {
    input: Option<NavInput>,
    rail: FocusGrid,
    tabs: FocusGrid,
    actions: FocusGrid,
    zone: ConsoleZone,
    tab: ConsoleTab,
    overview: bool,
    /// Item focado no menu "•••"; `None` quando o menu está fechado.
    menu: Option<usize>,
    /// Seleção da lista de notícias e o tamanho atual do feed.
    news_index: usize,
    news_count: usize,
    clock: String,
    notice: Option<String>,
}

impl BigPicture {
    pub fn new() -> Self {
        let mut tabs = FocusGrid::new(ConsoleTab::ALL.len(), 1);
        tabs.set_index(ConsoleTab::Library.index());

        Self {
            input: None,
            rail: FocusGrid::new(0, 1),
            tabs,
            actions: FocusGrid::new(ACTION_COUNT, 1),
            zone: ConsoleZone::Rail,
            tab: ConsoleTab::Library,
            overview: false,
            menu: None,
            news_index: 0,
            news_count: 0,
            clock: String::new(),
            notice: None,
        }
    }

    /// Prepara o modo ao entrar: aba Biblioteca, painel fechado e foco no
    /// primeiro jogo. O `gilrs` só é criado aqui, não no boot do desktop.
    pub fn enter(&mut self, game_count: usize) {
        match &mut self.input {
            Some(input) => input.reset(),
            None => self.input = Some(NavInput::new()),
        }

        self.tab = ConsoleTab::Library;
        self.overview = false;
        self.menu = None;
        self.zone = ConsoleZone::Rail;
        self.notice = None;
        self.tabs.set_index(ConsoleTab::Library.index());
        self.actions.set_index(ACTION_PLAY);
        // Foco inicial no primeiro jogo, como pede o modo console.
        self.rail = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.refresh_clock();
    }

    /// Solta direções seguradas para o próximo acesso não herdar um repeat.
    pub fn leave(&mut self) {
        if let Some(input) = &mut self.input {
            input.reset();
        }
        self.overview = false;
        self.menu = None;
        self.zone = ConsoleZone::Rail;
        self.notice = None;
    }

    /// Reconstrói o trilho preservando a posição dentro dos novos limites.
    pub fn sync_games(&mut self, game_count: usize) {
        if self.rail.cols() == game_count {
            return;
        }

        let index = self.rail.index().min(game_count.saturating_sub(1));
        self.rail = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.rail.set_index(index);
        if game_count == 0 {
            self.overview = false;
            if self.zone == ConsoleZone::Rail {
                self.zone = ConsoleZone::Tabs;
            }
        }
    }

    /// Drena o gamepad com o delta do quadro.
    pub fn tick(&mut self, dt: Duration) -> Vec<NavEvent> {
        self.input
            .as_mut()
            .map(|input| input.tick(dt))
            .unwrap_or_default()
    }

    pub fn reset_input(&mut self) {
        if let Some(input) = &mut self.input {
            input.reset();
        }
    }

    /// Atualiza o relógio da top bar só quando o minuto muda, para o modo
    /// console não redesenhar a cada tick de 16 ms.
    pub fn refresh_clock(&mut self) {
        let clock = chrono::Local::now().format("%H:%M").to_string();
        if clock != self.clock {
            self.clock = clock;
        }
    }

    pub fn clock(&self) -> &str {
        &self.clock
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn zone(&self) -> ConsoleZone {
        self.zone
    }

    pub fn tab(&self) -> ConsoleTab {
        self.tab
    }

    pub fn rail_index(&self) -> usize {
        self.rail.index()
    }

    pub fn tab_index(&self) -> usize {
        self.tabs.index()
    }

    pub fn action_index(&self) -> usize {
        self.actions.index()
    }

    pub fn overview_open(&self) -> bool {
        self.overview
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn menu_index(&self) -> usize {
        self.menu.unwrap_or(0)
    }

    pub fn news_index(&self) -> usize {
        self.news_index
    }

    /// Atualiza o tamanho do feed e mantém a seleção dentro dos limites.
    pub fn sync_news(&mut self, count: usize) {
        self.news_count = count;
        if count == 0 {
            self.news_index = 0;
        } else {
            self.news_index = self.news_index.min(count - 1);
        }
    }

    /// Seleção vinda do clique numa linha da aba Notícias.
    pub fn select_news(&mut self, index: usize) {
        if index < self.news_count {
            self.news_index = index;
        }
    }

    /// Move o foco do trilho sem abrir o painel (usado ao refiltrar favoritos).
    pub fn focus_rail(&mut self, index: usize) {
        if self.rail.cols() == 0 {
            return;
        }
        self.rail.set_index(index.min(self.rail.cols() - 1));
    }

    pub fn selected<'g>(&self, games: &[&'g Game]) -> Option<&'g Game> {
        games.get(self.rail.index()).copied()
    }

    /// Consome um evento de navegação e devolve o efeito a executar.
    ///
    /// `Back` fecha primeiro o menu "•••", depois o painel; sem nenhum dos
    /// dois, sai do modo (mesmo caminho de Esc no teclado). `Quit`/Guide sai
    /// sempre.
    pub fn handle(&mut self, event: NavEvent, games: &[&Game]) -> ConsoleEffect {
        self.notice = None;

        if self.menu.is_some() {
            return self.handle_menu(event, games);
        }

        if self.tab == ConsoleTab::News {
            match event {
                NavEvent::Up => {
                    self.news_index = self.news_index.saturating_sub(1);
                    return ConsoleEffect::None;
                }
                NavEvent::Down => {
                    if self.news_count > 0 {
                        self.news_index = (self.news_index + 1).min(self.news_count - 1);
                    }
                    return ConsoleEffect::None;
                }
                _ => {}
            }
        }

        match event {
            NavEvent::Quit => ConsoleEffect::Exit,
            NavEvent::PageLeft => {
                self.step_tab(-1, games);
                ConsoleEffect::None
            }
            NavEvent::PageRight => {
                self.step_tab(1, games);
                ConsoleEffect::None
            }
            NavEvent::Up => {
                self.move_up();
                ConsoleEffect::None
            }
            NavEvent::Down => {
                self.move_down(games);
                ConsoleEffect::None
            }
            NavEvent::Left => {
                self.move_horizontal(-1);
                ConsoleEffect::None
            }
            NavEvent::Right => {
                self.move_horizontal(1);
                ConsoleEffect::None
            }
            NavEvent::Confirm => self.confirm(games),
            NavEvent::Back => {
                if self.overview {
                    self.close_overview();
                    ConsoleEffect::None
                } else {
                    ConsoleEffect::Exit
                }
            }
            NavEvent::Menu => {
                if !self.overview {
                    self.open_overview(games);
                }
                ConsoleEffect::None
            }
            NavEvent::Options => self.favorite_effect(games),
        }
    }

    /// Clique do mouse num card do trilho.
    ///
    /// O primeiro clique foca; clicar de novo no card focado abre o painel —
    /// o mesmo gesto do Confirm no gamepad. Devolve `true` quando o foco mudou
    /// (aí o App rola o trilho).
    pub fn press_game(&mut self, index: usize, games: &[&Game]) -> (bool, ConsoleEffect) {
        if index >= games.len() {
            return (false, ConsoleEffect::None);
        }
        if self.overview {
            self.close_overview();
        }
        self.menu = None;
        // O trilho pertence à Biblioteca: clicar num card em outra aba traz o
        // usuário de volta para a aba certa em vez de deixar o foco escondido.
        if self.tab != ConsoleTab::Library {
            self.tab = ConsoleTab::Library;
            self.tabs.set_index(ConsoleTab::Library.index());
        }

        if self.rail.set_index(index) {
            self.zone = ConsoleZone::Rail;
            self.notice = None;
            (true, ConsoleEffect::None)
        } else {
            self.zone = ConsoleZone::Rail;
            self.open_overview(games);
            (false, ConsoleEffect::None)
        }
    }

    /// Clique do mouse numa aba da top bar.
    pub fn select_tab(&mut self, tab: ConsoleTab, games: &[&Game]) {
        self.overview = false;
        self.menu = None;
        self.tabs.set_index(tab.index());
        self.tab = tab;
        self.notice = None;
        self.zone = if tab.is_library() && !games.is_empty() {
            ConsoleZone::Rail
        } else {
            ConsoleZone::Tabs
        };
    }

    /// Clique/ativação de um botão do painel do jogo.
    pub fn press_action(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        if games.is_empty() {
            return ConsoleEffect::None;
        }
        self.actions.set_index(index);
        self.zone = ConsoleZone::Overview;
        self.overview = true;
        self.menu = None;
        self.notice = None;
        self.activate_action(index, games)
    }

    /// Clique/ativação de um item do menu "•••".
    pub fn press_menu(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        if index >= MENU_COUNT {
            return ConsoleEffect::None;
        }
        self.menu = None;
        self.menu_effect(index, games)
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    pub fn close_overview(&mut self) {
        self.overview = false;
        self.menu = None;
        self.zone = ConsoleZone::Rail;
    }

    fn step_tab(&mut self, delta: isize, games: &[&Game]) {
        let count = ConsoleTab::ALL.len() as isize;
        let index = (self.tabs.index() as isize + delta).rem_euclid(count) as usize;
        self.overview = false;
        self.menu = None;
        self.tabs.set_index(index);
        self.tab = ConsoleTab::ALL[index];
        self.notice = None;
        self.zone = if self.tab.is_library() && !games.is_empty() {
            ConsoleZone::Rail
        } else {
            ConsoleZone::Tabs
        };
    }

    fn move_up(&mut self) {
        match self.zone {
            ConsoleZone::Rail => self.zone = ConsoleZone::Tabs,
            ConsoleZone::Overview => self.close_overview(),
            ConsoleZone::Tabs => {}
        }
    }

    fn move_down(&mut self, games: &[&Game]) {
        match self.zone {
            ConsoleZone::Tabs => {
                if self.tab.is_library() && !games.is_empty() {
                    self.zone = ConsoleZone::Rail;
                }
            }
            ConsoleZone::Rail => self.open_overview(games),
            ConsoleZone::Overview => {}
        }
    }

    fn move_horizontal(&mut self, delta: i32) {
        match self.zone {
            ConsoleZone::Tabs => {
                // Nas abas, o direcional já troca a aba (o rodapé confirma).
                let count = ConsoleTab::ALL.len() as isize;
                let index =
                    (self.tabs.index() as isize + delta as isize).rem_euclid(count) as usize;
                self.tabs.set_index(index);
                self.tab = ConsoleTab::ALL[index];
                self.notice = None;
            }
            ConsoleZone::Rail => {
                if delta < 0 {
                    self.rail.move_left();
                } else {
                    self.rail.move_right();
                }
            }
            ConsoleZone::Overview => {
                if delta < 0 {
                    self.actions.move_left();
                } else {
                    self.actions.move_right();
                }
            }
        }
    }

    fn confirm(&mut self, games: &[&Game]) -> ConsoleEffect {
        match self.zone {
            ConsoleZone::Tabs => {
                self.zone = if self.tab.is_library() && !games.is_empty() {
                    ConsoleZone::Rail
                } else {
                    ConsoleZone::Tabs
                };
                ConsoleEffect::None
            }
            ConsoleZone::Rail => {
                self.open_overview(games);
                ConsoleEffect::None
            }
            ConsoleZone::Overview => self.activate_action(self.actions.index(), games),
        }
    }

    fn open_overview(&mut self, games: &[&Game]) {
        if games.is_empty() {
            return;
        }
        self.overview = true;
        self.menu = None;
        self.zone = ConsoleZone::Overview;
        self.actions.set_index(ACTION_PLAY);
        self.notice = None;
    }

    fn activate_action(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        let Some(game) = self.selected(games) else {
            return ConsoleEffect::None;
        };

        match index {
            ACTION_PLAY if !game.installed => ConsoleEffect::Notice(format!(
                "\"{}\" não está instalado — instale pela tela desktop nesta onda.",
                game.title
            )),
            ACTION_PLAY => ConsoleEffect::LaunchGame(game.id.clone()),
            ACTION_TRAILER => ConsoleEffect::Trailer(game.id.clone()),
            ACTION_FAVORITE => ConsoleEffect::ToggleFavorite(game.id.clone()),
            ACTION_MORE => {
                self.menu = Some(MENU_OPEN_DETAIL);
                ConsoleEffect::None
            }
            _ => ConsoleEffect::None,
        }
    }

    fn menu_effect(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        let Some(game) = self.selected(games) else {
            return ConsoleEffect::None;
        };

        match index {
            MENU_OPEN_DETAIL => ConsoleEffect::OpenGameDetail(game.id.clone()),
            MENU_FAVORITE => ConsoleEffect::ToggleFavorite(game.id.clone()),
            MENU_TRAILER => ConsoleEffect::Trailer(game.id.clone()),
            _ => ConsoleEffect::None,
        }
    }

    fn handle_menu(&mut self, event: NavEvent, games: &[&Game]) -> ConsoleEffect {
        match event {
            NavEvent::Quit => ConsoleEffect::Exit,
            NavEvent::Back => {
                self.close_menu();
                ConsoleEffect::None
            }
            NavEvent::Up => {
                let index = self.menu_index();
                self.menu = Some((index + MENU_COUNT - 1) % MENU_COUNT);
                ConsoleEffect::None
            }
            NavEvent::Down => {
                let index = self.menu_index();
                self.menu = Some((index + 1) % MENU_COUNT);
                ConsoleEffect::None
            }
            NavEvent::Confirm => {
                let index = self.menu_index();
                self.close_menu();
                self.menu_effect(index, games)
            }
            _ => ConsoleEffect::None,
        }
    }

    fn favorite_effect(&self, games: &[&Game]) -> ConsoleEffect {
        match self.selected(games) {
            Some(game) => ConsoleEffect::ToggleFavorite(game.id.clone()),
            None => ConsoleEffect::Notice("Nenhum jogo selecionado.".to_string()),
        }
    }
}

impl Default for BigPicture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, installed: bool) -> Game {
        Game {
            id: id.to_string(),
            title: format!("Jogo {id}"),
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
            installed,
            hidden: false,
            favorite: false,
            categories: Vec::new(),
        }
    }

    fn games(count: usize) -> Vec<Game> {
        (0..count)
            .map(|index| game(&index.to_string(), true))
            .collect()
    }

    fn state(count: usize) -> BigPicture {
        let mut state = BigPicture::new();
        state.sync_games(count);
        state
    }

    #[test]
    fn initial_focus_is_the_first_game() {
        let state = state(4);
        assert_eq!(state.zone(), ConsoleZone::Rail);
        assert_eq!(state.rail_index(), 0);
        assert_eq!(state.tab(), ConsoleTab::Library);
        assert!(!state.overview_open());
    }

    #[test]
    fn rail_moves_right_and_clamps_at_the_last_game() {
        let library = games(3);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(3);

        assert_eq!(state.handle(NavEvent::Right, &refs), ConsoleEffect::None);
        assert_eq!(state.rail_index(), 1);
        state.handle(NavEvent::Right, &refs);
        state.handle(NavEvent::Right, &refs);
        assert_eq!(state.rail_index(), 2);
    }

    #[test]
    fn down_opens_the_panel_and_confirm_launches_the_game() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        assert_eq!(state.handle(NavEvent::Down, &refs), ConsoleEffect::None);
        assert!(state.overview_open());
        assert_eq!(state.zone(), ConsoleZone::Overview);
        assert_eq!(state.action_index(), ACTION_PLAY);

        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::LaunchGame("0".to_string())
        );
    }

    #[test]
    fn back_closes_the_panel_then_leaves_the_mode() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::None);
        assert!(!state.overview_open());
        assert_eq!(state.zone(), ConsoleZone::Rail);
        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::Exit);
    }

    #[test]
    fn guide_always_exits() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.handle(NavEvent::Quit, &refs), ConsoleEffect::Exit);
    }

    #[test]
    fn bumpers_switch_tabs_and_library_returns_to_the_rail() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        state.handle(NavEvent::PageRight, &refs);
        assert_eq!(state.tab(), ConsoleTab::Stores);
        assert_eq!(state.zone(), ConsoleZone::Tabs);

        state.handle(NavEvent::PageLeft, &refs);
        assert_eq!(state.tab(), ConsoleTab::Library);
        assert_eq!(state.zone(), ConsoleZone::Rail);
    }

    #[test]
    fn up_goes_to_the_tabs_and_down_returns_to_the_rail() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        state.handle(NavEvent::Up, &refs);
        assert_eq!(state.zone(), ConsoleZone::Tabs);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.zone(), ConsoleZone::Rail);
    }

    #[test]
    fn empty_library_never_opens_the_panel() {
        let library: Vec<Game> = Vec::new();
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(0);

        assert_eq!(state.handle(NavEvent::Down, &refs), ConsoleEffect::None);
        assert!(!state.overview_open());
        assert_eq!(state.handle(NavEvent::Confirm, &refs), ConsoleEffect::None);
    }

    #[test]
    fn sync_games_clamps_the_selection() {
        let library = games(5);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(5);

        for _ in 0..4 {
            state.handle(NavEvent::Right, &refs);
        }
        assert_eq!(state.rail_index(), 4);

        state.sync_games(2);
        assert_eq!(state.rail_index(), 1);
        state.sync_games(0);
        assert_eq!(state.rail_index(), 0);
        assert!(!state.overview_open());
    }

    #[test]
    fn uninstalled_game_shows_a_notice_instead_of_opening() {
        let library = vec![game("1", false)];
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        state.handle(NavEvent::Down, &refs);
        assert!(matches!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::Notice(_)
        ));
    }

    #[test]
    fn notice_is_cleared_by_the_next_event() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.handle(NavEvent::Down, &refs);
        assert!(matches!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::LaunchGame(_)
        ));

        state.set_notice("teste");
        assert!(state.notice().is_some());
        state.handle(NavEvent::Left, &refs);
        assert!(state.notice().is_none());
    }

    #[test]
    fn pressing_the_focused_card_opens_the_panel() {
        let library = games(3);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(3);

        let (moved, _) = state.press_game(2, &refs);
        assert!(moved);
        assert_eq!(state.rail_index(), 2);
        assert!(!state.overview_open());

        let (moved, _) = state.press_game(2, &refs);
        assert!(!moved);
        assert!(state.overview_open());
    }

    #[test]
    fn clicking_a_card_returns_to_the_library_tab() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        state.handle(NavEvent::PageRight, &refs);
        assert_eq!(state.tab(), ConsoleTab::Stores);

        state.press_game(1, &refs);
        assert_eq!(state.tab(), ConsoleTab::Library);
        assert_eq!(state.zone(), ConsoleZone::Rail);
        assert_eq!(state.rail_index(), 1);
    }

    #[test]
    fn favorite_action_emits_toggle_for_the_selected_game() {
        let library = games(2);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(2);

        state.handle(NavEvent::Down, &refs);
        state.handle(NavEvent::Right, &refs);
        state.handle(NavEvent::Right, &refs);
        assert_eq!(state.action_index(), ACTION_FAVORITE);

        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::ToggleFavorite("0".to_string())
        );
    }

    #[test]
    fn options_button_toggles_favorite_without_opening_the_panel() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        assert_eq!(
            state.handle(NavEvent::Options, &refs),
            ConsoleEffect::ToggleFavorite("0".to_string())
        );
        assert!(!state.overview_open());
    }

    #[test]
    fn more_opens_menu_and_each_item_maps_to_an_effect() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        state.handle(NavEvent::Down, &refs);
        for _ in 0..ACTION_MORE {
            state.handle(NavEvent::Right, &refs);
        }
        assert_eq!(state.action_index(), ACTION_MORE);
        assert_eq!(state.handle(NavEvent::Confirm, &refs), ConsoleEffect::None);
        assert!(state.menu_open());
        assert_eq!(state.menu_index(), MENU_OPEN_DETAIL);

        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::OpenGameDetail("0".to_string())
        );
        assert!(!state.menu_open());

        state.press_action(ACTION_MORE, &refs);
        assert!(state.menu_open());
        state.handle(NavEvent::Down, &refs);
        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::ToggleFavorite("0".to_string())
        );

        state.press_action(ACTION_MORE, &refs);
        state.handle(NavEvent::Down, &refs);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::Trailer("0".to_string())
        );
    }

    #[test]
    fn back_closes_menu_before_the_overview() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        state.handle(NavEvent::Down, &refs);
        state.press_action(ACTION_MORE, &refs);
        assert!(state.menu_open());

        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::None);
        assert!(!state.menu_open());
        assert!(state.overview_open());

        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::None);
        assert!(!state.overview_open());
        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::Exit);
    }

    #[test]
    fn menu_navigation_wraps_between_items() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        state.handle(NavEvent::Down, &refs);
        state.press_action(ACTION_MORE, &refs);
        assert_eq!(state.menu_index(), MENU_OPEN_DETAIL);

        state.handle(NavEvent::Up, &refs);
        assert_eq!(state.menu_index(), MENU_TRAILER);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.menu_index(), MENU_OPEN_DETAIL);
    }

    #[test]
    fn news_selection_moves_and_clamps_with_the_feed_size() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.sync_news(3);
        state.select_tab(ConsoleTab::News, &refs);

        state.handle(NavEvent::Down, &refs);
        state.handle(NavEvent::Down, &refs);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.news_index(), 2);

        state.handle(NavEvent::Up, &refs);
        assert_eq!(state.news_index(), 1);
        state.sync_news(0);
        assert_eq!(state.news_index(), 0);
    }

    #[test]
    fn focus_rail_keeps_the_same_game_after_the_favorite_sort() {
        let library = games(3);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(3);

        state.focus_rail(2);
        assert_eq!(state.rail_index(), 2);
        assert!(state.selected(&refs).is_some());

        state.focus_rail(99);
        assert_eq!(state.rail_index(), 2);
    }
}
