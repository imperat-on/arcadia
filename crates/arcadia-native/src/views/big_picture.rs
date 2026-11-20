//! Modo console (Big Picture): hero do jogo focado, top bar de abas, trilho
//! horizontal de capas e painel do jogo.
//!
//! Visual portado do `PS5Launcher`/`HeroSection`/`GameRail`/`GameOverview` da UI
//! web, com as simplificações possíveis no iced: sem blur/backdrop-filter,
//! gradientes viram camadas translúcidas e a capa focada ganha moldura branca
//! em vez de escala animada.

use iced::widget::scrollable::{AbsoluteOffset, Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding, Task};

use crate::app::{App, LibraryStatus, LoadStatus, Message};
use crate::backend::Game;
use crate::big_picture::{
    ConsoleTab, ConsoleZone, ACTION_FAVORITE, ACTION_MORE, ACTION_PLAY, ACTION_TRAILER,
    MENU_COUNT, MENU_FAVORITE, MENU_OPEN_DETAIL, MENU_TRAILER,
};
use crate::media;
use crate::theme;

/// Medidas do trilho compartilhadas com o cálculo de scroll.
pub const RAIL_CARD_WIDTH: f32 = 148.0;
pub const RAIL_CARD_GAP: f32 = 18.0;
const RAIL_COVER_HEIGHT: f32 = 188.0;
const RAIL_CARD_HEIGHT: f32 = 244.0;
const RAIL_HEIGHT: f32 = 300.0;
const GUTTER: f32 = 56.0;
const RAIL_ID: &str = "arcadia-console-rail";

const MUTED: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.62);
const FAINT: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.40);

/// Rola o trilho para manter o jogo focado visível (centralizado quando cabe).
pub fn rail_scroll_task(index: usize, viewport_width: f32) -> Task<Message> {
    let x = (index as f32 * (RAIL_CARD_WIDTH + RAIL_CARD_GAP) + RAIL_CARD_WIDTH / 2.0
        - viewport_width / 2.0)
        .max(0.0);

    iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(RAIL_ID),
        AbsoluteOffset {
            x: Some(x),
            y: Some(0.0),
        },
    ))
}

pub fn view(app: &App) -> Element<'_, Message> {
    let games = app.library.visible_all();
    let selected = app.big_picture.selected(&games);

    let base = column![
        top_bar(app),
        container(copy_area(app, selected))
            .width(Fill)
            .height(Fill)
            .align_y(iced::Bottom)
            .padding(Padding {
                top: 0.0,
                right: GUTTER,
                bottom: 4.0,
                left: GUTTER,
            }),
        rail(app, &games),
    ]
    .width(Fill)
    .height(Fill);

    let mut layers = Stack::new()
        .width(Fill)
        .height(Fill)
        .push(background(app, selected))
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::console_wash),
        )
        .push(
            container(
                container(Space::new())
                    .width(Fill)
                    .height(360)
                    .style(theme::console_bottom_wash),
            )
            .width(Fill)
            .height(Fill)
            .align_y(iced::Bottom),
        )
        .push(base);

    if app.big_picture.overview_open() {
        if let Some(game) = selected {
            layers = layers.push(overview(app, game));
            if app.big_picture.menu_open() {
                layers = layers.push(more_menu(app, game));
            }
        }
    }

    layers.into()
}

fn background<'a>(app: &'a App, selected: Option<&'a Game>) -> Element<'a, Message> {
    let url = selected.and_then(art_url);

    match app.covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            column![
                text("ARCADIA")
                    .size(76)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.10)),
                text(match &app.library.status {
                    LibraryStatus::Loading => "Carregando a biblioteca local…",
                    LibraryStatus::Error(_) => "Biblioteca indisponível",
                    LibraryStatus::Ready => "Modo console",
                })
                .size(13)
                .color(FAINT),
            ]
            .spacing(8)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::app_background)
        .into(),
    }
}

