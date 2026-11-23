//! Menu de contexto (botão direito) dos cards da Biblioteca.
//!
//! A view do painel é uma função pura sobre [`MenuData`] para permitir preview
//! sintético. O wrapper [`right_click`] captura o botão direito com a posição
//! global do cursor — sem subscription de mouse a cada pixel — e a camada
//! [`dismiss_layer`] fecha no clique/scroll fora, sem roubar o scroll do
//! subpainel de categorias. Nada inicia jogo no botão direito.

use iced::advanced::{
    layout, mouse,
    widget::{tree, Operation},
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space, Stack};
use iced::{
    Border, Color, Element, Event, Fill, Length, Padding, Point, Rectangle, Shadow, Size, Vector,
};

use crate::app::{App, Message};
use crate::backend::Game;
use crate::i18n::I18n;
use crate::theme;

pub const PANEL_WIDTH: f32 = 252.0;
const ITEM_HEIGHT: f32 = 36.0;
const PANEL_PADDING: f32 = 6.0;
const HEADER_HEIGHT: f32 = 34.0;
const LIST_HEIGHT: f32 = 210.0;
const ADD_ROW_HEIGHT: f32 = 44.0;
const NOTICE_HEIGHT: f32 = 54.0;
const WINDOW_MARGIN: f32 = 8.0;

/// Ações na mesma ordem do `GameContextMenu.tsx`, sem "Desinstalar".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Primary,
    Details,
    Settings,
    Logs,
    Edit,
    Metadata,
    ToggleHidden,
    ToggleFavorite,
    Categories,
}

/// Ordem única usada pelo painel real e pelos testes.
pub const MAIN_ACTIONS: [MenuAction; 9] = [
    MenuAction::Primary,
    MenuAction::Details,
    MenuAction::Settings,
    MenuAction::Logs,
    MenuAction::Edit,
    MenuAction::Metadata,
    MenuAction::ToggleHidden,
    MenuAction::ToggleFavorite,
    MenuAction::Categories,
];

/// Aviso inline do menu. Guarda a intenção, não o texto traduzido, para trocar
/// de idioma sem reabrir o menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNotice {
    NoLogs,
    LogsFailed,
    SteamInvalid,
    SteamOpenFailed,
    InstallUnavailable(String),
    Busy,
    SaveFailed(String),
}

impl MenuNotice {
    pub fn text(&self, i18n: &I18n) -> String {
        match self {
            Self::NoLogs => i18n.t("library.sem_registros").to_string(),
            Self::LogsFailed => i18n.t("native.library.menu.logs_failed").to_string(),
            Self::SteamInvalid => i18n.t("native.library.menu.steam_invalid").to_string(),
            Self::SteamOpenFailed => i18n.t("native.library.menu.steam_open_failed").to_string(),
            Self::InstallUnavailable(launcher) => i18n.t_args(
                "native.library.menu.install_unavailable",
                &[("launcher", launcher)],
            ),
            Self::Busy => i18n.t("native.library.menu.busy").to_string(),
            Self::SaveFailed(error) => i18n.t_args(
                "native.library.menu.override_failed",
                &[("error", &shorten(error, 140))],
            ),
        }
    }
}

/// Estado do menu: o jogo é sempre referenciado pelo id estável, nunca pelo
/// índice da grade (que muda quando favoritos são reordenados).
#[derive(Debug, Clone, PartialEq)]
pub struct GameMenuState {
    pub game_id: String,
    pub anchor: Point,
    /// Identidade da instância do menu. Resultados assíncronos carregam este
    /// valor para não fecharem um menu de outro jogo/reaberto.
    pub instance: u64,
    pub categories_open: bool,
    pub category_draft: String,
    pub notice: Option<MenuNotice>,
}

impl GameMenuState {
    pub fn new(game_id: impl Into<String>, anchor: Point) -> Self {
        Self::with_instance(game_id, anchor, 0)
    }

    pub fn with_instance(game_id: impl Into<String>, anchor: Point, instance: u64) -> Self {
        Self {
            game_id: game_id.into(),
            anchor,
            instance,
            categories_open: false,
            category_draft: String::new(),
            notice: None,
        }
    }

    pub fn height(&self) -> f32 {
        panel_height(self.categories_open, self.notice.is_some())
    }

