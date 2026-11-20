//! Catálogo Retro nativo: `retro:list` com busca, filtro por sistema,
//! paginação e capas pelo mesmo `CoverCache` da biblioteca.
//!
//! A arte vem de `cover`/`fallbackCover`/`artwork` e é baixada por HTTP no
//! backend nativo; o canal `retro:covers` grava arquivos para o webview e não
//! é necessário aqui.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, pick_list, row, scrollable, text, text_input, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, LoadStatus, Message, RetroState, Section};
use crate::backend::{retro_system_filters, RetroGame};
use crate::covers::CoverCache;
use crate::theme;
use crate::views;
use crate::views::cards;
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.retro;
    let header = views::page_header(
        "Retro",
        "Catálogo de clássicos das fontes Hydra marcadas Classics.",
        retro_count_label(state),
        Some(Message::Refresh(Section::Stores)),
    );

    let body = scrollable(container(body_content(app)).width(Fill).padding(Padding {
        top: theme::SPACE_2,
        right: theme::PAGE_PADDING,
        bottom: theme::PAGE_PADDING,
        left: theme::PAGE_PADDING,
    }))
    .height(Fill)
    .style(theme::scrollable);

    column![header, body].width(Fill).height(Fill).into()
}

fn retro_count_label(state: &RetroState) -> Option<String> {
    let total = state.total?;
    Some(match state.total_offers {
        Some(offers) => format!("{total} jogos · {offers} ofertas"),
        None => format!("{total} jogos"),
    })
}

fn body_content(app: &App) -> Element<'_, Message> {
    let state = &app.retro;
    let mut content = column![search_controls(state)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if !state.applied_query.is_empty() && state.status != LoadStatus::Loading {
        content = content.push(
            text(format!("Resultados para “{}”", state.applied_query))
                .size(11)
                .color(theme::TEXT_3),
        );
    }

    if let Some(error) = state.error.as_deref().filter(|_| !state.games.is_empty()) {
        content = content.push(views::action_error_banner(
            error,
            Some(Message::RetroSearchSubmitted),
        ));
    }

    content = content.push(retro_body(app));

    content.into()
}

fn search_controls(state: &RetroState) -> Element<'_, Message> {
    let systems = pick_list(
        retro_system_filters(),
        Some(state.system),
        Message::RetroSystemChanged,
    )
    .width(Length::Fixed(190.0))
    .padding([10.0, 12.0])
    .text_size(12)
    .style(theme::pick_list)
    .menu_style(theme::pick_list_menu);

    let input = Stack::new()
        .width(Fill)
        .push(
            text_input("Buscar no catálogo Retro...", &state.query)
                .on_input(Message::RetroQueryChanged)
                .on_submit(Message::RetroSearchSubmitted)
                .padding(Padding {
                    top: 10.0,
                    right: 14.0,
                    bottom: 10.0,
                    left: 38.0,
                })
                .size(13)
                .width(Fill)
                .style(theme::search_input),
        )
        .push(
            container(icons::icon(Icon::Search, 15.0, 0.40))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Left)
                .align_y(iced::Center)
                .padding([0.0, 14.0]),
        );

    let loading = state.status == LoadStatus::Loading;
    let mut search_button = button(
        text(if loading { "Buscando..." } else { "Buscar" })
            .size(12)
            .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 18.0]);
    if !loading {
        search_button = search_button.on_press(Message::RetroSearchSubmitted);
    }

    let mut clear_button = button(text("Limpar").size(12).font(theme::FONT_MEDIUM))
        .padding([10.0, 14.0])
        .style(theme::ghost_button);
    if !state.query.is_empty() {
        clear_button = clear_button.on_press(Message::RetroClearSearch);
    }

    row![
        systems,
        input,
        search_button.style(theme::primary_button),
        clear_button
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center)
    .into()
}