fn top_bar(app: &App) -> Element<'_, Message> {
    let bp = &app.big_picture;
    let mut tabs = iced::widget::Row::new().spacing(30).align_y(iced::Center);

    for tab in ConsoleTab::ALL {
        let active = bp.tab() == tab;
        let focused = bp.zone() == ConsoleZone::Tabs && bp.tab_index() == tab.index();
        let underline = container(Space::new().width(18).height(2)).style(if active {
            theme::console_tab_underline
        } else {
            theme::transparent_bar
        });

        tabs = tabs.push(
            button(
                column![
                    text(tab.label())
                        .size(17)
                        .font(if active {
                            theme::FONT_SEMIBOLD
                        } else {
                            theme::FONT
                        })
                        .color(if active { theme::TEXT_1 } else { MUTED }),
                    underline,
                ]
                .spacing(5)
                .align_x(iced::Center),
            )
            .on_press(Message::ConsoleSelectTab(tab))
            .padding([8.0, 10.0])
            .style(move |style_theme, status| {
                theme::console_tab(style_theme, status, active, focused)
            }),
        );
    }

    let profile = container(
        row![
            container(
                text("J")
                    .size(13)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.85)),
            )
            .width(30)
            .height(30)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(theme::avatar),
            column![
                text("Jogador local")
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text("Perfil local").size(9).color(FAINT),
            ]
            .spacing(1),
        ]
        .spacing(9)
        .align_y(iced::Center),
    )
    .padding([4.0, 10.0])
    .style(theme::console_profile);

    let exit = button(
        text("Sair")
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.78)),
    )
    .on_press(Message::ExitBigPicture)
    .padding([8.0, 14.0])
    .style(theme::ghost_button);

    container(
        row![
            tabs,
            Space::new().width(Fill),
            row![
                connection(app),
                text(bp.clock())
                    .size(22)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                profile,
                exit,
            ]
            .spacing(22)
            .align_y(iced::Center),
        ]
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(92)
    .padding([0.0, GUTTER])
    .style(theme::console_topbar)
    .into()
}

fn connection(app: &App) -> Element<'_, Message> {
    let (color, label) = match &app.library.status {
        LibraryStatus::Ready => (theme::SUCCESS, "Local conectado"),
        LibraryStatus::Loading => (theme::WARNING, "Conectando"),
        LibraryStatus::Error(_) => (theme::DANGER, "Sem conexão"),
    };

    row![
        container(Space::new().width(8).height(8))
            .style(move |_style_theme| theme::console_status_dot(color)),
        text(label).size(11).color(MUTED),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn copy_area<'a>(app: &'a App, selected: Option<&'a Game>) -> Element<'a, Message> {
    let tab = app.big_picture.tab();

    if tab == ConsoleTab::News {
        return news_view(app);
    }

    if !tab.is_library() {
        return tab_placeholder(tab);
    }

    match (&app.library.status, selected) {
        (LibraryStatus::Loading, _) => empty_panel(
            "Carregando a biblioteca",
            "Aguardando `library:get` — o trilho aparece assim que os jogos chegarem.",
            Vec::new(),
        ),
        (LibraryStatus::Error(error), _) => {
            let actions = vec![
                button(text("Tentar novamente").size(13).font(theme::FONT_MEDIUM))
                    .on_press(Message::Reload)
                    .padding([10.0, 18.0])
                    .style(theme::primary_button)
                    .into(),
                exit_button(),
            ];
            container(
                column![
                    text("A biblioteca não carregou")
                        .size(24)
                        .font(theme::FONT_BOLD)
                        .color(theme::DANGER),
                    text(error.as_str())
                        .size(13)
                        .color(theme::TEXT_2)
                        .wrapping(Wrapping::WordOrGlyph),
                    row(actions).spacing(12),
                ]
                .spacing(14)
                .max_width(720),
            )
            .padding(24)
            .style(theme::error_panel)
            .into()
        }
        (_, Some(game)) => hero_copy(app, game),
        (LibraryStatus::Ready, None) => empty_panel(
            "Sua biblioteca está vazia",
            "Adicione jogos no Arcadia desktop e volte ao Big Picture.",
            vec![exit_button()],
        ),
    }
}

fn hero_copy<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let tag = game
        .genre
        .as_deref()
        .filter(|genre| !genre.is_empty())
        .unwrap_or("Em destaque")
        .to_uppercase();

    let mut details = row![
        text(game.playtime_label()).size(12).color(MUTED),
        text("·").size(12).color(FAINT),
        text(if game.installed {
            "Instalado"
        } else {
            "Não instalado"
        })
        .size(12)
        .color(if game.installed {
            theme::SUCCESS
        } else {
            theme::WARNING
        }),
        text("·").size(12).color(FAINT),
        text(game.launcher.clone()).size(12).color(MUTED),
    ]
    .spacing(10)
    .align_y(iced::Center);

    if let Some(year) = game.year.as_deref().filter(|year| !year.is_empty()) {
        details = details.push(text("·").size(12).color(FAINT));
        details = details.push(text(year).size(12).color(MUTED));
    }

    column![
        container(
            text(tag)
                .size(10)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_1)
        )
        .padding([4.0, 9.0])
        .style(theme::badge),
        text(game.title.clone())
            .size(50)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .wrapping(Wrapping::WordOrGlyph),
        text(game.metadata_line()).size(14).color(MUTED),
        details,
        text(
            app.big_picture
                .notice()
                .unwrap_or("◀ ▶ escolher · A abrir · ↓ detalhes · L1/R1 abas · Guide sair")
        )
        .size(11)
        .color(if app.big_picture.notice().is_some() {
            theme::WARNING
        } else {
            FAINT
        })
        .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(10)
    .max_width(780)
    .into()
}

