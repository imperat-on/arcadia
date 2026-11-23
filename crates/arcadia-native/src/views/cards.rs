//! Card de jogo compartilhado entre a Biblioteca e a Home.
//!
//! Mesmas proporções da web: capa 2:3 dentro de um card `minmax(170px, 1fr)`,
//! raio de 3 px do shell desktop, título de uma linha e horas abaixo.

#[path = "hover_motion.rs"]
pub(super) mod hover_motion;

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
    let portrait = game.library_portrait_url();
    let art = portrait
        .as_deref()
        // Steam's recovery path can return a banner for a missing portrait.
        .filter(|url| covers.size(Some(url)).is_some_and(|(width, height)| height > width))
        .or(game.cover_url.as_deref());
    let fit = if covers.size(art).is_some_and(|(width, height)| height > width) {
        ContentFit::Cover
    } else {
        ContentFit::Contain
    };
    let cover: Element<'a, Message> = match covers.ready(art) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(fit)
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
                    .size(16)
                    .font(theme::FONT_MEDIUM)
                    .color(TITLE_COLOR)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .height(20)
            .clip(true),
            row![
                text(game.playtime_label_i18n(i18n))
                    .size(theme::TEXT_CAPTION)
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

    iced::widget::mouse_area(hover_motion::wrap(
        button(column![container(cover).width(Fill).height(Fill).clip(true), info].height(Fill))
            .on_press(Message::OpenGame(game.id.clone()))
            .width(Fill)
            .height(Fill)
            .padding(0)
            .style(theme::card),
        hover_motion::Kind::Card,
    )).on_enter(Message::PrefetchGame(game.id.clone())).into()
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
            .size(theme::TEXT_BODY)
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

/// Cabeçalho de seção.
///
/// Recebe `impl Into<String>` em vez de `&'a str`: quem chama quase sempre tem
/// um título montado na hora (`format!`, i18n com argumento, nome de
/// plataforma) e um `&str` emprestado obrigaria o `String` a viver até o fim
/// da função que monta a tela, o que não é possível quando o título nasce
/// dentro de um laço.
pub fn section_heading<'a>(
    title: impl Into<String>,
    count: Option<usize>,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let title = title.into();
    let mut heading = row![text(title.to_uppercase())
        .size(20)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1),]
    .spacing(10)
    .align_y(iced::Center);

    if let Some(count) = count {
        heading = heading.push(
            container(
                text(count.to_string())
                    .size(theme::TEXT_BODY)
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
