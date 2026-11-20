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
/// Jogar (abre a ficha do jogo no desktop).
pub const ACTION_PLAY: usize = 0;
/// Trailer — stub desta onda; só exibe o aviso de próximo passo.
pub const ACTION_TRAILER: usize = 1;
/// Favorito — stub: a escrita de favoritos ainda não tem canal no nativo.
pub const ACTION_FAVORITE: usize = 2;
/// "•••" — stub de opções do jogo.
pub const ACTION_MORE: usize = 3;

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
    /// Aviso curto no rodapé do painel (stub explícito).
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

    pub fn selected<'g>(&self, games: &[&'g Game]) -> Option<&'g Game> {
        games.get(self.rail.index()).copied()
    }

    /// Consome um evento de navegação e devolve o efeito a executar.
    ///
    /// `Back` fecha o painel quando ele está aberto; sem painel, sai do modo
    /// (mesmo caminho de Esc no teclado). `Quit`/Guide sai sempre.
    pub fn handle(&mut self, event: NavEvent, games: &[&Game]) -> ConsoleEffect {
        self.notice = None;

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
            NavEvent::Options => self.favorite_notice(games),
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
        self.notice = None;
        self.activate_action(index, games)
    }

    pub fn close_overview(&mut self) {
        self.overview = false;
        self.zone = ConsoleZone::Rail;
    }

    fn step_tab(&mut self, delta: isize, games: &[&Game]) {
        let count = ConsoleTab::ALL.len() as isize;
        let index = (self.tabs.index() as isize + delta).rem_euclid(count) as usize;
        self.overview = false;
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
            ACTION_TRAILER => ConsoleEffect::Notice(
                "O player de trailer do console chega na próxima onda; abra a ficha do jogo no desktop para assistir."
                    .to_string(),
            ),
            ACTION_FAVORITE => self.favorite_notice(games),
            ACTION_MORE => ConsoleEffect::Notice(
                "As opções do jogo (•••) ainda não têm menu no console; use a ficha do jogo no desktop."
                    .to_string(),
            ),
            _ => ConsoleEffect::None,
        }
    }

    fn favorite_notice(&mut self, games: &[&Game]) -> ConsoleEffect {
        match self.selected(games) {
            Some(game) => ConsoleEffect::Notice(format!(
                "\"{}\" {} nos favoritos. Marcar/desmarcar pelo console ainda não persiste nesta onda.",
                game.title,
                if game.favorite { "está" } else { "não está" }
            )),
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
}