fn tab_placeholder(tab: ConsoleTab) -> Element<'static, Message> {
    let (title, detail) = match tab {
        ConsoleTab::Home => (
            "A Home do console ainda não foi portada",
            "Nesta onda o Big Picture entrega a Biblioteca em tela cheia; destaques e continuar jogando chegam na próxima.",
        ),
        ConsoleTab::Stores => (
            "A Loja não tem backend nativo ainda",
            "A vitrine de lojas continua apenas no desktop/web. Use a aba Biblioteca para navegar pelos seus jogos.",
        ),
        ConsoleTab::News => (
            "Notícias",
            "A lista de notícias usa o feed `news:get`.",
        ),
        ConsoleTab::Library => ("Biblioteca", "Use o trilho para escolher um jogo."),
    };

    container(
        column![
            text(tab.label().to_uppercase())
                .size(11)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::ACCENT),
            text(title)
                .size(28)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(detail)
                .size(14)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
            row![
                button(
                    text("Ir para a Biblioteca")
                        .size(13)
                        .font(theme::FONT_MEDIUM)
                )
                .on_press(Message::ConsoleSelectTab(ConsoleTab::Library))
                .padding([10.0, 18.0])
                .style(theme::primary_button),
                exit_button(),
            ]
            .spacing(12),
        ]
        .spacing(14)
        .max_width(680),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn news_view(app: &App) -> Element<'_, Message> {
    let state = &app.console_news;
    let header = row![
        column![
            text("NOTÍCIAS")
                .size(11)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::ACCENT),
            text("Feed via `news:get`; os links aparecem como texto porque o shell nativo ainda não expõe `app:openExternal`.")
                .size(11)
                .color(FAINT)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(3),
        Space::new().width(Fill),
        button(text("Atualizar").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::ConsoleNewsReload)
            .padding([8.0, 14.0])
            .style(theme::ghost_button),
    ]
    .align_y(iced::Center);

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            super::loading_state("Carregando notícias…")
        }
        LoadStatus::Error(error) => container(
            column![
                text("As notícias não carregaram")
                    .size(20)
                    .font(theme::FONT_BOLD)
                    .color(theme::DANGER),
                text(error.as_str())
                    .size(13)
                    .color(theme::TEXT_2)
                    .wrapping(Wrapping::WordOrGlyph),
                button(text("Tentar novamente").size(12).font(theme::FONT_MEDIUM))
                    .on_press(Message::ConsoleNewsReload)
                    .padding([8.0, 14.0])
                    .style(theme::primary_button),
            ]
            .spacing(10)
            .max_width(680),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
        LoadStatus::Ready if state.items.is_empty() => super::empty_state(
            "Nenhuma notícia no feed",
            "O `news:get` respondeu sem itens. Isso pode ser falta de rede ou o filtro anti-oferta do feed; tente atualizar.",
        ),
        LoadStatus::Ready => {
            let mut list = column![].spacing(8);
            for (index, item) in state.items.iter().enumerate() {
                let selected = app.big_picture.news_index() == index;
                let mut content = column![
                    text(item.title.clone())
                        .size(15)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_1)
                        .wrapping(Wrapping::WordOrGlyph),
                    row![
                        text(item.source.clone())
                            .size(10)
                            .font(theme::FONT_SEMIBOLD)
                            .color(theme::ACCENT),
                        text("·").size(10).color(FAINT),
                        text(item.date_label()).size(10).color(MUTED),
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                    text(item.url.clone())
                        .size(10)
                        .color(FAINT)
                        .wrapping(Wrapping::WordOrGlyph),
                ]
                .spacing(4);

                if selected && !item.summary.is_empty() {
                    content = content.push(
                        text(shorten(&item.summary, 240))
                            .size(12)
                            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.74))
                            .wrapping(Wrapping::WordOrGlyph),
                    );
                }

                list = list.push(
                    button(content)
                        .on_press(Message::ConsoleNewsSelected(index))
                        .width(Fill)
                        .padding([10.0, 14.0])
                        .style(move |style_theme, status| {
                            theme::console_action(style_theme, status, selected)
                        }),
                );
            }

            scrollable(list)
                .height(Fill)
                .style(theme::console_scrollable)
                .into()
        }
    };

    container(column![header, body].spacing(14))
        .width(Fill)
        .height(Fill)
        .padding([20.0, 0.0])
        .into()
}

