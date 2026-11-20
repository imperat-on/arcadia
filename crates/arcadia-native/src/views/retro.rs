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

use crate::app::{App, LoadStatus, Message, RetroDetailState, RetroState, Section};
use crate::backend::{retro_system_filters, RetroGame, RetroSource};
use crate::covers::CoverCache;
use crate::theme;
use crate::views;
use crate::views::cards;
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(detail) = app.retro.detail.as_ref() {
        return detail_view(app, detail);
    }

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

    button(
        column![
            container(cover).width(Fill).height(Fill).clip(true),
            info,
        ]
        .height(Fill),
    )
    .on_press(Message::RetroOpenDetail(game.clone()))
    .width(Fill)
    .height(Fill)
    .padding(0)
    .style(theme::card)
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

fn detail_view<'a>(app: &'a App, detail: &'a RetroDetailState) -> Element<'a, Message> {
    let game = detail
        .detail
        .as_ref()
        .map(|full| &full.game)
        .unwrap_or(&detail.game);
    let backdrop = game
        .hero_url
        .as_deref()
        .or(game.cover_url.as_deref())
        .or(game.fallback_cover_url.as_deref());

    let art: Element<'a, Message> = match app.covers.ready(backdrop) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(game.title.to_uppercase())
                .size(42)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.08)),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    };

    let mut meta = row![
        text(game.system_label())
            .size(12)
            .color(theme::TEXT_2)
            .wrapping(Wrapping::None),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if let Some(year) = game.release_year {
        meta = meta.push(badge(year.to_string(), theme::TEXT_2));
    }
    if let Some(developer) = game.developer.as_deref() {
        meta = meta.push(text(developer).size(12).color(theme::TEXT_2));
    }
    if let Some(source) = game.source_title.as_deref() {
        meta = meta.push(badge(source.to_string(), theme::TEXT_3));
    }

    let hero_copy = container(
        row![
            retro_portrait(game, &app.covers),
            column![
                text(game.title.as_str())
                    .size(34)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(format!("id do catálogo · {}", game.id))
                    .size(11)
                    .color(theme::TEXT_3),
            ]
            .spacing(8)
            .width(Fill),
        ]
        .spacing(theme::SPACE_5)
        .align_y(iced::Bottom),
    )
    .width(Fill)
    .height(Fill)
    .align_y(iced::Bottom)
    .padding([theme::SPACE_5, theme::PAGE_PADDING]);

    let hero = Stack::new()
        .width(Fill)
        .height(360.0)
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::hero_overlay),
        )
        .push(hero_copy);

    let mut content = column![
        container(
            button(
                row![
                    text("‹").size(18).color(theme::TEXT_1),
                    text("Voltar para o catálogo Retro")
                        .size(12)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_1),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(Message::RetroCloseDetail)
            .padding([7.0, 13.0])
            .style(theme::ghost_button)
        )
        .width(Fill)
        .padding([theme::SPACE_3, theme::PAGE_PADDING]),
        hero,
    ]
    .width(Fill);

    if let Some(error) = detail.error.as_deref() {
        content = content.push(
            container(views::action_error_banner(
                error,
                Some(Message::RetroOpenDetail(detail.game.clone())),
            ))
            .width(Fill)
            .padding(Padding {
                top: theme::SPACE_3,
                right: theme::PAGE_PADDING,
                bottom: 0.0,
                left: theme::PAGE_PADDING,
            }),
        );
    }
    if detail.status == LoadStatus::Loading {
        content = content.push(
            container(
                text("Carregando a ficha completa em retro:game...")
                    .size(12)
                    .color(theme::TEXT_2),
            )
            .width(Fill)
            .padding(Padding {
                top: theme::SPACE_2,
                right: theme::PAGE_PADDING,
                bottom: 0.0,
                left: theme::PAGE_PADDING,
            }),
        );
    }

    let details = container(
        row![
            column![
                retro_description(detail, game),
                retro_offers_panel(detail)
            ]
            .spacing(theme::SPACE_4)
            .width(Fill),
            retro_info_panel(detail, game, &app.retro.sources),
        ]
        .spacing(theme::SPACE_5)
        .align_y(iced::Top),
    )
    .width(Fill)
    .padding([theme::SPACE_5, theme::PAGE_PADDING]);

    content = content.push(details);

    scrollable(content)
        .height(Fill)
        .style(theme::scrollable)
        .into()
}