    pub fn rect(&self, window: Size) -> Rectangle {
        let origin = clamp_menu_origin(self.anchor, window, self.height());
        Rectangle::new(origin, Size::new(PANEL_WIDTH, self.height()))
    }
}

/// Dados que o painel desenha. Separado do `Game` para uma preview sintética
/// poder chamar [`panel`] sem construir o app inteiro.
#[derive(Debug, Clone, Copy)]
pub struct MenuData<'a> {
    pub installed: bool,
    pub favorite: bool,
    pub hidden: bool,
    pub categories: &'a [String],
}

impl<'a> MenuData<'a> {
    pub fn from_game(game: &'a Game) -> Self {
        Self {
            installed: game.installed,
            favorite: game.favorite,
            hidden: game.hidden,
            categories: game.categories.as_slice(),
        }
    }
}

pub fn panel_height(categories_open: bool, has_notice: bool) -> f32 {
    let notice = if has_notice { NOTICE_HEIGHT } else { 0.0 };
    if categories_open {
        PANEL_PADDING * 2.0 + HEADER_HEIGHT + LIST_HEIGHT + ADD_ROW_HEIGHT + notice
    } else {
        PANEL_PADDING * 2.0 + ITEM_HEIGHT * 9.0 + notice
    }
}

/// Mantém o menu inteiro dentro da janela nas quatro bordas.
pub fn clamp_menu_origin(anchor: Point, window: Size, height: f32) -> Point {
    let max_x = (window.width - PANEL_WIDTH - WINDOW_MARGIN).max(WINDOW_MARGIN);
    let max_y = (window.height - height - WINDOW_MARGIN).max(WINDOW_MARGIN);
    Point::new(
        anchor.x.clamp(WINDOW_MARGIN, max_x),
        anchor.y.clamp(WINDOW_MARGIN, max_y),
    )
}

fn shorten(value: &str, limit: usize) -> String {
    let mut text: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        text.push('…');
    }
    text
}

/// Ícones vetoriais desenhados em canvas (sem assets/emoji).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIcon {
    Play,
    Install,
    Details,
    Settings,
    Logs,
    Edit,
    Metadata,
    Eye,
    EyeOff,
    Heart,
    HeartFilled,
    Categories,
    Back,
    Check,
    Plus,
}

pub fn vector_icon(kind: VectorIcon, size: f32, opacity: f32) -> Element<'static, Message> {
    Canvas::new(IconPlot {
        kind,
        color: Color::from_rgba(1.0, 1.0, 1.0, opacity.clamp(0.0, 1.0)),
    })
    .width(size)
    .height(size)
    .into()
}

fn check_icon(size: f32, color: Color) -> Element<'static, Message> {
    Canvas::new(IconPlot {
        kind: VectorIcon::Check,
        color,
    })
    .width(size)
    .height(size)
    .into()
}

struct IconPlot {
    kind: VectorIcon,
    color: Color,
}