fn empty_panel<'a>(
    title: &'a str,
    detail: &'a str,
    actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![
        text(title)
            .size(26)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(detail)
            .size(14)
            .color(theme::TEXT_2)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(12);

    if !actions.is_empty() {
        content = content.push(row(actions).spacing(12));
    }

    container(content.max_width(620))
        .padding(22)
        .style(theme::console_panel)
        .into()
}

fn exit_button<'a>() -> Element<'a, Message> {
    button(
        text("Sair do Big Picture")
            .size(12)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::ExitBigPicture)
    .padding([10.0, 16.0])
    .style(theme::ghost_button)
    .into()
}

fn rail<'a>(app: &'a App, games: &[&'a Game]) -> Element<'a, Message> {
    let label = row![
        text("SEUS JOGOS")
            .size(10)
            .font(theme::FONT_SEMIBOLD)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.52)),
        Space::new().width(Fill),
        text("A abrir · B voltar · L1/R1 trocar aba")
            .size(10)
            .color(FAINT),
    ]
    .align_y(iced::Center);

    if games.is_empty() {
        return container(
            column![
                label,
                container(
                    text("O trilho aparece quando houver jogos na biblioteca.")
                        .size(12)
                        .color(FAINT),
                )
                .width(Fill)
                .padding([18.0, 4.0]),
            ]
            .spacing(6),
        )
        .width(Fill)
        .height(Length::Fixed(120.0))
        .padding(Padding {
            top: 0.0,
            right: GUTTER,
            bottom: 16.0,
            left: GUTTER,
        })
        .into();
    }

    let cards: Vec<Element<'a, Message>> = games
        .iter()
        .enumerate()
        .map(|(index, game)| card(app, index, game))
        .collect();

    let content = iced::widget::Row::with_children(cards)
        .spacing(RAIL_CARD_GAP)
        .padding(Padding {
            top: 6.0,
            right: GUTTER,
            bottom: 6.0,
            left: GUTTER,
        });

    let scroller = scrollable(content)
        .direction(Direction::Horizontal(
            Scrollbar::new().width(4).margin(0).scroller_width(4),
        ))
        .id(RAIL_ID)
        .height(Length::Fixed(RAIL_CARD_HEIGHT + 14.0))
        .style(theme::console_scrollable);

    container(column![label, scroller].spacing(4))
        .width(Fill)
        .height(Length::Fixed(RAIL_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 12.0,
            left: 0.0,
        })
        .into()
}

