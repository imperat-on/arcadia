//! Card de jogo compartilhado entre a Biblioteca e a Home.
//!
//! Mesmas proporções da web: capa 2:3 dentro de um card `minmax(170px, 1fr)`,
//! raio de 3 px do shell desktop, título de uma linha e horas abaixo.

use iced::border::Radius;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, image, row, text, Space, Stack};
use iced::{Color, ContentFit, Element, Fill, Length};

use crate::app::Message;
use crate::backend::Game;
use crate::covers::CoverCache;
use crate::i18n::I18n;
use crate::theme;
use crate::views::icons::{self, Icon};

const TITLE_COLOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.85);

pub fn game_card<'a>(
    i18n: &'a I18n,
    game: &'a Game,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
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

    let cover = if game.favorite {
        Stack::new()
            .width(Fill)
            .height(Fill)
            .push(cover)
            .push(
                container(
                    container(icons::icon(Icon::Heart, 13.0, 1.0))
                        .width(24)
                        .height(24)
                        .align_x(iced::Center)
                        .align_y(iced::Center)
                        .style(theme::heart_badge),
                )
                .width(Fill)
                .height(Fill)
                .align_x(iced::Right)
                .align_y(iced::Top)
                .padding(8),
            )
            .into()
    } else {
        cover
    };

    let info = container(
        column![
            container(
                text(ellipsize(&game.title, 20))
                    .size(14)
                    .font(theme::FONT_MEDIUM)
                    .color(TITLE_COLOR)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .height(20)
            .clip(true),
            row![
                text(game.playtime_label_i18n(i18n))
                    .size(11)
                    .color(theme::TEXT_3),
                Space::new().width(Fill),
            ]
            .align_y(iced::Center),
        ]
        .spacing(2),
    )
    .width(Fill)
    .height(56)
    .padding([9.0, 12.0]);

    button(column![container(cover).width(Fill).height(Fill).clip(true), info].height(Fill))
        .on_press(Message::OpenGame(game.id.clone()))
        .width(Fill)
        .height(Fill)
        .padding(0)
        .style(theme::card)
        .into()
}

fn ellipsize(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut short: String = text.chars().take(limit.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn cover_placeholder(game: &Game) -> Element<'_, Message> {
    container(
        text(game.title.as_str())
            .size(12)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30))
            .wrapping(Wrapping::WordOrGlyph)
            .align_x(iced::Center),
    )
    .width(Fill)
    .height(Fill)
    .padding([12.0, 10.0])
    .center_x(Fill)
    .center_y(Fill)
    .style(theme::cover_placeholder)
    .into()
}

pub fn section_heading<'a>(
    title: &'a str,
    count: Option<usize>,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut heading = row![text(title.to_uppercase())
        .size(19)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1),]
    .spacing(10)
    .align_y(iced::Center);

    if let Some(count) = count {
        heading = heading.push(
            container(
                text(count.to_string())
                    .size(12)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.70)),
            )
            .padding([3.0, 8.0])
            .style(theme::badge),
        );
    }

    heading = heading.push(Space::new().width(Length::Fill));

    if let Some(action) = action {
        heading = heading.push(action);
    }

    heading.into()
}

pub fn grid<'a>(cards: Vec<Element<'a, Message>>, aspect: (f32, f32)) -> Element<'a, Message> {
    iced::widget::Grid::with_children(cards)
        .fluid(theme::CARD_WIDTH)
        .spacing(theme::CARD_GAP)
        .height(iced::widget::grid::aspect_ratio(aspect.0, aspect.1))
        .into()
}

/// Card horizontal usado nos trilhos da Home (capa pequena + texto).
pub fn compact_card<'a>(
    i18n: &'a I18n,
    game: &'a Game,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(46)
            .height(62)
            .content_fit(ContentFit::Cover)
            .border_radius(2)
            .into(),
        None => container(
            text(
                game.title
                    .chars()
                    .next()
                    .map(|character| character.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_string()),
            )
            .size(16)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_2),
        )
        .width(46)
        .height(62)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::game_icon_placeholder)
        .into(),
    };

    button(
        row![
            cover,
            column![
                container(
                    text(ellipsize(&game.title, 24))
                        .size(13)
                        .font(theme::FONT_MEDIUM)
                        .color(TITLE_COLOR)
                        .wrapping(Wrapping::None),
                )
                .width(Fill)
                .height(18)
                .clip(true),
                text(game.playtime_label_i18n(i18n))
                    .size(11)
                    .color(theme::TEXT_3),
            ]
            .spacing(3)
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenGame(game.id.clone()))
    .width(Length::Fixed(250.0))
    .padding(8)
    .style(theme::home_card)
    .into()
}
