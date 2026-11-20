//! Página do jogo: hero/backdrop, metadados, descrição e áreas reservadas.

use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{ContentFit, Element, Fill};

use crate::app::Message;
use crate::backend::Game;
use crate::covers::CoverCache;
use crate::input;
use crate::media;
use crate::theme;

pub fn view<'a>(game: &'a Game, covers: &'a CoverCache) -> Element<'a, Message> {
    let hero_url = game.hero_url.as_deref().or(game.cover_url.as_deref());

    let hero: Element<'a, Message> = match covers.ready(hero_url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let back = button(
        row![
            text("‹").size(22).color(theme::TEXT_1),
            text("Biblioteca")
                .size(12)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::BackToLibrary)
    .padding([6.0, 12.0])
    .style(theme::ghost_button);

    let meta = column![
        text(game.title.as_str())
            .size(40)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(game.metadata_line())
            .size(14)
            .color(theme::TEXT_STRONG),
        row![
            text(game.playtime_label()).size(12).color(theme::TEXT_2),
            text("·").size(12).color(theme::TEXT_3),
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
        ]
        .spacing(8),
    ]
    .spacing(6);

    let overlay = container(
        column![back, Space::new().height(Fill), meta,]
            .height(Fill)
            .width(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding(theme::PAGE_PADDING)
    .style(theme::hero_overlay);

    let hero = Stack::new()
        .width(Fill)
        .height(theme::HERO_HEIGHT)
        .push(hero)
        .push(overlay);

    let description_panel = container(
        column![
            text("Sobre o jogo")
                .size(16)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text("Descrição em texto puro — o HTML da loja é limpo no backend.")
                .size(11)
                .color(theme::TEXT_3),
            scrollable(
                container(
                    text(
                        game.description
                            .as_deref()
                            .unwrap_or("Sem descrição disponível para este jogo.")
                    )
                    .size(13)
                    .color(theme::TEXT_2),
                )
                .padding([4.0, 0.0]),
            )
            .height(Fill)
            .style(theme::scrollable),
        ]
        .spacing(theme::SPACE_3),
    )
    .width(iced::FillPortion(2))
    .height(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel);

    let media_player = media::MediaPlayer::new();
    let trailer_body = if media_player.is_available() {
        "Player pronto."
    } else {
        media_player.status_label()
    };

    let mut gamepad = input::GamepadInput::new();
    let gamepad_state = gamepad.poll();
    let gamepad_body = if gamepad_state.connected {
        "Controle conectado."
    } else {
        input::GAMEPAD_PLACEHOLDER
    };

    let extras_panel = container(
        column![
            text("Mídia e extras")
                .size(16)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            placeholder_card("Trailer", trailer_body),
            placeholder_card(
                "Conquistas",
                "A integração com Steam/RetroAchievements entra numa próxima onda.",
            ),
            placeholder_card("Controle", gamepad_body),
        ]
        .spacing(theme::SPACE_3),
    )
    .width(iced::FillPortion(1))
    .height(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel);

    let body = row![description_panel, extras_panel]
        .spacing(theme::SPACE_4)
        .padding([theme::SPACE_4, theme::PAGE_PADDING])
        .width(Fill)
        .height(Fill);

    column![hero, body].width(Fill).height(Fill).into()
}

fn placeholder_card<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(message).size(11).color(theme::TEXT_3),
        ]
        .spacing(4),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}