fn card<'a>(app: &'a App, index: usize, game: &'a Game) -> Element<'a, Message> {
    let selected = app.big_picture.rail_index() == index;
    let focused = selected && app.big_picture.zone() != ConsoleZone::Tabs;

    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(RAIL_COVER_HEIGHT)
            .content_fit(ContentFit::Cover)
            .border_radius(6)
            .into(),
        None => container(
            column![
                text(initial(&game.title))
                    .size(30)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.20)),
                text(ellipsize(&game.title, 16))
                    .size(11)
                    .color(FAINT)
                    .wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(8)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(RAIL_COVER_HEIGHT)
        .padding(10)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
        .into(),
    };

    let title_color = if focused {
        theme::TEXT_1
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.72)
    };

    button(
        column![
            container(cover)
                .width(Fill)
                .height(RAIL_COVER_HEIGHT)
                .clip(true),
            container(
                text(ellipsize(&game.title, 18))
                    .size(12)
                    .font(if selected {
                        theme::FONT_SEMIBOLD
                    } else {
                        theme::FONT_MEDIUM
                    })
                    .color(title_color)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .height(18)
            .clip(true),
            row![
                text(game.year.clone().unwrap_or_else(|| "—".to_string()))
                    .size(9)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::ACCENT),
                Space::new().width(Fill),
                text(game.playtime_label()).size(9).color(FAINT),
            ]
            .align_y(iced::Center),
        ]
        .spacing(4),
    )
    .on_press(Message::ConsoleSelectGame(index))
    .width(Length::Fixed(RAIL_CARD_WIDTH))
    .height(Length::Fixed(RAIL_CARD_HEIGHT))
    .padding([6.0, 6.0])
    .style(move |style_theme, status| theme::console_card(style_theme, status, selected, focused))
    .into()
}

