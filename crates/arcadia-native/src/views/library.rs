//! Biblioteca: grade responsiva de capas 2:3 com título e horas.

use iced::border::Radius;
use iced::widget::{
    button, column, container, image, row, scrollable, text, text_input, Grid, Space,
};
use iced::{ContentFit, Element, Fill};

use crate::app::{App, LibraryStatus, Message};
use crate::backend::Game;
use crate::covers::CoverCache;
use crate::theme;

pub fn view(app: &App) -> Element<'_, Message> {
    let games = app.library.visible_games(&app.search);

    let title = row![
        text("Biblioteca")
            .size(30)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        container(
            text(format!("{}", games.len()))
                .size(12)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_2)
        )
        .padding([3.0, 8.0])
        .style(theme::badge),
    ]
    .spacing(10)
    .align_y(iced::Center);

    let heading = column![
        title,
        text("Sua coleção real, carregada pelo dispatcher Rust do Arcadia.")
            .size(13)
            .color(theme::TEXT_2),
    ]
    .spacing(4)
    .width(Fill);

    let search = text_input("Buscar na biblioteca...", &app.search)
        .on_input(Message::SearchChanged)
        .padding([10.0, 14.0])
        .size(13)
        .width(340)
        .style(theme::search_input);

    let header = container(
        row![heading, search]
            .spacing(theme::SPACE_4)
            .align_y(iced::Center),
    )
    .width(Fill)
    .padding([24.0, theme::PAGE_PADDING]);

    let body: Element<'_, Message> = match &app.library.status {
        LibraryStatus::Loading => centered_message("Carregando biblioteca..."),
        LibraryStatus::Error(error) => error_state(error),
        LibraryStatus::Ready if app.library.games.is_empty() => {
            centered_message("Sua biblioteca está vazia. Adicione jogos no Arcadia web/desktop.")
        }
        LibraryStatus::Ready if games.is_empty() => {
            centered_message("Nenhum jogo corresponde à busca.")
        }
        LibraryStatus::Ready => {
            let cards: Vec<Element<'_, Message>> = games
                .into_iter()
                .map(|game| card(game, &app.covers))
                .collect();

            let grid = Grid::with_children(cards)
                .fluid(theme::CARD_WIDTH)
                .spacing(theme::CARD_GAP)
                .height(iced::widget::grid::aspect_ratio(2.0, 3.6));

            scrollable(
                container(grid)
                    .width(Fill)
                    .padding([4.0, theme::PAGE_PADDING]),
            )
            .height(Fill)
            .style(theme::scrollable)
            .into()
        }
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn card<'a>(game: &'a Game, covers: &'a CoverCache) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(Radius {
                top_left: theme::CARD_RADIUS,
                top_right: theme::CARD_RADIUS,
                ..Radius::default()
            })
            .into(),
        None => cover_placeholder(game),
    };

    let info = container(
        column![
            text(game.title.as_str())
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(game.playtime_label()).size(11).color(theme::TEXT_3),
        ]
        .spacing(2),
    )
    .width(Fill)
    .padding([8.0, 10.0]);

    let content = column![container(cover).width(Fill).height(Fill).clip(true), info,]
        .width(Fill)
        .height(Fill);

    button(content)
        .on_press(Message::OpenGame(game.id.clone()))
        .width(Fill)
        .height(Fill)
        .padding(0)
        .style(theme::card)
        .into()
}

fn cover_placeholder(game: &Game) -> Element<'_, Message> {
    let initial = game
        .title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    container(
        column![
            text(initial)
                .size(42)
                .font(theme::FONT_BOLD)
                .color(theme::ACCENT),
            text("SEM CAPA")
                .size(9)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_3),
        ]
        .spacing(6)
        .align_x(iced::Center),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .style(theme::cover_placeholder)
    .into()
}

fn centered_message(message: &str) -> Element<'_, Message> {
    container(text(message).size(14).color(theme::TEXT_2))
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .padding(theme::PAGE_PADDING)
        .into()
}

fn error_state(error: &str) -> Element<'_, Message> {
    container(
        column![
            text("Não foi possível carregar a biblioteca")
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::DANGER),
            text(error).size(13).color(theme::TEXT_2),
            row![
                button(text("Tentar novamente").size(13).font(theme::FONT_MEDIUM))
                    .on_press(Message::Reload)
                    .padding([10.0, 18.0])
                    .style(theme::primary_button),
                Space::new().width(theme::SPACE_2),
            ],
        ]
        .spacing(theme::SPACE_3),
    )
    .width(Fill)
    .max_width(720)
    .padding(theme::SPACE_5)
    .style(theme::error_panel)
    .into()
}
