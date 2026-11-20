//! Sidebar fixa de 212 px, espelhando a `Sidebar.tsx` do shell desktop:
//! wordmark, perfil local, navegação com ícones, lista de jogos com busca,
//! Big Picture e versão.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, row, scrollable, text, text_input, Column, Space,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, Message, Screen, Section};
use crate::theme;
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;

    let brand = container(
        text("ARCADIA")
            .size(19)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(Fill)
    .height(37)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(theme::sidebar_brand);

    let brand_line = container(Space::new().width(Fill).height(1)).style(theme::divider_accent);

    let profile = container(
        row![
            container(
                text(initial(i18n.t("native.profile.local")))
                    .size(14)
                    .font(theme::FONT_SEMIBOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
            )
            .width(36)
            .height(36)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(theme::avatar),
            text(i18n.t("native.profile.local"))
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([12.0, 20.0]);

    let mut nav = Column::new().spacing(2).padding([0.0, 8.0]);
    for section in Section::ALL {
        nav = nav.push(nav_item(app, section));
    }

    let games = app.library.visible_games(&app.sidebar_search);
    let games_header = container(
        row![
            button(icons::icon(Icon::ChevronDown, 12.0, 0.50))
                .on_press(Message::ToggleSidebarGames)
                .padding(0)
                .style(theme::plain_button),
            text(i18n.t("sidebar.jogos").to_uppercase())
                .size(11)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.50)),
            container(
                text(games.len().to_string())
                    .size(10)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.60)),
            )
            .padding([1.0, 5.0])
            .style(theme::badge),
            Space::new().width(Fill),
            icons::icon_button(Icon::Refresh, 14.0, 4.0, Message::Reload),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([8.0, 16.0]);

    let mut body = column![brand, brand_line, profile, nav].width(Fill);

    if app.sidebar_games_open {
        let games_search = container(
            text_input(i18n.t("library.buscar"), &app.sidebar_search)
                .on_input(Message::SidebarSearchChanged)
                .padding([4.0, 8.0])
                .size(11)
                .width(Fill)
                .style(theme::sidebar_search_input),
        )
        .width(Fill)
        .padding([4.0, 12.0]);

        let mut game_list = Column::new().spacing(1).padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 8.0,
            left: 8.0,
        });
        if games.is_empty() {
            game_list = game_list.push(
                container(
                    text(if app.library.games.is_empty() {
                        i18n.t("library.vazia")
                    } else {
                        i18n.t("library.vazio")
                    })
                    .size(11)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30))
                    .align_x(iced::Center),
                )
                .width(Fill)
                .padding([16.0, 4.0]),
            );
        } else {
            for game in &games {
                game_list = game_list.push(game_item(app, game));
            }
        }

        body = body
            .push(games_header)
            .push(games_search)
            .push(scrollable(game_list).height(Fill).style(theme::scrollable));
    } else {
        body = body.push(games_header).push(Space::new().height(Fill));
    }

    let big_picture = button(
        row![
            icons::icon(Icon::Monitor, 16.0, 0.75),
            text(i18n.t("sidebar.bigpicture"))
                .size(12)
                .font(theme::FONT_SEMIBOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.70)),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::EnterBigPicture)
    .width(Fill)
    .padding([9.0, 10.0])
    .style(theme::big_picture);

    let version = row![
        text(format!(
            "{} · {}",
            i18n.t("app.name"),
            i18n.t("sidebar.modo_desktop")
        ))
        .size(10)
        .color(theme::TEXT_3),
        Space::new().width(Fill),
        text(env!("CARGO_PKG_VERSION"))
            .size(10)
            .color(theme::TEXT_3),
    ]
    .align_y(iced::Center);

    let footer = container(column![big_picture, version].spacing(8))
        .width(Fill)
        .padding([10.0, 12.0]);

    container(body.push(footer).height(Fill))
        .width(theme::SIDEBAR_WIDTH)
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

fn nav_item(app: &App, section: Section) -> Element<'_, Message> {
    let active = is_active(app, section);
    let marker = container(Space::new().width(Length::Fixed(2.0)).height(20)).style(if active {
        theme::accent_bar
    } else {
        theme::transparent_bar
    });

    let label = text(section.label(&app.i18n)).size(13).color(if active {
        theme::TEXT_1
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.85)
    });

    button(
        row![
            marker,
            icons::icon(icon_for(section), 18.0, if active { 1.0 } else { 0.59 }),
            label,
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::Navigate(section))
    .width(Fill)
    .height(35)
    .padding([0.0, 4.0])
    .style(move |style_theme, status| theme::sidebar_item(style_theme, status, active))
    .into()
}

fn game_item<'a>(app: &'a App, game: &'a crate::backend::Game) -> Element<'a, Message> {
    let active = matches!(&app.screen, Screen::Game(id) if id == &game.id);
    let marker = container(Space::new().width(Length::Fixed(2.0)).height(16)).style(if active {
        theme::accent_bar
    } else {
        theme::transparent_bar
    });

    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(21)
            .height(21)
            .content_fit(ContentFit::Cover)
            .border_radius(2)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(10)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
        )
        .width(21)
        .height(21)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::game_icon_placeholder)
        .into(),
    };

    button(
        row![
            marker,
            cover,
            container(
                text(ellipsize(&game.title, 22))
                    .size(12)
                    .color(if active {
                        theme::TEXT_1
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.75)
                    })
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .clip(true),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenGame(game.id.clone()))
    .width(Fill)
    .padding([4.0, 6.0])
    .style(move |style_theme, status| theme::game_item(style_theme, status, active))
    .into()
}

fn icon_for(section: Section) -> Icon {
    match section {
        Section::Home => Icon::Home,
        Section::Library => Icon::Grid,
        Section::Stores => Icon::Store,
        Section::Components => Icon::Plugin,
        Section::Downloads => Icon::Download,
        Section::Sources => Icon::Link,
        Section::Friends => Icon::Users,
        Section::Settings => Icon::Gear,
        Section::BigPicture => Icon::Monitor,
    }
}

fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn is_active(app: &App, section: Section) -> bool {
    match &app.screen {
        Screen::Library | Screen::Game(_) => section == Section::Library,
        Screen::Section(current) => *current == section,
        Screen::BigPicture => section == Section::BigPicture,
    }
}
