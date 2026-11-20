//! Página do jogo: hero/backdrop, metadados, descrição e trailer GStreamer.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, opaque, row, scrollable, slider, text, text_input, Space,
    Stack,
};
use iced::{ContentFit, Element, Fill};

use crate::app::{GameLaunchState, LoadStatus, Message, TrailerPickerState};
use crate::backend::{Game, GameMedia, TrailerResult};
use crate::covers::CoverCache;
use crate::input;
use crate::media::{self, MediaPlayer, MediaStatus};
use crate::theme;

pub fn view<'a>(
    game: &'a Game,
    covers: &'a CoverCache,
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
    launch: &'a GameLaunchState,
    trailer_picker: &'a TrailerPickerState,
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

    let mut meta = column![
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

    meta = meta.push(launch_controls(game, launch));
    if let Some(error) = launch.error.as_deref() {
        meta = meta.push(super::action_error_banner(error, None));
    }

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

    let page = column![hero, body].width(Fill).height(Fill);

    if trailer_picker.open {
        Stack::new()
            .width(Fill)
            .height(Fill)
            .push(page)
            .push(trailer_picker_overlay(trailer_picker, covers))
            .into()
    } else {
        page.into()
    }
}

/// Botão Jogar/Parar da ficha, com o estado da sessão ao lado.
fn launch_controls<'a>(game: &'a Game, launch: &'a GameLaunchState) -> Element<'a, Message> {
    let running = launch.is_running(&game.id);
    let (label, message) = if !game.installed {
        ("Instale para jogar".to_string(), None)
    } else if launch.is_launching(&game.id) {
        ("Abrindo…".to_string(), None)
    } else if running {
        ("■  Parar".to_string(), Some(Message::StopGame))
    } else if launch.is_stopping(&game.id) {
        ("Parando…".to_string(), None)
    } else if launch.busy_with_other(&game.id) {
        ("Jogar".to_string(), None)
    } else {
        (
            "▶  Jogar".to_string(),
            Some(Message::LaunchGame(game.id.clone())),
        )
    };

    let play_style: fn(&iced::Theme, button::Status) -> button::Style = if running {
        theme::ghost_button
    } else {
        theme::primary_button
    };

    let play = button(text(label).size(13).font(theme::FONT_SEMIBOLD))
        .on_press_maybe(message)
        .padding([10.0, 22.0])
        .style(play_style);

    let status: Element<'a, Message> = if running {
        text("Em execução — Parar encerra o processo.")
            .size(11)
            .color(theme::SUCCESS)
            .into()
    } else if launch.is_launching(&game.id) {
        text("Aguardando o processo do jogo…")
            .size(11)
            .color(theme::TEXT_2)
            .into()
    } else if launch.is_stopping(&game.id) {
        text("Encerrando o processo…")
            .size(11)
            .color(theme::TEXT_2)
            .into()
    } else if launch.busy_with_other(&game.id) {
        text("Feche o jogo em execução antes de abrir este.")
            .size(11)
            .color(theme::TEXT_3)
            .into()
    } else {
        Space::new().width(0).into()
    };

    row![play, status]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center)
        .into()
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
            Space::new().width(Fill),
            button(text("Escolher trailer").size(10).font(theme::FONT_MEDIUM))
                .on_press(Message::OpenTrailerPicker)
                .padding([5.0, 10.0])
                .style(theme::ghost_button),
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
        Some(uri) if uri.starts_with("file://") || !uri.contains("://") => {
            "Arquivo local · GStreamer"
        }
        Some(uri) if uri.to_ascii_lowercase().contains(".m3u8") => "HLS · GStreamer",
        Some(_) => "MP4/WebM · GStreamer",
        None => "GStreamer",
    }
}

// ── Seletor de trailers (overlay) ──────────────────────────────────────────

