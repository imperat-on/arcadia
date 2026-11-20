//! Estado do modo console (Big Picture).
//!
//! O renderer iced não vê botões físicos: [`BigPicture::handle`] consome
//! [`NavEvent`] do crate `arcadia-native-input` e devolve efeitos de alto nível
//! (sair, abrir a ficha do jogo, avisar stub). O foco usa [`FocusGrid`] — o
//! trilho é uma grade 1×N, as abas e as ações do painel também —, então limites
//! e bordas são exatamente os testados no crate de input.

use std::collections::BTreeSet;
use std::time::Duration;

use arcadia_native_input::{EdgeBehavior, FocusGrid, NavEvent, NavInput};

use crate::backend::Game;

/// Colunas da grade da Loja no console. Fixo para o movimento do gamepad
/// bater com o desenho (o `Grid` do iced usa o mesmo número de colunas).
pub const STORE_GRID_COLS: usize = 5;

/// Duração das transições de entrada de painel e do pulso de foco.
const MOTION_PANEL_SECONDS: f32 = 0.18;
const MOTION_FOCUS_SECONDS: f32 = 0.28;

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

    /// Chave de i18n usada pela top bar; a web já tem todas essas strings.
    pub fn i18n_key(self) -> &'static str {
        match self {
            ConsoleTab::Home => "sidebar.inicio",
            ConsoleTab::Library => "sidebar.biblioteca",
            ConsoleTab::Stores => "store.titulo",
            ConsoleTab::News => "topbar.noticias",
        }
    }

    pub const fn has_game_rail(self) -> bool {
        matches!(self, ConsoleTab::Home | ConsoleTab::Library)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

/// Onde o foco está agora.
///
/// A ordem abaixo também documenta a ordem de foco por Tab: abas → conteúdo
/// (trilho da Home/Biblioteca ou busca/grade da Loja) → ações do painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleZone {
    Tabs,
    Rail,
    Overview,
    StoreSearch,
    StoreGrid,
    StoreDetail,
}

/// Avisos do console que precisam de tradução no `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleNotice {
    NotInstalled(String),
    NoGameSelected,
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
    Notice(ConsoleNotice),
    /// Abre a ficha da loja para o item `usize` da lista atual.
    OpenStoreDetail(usize),
    /// Fecha a ficha da loja e volta à grade.
    CloseStoreDetail,
    /// Pede a próxima página de `store:recent` sem sair do console.
    StoreLoadMore,
    /// Enter/A com a busca em edição; o App dispara `store:search`.
    StoreSearch,
}