fn point(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

impl canvas::Program<Message> for IconPlot {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let color = self.color;
        let stroke = Stroke::default()
            .with_color(color)
            .with_width(1.3)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        let line = |frame: &mut Frame, from: Point, to: Point| {
            frame.stroke(&Path::line(from, to), stroke);
        };
        let circle = |frame: &mut Frame, center: Point, radius: f32, fill: bool| {
            let path = Path::circle(center, radius);
            if fill {
                frame.fill(&path, color);
            } else {
                frame.stroke(&path, stroke);
            }
        };

        match self.kind {
            VectorIcon::Play => {
                let path = Path::new(|builder| {
                    builder.move_to(point(4.5, 3.0));
                    builder.line_to(point(13.0, 8.0));
                    builder.line_to(point(4.5, 13.0));
                    builder.close();
                });
                frame.fill(&path, color);
            }
            VectorIcon::Install => {
                line(&mut frame, point(8.0, 2.0), point(8.0, 11.0));
                line(&mut frame, point(4.5, 7.5), point(8.0, 11.0));
                line(&mut frame, point(11.5, 7.5), point(8.0, 11.0));
                line(&mut frame, point(3.0, 14.0), point(13.0, 14.0));
            }
            VectorIcon::Details | VectorIcon::Logs => {
                let path = Path::new(|builder| {
                    builder.move_to(point(4.0, 2.0));
                    builder.line_to(point(9.5, 2.0));
                    builder.line_to(point(13.0, 5.5));
                    builder.line_to(point(13.0, 14.0));
                    builder.line_to(point(4.0, 14.0));
                    builder.close();
                });
                frame.stroke(&path, stroke);
                line(&mut frame, point(9.5, 2.0), point(9.5, 5.5));
                line(&mut frame, point(9.5, 5.5), point(13.0, 5.5));
                if self.kind == VectorIcon::Logs {
                    line(&mut frame, point(6.0, 8.5), point(11.0, 8.5));
                    line(&mut frame, point(6.0, 11.0), point(11.0, 11.0));
                } else {
                    line(&mut frame, point(6.0, 8.0), point(11.0, 8.0));
                    line(&mut frame, point(6.0, 10.5), point(11.0, 10.5));
                    line(&mut frame, point(6.0, 12.5), point(9.0, 12.5));
                }
            }
            VectorIcon::Settings => {
                circle(&mut frame, point(8.0, 8.0), 2.4, false);
                circle(&mut frame, point(8.0, 8.0), 5.6, false);
                for step in 0..8 {
                    let angle = step as f32 * std::f32::consts::TAU / 8.0;
                    line(
                        &mut frame,
                        point(8.0 + angle.cos() * 5.6, 8.0 + angle.sin() * 5.6),
                        point(8.0 + angle.cos() * 7.0, 8.0 + angle.sin() * 7.0),
                    );
                }
            }
            VectorIcon::Edit => {
                line(&mut frame, point(3.0, 13.0), point(3.0, 11.0));
                line(&mut frame, point(3.0, 11.0), point(10.5, 3.5));
                line(&mut frame, point(10.5, 3.5), point(12.5, 5.5));
                line(&mut frame, point(12.5, 5.5), point(5.0, 13.0));
                line(&mut frame, point(5.0, 13.0), point(3.0, 13.0));
            }
            VectorIcon::Metadata => {
                for (y, dot_x) in [(4.0, 5.0), (8.0, 11.0), (12.0, 7.0)] {
                    line(&mut frame, point(2.0, y), point(14.0, y));
                    circle(&mut frame, point(dot_x, y), 1.6, true);
                }
            }
            VectorIcon::Eye | VectorIcon::EyeOff => {
                let eye = Path::new(|builder| {
                    builder.move_to(point(1.5, 8.0));
                    builder.quadratic_curve_to(point(8.0, 2.5), point(14.5, 8.0));
                    builder.quadratic_curve_to(point(8.0, 13.5), point(1.5, 8.0));
                    builder.close();
                });
                frame.stroke(&eye, stroke);
                circle(&mut frame, point(8.0, 8.0), 2.2, false);
                if self.kind == VectorIcon::EyeOff {
                    line(&mut frame, point(2.0, 2.0), point(14.0, 14.0));
                }
            }
            VectorIcon::Heart | VectorIcon::HeartFilled => {
                let heart = Path::new(|builder| {
                    builder.move_to(point(8.0, 13.5));
                    builder.bezier_curve_to(point(2.2, 9.2), point(1.6, 5.0), point(4.4, 3.6));
                    builder.bezier_curve_to(point(6.2, 2.8), point(7.5, 3.9), point(8.0, 5.2));
                    builder.bezier_curve_to(point(8.5, 3.9), point(9.8, 2.8), point(11.6, 3.6));
                    builder.bezier_curve_to(point(14.4, 5.0), point(13.8, 9.2), point(8.0, 13.5));
                    builder.close();
                });
                if self.kind == VectorIcon::HeartFilled {
                    frame.fill(&heart, color);
                } else {
                    frame.stroke(&heart, stroke);
                }
            }
            VectorIcon::Categories => {
                for y in [4.0, 8.0, 12.0] {
                    circle(&mut frame, point(3.0, y), 1.2, true);
                    line(&mut frame, point(6.0, y), point(14.0, y));
                }
            }
            VectorIcon::Back => {
                line(&mut frame, point(13.0, 8.0), point(3.0, 8.0));
                line(&mut frame, point(7.0, 4.0), point(3.0, 8.0));
                line(&mut frame, point(7.0, 12.0), point(3.0, 8.0));
            }
            VectorIcon::Check => {
                line(&mut frame, point(3.5, 8.5), point(6.6, 11.6));
                line(&mut frame, point(6.6, 11.6), point(12.6, 4.4));
            }
            VectorIcon::Plus => {
                line(&mut frame, point(8.0, 3.0), point(8.0, 13.0));
                line(&mut frame, point(3.0, 8.0), point(13.0, 8.0));
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Painel puro: não conhece `App`, só os dados do jogo e o estado local.
pub fn panel<'a>(
    i18n: &'a I18n,
    data: MenuData<'a>,
    all_categories: &[String],
    state: &'a GameMenuState,
) -> Element<'a, Message> {
    let height = panel_height(state.categories_open, state.notice.is_some());
    let content = if state.categories_open {
        categories_panel(i18n, data, all_categories, state)
    } else {
        actions_panel(i18n, data)
    };

    let mut body = column![content].spacing(0).height(Length::Shrink);
    if let Some(notice) = state.notice.as_ref() {
        body = body.push(
            container(Space::new().width(Fill).height(1))
                .width(Fill)
                .height(1)
                .style(|_| container::Style {
                    background: Some(theme::BORDER.into()),
                    ..container::Style::default()
                }),
        );
        body = body.push(
            container(
                text(notice.text(i18n))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_2)
                    .width(Fill),
            )
            .width(Fill)
            .height(53)
            .clip(true)
            .padding([6.0, 10.0]),
        );
    }

    container(body)
        .width(Length::Fixed(PANEL_WIDTH))
        .height(Length::Fixed(height))
        .padding(PANEL_PADDING)
        .style(menu_surface)
        .into()
}

fn actions_panel<'a>(i18n: &'a I18n, data: MenuData<'a>) -> Element<'a, Message> {
    let mut items = column![].spacing(0);
    for action in MAIN_ACTIONS {
        let (icon, label) = action_presentation(action, data, i18n);
        items = items.push(item(icon, label, action));
    }
    items.into()
}

fn action_presentation<'a>(
    action: MenuAction,
    data: MenuData<'a>,
    i18n: &'a I18n,
) -> (VectorIcon, &'a str) {
    match action {
        MenuAction::Primary if data.installed => (VectorIcon::Play, i18n.t("library.jogar")),
        MenuAction::Primary => (VectorIcon::Install, i18n.t("library.instalar")),
        MenuAction::Details => (VectorIcon::Details, i18n.t("library.detalhes")),
        MenuAction::Settings => (VectorIcon::Settings, i18n.t("library.config_jogo")),
        MenuAction::Logs => (VectorIcon::Logs, i18n.t("library.registros")),
        MenuAction::Edit => (VectorIcon::Edit, i18n.t("library.editar_jogo")),
        MenuAction::Metadata => (VectorIcon::Metadata, i18n.t("library.metadados")),
        MenuAction::ToggleHidden if data.hidden => {
            (VectorIcon::Eye, i18n.t("library.mostrar_jogo"))
        }
        MenuAction::ToggleHidden => (VectorIcon::EyeOff, i18n.t("library.ocultar_jogo")),
        MenuAction::ToggleFavorite if data.favorite => {
            (VectorIcon::HeartFilled, i18n.t("library.remover_favoritos"))
        }
        MenuAction::ToggleFavorite => (VectorIcon::Heart, i18n.t("library.adicionar_favoritos")),
        MenuAction::Categories => (VectorIcon::Categories, i18n.t("library.categorias")),
    }
}