fn retro_portrait<'a>(game: &'a RetroGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let url = game.cover_url.as_deref().or(game.fallback_cover_url.as_deref());
    match covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(116)
            .height(164)
            .content_fit(ContentFit::Cover)
            .border_radius(3)
            .into(),
        None => container(
            text(initial(game))
                .size(34)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
        )
        .width(116)
        .height(164)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    }
}

fn retro_description<'a>(
    detail: &'a RetroDetailState,
    game: &'a RetroGame,
) -> Element<'a, Message> {
    let description = detail
        .detail
        .as_ref()
        .and_then(|full| full.game.description.as_deref())
        .or(game.description.as_deref());
    let body: Element<'a, Message> = match description {
        Some(description) => text(description)
            .size(13)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.74))
            .wrapping(Wrapping::WordOrGlyph)
            .into(),
        None if detail.status == LoadStatus::Loading => text("Carregando descrição...")
            .size(12)
            .color(theme::TEXT_2)
            .into(),
        None => text("O catálogo não informou uma descrição para este jogo.")
            .size(12)
            .color(theme::TEXT_3)
            .into(),
    };

    container(
        column![views::section_label("Sobre o jogo"), body].spacing(theme::SPACE_3),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel)
    .into()
}

fn retro_info_panel<'a>(
    detail: &'a RetroDetailState,
    game: &'a RetroGame,
    page_sources: &'a [RetroSource],
) -> Element<'a, Message> {
    let mut rows = column![].spacing(theme::SPACE_2);
    let mut has_row = true;

    let system_label = game.system_label();
    rows = rows.push(info_row("Sistema", system_label.clone()));
    if let Some(platform) = game
        .platform
        .as_deref()
        .filter(|platform| *platform != system_label.as_str())
    {
        rows = rows.push(info_row("Plataforma", platform.to_string()));
        has_row = true;
    }
    if let Some(year) = game.release_year {
        rows = rows.push(info_row("Lançamento", year.to_string()));
        has_row = true;
    }
    if let Some(developer) = game.developer.as_deref() {
        rows = rows.push(info_row("Desenvolvedora", developer.to_string()));
        has_row = true;
    }
    if let Some(publisher) = game.publisher.as_deref() {
        rows = rows.push(info_row("Distribuidora", publisher.to_string()));
        has_row = true;
    }
    if !game.genres.is_empty() {
        rows = rows.push(info_row("Gêneros", game.genres.join(", ")));
        has_row = true;
    }
    if let Some(size) = game.file_size.as_deref() {
        rows = rows.push(info_row("Tamanho", size.to_string()));
        has_row = true;
    }
    let offer_count = detail
        .detail
        .as_ref()
        .map(|full| full.offers.len() as u64)
        .filter(|count| *count > 0)
        .or(game.offer_count);
    if let Some(count) = offer_count {
        rows = rows.push(info_row("Ofertas", count.to_string()));
        has_row = true;
    }
    let sources = detail_sources(detail, page_sources);
    if !sources.is_empty() {
        rows = rows.push(info_row("Fontes", sources.join(", ")));
        has_row = true;
    }
    if !has_row {
        rows = rows.push(
            text("O catálogo não retornou metadados adicionais.")
                .size(12)
                .color(theme::TEXT_3),
        );
    }

    container(column![views::section_label("Ficha"), rows].spacing(theme::SPACE_3))
        .width(Length::Fixed(320.0))
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn retro_offers_panel<'a>(detail: &'a RetroDetailState) -> Element<'a, Message> {
    let offers = detail
        .detail
        .as_ref()
        .map(|full| full.offers.as_slice())
        .unwrap_or_default();
    let mut content =
        column![views::section_label("Ofertas e fontes")].spacing(theme::SPACE_2);

    if offers.is_empty() {
        content = content.push(
            text(if detail.status == LoadStatus::Loading {
                "Consultando ofertas em retro:game..."
            } else {
                "O catálogo não retornou ofertas para este jogo."
            })
            .size(12)
            .color(theme::TEXT_3),
        );
    } else {
        for offer in offers {
            let title = offer
                .original_title
                .as_deref()
                .or(offer.source_title.as_deref())
                .unwrap_or(offer.id.as_str());
            let source = offer
                .source_title
                .as_deref()
                .or(offer.source_id.as_deref())
                .unwrap_or("Fonte não informada");
            let mut meta = Vec::new();
            if let Some(size) = offer.file_size.as_deref() {
                meta.push(size.to_string());
            }
            if let Some(date) = offer.upload_date.as_deref() {
                meta.push(date.to_string());
            }
            if offer.uri_count > 0 {
                meta.push(format!("{} link(s)", offer.uri_count));
            } else if offer.has_uris {
                meta.push("links disponíveis".to_string());
            } else {
                meta.push("sem links".to_string());
            }

            content = content.push(
                container(
                    row![
                        column![
                            text(title)
                                .size(12)
                                .font(theme::FONT_MEDIUM)
                                .color(theme::TEXT_STRONG)
                                .wrapping(Wrapping::WordOrGlyph),
                            text(format!("Fonte: {source}"))
                                .size(10)
                                .color(theme::TEXT_3)
                                .wrapping(Wrapping::WordOrGlyph),
                            text(meta.join(" · ")).size(10).color(theme::TEXT_3),
                        ]
                        .spacing(2)
                        .width(Fill),
                        button(text("Ver links").size(10).font(theme::FONT_MEDIUM))
                            .on_press(Message::RetroOfferRequested(offer.id.clone()))
                            .padding([5.0, 9.0])
                            .style(theme::ghost_button),
                    ]
                    .spacing(theme::SPACE_3)
                    .align_y(iced::Center),
                )
                .width(Fill)
                .padding([7.0, 9.0])
                .style(theme::stub_panel),
            );
        }
    }

    if detail.selected_offer.is_some() {
        content = content.push(offer_links_panel(detail));
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn offer_links_panel<'a>(detail: &'a RetroDetailState) -> Element<'a, Message> {
    let mut content = column![].spacing(theme::SPACE_2);
    match detail.offer_status {
        LoadStatus::Idle => {
            content = content.push(text("Selecione uma oferta.").size(11).color(theme::TEXT_3));
        }
        LoadStatus::Loading => {
            content = content.push(
                text("Consultando links em retro:offer...")
                    .size(11)
                    .color(theme::TEXT_2),
            );
        }
        LoadStatus::Error(_) => {
            content = content.push(views::action_error_banner(
                detail
                    .offer_error
                    .as_deref()
                    .unwrap_or("O dispatcher não respondeu retro:offer."),
                detail
                    .selected_offer
                    .clone()
                    .map(Message::RetroOfferRequested),
            ));
        }
        LoadStatus::Ready => match detail.offer_detail.as_ref() {
            Some(offer) if offer.uris.is_empty() => {
                content = content.push(
                    text("A oferta não retornou URIs (magnet ou HTTP).")
                        .size(11)
                        .color(theme::TEXT_3),
                );
            }
            Some(offer) => {
                content = content.push(
                    text(format!("Links da oferta {} (retro:offer):", offer.id))
                        .size(10)
                        .color(theme::TEXT_3),
                );
                for uri in &offer.uris {
                    content = content.push(
                        text(uri)
                            .size(10)
                            .color(theme::TEXT_2)
                            .wrapping(Wrapping::WordOrGlyph),
                    );
                }
                content = content.push(
                    text(
                        "Links apenas informativos: esta build não inicia download retrô pela ficha.",
                    )
                    .size(10)
                    .color(theme::TEXT_3)
                    .wrapping(Wrapping::WordOrGlyph),
                );
            }
            None => {
                content = content.push(
                    text("O dispatcher não retornou detalhes desta oferta.")
                        .size(11)
                        .color(theme::TEXT_3),
                );
            }
        },
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn detail_sources(detail: &RetroDetailState, page_sources: &[RetroSource]) -> Vec<String> {
    let mut titles: Vec<String> = Vec::new();
    let mut consider = |title: &str| {
        let title = title.trim();
        if !title.is_empty() && !titles.iter().any(|current| current == title) {
            titles.push(title.to_string());
        }
    };
    for source in page_sources {
        consider(&source.title);
    }
    if let Some(full) = detail.detail.as_ref() {
        for source in &full.sources {
            consider(&source.title);
        }
        for offer in &full.offers {
            if let Some(source) = offer.source_title.as_deref() {
                consider(source);
            } else if let Some(source) = offer.source_id.as_deref() {
                consider(source);
            }
        }
    }
    titles
}

fn info_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    column![
        text(label).size(10).color(theme::TEXT_3),
        text(value)
            .size(12)
            .color(theme::TEXT_STRONG)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(2)
    .width(Fill)
    .into()
}

fn badge<'a>(label: String, color: Color) -> Element<'a, Message> {
    container(text(label).size(9).font(theme::FONT_MEDIUM).color(color))
        .padding([2.0, 6.0])
        .style(theme::badge)
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