fn retro_body(app: &App) -> Element<'_, Message> {
    let state = &app.retro;

    if state.games.is_empty() {
        return match state.status {
            LoadStatus::Idle | LoadStatus::Loading => {
                views::loading_state("Carregando catálogo Retro...")
            }
            LoadStatus::Error(_) => container(views::error_state(
                "Não foi possível carregar o catálogo Retro",
                state
                    .error
                    .as_deref()
                    .unwrap_or("O dispatcher não respondeu retro:list."),
                Message::RetroSearchSubmitted,
            ))
            .width(Fill)
            .into(),
            LoadStatus::Ready => {
                if state.applied_query.is_empty() {
                    inline_empty(
                        "Catálogo Retro vazio",
                        "retro:list respondeu sem jogos. Verifique as fontes Classics ou tente atualizar.",
                    )
                } else {
                    inline_empty(
                        "Nenhum jogo Retro encontrado",
                        "Tente outro título ou troque o filtro de sistema.",
                    )
                }
            }
        };
    }

    let cards: Vec<Element<'_, Message>> = state
        .games
        .iter()
        .map(|game| retro_card(game, &app.covers))
        .collect();

    let mut content = column![].width(Fill).spacing(theme::SPACE_3);
    if state.status == LoadStatus::Loading {
        content = content.push(text("Carregando página...").size(11).color(theme::TEXT_3));
    }
    content = content.push(cards::grid(cards, (2.0, 3.55)));
    content = content.push(pagination(state));

    content.into()
}

fn retro_card<'a>(game: &'a RetroGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match covers
        .ready(game.cover_url.as_deref())
        .or_else(|| covers.ready(game.fallback_cover_url.as_deref()))
    {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            column![
                text(initial(game))
                    .size(30)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30)),
                text(game.system_label())
                    .size(9)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_3)
                    .wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(4)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .padding(10)
        .style(theme::cover_placeholder)
        .into(),
    };

    let mut meta = row![
        text(game.system_label())
            .size(10)
            .color(theme::TEXT_3)
            .wrapping(Wrapping::None),
    ]
    .spacing(6)
    .align_y(iced::Center);

    if let Some(offers) = game.offer_count.filter(|count| *count > 0) {
        meta = meta.push(
            text(format!("{offers} oferta(s)"))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    let mut info_content = column![
        container(
            text(ellipsize(&game.title, 24))
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.88))
                .wrapping(Wrapping::None),
        )
        .width(Fill)
        .height(18)
        .clip(true),
    ]
    .spacing(2);

    let meta_line = game.meta_line();
    if !meta_line.is_empty() {
        info_content = info_content.push(
            text(ellipsize(&meta_line, 30))
                .size(9)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::None),
        );
    }
    info_content = info_content.push(meta);

    let info = container(info_content)
        .width(Fill)
        .height(64)
        .padding([8.0, 11.0]);

    container(
        column![
            container(cover).width(Fill).height(Fill).clip(true),
            info,
        ]
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(theme::stub_panel)
    .into()
}

fn pagination(state: &RetroState) -> Element<'_, Message> {
    let loading = state.status == LoadStatus::Loading;
    let page = state.offset / crate::app::RETRO_PAGE_SIZE + 1;
    let total_pages = state.total.map(|total| {
        ((total + crate::app::RETRO_PAGE_SIZE - 1) / crate::app::RETRO_PAGE_SIZE).max(1)
    });

    let page_label = match total_pages {
        Some(total) => format!("Página {page} de {total}"),
        None => format!("Página {page}"),
    };

    let mut previous = button(text("‹").size(18).color(theme::TEXT_2)).padding([4.0, 12.0]);
    if state.offset > 0 && !loading {
        previous = previous.on_press(Message::RetroPrevPage);
    }

    let mut next = button(text("›").size(18).color(theme::TEXT_2)).padding([4.0, 12.0]);
    if state.has_more && !loading {
        next = next.on_press(Message::RetroNextPage);
    }

    container(
        row![
            previous.style(theme::ghost_button),
            text(page_label).size(11).color(theme::TEXT_3),
            next.style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    )
    .width(Fill)
    .center_x(Fill)
    .padding([theme::SPACE_2, 0.0])
    .into()
}

fn inline_empty<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(16)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_STRONG),
            text(message).size(12).color(theme::TEXT_2),
            Space::new().height(2),
        ]
        .spacing(theme::SPACE_2),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::stub_panel)
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

fn initial(game: &RetroGame) -> String {
    let platform = game
        .system_label()
        .replace("Nintendo ", "")
        .replace("Sony ", "");
    let initials: String = platform
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(4)
        .collect::<String>()
        .to_uppercase();
    if initials.is_empty() {
        "RETRO".to_string()
    } else {
        initials
    }
}