fn item<'a>(icon: VectorIcon, label: &'a str, action: MenuAction) -> Element<'a, Message> {
    button(
        row![
            vector_icon(icon, 16.0, 0.82),
            text(label).size(theme::TEXT_CAPTION)
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::GameMenuAction(action))
    .width(Fill)
    .height(Length::Fixed(ITEM_HEIGHT))
    .padding([0.0, 12.0])
    .style(menu_item)
    .into()
}

fn categories_panel<'a>(
    i18n: &'a I18n,
    data: MenuData<'a>,
    all_categories: &[String],
    state: &'a GameMenuState,
) -> Element<'a, Message> {
    let back = button(
        row![
            vector_icon(VectorIcon::Back, 13.0, 0.60),
            text(i18n.t("library.categorias"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_2),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::GameMenuCategoryBack)
    .width(Fill)
    .height(Length::Fixed(HEADER_HEIGHT))
    .padding([0.0, 10.0])
    .style(menu_item);

    let mut list = column![].spacing(0).width(Fill);
    if all_categories.is_empty() {
        list = list.push(
            container(
                text(i18n.t("library.sem_categorias"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            )
            .width(Fill)
            .padding([8.0, 10.0]),
        );
    }
    for category in all_categories {
        let checked = game_has_category(&data, category);
        list = list.push(category_row(category.clone(), checked));
    }

    let list = container(scrollable(list).height(Length::Fixed(LIST_HEIGHT)))
        .width(Fill)
        .height(Length::Fixed(LIST_HEIGHT))
        .clip(true);

    let add = row![
        text_input(i18n.t("library.nova_categoria"), &state.category_draft)
            .on_input(Message::GameMenuCategoryDraftChanged)
            .on_submit(Message::GameMenuCategoryAdded)
            .padding([6.0, 8.0])
            .size(theme::TEXT_MICRO)
            .width(Fill)
            .style(theme::search_input),
        button(
            container(vector_icon(VectorIcon::Plus, 12.0, 0.95))
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill),
        )
        .on_press_maybe(
            (!state.category_draft.trim().is_empty()).then_some(Message::GameMenuCategoryAdded)
        )
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .style(theme::primary_button),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .height(Length::Fixed(ADD_ROW_HEIGHT))
    .padding(Padding {
        top: 7.0,
        right: 4.0,
        bottom: 7.0,
        left: 4.0,
    });

    column![back, list, add]
        .spacing(0)
        .height(Length::Shrink)
        .into()
}

fn category_row<'a>(category: String, checked: bool) -> Element<'a, Message> {
    let box_style = move |_theme: &iced::Theme| container::Style {
        background: if checked {
            Some(theme::ACCENT.into())
        } else {
            None
        },
        border: Border {
            color: if checked {
                Color::TRANSPARENT
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.30)
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    };

    let checkbox = container(check_icon(
        11.0,
        if checked {
            theme::BACKGROUND
        } else {
            Color::TRANSPARENT
        },
    ))
    .width(15)
    .height(15)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(box_style);

    let label = category.clone();
    button(
        row![checkbox, text(label).size(theme::TEXT_MICRO)]
            .spacing(10)
            .align_y(iced::Center),
    )
    .on_press(Message::GameMenuCategoryToggled(category))
    .width(Fill)
    .height(Length::Fixed(32.0))
    .padding([0.0, 10.0])
    .style(menu_item)
    .into()
}

fn game_has_category(data: &MenuData<'_>, category: &str) -> bool {
    data.categories.iter().any(|item| item == category)
}

/// Superfície grafite do menu: raio 12, borda sutil e sombra curta.
fn menu_surface(_theme: &iced::Theme) -> container::Style {
    container::Style {
        text_color: Some(theme::TEXT_1),
        background: Some(Color::from_rgb8(0x12, 0x13, 0x17).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.62),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

fn menu_item(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        text_color: if hovered {
            theme::TEXT_1
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.86)
        },
        border: Border::default().rounded(8),
        ..button::Style::default()
    }
}

/// Overlay completo: camada de dispensa + painel posicionado no cursor.
pub fn overlay<'a>(app: &'a App) -> Element<'a, Message> {
    let Some(state) = app.game_menu.as_ref() else {
        return Space::new().into();
    };
    let Some(game) = app.library.game(&state.game_id) else {
        return Space::new().into();
    };
    let data = MenuData::from_game(game);
    let all_categories = app.library.all_categories();
    let rect = state.rect(app.window_size);
    let panel = capture_press(panel(&app.i18n, data, &all_categories, state));

    let placed = container(panel)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Left)
        .align_y(iced::Top)
        .padding(Padding {
            top: rect.y,
            right: 0.0,
            bottom: 0.0,
            left: rect.x,
        });

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(dismiss_layer(rect))
        .push(placed)
        .into()
}

// ── Wrappers de mouse ───────────────────────────────────────────────────────

/// Resultado puro do wrapper: `Some` só no botão direito dentro do card, com
/// a posição global do cursor. O widget real publica o menu e retorna sem
/// encaminhar o evento ao card; `None` mantém o clique esquerdo intacto.
fn right_click_capture(
    event: &Event,
    cursor: mouse::Cursor,
    bounds: Rectangle,
    viewport: &Rectangle,
    game_id: &str,
) -> Option<(String, Point)> {
    if !matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
    ) || !cursor.is_over(bounds)
        || !cursor.is_over(*viewport)
    {
        return None;
    }
    cursor
        .position()
        .map(|anchor| (game_id.to_string(), anchor))
}

