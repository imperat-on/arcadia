//! Casca cinemática compartilhada pela ficha da biblioteca e da loja.
//!
//! O desenho segue a `StoreGamePage.tsx`: backdrop largo, capa retrato
//! flutuante, título grande com chips, botão voltar fixo em vidro e corpo em
//! duas colunas. A página rola por inteiro, mas o voltar fica sempre acessível.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, image, row, scrollable, slider, text, Space, Stack};
use iced::{Border, Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{Message, RequirementsTab};
use crate::backend::{strip_html, Movie, Screenshot};
use crate::covers::CoverCache;
use crate::i18n::I18n;
use crate::media::{self, MediaPlayer, MediaStatus};
use crate::theme;

const PORTRAIT_WIDTH: f32 = 150.0;
const PORTRAIT_HEIGHT: f32 = 225.0;

/// Id do scroll da ficha, para abrir sempre no topo ao trocar de jogo.
pub(crate) const DETAIL_SCROLL_ID: &str = "native-detail-page";

/// Task que devolve a ficha ao topo; o scrollable guarda a posição anterior
/// entre jogos e sem isso a próxima ficha abre no meio do herói.
pub(crate) fn scroll_to_top() -> iced::Task<Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(DETAIL_SCROLL_ID),
        iced::widget::scrollable::AbsoluteOffset {
            x: Some(0.0),
            y: Some(0.0),
        },
    ))
}

pub(crate) fn view<'a>(
    hero_height: f32,
    covers: &'a CoverCache,
    backdrop: Option<String>,
    portrait: Option<String>,
    title: &'a str,
    chips: Vec<Element<'a, Message>>,
    back: Message,
    actions: Option<Element<'a, Message>>,
    left: Vec<Element<'a, Message>>,
    right: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let hero_height = hero_height.clamp(300.0, 430.0);

    let art: Element<'a, Message> = match covers.ready(backdrop.as_deref()) {
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

    let portrait: Element<'a, Message> = match covers.ready(portrait.as_deref()) {
        Some(handle) => container(
            image(handle.clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Cover),
        )
        .width(Length::Fixed(PORTRAIT_WIDTH))
        .height(Length::Fixed(PORTRAIT_HEIGHT))
        .clip(true)
        .style(theme::portrait_frame)
        .into(),
        None => container(
            text(initial(title))
                .size(44)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_3),
        )
        .width(Length::Fixed(PORTRAIT_WIDTH))
        .height(Length::Fixed(PORTRAIT_HEIGHT))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::portrait_frame)
        .into(),
    };

    let mut copy = column![text(title)
        .size(42)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)
        .wrapping(Wrapping::WordOrGlyph)]
    .spacing(12)
    .width(Fill);

    if !chips.is_empty() {
        let mut chip_row = row![].spacing(8).align_y(iced::Center);
        for chip in chips {
            chip_row = chip_row.push(chip);
        }
        copy = copy.push(chip_row);
    }

    let bottom = container(
        row![portrait, copy]
            .spacing(theme::SPACE_5)
            .align_y(iced::Bottom)
            .width(Fill),
    )
    .width(Fill)
    .height(Fill)
    .align_y(iced::Bottom)
    .padding(Padding {
        top: 0.0,
        right: theme::PAGE_PADDING,
        bottom: theme::SPACE_5,
        left: theme::PAGE_PADDING,
    });

    let hero = Stack::new()
        .width(Fill)
        .height(Length::Fixed(hero_height))
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::hero_overlay),
        )
        .push(bottom);

    let mut content = column![hero].width(Fill);

    if let Some(actions) = actions {
        content = content.push(
            container(
                container(actions)
                    .width(Fill)
                    .align_x(iced::Right)
                    .padding([theme::SPACE_3, theme::PAGE_PADDING]),
            )
            .width(Fill)
            .style(theme::action_bar),
        );
    }

    let body = if right.is_empty() {
        row![column(left).spacing(theme::SPACE_4).width(Fill)]
            .spacing(theme::SPACE_5)
            .align_y(iced::Top)
            .width(Fill)
    } else {
        // 3:1 em favor da coluna principal: o player do trailer ganha largura
        // e a barra lateral mantém a largura de leitura dos painéis.
        row![
            column(left)
                .spacing(theme::SPACE_4)
                .width(iced::FillPortion(3)),
            column(right)
                .spacing(theme::SPACE_4)
                .width(iced::FillPortion(1)),
        ]
        .spacing(theme::SPACE_5)
        .align_y(iced::Top)
        .width(Fill)
    };

    content = content.push(
        container(body)
            .width(Fill)
            .max_width(1680.0)
            .padding([theme::SPACE_5, theme::PAGE_PADDING]),
    );

    let page = scrollable(content)
        .id(iced::widget::Id::new(DETAIL_SCROLL_ID))
        .width(Fill)
        .height(Fill)
        .style(theme::scrollable);

    // O glifo "‹" tem mais bearing à esquerda que à direita na própria caixa,
    // então num círculo ele parece colado na borda esquerda. O padding
    // assimétrico corrige o centro óptico sem mexer no tamanho do alvo.
    // Chevron desenhado em canvas: centralização exata, independente do
    // *bearing* do glifo de fonte.
    let back = crate::views::back_button::circle(Some(back));

    let back_layer = container(back)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Left)
        .align_y(iced::Top)
        .padding(20);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(page)
        .push(back_layer)
        .into()
}

