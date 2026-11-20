//! Página do jogo: hero/backdrop, metadados, descrição e trailer GStreamer.

use iced::widget::{
    button, column, container, image, row, scrollable, slider, text, Space, Stack,
};
use iced::{ContentFit, Element, Fill};

use crate::app::Message;
use crate::backend::{Game, GameMedia};
use crate::covers::CoverCache;
use crate::input;
use crate::media::{self, MediaPlayer, MediaStatus};
use crate::theme;

pub fn view<'a>(
    game: &'a Game,
    covers: &'a CoverCache,
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
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

    let trailer_body = trailer_card(media, game_media);

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
            trailer_body,
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

/// Card "Trailer": frame RGBA do GStreamer, play/pause, seek e estado.
fn trailer_card<'a>(
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
    let surface: Element<'a, Message> = match media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Contain)
            .into(),
        None => missing_frame(media, game_media),
    };

    let surface = container(surface)
        .width(Fill)
        .height(180)
        .style(theme::cover_placeholder);

    let duration = media.duration_seconds();
    let position = media.position_seconds();
    let label = if media.is_playing() {
        "Pausar"
    } else {
        "Reproduzir"
    };
    let mut toggle = button(text(label).size(11).font(theme::FONT_MEDIUM))
        .padding([6, 12])
        .style(theme::ghost_button);
    if media.can_control() {
        toggle = toggle.on_press(Message::MediaTogglePlayback);
    }

    let mut controls = row![toggle].spacing(theme::SPACE_2).align_y(iced::Center);
    if duration > 0.5 {
        controls = controls.push(
            slider(0.0..=duration as f32, position as f32, |seconds| {
                Message::MediaSeek(f64::from(seconds))
            })
            .step(0.1_f32)
            .width(Fill),
        );
    } else {
        controls = controls.push(
            text("aguardando a duração do trailer…")
                .size(9)
                .color(theme::TEXT_3),
        );
    }

    let status_color = if media.status().is_error() {
        theme::DANGER
    } else {
        theme::TEXT_3
    };

    let timing = media
        .first_frame_ms()
        .map(|elapsed| format!("  ·  1º frame em {elapsed} ms"))
        .unwrap_or_default();

    let body = column![
        row![
            text("Trailer")
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(source_label(media.uri()))
                .size(9)
                .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
        surface,
        controls,
        row![
            text(format!(
                "{} / {}{timing}",
                media::format_seconds(position),
                media::format_seconds(duration)
            ))
            .size(10)
            .color(theme::TEXT_2),
            Space::new().width(Fill),
            text(short_status(media.status()))
                .size(10)
                .color(status_color),
        ]
        .align_y(iced::Center),
        text(media::AUDIO_NOTE).size(9).color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_2);

    // Com o último frame ainda na tela, o detalhe do erro precisa de uma linha
    // própria (a área do vídeo só mostra o placeholder quando não há frame).
    let body = match (media.texture().is_some(), media.status()) {
        (true, MediaStatus::Error(message)) => body.push(
            text(shorten(message, 180))
                .size(9)
                .color(theme::DANGER),
        ),
        _ => body,
    };

    container(body)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

/// Área do vídeo antes do primeiro frame: carregando, erro, `info:null` etc.
fn missing_frame<'a>(
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
    let (title, detail, color) = if let MediaStatus::Error(message) = media.status() {
        (
            shorten(message, 180),
            "O trailer não pôde ser aberto.".to_string(),
            theme::DANGER,
        )
    } else if let Some(notice) = game_media.and_then(|info| info.notice.as_deref()) {
        (
            shorten(notice, 140),
            "Nenhum trailer será reproduzido.".to_string(),
            theme::WARNING,
        )
    } else if game_media.is_none() {
        (
            "Consultando a loja…".to_string(),
            "game:sysinfo · hls → mp4 → webm".to_string(),
            theme::TEXT_2,
        )
    } else {
        (
            media.status().label().to_string(),
            "Aguardando o primeiro frame do GStreamer.".to_string(),
            theme::TEXT_2,
        )
    };

    container(
        column![
            text(title).size(11).color(color),
            text(detail).size(9).color(theme::TEXT_3),
        ]
        .spacing(4)
        .align_x(iced::Center),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .padding(theme::SPACE_2)
    .into()
}

/// Rótulo curto na linha de controles; o erro completo fica na área do vídeo.
fn short_status(status: &MediaStatus) -> &'static str {
    match status {
        MediaStatus::Idle => "sem trailer",
        MediaStatus::Loading => "carregando",
        MediaStatus::Playing => "tocando",
        MediaStatus::Paused => "pausado",
        MediaStatus::Ended => "fim",
        MediaStatus::Error(_) => "erro",
    }
}

/// Origem escolhida pelo wrapper (`hls` tem prioridade sobre `mp4`/`webm`).
fn source_label(uri: Option<&str>) -> &'static str {
    match uri {
        Some(uri) if uri.to_ascii_lowercase().contains(".m3u8") => "HLS · GStreamer",
        Some(_) => "MP4/WebM · GStreamer",
        None => "GStreamer",
    }
}

fn shorten(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}