/// Véu + cartão central do seletor; o player da ficha continua montado atrás.
fn trailer_picker_overlay<'a>(
    picker: &'a TrailerPickerState,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let header = row![
        column![
            text("Escolher trailer")
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(format!("{} — vídeos do YouTube", picker.game_title))
                .size(11)
                .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        button(text("Fechar").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::CloseTrailerPicker)
            .padding([7.0, 14.0])
            .style(theme::ghost_button),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    let search = row![
        text_input("Buscar trailer no YouTube...", picker.query.as_str())
            .on_input(Message::TrailerQueryChanged)
            .on_submit(Message::TrailerSearchSubmitted)
            .padding([9.0, 12.0])
            .size(12)
            .width(Fill)
            .style(theme::search_input),
        button(text("Buscar").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::TrailerSearchSubmitted)
            .padding([9.0, 16.0])
            .style(theme::primary_button),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    let mut content = column![header, search].spacing(theme::SPACE_4);
    if let Some(path) = picker.existing_path.as_deref() {
        content = content.push(existing_trailer_panel(path));
    }
    if let Some(error) = picker.download_error.as_deref() {
        content = content.push(super::action_error_banner(error, None));
    }
    if let Some(video) = picker.downloading.as_ref() {
        content = content.push(downloading_panel(video, covers));
    } else {
        content = content.push(results_panel(picker, covers));
    }

    let card = container(content)
        .width(Fill)
        .max_width(840.0)
        .max_height(700.0)
        .padding(theme::SPACE_5)
        .style(theme::panel);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::scrim),
        )
        .push(opaque(
            container(card)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .padding(theme::SPACE_5),
        ))
        .into()
}

fn placeholder_panel<'a>(message: &'a str, color: iced::Color) -> Element<'a, Message> {
    container(text(message).size(12).color(color))
        .width(Fill)
        .height(320)
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn results_panel<'a>(
    picker: &'a TrailerPickerState,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    match &picker.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            placeholder_panel("Buscando vídeos no YouTube…", theme::TEXT_2)
        }
        LoadStatus::Error(error) => container(
            column![
                text("Falha ao buscar trailers")
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::DANGER),
                text(shorten(error, 260)).size(11).color(theme::TEXT_2),
                button(text("Tentar novamente").size(12).font(theme::FONT_MEDIUM))
                    .on_press(Message::TrailerSearchSubmitted)
                    .padding([7.0, 13.0])
                    .style(theme::ghost_button),
            ]
            .spacing(theme::SPACE_2)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(320)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
        LoadStatus::Ready if picker.results.is_empty() => {
            placeholder_panel("Nenhum vídeo encontrado para esta busca.", theme::TEXT_2)
        }
        LoadStatus::Ready => scrollable(
            column(
                picker
                    .results
                    .iter()
                    .map(|result| trailer_result_row(result, covers))
                    .collect::<Vec<_>>(),
            )
            .spacing(theme::SPACE_2)
            .width(Fill),
        )
        .height(320)
        .style(theme::scrollable)
        .into(),
    }
}

fn trailer_thumbnail<'a>(
    video: &'a TrailerResult,
    covers: &'a CoverCache,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    match covers.ready(video.thumbnail.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(width)
            .height(height)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(if video.thumbnail.is_some() {
                "carregando…"
            } else {
                "sem prévia"
            })
            .size(9)
            .color(theme::TEXT_3),
        )
        .width(width)
        .height(height)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
        .into(),
    }
}

fn trailer_result_row<'a>(
    video: &'a TrailerResult,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let channel = if video.channel.trim().is_empty() {
        "Canal não informado"
    } else {
        video.channel.as_str()
    };
    let mut meta = row![text(channel).size(10).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    button(
        row![
            trailer_thumbnail(video, covers, 160.0, 90.0),
            column![
                text(video.title.as_str())
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text("Clique para baixar e abrir no player")
                    .size(9)
                    .color(theme::TEXT_3),
            ]
            .spacing(5)
            .width(Fill),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .on_press(Message::TrailerDownloadRequested(video.clone()))
    .width(Fill)
    .padding(theme::SPACE_2)
    .style(theme::card)
    .into()
}

fn downloading_panel<'a>(
    video: &'a TrailerResult,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let channel = if video.channel.trim().is_empty() {
        "Canal não informado"
    } else {
        video.channel.as_str()
    };
    let mut meta = row![text(channel).size(10).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    container(
        row![
            trailer_thumbnail(video, covers, 200.0, 112.0),
            column![
                text("Baixando…")
                    .size(14)
                    .font(theme::FONT_BOLD)
                    .color(theme::ACCENT),
                text(video.title.as_str())
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text("O yt-dlp baixa e remuxa o vídeo; isso pode levar alguns minutos.")
                    .size(9)
                    .color(theme::TEXT_3),
            ]
            .spacing(5)
            .width(Fill),
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

fn existing_trailer_panel(path: &str) -> Element<'_, Message> {
    container(
        row![
            column![
                text("Trailer baixado disponível")
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text(shorten(path, 110)).size(9).color(theme::TEXT_3),
            ]
            .spacing(3)
            .width(Fill),
            button(
                text("Abrir trailer baixado")
                    .size(11)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::TrailerOpenDownloaded)
            .padding([7.0, 12.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
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