pub(crate) fn chip<'a>(label: impl Into<String>, accent: bool) -> Element<'a, Message> {
    let mut label = label.into();
    if label.chars().count() > 34 {
        label = label.chars().take(33).collect::<String>();
        label.push('…');
    }
    container(
        text(label)
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM)
            .color(if accent {
                theme::ACCENT
            } else {
                theme::TEXT_STRONG
            }),
    )
    .padding([5.0, 10.0])
    .style(if accent {
        theme::chip_accent
    } else {
        theme::chip
    })
    .into()
}

/// Galeria única da ficha: trailer + screenshots na mesma faixa de miniaturas,
/// como a `GameMediaGallery` da UI web. A coluna esquerda da ficha usa este
/// painel antes da descrição.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gallery_panel<'a>(
    i18n: &'a I18n,
    media: &'a MediaPlayer,
    covers: &'a CoverCache,
    movies: &'a [Movie],
    screenshots: &'a [Screenshot],
    selected_screenshot: usize,
    show_trailer: bool,
    select_trailer: Message,
    select_screenshot: impl Fn(usize) -> Message + 'a,
    actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let has_trailer = !movies.is_empty();
    let show_video = show_trailer && (has_trailer || media.is_active());

    let body: Element<'a, Message> = if show_video {
        let poster = movies
            .first()
            .and_then(|movie| movie.thumb.as_deref())
            .or_else(|| {
                screenshots
                    .first()
                    .and_then(|screenshot| screenshot.full_or_thumb())
            });
        trailer_body(i18n, media, covers, poster)
    } else if screenshots.is_empty() {
        hint(i18n.t("native.game.screenshots_missing"), theme::TEXT_3)
    } else {
        let index = selected_screenshot.min(screenshots.len() - 1);
        let screenshot = &screenshots[index];
        let large: Element<'a, Message> = match covers.ready(screenshot.full_or_thumb())
            .or_else(|| covers.ready(screenshot.thumb_or_full())) {
            Some(handle) => image(handle.clone())
                .width(Fill)
                .height(340)
                .content_fit(ContentFit::Contain)
                .into(),
            None => container(
                text(i18n.t("native.game.screenshot_loading"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            )
            .width(Fill)
            .height(340)
            // `align_*`, não `center_*`: os atalhos `center_*` chamam
            // `width/height(Fill)` e apagariam o `height(340)` acima.
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(theme::cover_placeholder)
            .into(),
        };
        column![
            large,
            text(i18n.t_args(
                "native.game.screenshot_counter",
                &[
                    ("index", &(index + 1).to_string()),
                    ("total", &screenshots.len().to_string()),
                ],
            ))
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .into()
    };

    let mut thumbs: Vec<Element<'a, Message>> = Vec::new();
    for movie in movies.iter().take(3) {
        let picture = thumb_picture(covers, movie.thumb.as_deref());
        let overlay = container(
            container(text("▶").size(theme::TEXT_READ).color(Color::WHITE))
                .padding([5.0, 7.0])
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    border: Border::default().rounded(20.0),
                    ..container::Style::default()
                }),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill);
        let visual = Stack::new()
            .width(Fill)
            .height(Fill)
            .push(picture)
            .push(overlay);
        thumbs.push(
            button(
                container(visual)
                    .width(Length::Fixed(118.0))
                    .height(Length::Fixed(66.0))
                    .clip(true),
            )
            .on_press(select_trailer.clone())
            .padding(0)
            .style(thumb_button(show_trailer))
            .into(),
        );
    }
    for (index, screenshot) in screenshots.iter().take(6).enumerate() {
        let picture = thumb_picture(covers, screenshot.thumb_or_full());
        thumbs.push(
            button(
                container(picture)
                    .width(Length::Fixed(118.0))
                    .height(Length::Fixed(66.0))
                    .clip(true),
            )
            .on_press(select_screenshot(index))
            .padding(0)
            .style(thumb_button(
                !show_trailer
                    && index == selected_screenshot.min(screenshots.len().saturating_sub(1)),
            ))
            .into(),
        );
    }

    let strip: Element<'a, Message> = if thumbs.is_empty() {
        Space::new().height(0).into()
    } else {
        scrollable(row(thumbs).spacing(theme::SPACE_2))
            .horizontal()
            .width(Fill)
            .height(76)
            .style(theme::scrollable_bare)
            .into()
    };

    // Sem cabeçalho: nada de barra de accent nem de ações (recarregar) em
    // cima do player. O bloco é só vídeo + miniaturas.
    drop(actions);

    // Container pai do player/galeria: sem fundo e sem borda, para o vídeo e
    // as miniaturas ficarem soltos na página em vez de dentro de uma placa
    // preta. O layout (padding, espaçamento, largura) segue igual.
    container(column![body, strip].spacing(theme::SPACE_3))
        .width(Fill)
        .padding(theme::SPACE_4)
        .style(theme::panel_bare)
        .into()
}