/// Captura o botão direito com a posição global do cursor. O clique esquerdo
/// continua descendo para o card exatamente como antes.
pub fn right_click<'a>(
    content: impl Into<Element<'a, Message>>,
    game_id: String,
) -> Element<'a, Message> {
    Element::new(RightClick {
        content: content.into(),
        game_id,
    })
}

struct RightClick<'a> {
    content: Element<'a, Message>,
    game_id: String,
}

impl Widget<Message, iced::Theme, iced::Renderer> for RightClick<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }
    fn state(&self) -> tree::State {
        tree::State::None
    }
    fn children(&self) -> Vec<tree::Tree> {
        vec![tree::Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut tree::Tree) {
        tree.diff_children(&[&self.content]);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut tree::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some((game_id, anchor)) =
            right_click_capture(event, cursor, layout.bounds(), viewport, &self.game_id)
        {
            shell.publish(Message::OpenGameContextMenu { game_id, anchor });
            shell.capture_event();
            return;
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// Engole pressões que não foram tratadas por um botão/campo interno. Sem
/// isto um clique direito no vazio do painel atravessaria até o card de trás.
pub(crate) fn capture_press<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(PressGuard {
        content: content.into(),
    })
}

struct PressGuard<'a> {
    content: Element<'a, Message>,
}

impl Widget<Message, iced::Theme, iced::Renderer> for PressGuard<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }
    fn state(&self) -> tree::State {
        tree::State::None
    }
    fn children(&self) -> Vec<tree::Tree> {
        vec![tree::Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut tree::Tree) {
        tree.diff_children(&[&self.content]);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() {
            return;
        }
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && cursor.is_over(layout.bounds())
            && cursor.is_over(*viewport)
        {
            shell.capture_event();
        }
    }
    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

fn dismiss_layer<'a>(menu_rect: Rectangle) -> Element<'a, Message> {
    Element::new(DismissLayer {
        content: Space::new().width(Fill).height(Fill).into(),
        menu_rect,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DismissOutcome {
    Close,
    CloseAndCapture,
    Swallow,
    None,
}

fn dismiss_outcome(event: &Event, cursor: mouse::Cursor, menu_rect: Rectangle) -> DismissOutcome {
    let Some(position) = cursor.position() else {
        return DismissOutcome::None;
    };
    let inside = menu_rect.contains(position);
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if !inside => {
            DismissOutcome::CloseAndCapture
        }
        Event::Mouse(mouse::Event::WheelScrolled { .. }) if inside => DismissOutcome::Swallow,
        Event::Mouse(mouse::Event::WheelScrolled { .. }) => DismissOutcome::CloseAndCapture,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if inside => {
            DismissOutcome::Swallow
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => DismissOutcome::Close,
        _ => DismissOutcome::None,
    }
}

struct DismissLayer<'a> {
    content: Element<'a, Message>,
    menu_rect: Rectangle,
}

impl Widget<Message, iced::Theme, iced::Renderer> for DismissLayer<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }
    fn state(&self) -> tree::State {
        tree::State::None
    }
    fn children(&self) -> Vec<tree::Tree> {
        vec![tree::Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut tree::Tree) {
        tree.diff_children(&[&self.content]);
    }
    fn size(&self) -> Size<Length> {
        Size::new(Fill, Fill)
    }
    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() {
            return;
        }
        match dismiss_outcome(event, cursor, self.menu_rect) {
            DismissOutcome::Close => {
                // Fecha no clique direito fora, mas sem capturar: se o card
                // de trás publicar `OpenGameContextMenu`, o novo menu vence.
                shell.publish(Message::CloseGameContextMenu);
            }
            DismissOutcome::CloseAndCapture => {
                shell.publish(Message::CloseGameContextMenu);
                shell.capture_event();
            }
            DismissOutcome::Swallow => {
                // O scroll sobre o painel não pode rolar a grade atrás.
                shell.capture_event();
            }
            DismissOutcome::None => {}
        }
    }
    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(categories_open: bool, notice: bool) -> GameMenuState {
        let mut state = GameMenuState::new("steam:440", Point::new(100.0, 100.0));
        state.categories_open = categories_open;
        state.notice = notice.then_some(MenuNotice::NoLogs);
        state
    }

    #[test]
    fn clamps_on_all_four_edges() {
        let window = Size::new(1280.0, 800.0);
        let height = panel_height(false, false);
        let top_left = clamp_menu_origin(Point::new(0.0, 0.0), window, height);
        assert_eq!(top_left, Point::new(WINDOW_MARGIN, WINDOW_MARGIN));
        let bottom_right = clamp_menu_origin(Point::new(5000.0, 5000.0), window, height);
        assert_eq!(bottom_right.x, 1280.0 - PANEL_WIDTH - WINDOW_MARGIN);
        assert_eq!(bottom_right.y, 800.0 - height - WINDOW_MARGIN);
        let rect = state(false, false).rect(window);
        assert!(rect.x >= WINDOW_MARGIN);
        assert!(rect.y >= WINDOW_MARGIN);
        assert!(rect.x + rect.width <= window.width);
        assert!(rect.y + rect.height <= window.height);
    }

    #[test]
    fn tiny_window_keeps_the_menu_origin_inside() {
        let window = Size::new(200.0, 120.0);
        let origin = clamp_menu_origin(Point::new(999.0, 999.0), window, 400.0);
        assert_eq!(origin, Point::new(WINDOW_MARGIN, WINDOW_MARGIN));
    }

    #[test]
    fn panel_renders_for_synthetic_data_without_the_app() {
        let i18n = I18n::default();
        let categories = vec!["Ação".to_string(), "Com amigos".to_string()];
        let data = MenuData {
            installed: false,
            favorite: true,
            hidden: false,
            categories: &categories,
        };
        let mut state = state(false, true);
        drop(panel(&i18n, data, &categories, &state));
        state.categories_open = true;
        drop(panel(&i18n, data, &categories, &state));
    }

    #[test]
    fn menu_actions_follow_the_screenshot_order_exactly() {
        assert_eq!(
            MAIN_ACTIONS,
            [
                MenuAction::Primary,
                MenuAction::Details,
                MenuAction::Settings,
                MenuAction::Logs,
                MenuAction::Edit,
                MenuAction::Metadata,
                MenuAction::ToggleHidden,
                MenuAction::ToggleFavorite,
                MenuAction::Categories,
            ]
        );
    }

    #[test]
    fn right_click_captures_the_global_cursor_and_does_not_forward() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 100.0));
        let viewport = Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0));
        let cursor = mouse::Cursor::Available(Point::new(55.0, 75.0));
        let right = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        assert_eq!(
            right_click_capture(&right, cursor, bounds, &viewport, "steam:440"),
            Some(("steam:440".to_string(), Point::new(55.0, 75.0)))
        );

        // Clique esquerdo e clique fora continuam sendo encaminhados ao card.
        let left = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(
            right_click_capture(&left, cursor, bounds, &viewport, "steam:440"),
            None
        );
        let outside = mouse::Cursor::Available(Point::new(5.0, 5.0));
        assert_eq!(
            right_click_capture(&right, outside, bounds, &viewport, "steam:440"),
            None
        );
    }

    #[test]
    fn dismiss_closes_on_outside_click_and_scroll_but_keeps_submenu_scroll() {
        let menu = Rectangle::new(Point::new(100.0, 100.0), Size::new(200.0, 200.0));
        let inside = mouse::Cursor::Available(Point::new(150.0, 150.0));
        let outside = mouse::Cursor::Available(Point::new(50.0, 50.0));
        let left = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let right = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });

        assert_eq!(
            dismiss_outcome(&left, outside, menu),
            DismissOutcome::CloseAndCapture
        );
        assert_eq!(dismiss_outcome(&left, inside, menu), DismissOutcome::None);
        assert_eq!(
            dismiss_outcome(&wheel, outside, menu),
            DismissOutcome::CloseAndCapture
        );
        assert_eq!(
            dismiss_outcome(&wheel, inside, menu),
            DismissOutcome::Swallow
        );
        assert_eq!(
            dismiss_outcome(&right, outside, menu),
            DismissOutcome::Close
        );
        assert_eq!(
            dismiss_outcome(&right, inside, menu),
            DismissOutcome::Swallow
        );
    }

    #[test]
    fn menu_height_is_exact_for_both_panels() {
        let main = panel_height(false, false);
        assert_eq!(main, PANEL_PADDING * 2.0 + ITEM_HEIGHT * 9.0);
        let categories = panel_height(true, true);
        assert_eq!(
            categories,
            PANEL_PADDING * 2.0 + HEADER_HEIGHT + LIST_HEIGHT + ADD_ROW_HEIGHT + NOTICE_HEIGHT
        );
    }
}