/// Estado do modo console: foco, aba ativa, painel e o leitor de gamepad.
pub struct BigPicture {
    input: Option<NavInput>,
    rail: FocusGrid,
    /// Foco do trilho da Home; a ordem combina "continuar jogando" e o resto
    /// da biblioteca, então não pode compartilhar índice com a Biblioteca.
    home: FocusGrid,
    tabs: FocusGrid,
    actions: FocusGrid,
    /// Grade da Loja; o último item pode ser "Carregar mais".
    store: FocusGrid,
    zone: ConsoleZone,
    tab: ConsoleTab,
    overview: bool,
    /// Item focado no menu "•••"; `None` quando o menu está fechado.
    menu: Option<usize>,
    /// Seleção da lista de notícias e o tamanho atual do feed.
    news_index: usize,
    news_count: usize,
    /// Quantidade atual de jogos visíveis (Home/Biblioteca compartilham).
    library_count: usize,
    /// Itens de loja (sem contar o card "Carregar mais") e se há mais páginas.
    store_items: usize,
    store_has_more: bool,
    /// A busca da loja está recebendo texto do teclado.
    store_editing: bool,
    /// A ficha da loja está aberta por cima da grade.
    store_detail: bool,
    /// `false` quando `config.no_anim` está ligado ou a build pediu movimento
    /// reduzido; nesse caso painéis entram sem fade e o foco não pulsa.
    motion: bool,
    overview_fade: f32,
    menu_fade: f32,
    store_fade: f32,
    focus_glow: f32,
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
            home: FocusGrid::new(0, 1),
            tabs,
            actions: FocusGrid::new(ACTION_COUNT, 1),
            store: FocusGrid::new(0, 1),
            zone: ConsoleZone::Rail,
            tab: ConsoleTab::Library,
            overview: false,
            menu: None,
            news_index: 0,
            news_count: 0,
            library_count: 0,
            store_items: 0,
            store_has_more: false,
            store_editing: false,
            store_detail: false,
            motion: true,
            overview_fade: 0.0,
            menu_fade: 0.0,
            store_fade: 0.0,
            focus_glow: 0.0,
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
        self.store_editing = false;
        self.store_detail = false;
        self.overview_fade = 0.0;
        self.menu_fade = 0.0;
        self.store_fade = 0.0;
        self.zone = ConsoleZone::Rail;
        self.notice = None;
        self.tabs.set_index(ConsoleTab::Library.index());
        self.actions.set_index(ACTION_PLAY);
        // Foco inicial no primeiro jogo, como pede o modo console.
        self.library_count = game_count;
        self.rail = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.home = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.touch_focus();
        self.refresh_clock();
    }

    /// Solta direções seguradas para o próximo acesso não herdar um repeat.
    pub fn leave(&mut self) {
        if let Some(input) = &mut self.input {
            input.reset();
        }
        self.overview = false;
        self.menu = None;
        self.store_editing = false;
        self.store_detail = false;
        self.zone = ConsoleZone::Rail;
        self.notice = None;
        self.overview_fade = 0.0;
        self.menu_fade = 0.0;
        self.store_fade = 0.0;
        self.focus_glow = 0.0;
    }

    /// Reconstrói os trilhos preservando a posição dentro dos novos limites.
    pub fn sync_games(&mut self, game_count: usize) {
        self.library_count = game_count;
        let rail_index = self.rail.index().min(game_count.saturating_sub(1));
        self.rail = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.rail.set_index(rail_index);

        let home_index = self.home.index().min(game_count.saturating_sub(1));
        self.home = FocusGrid::with_edge(game_count, 1, EdgeBehavior::Clamp);
        self.home.set_index(home_index);

        if game_count == 0 {
            self.overview = false;
            if self.zone == ConsoleZone::Rail {
                self.zone = ConsoleZone::Tabs;
            }
        }
    }

    /// Reconstrói a grade da loja. `items` não inclui o card "Carregar mais";
    /// quando `has_more`, ele vira a última célula focável.
    pub fn sync_store(&mut self, items: usize, has_more: bool) {
        self.store_items = items;
        self.store_has_more = has_more;
        let count = items + usize::from(has_more && items > 0);
        let rows = count.div_ceil(STORE_GRID_COLS);
        let index = self.store.index().min(count.saturating_sub(1));
        self.store = FocusGrid::with_edge(STORE_GRID_COLS, rows, EdgeBehavior::Clamp);
        self.store.set_index(index);

        if count == 0 && matches!(self.zone, ConsoleZone::StoreGrid) {
            self.zone = ConsoleZone::StoreSearch;
        }
    }

    /// Drena o gamepad com o delta do quadro e avança as transições visuais.
    pub fn tick(&mut self, dt: Duration) -> Vec<NavEvent> {
        self.advance_motion(dt);
        self.input
            .as_mut()
            .map(|input| input.tick(dt))
            .unwrap_or_default()
    }

    fn advance_motion(&mut self, dt: Duration) {
        let panel = if self.motion {
            (dt.as_secs_f32() / MOTION_PANEL_SECONDS).clamp(0.0, 1.0)
        } else {
            1.0
        };

        self.overview_fade = approach(
            self.overview_fade,
            if self.overview { 1.0 } else { 0.0 },
            panel,
        );
        self.menu_fade = approach(
            self.menu_fade,
            if self.menu.is_some() { 1.0 } else { 0.0 },
            panel,
        );
        self.store_fade = approach(
            self.store_fade,
            if self.store_detail { 1.0 } else { 0.0 },
            panel,
        );

        if self.motion {
            let focus = (dt.as_secs_f32() / MOTION_FOCUS_SECONDS).clamp(0.0, 1.0);
            self.focus_glow = (self.focus_glow - focus).max(0.0);
        } else {
            self.focus_glow = 0.0;
        }
    }

    fn touch_focus(&mut self) {
        if self.motion {
            self.focus_glow = 1.0;
        }
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

    /// Liga/desliga as transições. O App alimenta isto com `!config.no_anim`.
    pub fn set_motion(&mut self, enabled: bool) {
        self.motion = enabled;
        if !enabled {
            self.overview_fade = if self.overview { 1.0 } else { 0.0 };
            self.menu_fade = if self.menu.is_some() { 1.0 } else { 0.0 };
            self.store_fade = if self.store_detail { 1.0 } else { 0.0 };
            self.focus_glow = 0.0;
        }
    }

    #[allow(dead_code)] // leitura de estado usada por testes e chamadores futuros.
    pub fn motion(&self) -> bool {
        self.motion
    }

    pub fn overview_alpha(&self) -> f32 {
        if self.motion {
            self.overview_fade.clamp(0.0, 1.0)
        } else if self.overview {
            1.0
        } else {
            0.0
        }
    }

    pub fn menu_alpha(&self) -> f32 {
        if self.motion {
            self.menu_fade.clamp(0.0, 1.0)
        } else if self.menu.is_some() {
            1.0
        } else {
            0.0
        }
    }

    pub fn store_alpha(&self) -> f32 {
        if self.motion {
            self.store_fade.clamp(0.0, 1.0)
        } else if self.store_detail {
            1.0
        } else {
            0.0
        }
    }

    pub fn focus_glow(&self) -> f32 {
        self.focus_glow.clamp(0.0, 1.0)
    }

    pub fn zone(&self) -> ConsoleZone {
        self.zone
    }

    pub fn tab(&self) -> ConsoleTab {
        self.tab
    }

    #[allow(dead_code)] // leitura de estado usada por testes e chamadores futuros.
    pub fn rail_index(&self) -> usize {
        self.rail.index()
    }

    #[allow(dead_code)] // leitura de estado usada por testes e chamadores futuros.
    pub fn home_index(&self) -> usize {
        self.home.index()
    }

    /// Índice do trilho que a aba atual desenha.
    pub fn active_index(&self) -> usize {
        match self.tab {
            ConsoleTab::Home => self.home.index(),
            _ => self.rail.index(),
        }
    }

    pub fn tab_index(&self) -> usize {
        self.tabs.index()
    }

    pub fn action_index(&self) -> usize {
        self.actions.index()
    }

    pub fn store_index(&self) -> usize {
        self.store.index()
    }

    #[allow(dead_code)] // leitura de estado usada por testes e chamadores futuros.
    pub fn store_items(&self) -> usize {
        self.store_items
    }

    #[allow(dead_code)] // leitura de estado usada por testes e chamadores futuros.
    pub fn store_has_more(&self) -> bool {
        self.store_has_more
    }

    pub fn store_editing(&self) -> bool {
        self.store_editing
    }

    /// Entra no modo de digitação da busca (o App foca o `text_input`).
    pub fn begin_store_editing(&mut self) {
        if self.tab != ConsoleTab::Stores {
            return;
        }
        self.store_editing = true;
        self.zone = ConsoleZone::StoreSearch;
        self.touch_focus();
    }

    pub fn store_detail_open(&self) -> bool {
        self.store_detail
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
        let grid = match self.tab {
            ConsoleTab::Home => &mut self.home,
            _ => &mut self.rail,
        };
        if grid.cols() == 0 {
            return;
        }
        grid.set_index(index.min(grid.cols() - 1));
        self.touch_focus();
    }

    pub fn selected<'g>(&self, games: &[&'g Game]) -> Option<&'g Game> {
        games.get(self.active_index()).copied()
    }

    fn set_tab(&mut self, tab: ConsoleTab) {
        self.overview = false;
        self.menu = None;
        self.store_editing = false;
        self.store_detail = false;
        self.tabs.set_index(tab.index());
        self.tab = tab;
        self.notice = None;
        self.zone = match tab {
            ConsoleTab::Home | ConsoleTab::Library if self.library_count > 0 => ConsoleZone::Rail,
            ConsoleTab::Stores => ConsoleZone::StoreSearch,
            _ => ConsoleZone::Tabs,
        };
        self.touch_focus();
    }

    /// Aplica um passo de foco por Tab (ou Shift+Tab). A ordem é a mesma
    /// documentada em [`ConsoleZone`]: abas → conteúdo → painel.
    pub fn focus_step(&mut self, forward: bool) {
        if self.menu.is_some() {
            let index = if forward {
                (self.menu_index() + 1) % MENU_COUNT
            } else {
                (self.menu_index() + MENU_COUNT - 1) % MENU_COUNT
            };
            self.menu = Some(index);
            self.touch_focus();
            return;
        }

        if self.store_detail {
            self.touch_focus();
            return;
        }

        match self.tab {
            ConsoleTab::Home | ConsoleTab::Library => {
                if self.overview {
                    let index = step_index(self.action_index(), ACTION_COUNT, forward);
                    self.actions.set_index(index);
                } else if self.zone == ConsoleZone::Tabs && self.library_count > 0 {
                    self.zone = ConsoleZone::Rail;
                } else if self.zone == ConsoleZone::Rail {
                    self.zone = ConsoleZone::Tabs;
                }
            }
            ConsoleTab::Stores => {
                if self.store_editing {
                    self.store_editing = false;
                    self.zone = if self.store_items > 0 {
                        ConsoleZone::StoreGrid
                    } else {
                        ConsoleZone::Tabs
                    };
                } else {
                    let has_grid = self.store_items > 0;
                    self.zone = match (self.zone, forward) {
                        (ConsoleZone::Tabs, true) => ConsoleZone::StoreSearch,
                        (ConsoleZone::StoreSearch, true) if has_grid => ConsoleZone::StoreGrid,
                        (ConsoleZone::StoreSearch, true) => ConsoleZone::Tabs,
                        (ConsoleZone::StoreGrid, true) => ConsoleZone::Tabs,
                        (ConsoleZone::Tabs, false) if has_grid => ConsoleZone::StoreGrid,
                        (ConsoleZone::Tabs, false) => ConsoleZone::StoreSearch,
                        (ConsoleZone::StoreSearch, false) => ConsoleZone::Tabs,
                        (ConsoleZone::StoreGrid, false) => ConsoleZone::StoreSearch,
                        _ => ConsoleZone::StoreSearch,
                    };
                }
            }
            ConsoleTab::News => {}
        }
        self.touch_focus();
    }

    /// Consome um evento de navegação e devolve o efeito a executar.
    ///
    /// `Back` fecha primeiro o menu "•••", a ficha da loja ou o painel; sem
    /// nenhum deles, sai do modo (mesmo caminho de Esc no teclado).
    /// `Quit`/Guide sai sempre.
    pub fn handle(&mut self, event: NavEvent, games: &[&Game]) -> ConsoleEffect {
        self.notice = None;

        if self.menu.is_some() {
            return self.handle_menu(event, games);
        }

        if self.store_detail {
            return match event {
                NavEvent::Quit => ConsoleEffect::Exit,
                NavEvent::Back | NavEvent::Confirm => ConsoleEffect::CloseStoreDetail,
                NavEvent::PageLeft => {
                    self.close_store_detail();
                    self.step_tab(-1);
                    ConsoleEffect::None
                }
                NavEvent::PageRight => {
                    self.close_store_detail();
                    self.step_tab(1);
                    ConsoleEffect::None
                }
                _ => ConsoleEffect::None,
            };
        }

        if self.tab == ConsoleTab::Stores {
            return self.handle_store(event);
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
                self.step_tab(-1);
                ConsoleEffect::None
            }
            NavEvent::PageRight => {
                self.step_tab(1);
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
                if !self.overview && self.tab.has_game_rail() {
                    self.open_overview(games);
                }
                ConsoleEffect::None
            }
            NavEvent::Options => self.favorite_effect(games),
        }
    }

    fn handle_store(&mut self, event: NavEvent) -> ConsoleEffect {
        match event {
            NavEvent::Quit => ConsoleEffect::Exit,
            NavEvent::PageLeft => {
                self.step_tab(-1);
                ConsoleEffect::None
            }
            NavEvent::PageRight => {
                self.step_tab(1);
                ConsoleEffect::None
            }
            NavEvent::Back => {
                if self.store_editing {
                    self.store_editing = false;
                } else if self.zone == ConsoleZone::StoreGrid {
                    self.zone = ConsoleZone::StoreSearch;
                } else if self.zone == ConsoleZone::StoreSearch {
                    self.zone = ConsoleZone::Tabs;
                } else {
                    return ConsoleEffect::Exit;
                }
                self.touch_focus();
                ConsoleEffect::None
            }
            NavEvent::Confirm => {
                if self.store_editing {
                    self.store_editing = false;
                    return ConsoleEffect::StoreSearch;
                }
                match self.zone {
                    ConsoleZone::Tabs | ConsoleZone::StoreSearch => {
                        self.zone = ConsoleZone::StoreSearch;
                        self.store_editing = true;
                        self.touch_focus();
                        ConsoleEffect::None
                    }
                    ConsoleZone::StoreGrid => {
                        let index = self.store.index();
                        if index < self.store_items {
                            ConsoleEffect::OpenStoreDetail(index)
                        } else {
                            ConsoleEffect::StoreLoadMore
                        }
                    }
                    ConsoleZone::StoreDetail => ConsoleEffect::CloseStoreDetail,
                    _ => ConsoleEffect::None,
                }
            }
            NavEvent::Up => {
                if !self.store_editing {
                    match self.zone {
                        ConsoleZone::StoreGrid if self.store.row() == 0 => {
                            self.zone = ConsoleZone::StoreSearch;
                            self.touch_focus();
                        }
                        ConsoleZone::StoreGrid => {
                            self.store.move_up();
                            self.touch_focus();
                        }
                        ConsoleZone::StoreSearch | ConsoleZone::StoreDetail => {
                            self.zone = ConsoleZone::Tabs;
                            self.touch_focus();
                        }
                        ConsoleZone::Tabs | ConsoleZone::Rail | ConsoleZone::Overview => {}
                    }
                }
                ConsoleEffect::None
            }
            NavEvent::Down => {
                if self.store_editing {
                    self.store_editing = false;
                    if self.store_items > 0 {
                        self.zone = ConsoleZone::StoreGrid;
                    }
                    self.touch_focus();
                } else {
                    match self.zone {
                        ConsoleZone::StoreSearch if self.store_items > 0 => {
                            self.zone = ConsoleZone::StoreGrid;
                            self.touch_focus();
                        }
                        ConsoleZone::StoreGrid => {
                            self.store.move_down();
                            self.touch_focus();
                        }
                        ConsoleZone::Tabs if self.store_items > 0 => {
                            self.zone = ConsoleZone::StoreGrid;
                            self.touch_focus();
                        }
                        _ => {}
                    }
                }
                ConsoleEffect::None
            }
            NavEvent::Left | NavEvent::Right => {
                if !self.store_editing {
                    match self.zone {
                        ConsoleZone::StoreGrid => {
                            if event == NavEvent::Left {
                                self.store.move_left();
                            } else {
                                self.store.move_right();
                            }
                            self.touch_focus();
                        }
                        ConsoleZone::Tabs => self.step_tab(if event == NavEvent::Left { -1 } else { 1 }),
                        _ => {}
                    }
                }
                ConsoleEffect::None
            }
            NavEvent::Menu | NavEvent::Options => ConsoleEffect::None,
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
        // Cards só existem na Home e na Biblioteca. Clicar num card com outra
        // aba ativa traz o foco de volta para a aba certa.
        if !self.tab.has_game_rail() {
            self.set_tab(ConsoleTab::Library);
        }

        let grid = match self.tab {
            ConsoleTab::Home => &mut self.home,
            _ => &mut self.rail,
        };
        if grid.set_index(index) {
            self.zone = ConsoleZone::Rail;
            self.notice = None;
            self.touch_focus();
            (true, ConsoleEffect::None)
        } else {
            self.zone = ConsoleZone::Rail;
            self.open_overview(games);
            (false, ConsoleEffect::None)
        }
    }

    /// Clique do mouse numa aba da top bar.
    pub fn select_tab(&mut self, tab: ConsoleTab, games: &[&Game]) {
        self.sync_games(games.len());
        self.set_tab(tab);
    }

    /// Clique/ativação de um botão do painel do jogo.
    pub fn press_action(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        if games.is_empty() || !self.tab.has_game_rail() {
            return ConsoleEffect::None;
        }
        self.actions.set_index(index);
        self.zone = ConsoleZone::Overview;
        if !self.overview {
            self.overview = true;
            self.overview_fade = 0.0;
        }
        self.menu = None;
        self.notice = None;
        self.touch_focus();
        self.activate_action(index, games)
    }

    /// Clique/ativação de um item do menu "•••".
    pub fn press_menu(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        if index >= MENU_COUNT || !self.tab.has_game_rail() {
            return ConsoleEffect::None;
        }
        self.menu = None;
        self.menu_effect(index, games)
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// Abre a ficha da loja no console depois de o App confirmar o item.
    pub fn open_store_detail(&mut self) -> bool {
        if self.tab != ConsoleTab::Stores || self.store_items == 0 {
            return false;
        }
        self.store_detail = true;
        self.store_editing = false;
        self.store_fade = 0.0;
        self.zone = ConsoleZone::StoreDetail;
        self.touch_focus();
        true
    }

    pub fn close_store_detail(&mut self) {
        if !self.store_detail {
            return;
        }
        self.store_detail = false;
        self.zone = if self.store_items > 0 {
            ConsoleZone::StoreGrid
        } else {
            ConsoleZone::StoreSearch
        };
        self.touch_focus();
    }

    pub fn close_overview(&mut self) {
        self.overview = false;
        self.menu = None;
        self.zone = if self.library_count > 0 {
            ConsoleZone::Rail
        } else {
            ConsoleZone::Tabs
        };
    }

    fn step_tab(&mut self, delta: isize) {
        let count = ConsoleTab::ALL.len() as isize;
        let index = (self.tabs.index() as isize + delta).rem_euclid(count) as usize;
        self.set_tab(ConsoleTab::ALL[index]);
    }

    fn move_up(&mut self) {
        match self.zone {
            ConsoleZone::Rail => {
                self.zone = ConsoleZone::Tabs;
                self.touch_focus();
            }
            ConsoleZone::Overview => self.close_overview(),
            _ => {}
        }
    }

    fn move_down(&mut self, games: &[&Game]) {
        match self.zone {
            ConsoleZone::Tabs => {
                if self.tab.has_game_rail() && !games.is_empty() {
                    self.zone = ConsoleZone::Rail;
                } else if self.tab == ConsoleTab::Stores && self.store_items > 0 {
                    self.zone = ConsoleZone::StoreGrid;
                }
                self.touch_focus();
            }
            ConsoleZone::Rail => self.open_overview(games),
            _ => {}
        }
    }

    fn move_horizontal(&mut self, delta: i32) {
        match self.zone {
            ConsoleZone::Tabs => {
                // Nas abas, o direcional troca a aba sem tirar o foco da top
                // bar; descer continua sendo o gesto para entrar no conteúdo.
                self.step_tab(delta as isize);
                self.zone = ConsoleZone::Tabs;
                self.touch_focus();
            }
            ConsoleZone::Rail => {
                let grid = match self.tab {
                    ConsoleTab::Home => &mut self.home,
                    _ => &mut self.rail,
                };
                if delta < 0 {
                    grid.move_left();
                } else {
                    grid.move_right();
                }
                self.touch_focus();
            }
            ConsoleZone::Overview => {
                if delta < 0 {
                    self.actions.move_left();
                } else {
                    self.actions.move_right();
                }
                self.touch_focus();
            }
            _ => {}
        }
    }

    fn confirm(&mut self, games: &[&Game]) -> ConsoleEffect {
        match self.zone {
            ConsoleZone::Tabs => {
                if self.tab.has_game_rail() && !games.is_empty() {
                    self.zone = ConsoleZone::Rail;
                } else if self.tab == ConsoleTab::Stores {
                    self.zone = ConsoleZone::StoreSearch;
                }
                self.touch_focus();
                ConsoleEffect::None
            }
            ConsoleZone::Rail => {
                self.open_overview(games);
                ConsoleEffect::None
            }
            ConsoleZone::Overview => self.activate_action(self.actions.index(), games),
            _ => ConsoleEffect::None,
        }
    }

    fn open_overview(&mut self, games: &[&Game]) {
        if games.is_empty() || !self.tab.has_game_rail() {
            return;
        }
        if !self.overview {
            self.overview_fade = 0.0;
        }
        self.overview = true;
        self.menu = None;
        self.zone = ConsoleZone::Overview;
        self.actions.set_index(ACTION_PLAY);
        self.notice = None;
        self.touch_focus();
    }

    fn activate_action(&mut self, index: usize, games: &[&Game]) -> ConsoleEffect {
        let Some(game) = self.selected(games) else {
            return ConsoleEffect::None;
        };

        match index {
            ACTION_PLAY if !game.installed => {
                ConsoleEffect::Notice(ConsoleNotice::NotInstalled(game.title.clone()))
            }
            ACTION_PLAY => ConsoleEffect::LaunchGame(game.id.clone()),
            ACTION_TRAILER => ConsoleEffect::Trailer(game.id.clone()),
            ACTION_FAVORITE => ConsoleEffect::ToggleFavorite(game.id.clone()),
            ACTION_MORE => {
                self.menu = Some(MENU_OPEN_DETAIL);
                self.menu_fade = 0.0;
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
                self.touch_focus();
                ConsoleEffect::None
            }
            NavEvent::Down => {
                let index = self.menu_index();
                self.menu = Some((index + 1) % MENU_COUNT);
                self.touch_focus();
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
        if !self.tab.has_game_rail() {
            return ConsoleEffect::None;
        }
        match self.selected(games) {
            Some(game) => ConsoleEffect::ToggleFavorite(game.id.clone()),
            None => ConsoleEffect::Notice(ConsoleNotice::NoGameSelected),
        }
    }
}

fn approach(current: f32, target: f32, step: f32) -> f32 {
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

fn step_index(index: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if forward {
        (index + 1) % count
    } else {
        (index + count - 1) % count
    }
}

impl Default for BigPicture {
    fn default() -> Self {
        Self::new()
    }
}

/// Ordem do trilho da Home: horas jogadas primeiro (desc), o resto da
/// biblioteca depois, preservando favoritos/título da ordenação existente.
///
/// Jogos recentes são derivados de `playtime_minutes` porque a biblioteca
/// local ainda não expõe o timestamp da última sessão; a Home mostra um
/// rótulo explícito ("Continuar jogando" ou "Recentes") conforme o caso.
pub fn home_shelf<'g>(games: &[&'g Game]) -> (Vec<&'g Game>, bool) {
    let mut played: Vec<&'g Game> = games
        .iter()
        .copied()
        .filter(|game| game.playtime_minutes > 0)
        .collect();
    played.sort_by(|left, right| {
        right
            .playtime_minutes
            .cmp(&left.playtime_minutes)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });

    let has_continue = !played.is_empty();
    if has_continue {
        let played_ids: BTreeSet<&str> = played.iter().map(|game| game.id.as_str()).collect();
        played.extend(
            games
                .iter()
                .copied()
                .filter(|game| !played_ids.contains(game.id.as_str())),
        );
    } else {
        played.extend(games.iter().copied());
    }

    (played, has_continue)
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
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);

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

    #[test]
    fn home_shelf_puts_continue_playing_first() {
        let mut a = game("a", true);
        a.title = "Alpha".to_string();
        a.playtime_minutes = 30;
        let mut b = game("b", true);
        b.title = "Beta".to_string();
        b.playtime_minutes = 120;
        let mut c = game("c", true);
        c.title = "Gama".to_string();
        c.playtime_minutes = 0;

        let library = [&a, &b, &c];
        let (shelf, has_continue) = home_shelf(&library);
        let ids: Vec<&str> = shelf.iter().map(|game| game.id.as_str()).collect();
        assert!(has_continue);
        assert_eq!(ids, vec!["b", "a", "c"]);

        let (empty, has_continue) = home_shelf(&[]);
        assert!(empty.is_empty());
        assert!(!has_continue);
    }

    #[test]
    fn home_rail_has_its_own_focus_index() {
        let library = games(3);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(3);

        state.handle(NavEvent::Right, &refs);
        assert_eq!(state.rail_index(), 1);

        state.select_tab(ConsoleTab::Home, &refs);
        assert_eq!(state.zone(), ConsoleZone::Rail);
        assert_eq!(state.home_index(), 0);
        state.handle(NavEvent::Right, &refs);
        state.handle(NavEvent::Right, &refs);
        assert_eq!(state.home_index(), 2);
        assert_eq!(state.rail_index(), 1);

        state.select_tab(ConsoleTab::Library, &refs);
        assert_eq!(state.rail_index(), 1);
    }

    #[test]
    fn tab_steps_focus_between_tabs_and_the_rail() {
        let mut state = state(2);

        assert_eq!(state.zone(), ConsoleZone::Rail);
        state.focus_step(false);
        assert_eq!(state.zone(), ConsoleZone::Tabs);
        state.focus_step(true);
        assert_eq!(state.zone(), ConsoleZone::Rail);
    }

    #[test]
    fn store_grid_moves_on_a_fixed_five_column_model() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.select_tab(ConsoleTab::Stores, &refs);
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);

        state.sync_store(7, true);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);
        assert_eq!(state.store_index(), 0);

        state.handle(NavEvent::Right, &refs);
        assert_eq!(state.store_index(), 1);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.store_index(), 6);
        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::OpenStoreDetail(6)
        );

        // 4 jogos + "Carregar mais": a última célula pede a próxima página.
        state.sync_store(4, true);
        state.handle(NavEvent::Down, &refs);
        while state.store_index() < 4 {
            state.handle(NavEvent::Right, &refs);
        }
        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::StoreLoadMore
        );
    }

    #[test]
    fn store_search_edits_submits_and_back_climbs_out() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.select_tab(ConsoleTab::Stores, &refs);
        state.sync_store(3, false);

        assert_eq!(state.handle(NavEvent::Confirm, &refs), ConsoleEffect::None);
        assert!(state.store_editing());
        assert_eq!(
            state.handle(NavEvent::Confirm, &refs),
            ConsoleEffect::StoreSearch
        );
        assert!(!state.store_editing());

        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);
        state.handle(NavEvent::Back, &refs);
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);
        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::None);
        assert_eq!(state.zone(), ConsoleZone::Tabs);
        assert_eq!(state.handle(NavEvent::Back, &refs), ConsoleEffect::Exit);
    }

    #[test]
    fn store_detail_closes_with_back_before_returning_to_the_grid() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.select_tab(ConsoleTab::Stores, &refs);
        state.sync_store(2, false);
        state.handle(NavEvent::Down, &refs);

        assert!(state.open_store_detail());
        assert!(state.store_detail_open());
        assert_eq!(
            state.handle(NavEvent::Back, &refs),
            ConsoleEffect::CloseStoreDetail
        );

        state.close_store_detail();
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);
    }

    #[test]
    fn store_sync_with_no_items_returns_focus_to_search() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.select_tab(ConsoleTab::Stores, &refs);
        state.sync_store(2, false);
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);

        state.sync_store(0, false);
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);
        assert_eq!(state.store_index(), 0);
    }

    #[test]
    fn store_tab_focus_order_is_tabs_search_grid() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);
        state.select_tab(ConsoleTab::Stores, &refs);
        state.sync_store(3, false);
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);

        state.focus_step(true);
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);
        state.focus_step(true);
        assert_eq!(state.zone(), ConsoleZone::Tabs);
        state.focus_step(false);
        assert_eq!(state.zone(), ConsoleZone::StoreGrid);
        state.focus_step(false);
        assert_eq!(state.zone(), ConsoleZone::StoreSearch);
    }

    #[test]
    fn panel_fades_and_disables_motion_with_the_config_flag() {
        let library = games(1);
        let refs: Vec<&Game> = library.iter().collect();
        let mut state = state(1);

        state.handle(NavEvent::Down, &refs);
        assert!(state.overview_open());
        assert_eq!(state.overview_alpha(), 0.0);
        for _ in 0..5 {
            state.tick(Duration::from_millis(16));
        }
        assert!(state.overview_alpha() > 0.3);
        assert!(state.overview_alpha() < 1.0);
        for _ in 0..20 {
            state.tick(Duration::from_millis(16));
        }
        assert_eq!(state.overview_alpha(), 1.0);

        state.set_motion(false);
        state.close_overview();
        state.handle(NavEvent::Down, &refs);
        assert_eq!(state.overview_alpha(), 1.0);
        assert_eq!(state.focus_glow(), 0.0);
    }
}