fn trailer_body<'a>(
    i18n: &'a I18n,
    media: &'a MediaPlayer,
    covers: &'a CoverCache,
    poster: Option<&str>,
) -> Element<'a, Message> {
    // ── Superfície do vídeo ────────────────────────────────────────────
    // O filme preenche a caixa; pôster + véu enquanto não há textura, e o
    // frame faltante só quando nem pôster existe. Nenhum rótulo técnico vai
    // para a tela — só um indicador discreto de carregamento.
    let surface: Element<'a, Message> = match media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Contain)
            .into(),
        None => match poster.and_then(|url| covers.ready(Some(url))) {
            Some(handle) => Stack::new()
                .width(Fill)
                .height(Fill)
                .push(
                    image(handle.clone())
                        .width(Fill)
                        .height(Fill)
                        .content_fit(ContentFit::Cover),
                )
                .push(
                    container(text("▶").size(34.0).color(Color::from_rgba(1.0, 1.0, 1.0, 0.85)))
                        .width(Fill)
                        .height(Fill)
                        .center_x(Fill)
                        .center_y(Fill)
                        .style(theme::media_loading_veil),
                )
                .into(),
            None => missing_frame(i18n, media),
        },
    };

    let overlay = controls_bar(i18n, media);
    let surface = Stack::new()
        .width(Fill)
        .height(Fill)
        .push(surface)
        .push(
            container(overlay)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Bottom)
                .padding(Padding {
                    top: 0.0,
                    right: theme::SPACE_4,
                    bottom: theme::SPACE_4,
                    left: theme::SPACE_4,
                }),
        );

    // 16:9 com teto de altura; o `contain` mantém o filme sem deformar.
    // Sem placa atrás: nenhum fundo/borda/margem em volta do vídeo. Só o
    // `clip` do canto arredondado — o quadro é o próprio vídeo.
    let frame: Element<'a, Message> = container(surface)
        .width(Fill)
        .height(400)
        .clip(true)
        .style(theme::player_bare_surface)
        .into();

    if let MediaStatus::Error(message) = media.status() {
        column![
            frame,
            text(truncate(message, 180))
                .size(theme::TEXT_MICRO)
                .color(theme::DANGER),
        ]
        .spacing(theme::SPACE_2)
        .into()
    } else {
        frame
    }
}