fn overview<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let focused = |index: usize| bp.zone() == ConsoleZone::Overview && bp.action_index() == index;

    let back = button(
        row![
            text("‹").size(20).color(theme::TEXT_1),
            text("Voltar")
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::ConsoleCloseOverview)
    .padding([8.0, 14.0])
    .style(theme::ghost_button);

    let header = row![
        back,
        Space::new().width(Fill),
        text(game.title.clone()).size(13).color(MUTED),
        Space::new().width(Fill),
        text(bp.clock())
            .size(22)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
    ]
    .align_y(iced::Center);

    let running = app.launch.is_running(&game.id);
    let (play_glyph, play_label, play_message) = if app.launch.is_launching(&game.id) {
        ("▶", "Abrindo…".to_string(), None)
    } else if running {
        ("■", "Parar".to_string(), Some(Message::StopGame))
    } else if app.launch.is_stopping(&game.id) {
        ("■", "Parando…".to_string(), None)
    } else {
        ("▶", "Jogar".to_string(), Some(Message::ConsolePlay))
    };

    let play = button(
        row![
            text(play_glyph).size(12),
            text(play_label).size(16).font(theme::FONT_SEMIBOLD),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press_maybe(play_message)
    .padding([13.0, 26.0])
    .style(move |style_theme, status| {
        theme::console_play(style_theme, status, focused(ACTION_PLAY))
    });

    let trailer = button(text("Trailer").size(14).font(theme::FONT_MEDIUM))
        .on_press(Message::ConsoleTrailer)
        .padding([12.0, 20.0])
        .style(move |style_theme, status| {
            theme::console_action(style_theme, status, focused(ACTION_TRAILER))
        });

    let favorite = button(
        text(if game.favorite { "♥" } else { "♡" })
            .size(20)
            .color(if game.favorite { theme::TEXT_1 } else { MUTED }),
    )
    .on_press(Message::ConsoleFavorite)
    .width(48)
    .height(48)
    .padding(0)
    .style(move |style_theme, status| {
        theme::console_round(style_theme, status, focused(ACTION_FAVORITE))
    });

    let more = button(text("•••").size(13).color(theme::TEXT_1))
        .on_press(Message::ConsoleMore)
        .width(48)
        .height(48)
        .padding(0)
        .style(move |style_theme, status| {
            theme::console_round(style_theme, status, focused(ACTION_MORE))
        });

    let tag = game
        .genre
        .as_deref()
        .filter(|genre| !genre.is_empty())
        .unwrap_or("Jogo")
        .to_uppercase();

    let description = game
        .description
        .as_deref()
        .map(|description| shorten(description, 300))
        .unwrap_or_else(|| {
            "Sem descrição disponível para este jogo. Os metadados vêm da biblioteca local."
                .to_string()
        });

    let mut activity = column![
        container(
            text(tag)
                .size(10)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_1)
        )
        .padding([4.0, 9.0])
        .style(theme::badge),
        text(game.title.clone())
            .size(44)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .wrapping(Wrapping::WordOrGlyph),
        text(game.metadata_line()).size(14).color(MUTED),
        text(description)
            .size(14)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.76))
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(12)
    .width(Fill)
    .max_width(700);

    if let Some(notice) = bp.notice() {
        activity = activity.push(super::warning_banner(notice));
    }

    if let Some(error) = app.launch.error.as_deref() {
        activity = activity.push(super::action_error_banner(error, None));
    }

    activity = activity.push(
        row![
            text(game.playtime_label()).size(12).color(MUTED),
            text("·").size(12).color(FAINT),
            text(if game.installed {
                "Instalado"
            } else {
                "Instalar pelo desktop"
            })
            .size(12)
            .color(if game.installed {
                theme::SUCCESS
            } else {
                theme::WARNING
            }),
            text("·").size(12).color(FAINT),
            text(game.launcher.clone()).size(12).color(MUTED),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );

    activity = activity.push(
        row![play, trailer, favorite, more]
            .spacing(16)
            .align_y(iced::Center),
    );

    if running {
        activity = activity.push(
            text("Em execução — Parar encerra o processo.")
                .size(11)
                .color(theme::SUCCESS),
        );
    }

    activity = activity.push(
        text("◀ ▶ ações · A confirmar · B voltar · Guide sair do Big Picture")
            .size(10)
            .color(FAINT),
    );

    let trailer = console_trailer(app, game);

    container(
        column![
            header,
            Space::new().height(Fill),
            row![activity, Space::new().width(Fill), trailer,]
                .spacing(48)
                .align_y(iced::Bottom),
        ]
        .width(Fill)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding([28.0, GUTTER])
    .style(theme::console_overview)
    .into()
}

/// Frame do GStreamer (ou poster do jogo) + play/pause no painel do console.
fn console_trailer<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let media = &app.media;

    let surface: Element<'a, Message> = match media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(203)
            .content_fit(ContentFit::Contain)
            .into(),
        None => match app.covers.ready(game.cover_url.as_deref()) {
            Some(handle) => image(handle.clone())
                .width(Fill)
                .height(203)
                .content_fit(ContentFit::Cover)
                .border_radius(6)
                .into(),
            None => container(
                text(initial(&game.title))
                    .size(42)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
            )
            .width(Fill)
            .height(203)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::cover_placeholder)
            .into(),
        },
    };

    let notice = app
        .game_media
        .as_ref()
        .and_then(|game_media| game_media.notice.as_deref());
    let (status, status_color) = if let Some(notice) = notice {
        (notice.to_string(), theme::WARNING)
    } else if media.status().is_error() {
        (media.status().label().to_string(), theme::DANGER)
    } else if app.game_media.is_none() {
        ("Consultando o trailer…".to_string(), MUTED)
    } else {
        (media.status().label().to_string(), MUTED)
    };

    let mut toggle = button(
        text(if media.is_playing() {
            "Pausar"
        } else {
            "Reproduzir"
        })
        .size(11)
        .font(theme::FONT_MEDIUM),
    )
    .padding([6.0, 12.0])
    .style(theme::ghost_button);
    if media.can_control() {
        toggle = toggle.on_press(Message::MediaTogglePlayback);
    }

    let mut controls = row![toggle].spacing(10).align_y(iced::Center);
    let duration = media.duration_seconds();
    if duration > 0.5 {
        controls = controls.push(
            text(format!(
                "{} / {}",
                media::format_seconds(media.position_seconds()),
                media::format_seconds(duration)
            ))
            .size(10)
            .color(MUTED),
        );
    } else {
        controls = controls.push(
            text("sem duração ainda")
                .size(9)
                .color(FAINT),
        );
    }

    container(
        column![
            container(surface)
                .width(Fill)
                .height(203)
                .clip(true)
                .style(theme::cover_placeholder),
            controls,
            text(status)
                .size(10)
                .color(status_color)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(8),
    )
    .width(340)
    .padding(10)
    .style(theme::console_panel)
    .into()
}

