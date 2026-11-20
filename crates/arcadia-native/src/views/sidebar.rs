//! Sidebar fixa (~240 px), espelhando a navegação da UI web.

use iced::widget::{button, column, container, row, scrollable, text, Column, Space};
use iced::{Element, Fill, Length};

use crate::app::{App, Message, Screen, Section};
use crate::theme;

pub fn view(app: &App) -> Element<'_, Message> {
    let logo = container(
        column![
            text("ARCADIA")
                .size(22)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text("NATIVE SHELL")
                .size(9)
                .font(theme::FONT_MEDIUM)
                .color(theme::ACCENT),
        ]
        .spacing(2)
        .align_x(iced::Center),
    )
    .width(Fill)
    .padding([18.0, 16.0])
    .center_x(Fill)
    .style(theme::sidebar_brand);

    let mut nav = Column::new().spacing(theme::SPACE_1).padding([0.0, 10.0]);
    for section in Section::ALL {
        nav = nav.push(nav_item(app, section));
    }

    let games = app.library.visible_all();
    let games_header = container(
        row![
            text("JOGOS")
                .size(10)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_3),
            Space::new().width(Fill),
            text(format!("{}", games.len()))
                .size(10)
                .color(theme::TEXT_3),
        ]
        .align_y(iced::Center),
    )
    .padding([8.0, 14.0]);

    let mut game_list = Column::new().spacing(2).padding([0.0, 8.0]);
    if games.is_empty() {
        game_list = game_list.push(
            text("Nenhum jogo na biblioteca")
                .size(11)
                .color(theme::TEXT_3),
        );
    } else {
        for game in games {
            game_list = game_list.push(game_item(app, game));
        }
    }

    let game_scroll = scrollable(game_list).height(Fill).style(theme::scrollable);

    let footer = container(
        column![
            button(
                text("Big Picture")
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_2)
            )
            .on_press(Message::Navigate(Section::BigPicture))
            .width(Fill)
            .padding([10.0, 12.0])
            .style(theme::big_picture),
            row![
                text("Arcadia").size(10).color(theme::TEXT_3),
                Space::new().width(Fill),
                text(env!("CARGO_PKG_VERSION"))
                    .size(10)
                    .color(theme::TEXT_3),
            ]
            .padding([4.0, 2.0]),
        ]
        .spacing(4),
    )
    .padding([10.0, 12.0]);

    container(column![logo, nav, games_header, game_scroll, footer].height(Fill))
        .width(theme::SIDEBAR_WIDTH)
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

fn nav_item(app: &App, section: Section) -> Element<'_, Message> {
    let active = is_active(app, section);
    let marker = container(Space::new().width(Length::Fixed(2.0)).height(14)).style(if active {
        theme::accent_bar
    } else {
        theme::transparent_bar
    });

    let label = text(section.label())
        .size(13)
        .font(if active {
            theme::FONT_MEDIUM
        } else {
            theme::FONT
        })
        .color(if active { theme::TEXT_1 } else { theme::TEXT_2 });

    button(row![marker, label].spacing(10).align_y(iced::Center))
        .on_press(Message::Navigate(section))
        .width(Fill)
        .padding([8.0, 10.0])
        .style(move |style_theme, status| theme::sidebar_item(style_theme, status, active))
        .into()
}

fn game_item<'a>(app: &'a App, game: &'a crate::backend::Game) -> Element<'a, Message> {
    let active = matches!(&app.screen, Screen::Game(id) if id == &game.id);
    let marker = container(Space::new().width(Length::Fixed(2.0)).height(12)).style(if active {
        theme::accent_bar
    } else {
        theme::transparent_bar
    });

    button(
        row![
            marker,
            text(game.title.as_str()).size(12).color(if active {
                theme::TEXT_1
            } else {
                theme::TEXT_2
            }),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenGame(game.id.clone()))
    .width(Fill)
    .padding([7.0, 8.0])
    .style(move |style_theme, status| theme::game_item(style_theme, status, active))
    .into()
}

fn is_active(app: &App, section: Section) -> bool {
    match &app.screen {
        Screen::Library | Screen::Game(_) => section == Section::Library,
        Screen::Section(current) => *current == section,
    }
}