/// Barra de controles flutuante sobre o vídeo.
///
/// Vidro escuro arredondado, cantos suaves, sem texto técnico: seek bar
/// full-width no topo, e uma linha com play/pause, ±15 s, tempo e total.
fn controls_bar<'a>(i18n: &'a I18n, media: &'a MediaPlayer) -> Element<'a, Message> {
    let duration = media.duration_seconds();
    let position = media.position_seconds();
    let playing = media.is_playing();

    // Scrubber: ocupa toda a largura e só aparece quando a duração é real.
    let mut seek = column![].spacing(theme::SPACE_1);
    if duration > 0.5 {
        seek = seek.push(
            slider(0.0..=duration as f32, position as f32, |seconds| {
                Message::MediaSeek(f64::from(seconds))
            })
            .step(0.1_f32)
            .width(Fill)
            .style(theme::player_slider),
        );
    }

    let mut play = button(
        text(if playing { "❚❚" } else { "▶" })
            .size(15.0)
            .color(Color::WHITE),
    )
    .padding([6.0, 10.0])
    .style(|theme, status| {
        let mut style = theme::player_control(theme, status);
        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            style.shadow = iced::Shadow {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.35),
                blur_radius: 10.0,
                ..Default::default()
            };
        }
        style
    });
    if media.can_control() {
        play = play.on_press(Message::MediaTogglePlayback);
    }

    // Saltos de 15 s, como no player de referência.
    let mut back15 = button(text("↺").size(14.0).color(Color::from_rgba(1.0, 1.0, 1.0, 0.75)))
        .padding([6.0, 8.0])
        .style(theme::player_control);
    if media.can_control() {
        back15 = back15.on_press(Message::MediaSeek((position - 15.0).max(0.0)));
    }
    let mut fwd15 = button(text("↻").size(14.0).color(Color::from_rgba(1.0, 1.0, 1.0, 0.75)))
        .padding([6.0, 8.0])
        .style(theme::player_control);
    if media.can_control() && duration > 0.5 {
        fwd15 = fwd15.on_press(Message::MediaSeek((position + 15.0).min(duration)));
    }

    let time_current = text(media::format_seconds(position))
        .size(theme::TEXT_MICRO)
        .font(theme::FONT_MEDIUM)
        .color(Color::WHITE);
    let time_total = if duration > 0.5 {
        text(media::format_seconds(duration))
            .size(theme::TEXT_MICRO)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.62))
    } else {
        text(i18n.t("native.game.media.loading"))
            .size(theme::TEXT_MICRO)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.45))
    };

    let row = row![
        play,
        back15,
        fwd15,
        time_current,
        Space::new().width(Fill),
        time_total,
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    container(column![seek, row].spacing(theme::SPACE_2))
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::player_controls)
        .into()
}

fn thumb_picture<'a>(covers: &'a CoverCache, url: Option<&str>) -> Element<'a, Message> {
    match covers.ready(url) {
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
    }
}

fn thumb_button(selected: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |style_theme, status| {
        let mut style = theme::segment_button(style_theme, status, false);
        // A miniatura já é a própria imagem: nenhum preenchimento atrás. O
        // selecionado se distingue só pelo anel accent; o hover só acende a
        // borda. O retângulo branco era a "caixa" que pesava no strip.
        style.background = Some(Color::TRANSPARENT.into());
        style.border = if selected {
            Border {
                color: theme::ACCENT,
                width: 1.5,
                radius: 4.0.into(),
            }
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.32),
                width: 1.0,
                radius: 4.0.into(),
            }
        } else {
            Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                width: 1.0,
                radius: 4.0.into(),
            }
        };
        style
    }
}