/// Menu "•••" do painel: Abrir ficha, Favoritar/Desfavoritar e Trailer.
fn more_menu<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let bp = &app.big_picture;
    let focused = |index: usize| bp.menu_index() == index;

    let favorite_label = if game.favorite {
        "Remover dos favoritos"
    } else {
        "Adicionar aos favoritos"
    };
    let trailer_label = if app.media.is_playing() {
        "Pausar trailer"
    } else {
        "Reproduzir trailer"
    };
    let labels = ["Abrir ficha", favorite_label, trailer_label];
    let actions = [MENU_OPEN_DETAIL, MENU_FAVORITE, MENU_TRAILER];

    let mut items = column![].spacing(4);
    for index in 0..MENU_COUNT {
        let selected = focused(index);
        items = items.push(
            button(text(labels[index]).size(16).font(theme::FONT_MEDIUM))
                .on_press(Message::ConsoleMenuAction(actions[index]))
                .width(Fill)
                .padding([12.0, 16.0])
                .style(move |style_theme, status| {
                    theme::console_action(style_theme, status, selected)
                }),
        );
    }

    let panel = container(
        column![
            container(
                text(game.title.clone())
                    .size(15)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .padding([12.0, 16.0])
            .style(theme::console_topbar),
            container(items).padding([8.0, 10.0]),
            container(
                text("↑↓ escolher · A confirmar · B fechar")
                    .size(10)
                    .color(FAINT),
            )
            .width(Fill)
            .padding([8.0, 16.0]),
        ]
        .spacing(0),
    )
    .width(420)
    .style(theme::console_panel);

    let veil = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::console_bottom_wash),
    )
    .on_press(Message::ConsoleCloseMenu);

    let panel_layer = container(panel)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(veil)
        .push(panel_layer)
        .into()
}

fn art_url(game: &Game) -> Option<&str> {
    game.hero_url
        .as_deref()
        .or(game.cover_url.as_deref())
        .filter(|url| !url.is_empty())
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn shorten(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= limit {
        return flattened;
    }

    let mut short: String = flattened.chars().take(limit).collect();
    short.push('…');
    short
}