fn missing_frame<'a>(i18n: &'a I18n, media: &'a MediaPlayer) -> Element<'a, Message> {
    let (title, detail, color) = if let MediaStatus::Error(message) = media.status() {
        (
            truncate(message, 180),
            i18n.t("native.game.trailer_open_error").to_string(),
            theme::DANGER,
        )
    } else {
        (
            i18n.t("native.game.querying_trailer").to_string(),
            i18n.t("native.game.sysinfo_pipeline").to_string(),
            theme::TEXT_2,
        )
    };

    container(
        column![
            text(title).size(theme::TEXT_CAPTION).color(color),
            text(detail).size(theme::TEXT_MICRO).color(theme::TEXT_3),
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

/// Painel de requisitos com abas Mínimos/Recomendados, como a
/// `RequirementsPanel` da UI web.
pub(crate) fn requirements_panel<'a>(
    i18n: &'a I18n,
    min: Option<&'a str>,
    rec: Option<&'a str>,
    tab: RequirementsTab,
) -> Option<Element<'a, Message>> {
    let min_lines = min.map(requirement_lines).unwrap_or_default();
    let rec_lines = rec.map(requirement_lines).unwrap_or_default();
    if min_lines.is_empty() && rec_lines.is_empty() {
        return None;
    }

    let has_min = !min_lines.is_empty();
    let has_rec = !rec_lines.is_empty();
    let active = if tab == RequirementsTab::Minimum && has_min {
        RequirementsTab::Minimum
    } else if has_rec {
        RequirementsTab::Recommended
    } else {
        RequirementsTab::Minimum
    };
    let lines = if active == RequirementsTab::Minimum {
        &min_lines
    } else {
        &rec_lines
    };

    let mut tabs = row![].spacing(4).width(Fill);
    if has_min {
        tabs = tabs.push(
            button(
                text(i18n.t("native.game.requirements_min"))
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::RequirementsTabSelected(RequirementsTab::Minimum))
            .width(Fill)
            .padding([6.0, 10.0])
            .style(move |style_theme, status| {
                theme::segment_button(style_theme, status, active == RequirementsTab::Minimum)
            }),
        );
    }
    if has_rec {
        tabs = tabs.push(
            button(
                text(i18n.t("native.game.requirements_rec"))
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::RequirementsTabSelected(
                RequirementsTab::Recommended,
            ))
            .width(Fill)
            .padding([6.0, 10.0])
            .style(move |style_theme, status| {
                theme::segment_button(style_theme, status, active == RequirementsTab::Recommended)
            }),
        );
    }

    let mut body = column![].spacing(5);
    for line in lines {
        body = body.push(
            text(format!("•  {line}"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    Some(detail_panel(
        i18n.t("native.game.requirements"),
        column![tabs, body].spacing(theme::SPACE_3).into(),
    ))
}

/// Painel de idiomas; o HTML da Steam vira texto limpo por linha.
pub(crate) fn languages_panel<'a>(
    i18n: &'a I18n,
    languages: Option<&'a str>,
) -> Option<Element<'a, Message>> {
    let languages = strip_html(languages?);
    let lines: Vec<String> = languages
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if lines.is_empty() {
        return None;
    }

    let mut body = column![].spacing(5);
    for line in lines {
        body = body.push(
            text(line)
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    Some(detail_panel(i18n.t("native.game.languages"), body.into()))
}

fn requirement_lines(html: &str) -> Vec<String> {
    strip_html(html)
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches(['-', '•']).trim())
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower != "minimum:"
                && lower != "recommended:"
                && lower != "mínimos:"
                && lower != "mínimo:"
                && lower != "recomendados:"
                && lower != "recomendado:"
                && !lower.contains("requer um sistema operacional e processador de 64 bits")
                && !lower.contains("requires a 64-bit processor and operating system")
        })
        .take(12)
        .map(str::to_string)
        .collect()
}

fn detail_panel<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![super::section_label(title), body]
            .spacing(theme::SPACE_3)
            .width(Fill),
    )
    .width(Fill)
    .padding(theme::SPACE_4)
    .style(theme::panel)
    .into()
}

fn hint<'a>(message: impl Into<String>, color: Color) -> Element<'a, Message> {
    text(message.into())
        .size(theme::TEXT_CAPTION)
        .color(color)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}
