//! Lojas: busca Steam/Hubcap (`store:search`), sugestões (`store:suggest`),
//! catálogo navegável (`store:recent`), ficha com `game:sysinfo` e instalação
//! por `store:installInfo`/`store:install`.
//!
//! A grade é a mesma `minmax(170px, 1fr)` da shell; a ficha repete o hero
//! cinemático da `StoreGamePage.tsx`. A ficha mostra o manifesto e as
//! bibliotecas de `store:libraries`; o botão Instalar só enfileira quando o
//! dispatcher aceita o payload — nada aqui simula download nem sucesso.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, row, scrollable, text, text_input, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{
    App, LoadStatus, Message, Section, StoreBrowseState, StoreDetailState, StoreSearchState,
    StoreState, StoreTab,
};
use crate::backend::{StoreDepot, StoreGame, StoreInfo, StoreInstallInfo};
use crate::covers::CoverCache;
use crate::theme;
use crate::views;
use crate::views::icons::{self, Icon};

const STORE_COVER_HEIGHT: f32 = 106.0;
const STORE_CARD_INFO_HEIGHT: f32 = 56.0;
const DETAIL_HERO_HEIGHT: f32 = 360.0;

pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(detail) = app.store.detail.as_ref() {
        return detail_view(app, detail);
    }

    let state = &app.store;
    let tabs = tab_strip(state.tab);
    match state.tab {
        StoreTab::Retro => column![tabs, views::retro::view(app)]
            .width(Fill)
            .height(Fill)
            .into(),
        StoreTab::Steam => {
            let header = views::page_header(
                "Lojas",
                "Busque no catálogo Steam/Hubcap; clique num resultado para abrir a ficha.",
                store_count_label(state),
                Some(Message::Refresh(Section::Stores)),
            );
            column![tabs, header, steam_body(app)]
                .width(Fill)
                .height(Fill)
                .into()
        }
    }
}

fn store_count_label(state: &StoreState) -> Option<String> {
    match &state.search {
        Some(search) => search
            .result
            .as_ref()
            .map(|result| format!("{} resultado(s)", result.games.len())),
        None => state
            .browse
            .total
            .map(|total| format!("{total} jogo(s)"))
            .or_else(|| Some(format!("{} jogo(s)", state.browse.games.len()))),
    }
}

// ── Segmented control ───────────────────────────────────────────────────────

fn tab_strip(active: StoreTab) -> Element<'static, Message> {
    let control = container(
        row![
            tab_button(StoreTab::Steam.label(), StoreTab::Steam, active),
            tab_button(StoreTab::Retro.label(), StoreTab::Retro, active),
        ]
        .spacing(2),
    )
    .padding(3)
    .style(theme::segmented);

    container(control)
        .width(Fill)
        .padding(Padding {
            top: theme::SPACE_4,
            right: theme::PAGE_PADDING,
            bottom: 0.0,
            left: theme::PAGE_PADDING,
        })
        .into()
}

fn tab_button(label: &'static str, tab: StoreTab, active: StoreTab) -> Element<'static, Message> {
    let selected = tab == active;
    button(
        text(label)
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(if selected {
                theme::TEXT_1
            } else {
                theme::TEXT_2
            }),
    )
    .on_press(Message::StoreTabChanged(tab))
    .padding([8.0, 16.0])
    .style(move |style_theme, status| theme::segment_button(style_theme, status, selected))
    .into()
}

// ── Aba Loja ────────────────────────────────────────────────────────────────

fn steam_body(app: &App) -> Element<'_, Message> {
    let state = &app.store;
    let mut content = column![search_section(state)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if !state.suggestions.is_empty() {
        content = content.push(suggestions_panel(app));
    }

    content = content.push(results_heading(state));

    if let Some(search) = state.search.as_ref() {
        if let Some(result) = search.result.as_ref() {
            for warning in &result.warnings {
                content = content.push(views::warning_banner(warning));
            }
        }
    }
    if state.search.is_none() {
        if let Some(error) = state.browse.error.as_deref() {
            content = content.push(views::action_error_banner(error, None));
        }
    }

    content = content.push(results_body(app));

    scrollable(container(content).width(Fill).padding(Padding {
        top: theme::SPACE_2,
        right: theme::PAGE_PADDING,
        bottom: theme::PAGE_PADDING,
        left: theme::PAGE_PADDING,
    }))
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn search_section(state: &StoreState) -> Element<'_, Message> {
    let input = Stack::new()
        .width(Fill)
        .push(
            text_input("Buscar na loja...", &state.query)
                .on_input(Message::StoreQueryChanged)
                .on_submit(Message::StoreSearchSubmitted)
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

    let busy = state
        .search
        .as_ref()
        .is_some_and(|search| search.status == LoadStatus::Loading);

    let mut search_button = button(
        text(if busy { "Buscando..." } else { "Buscar" })
            .size(12)
            .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 18.0]);
    if !busy && !state.query.trim().is_empty() {
        search_button = search_button.on_press(Message::StoreSearchSubmitted);
    }

    let mut clear_button = button(text("Limpar").size(12).font(theme::FONT_MEDIUM))
        .padding([10.0, 14.0])
        .style(theme::ghost_button);
    if !state.query.is_empty() || state.search.is_some() {
        clear_button = clear_button.on_press(Message::StoreClearSearch);
    }

    row![
        input,
        search_button.style(theme::primary_button),
        clear_button
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center)
    .into()
}

fn suggestions_panel(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(1).width(Fill);
    for game in &app.store.suggestions {
        list = list.push(suggestion_row(game, &app.covers));
    }

    container(list)
        .width(Fill)
        .padding(4)
        .style(theme::stub_panel)
        .into()
}

fn suggestion_row<'a>(game: &'a StoreGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let thumb: Element<'a, Message> = match covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(48)
            .height(27)
            .content_fit(ContentFit::Cover)
            .border_radius(2)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(12)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
        )
        .width(48)
        .height(27)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::game_icon_placeholder)
        .into(),
    };

    button(
        row![
            thumb,
            text(game.title.as_str())
                .size(12)
                .color(theme::TEXT_STRONG)
                .wrapping(Wrapping::None),
            Space::new().width(Fill),
            text("Buscar").size(10).color(theme::TEXT_3),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::StoreSuggestionPicked(game.clone()))
    .width(Fill)
    .padding([5.0, 8.0])
    .style(|style_theme, status| theme::game_item(style_theme, status, false))
    .into()
}

fn results_heading(state: &StoreState) -> Element<'_, Message> {
    let (label, count) = match &state.search {
        Some(search) => (
            "Resultados da busca",
            search.result.as_ref().map(|result| result.games.len()),
        ),
        None => ("Catálogo", Some(state.browse.games.len())),
    };

    let mut heading = row![
        views::section_label(label),
        container(
            text(count.unwrap_or(0).to_string())
                .size(10)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_2),
        )
        .padding([3.0, 8.0])
        .style(theme::badge),
    ]
    .spacing(8)
    .align_y(iced::Center);

    if let Some(search) = state.search.as_ref() {
        heading = heading.push(
            text(format!("para “{}”", search.query))
                .size(11)
                .color(theme::TEXT_3),
        );
        if let Some(source) = search
            .result
            .as_ref()
            .and_then(|result| result.source.as_deref())
        {
            heading = heading.push(badge(source, theme::TEXT_2));
        }
    } else {
        heading = heading.push(
            text("Catálogo paginado de store:recent")
                .size(11)
                .color(theme::TEXT_3),
        );
    }

    heading.into()
}

fn results_body(app: &App) -> Element<'_, Message> {
    let state = &app.store;

    if let Some(search) = state.search.as_ref() {
        return search_results(app, search);
    }

    let browse = &state.browse;
    match browse.status {
        LoadStatus::Idle | LoadStatus::Loading if browse.games.is_empty() => {
            views::loading_state("Carregando catálogo da loja...")
        }
        LoadStatus::Error(_) => container(views::error_state(
            "Não foi possível carregar o catálogo",
            browse
                .error
                .as_deref()
                .unwrap_or("O dispatcher não respondeu store:recent."),
            Message::Refresh(Section::Stores),
        ))
        .width(Fill)
        .into(),
        _ if browse.games.is_empty() => inline_empty(
            "Catálogo vazio",
            "store:recent respondeu sem jogos. Tente atualizar ou buscar por um título.",
        ),
        _ => browse_grid(browse, &app.covers),
    }
}

fn search_results<'a>(app: &'a App, search: &'a StoreSearchState) -> Element<'a, Message> {
    match search.status {
        LoadStatus::Loading => views::loading_state("Buscando na loja..."),
        LoadStatus::Error(_) => container(views::error_state(
            "Não foi possível buscar na loja",
            search
                .error
                .as_deref()
                .unwrap_or("O dispatcher não respondeu store:search."),
            Message::StoreSearchSubmitted,
        ))
        .width(Fill)
        .into(),
        LoadStatus::Idle => views::loading_state("Preparando a busca..."),
        LoadStatus::Ready => {
            let games = search
                .result
                .as_ref()
                .map(|result| result.games.as_slice())
                .unwrap_or_default();
            if games.is_empty() {
                inline_empty(
                    "Nenhum jogo encontrado",
                    "Tente outro termo, confira a grafia ou limpe a busca para voltar ao catálogo.",
                )
            } else {
                store_grid(games, &app.covers)
            }
        }
    }
}

fn browse_grid<'a>(browse: &'a StoreBrowseState, covers: &'a CoverCache) -> Element<'a, Message> {
    let mut content = column![store_grid(&browse.games, covers)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if browse.has_more {
        let label = if browse.loading_more {
            "Carregando..."
        } else {
            "Carregar mais"
        };
        let mut more = button(text(label).size(12).font(theme::FONT_MEDIUM)).padding([9.0, 18.0]);
        if !browse.loading_more {
            more = more.on_press(Message::StoreLoadMore);
        }
        content = content.push(
            container(more.style(theme::ghost_button))
                .width(Fill)
                .center_x(Fill),
        );
    } else {
        content = content.push(
            container(text("Fim do catálogo.").size(11).color(theme::TEXT_3))
                .width(Fill)
                .center_x(Fill),
        );
    }

    content.into()
}

fn store_grid<'a>(games: &'a [StoreGame], covers: &'a CoverCache) -> Element<'a, Message> {
    let cards: Vec<Element<'a, Message>> =
        games.iter().map(|game| store_card(game, covers)).collect();

    iced::widget::Grid::with_children(cards)
        .fluid(theme::CARD_WIDTH)
        .spacing(theme::CARD_GAP)
        .into()
}

fn store_card<'a>(game: &'a StoreGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(28)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.16)),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    };

    let mut meta = row![].spacing(6).align_y(iced::Center);
    if let Some(price) = game
        .price
        .as_deref()
        .filter(|price| !price.trim().is_empty())
    {
        meta = meta.push(
            text(price)
                .size(11)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::SUCCESS),
        );
    }
    if let Some(source) = game
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        meta = meta.push(badge(source, theme::TEXT_3));
    }
    if !game.genres.is_empty() {
        meta = meta.push(
            text(ellipsize(&game.genres.join(", "), 18))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    let info = container(
        column![
            container(
                text(ellipsize(&game.title, 28))
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.88))
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .height(19)
            .clip(true),
            meta,
        ]
        .spacing(3),
    )
    .width(Fill)
    .height(STORE_CARD_INFO_HEIGHT)
    .padding([8.0, 11.0]);

    button(
        column![
            container(cover)
                .width(Fill)
                .height(STORE_COVER_HEIGHT)
                .clip(true),
            info,
        ]
        .width(Fill),
    )
    .on_press(Message::StoreOpenDetail(game.clone()))
    .width(Fill)
    .padding(0)
    .style(theme::card)
    .into()
}

// ── Ficha da loja ───────────────────────────────────────────────────────────

fn detail_view<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let game = &detail.game;
    let info = detail.info.as_ref();
    let backdrop = info
        .and_then(|info| info.background_url.as_deref())
        .or_else(|| info.and_then(|info| info.header_url.as_deref()))
        .or(game.hero_url.as_deref())
        .or(game.cover_url.as_deref());

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

    let portrait = detail_portrait(game, &app.covers);
    let mut meta = row![].spacing(8).align_y(iced::Center);
    if let Some(price) = game
        .price
        .as_deref()
        .filter(|price| !price.trim().is_empty())
    {
        meta = meta.push(
            text(price)
                .size(14)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::SUCCESS),
        );
    } else {
        meta = meta.push(text("Preço não informado").size(12).color(theme::TEXT_3));
    }
    if let Some(source) = game.source.as_deref() {
        meta = meta.push(badge(source, theme::TEXT_2));
    }
    if game.manifest {
        meta = meta.push(badge("Manifesto disponível", theme::ACCENT));
    }

    let hero_copy = container(
        row![
            portrait,
            column![
                text(game.title.as_str())
                    .size(34)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(format!("appid Steam · {}", game.appid))
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
        .height(DETAIL_HERO_HEIGHT)
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
                    text("Voltar para a loja")
                        .size(12)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_1),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(Message::StoreCloseDetail)
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
                Some(Message::StoreOpenDetail(game.clone())),
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

    let description = detail_description(detail, info);
    let install = install_panel(detail);
    let details = container(
        row![
            column![description, install]
                .spacing(theme::SPACE_4)
                .width(Fill),
            info_panel(info, game, detail.status == LoadStatus::Loading),
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

fn detail_portrait<'a>(game: &'a StoreGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let url = game.portrait_url.as_deref().or(game.cover_url.as_deref());
    match covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(116)
            .height(164)
            .content_fit(ContentFit::Cover)
            .border_radius(3)
            .into(),
        None => container(
            text(initial(&game.title))
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

fn detail_description<'a>(
    detail: &'a StoreDetailState,
    info: Option<&'a StoreInfo>,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = match (
        detail.status.clone(),
        info.and_then(|info| info.description.as_deref()),
    ) {
        (LoadStatus::Loading, _) => text("Carregando descrição da loja...")
            .size(12)
            .color(theme::TEXT_2)
            .into(),
        (LoadStatus::Error(_), _) => text("Não foi possível carregar a descrição.")
            .size(12)
            .color(theme::TEXT_2)
            .into(),
        (_, Some(description)) => text(description)
            .size(13)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.74))
            .wrapping(Wrapping::WordOrGlyph)
            .into(),
        (_, None) => text("A loja não informou uma descrição para este jogo.")
            .size(12)
            .color(theme::TEXT_3)
            .into(),
    };

    container(column![views::section_label("Sobre o jogo"), body,].spacing(theme::SPACE_3))
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn install_panel<'a>(detail: &'a StoreDetailState) -> Element<'a, Message> {
    let mut content = column![views::section_label("Instalação")].spacing(theme::SPACE_3);

    match detail.install_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            content = content.push(
                text("Consultando o manifesto de instalação (store:installInfo)...")
                    .size(12)
                    .color(theme::TEXT_2)
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }
        LoadStatus::Error(_) => {
            content = content.push(views::action_error_banner(
                detail
                    .install_error
                    .as_deref()
                    .unwrap_or("O dispatcher não respondeu store:installInfo para este jogo."),
                Some(Message::StoreOpenDetail(detail.game.clone())),
            ));
        }
        LoadStatus::Ready => {
            if let Some(info) = detail.install.as_ref() {
                content = content.push(install_summary(info));
                content = content.push(libraries_block(detail));
                content = content.push(depot_picker(detail, info));
                content = content.push(install_actions(detail));
            } else {
                content = content.push(
                    text("store:installInfo respondeu sem manifesto utilizável para este jogo.")
                        .size(12)
                        .color(theme::TEXT_3),
                );
            }
        }
    }

    if let Some(message) = detail.install_message.as_deref() {
        content = content.push(
            container(text(message).size(12).color(theme::SUCCESS).width(Fill))
                .width(Fill)
                .padding([9.0, 12.0])
                .style(theme::stub_panel),
        );
    }
    if detail.install_status == LoadStatus::Ready {
        if let Some(error) = detail.install_error.as_deref() {
            content = content.push(views::action_error_banner(error, None));
        }
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn install_summary(info: &StoreInstallInfo) -> Element<'_, Message> {
    let mut rows = column![].spacing(theme::SPACE_2);
    rows = rows.push(info_row(
        "Manifesto",
        format!(
            "{} depot(s) · {} no total",
            info.depots.len(),
            format_bytes(info.total_size())
        ),
    ));
    if let Some(source) = info.source.as_deref().filter(|source| !source.is_empty()) {
        rows = rows.push(info_row("Fonte", source.to_string()));
    }
    if let Some(dir) = info
        .install_dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
    {
        rows = rows.push(info_row("Pasta sugerida", dir.to_string()));
    }
    rows = rows.push(info_row(
        "Token",
        if info.token.as_deref().is_some_and(|token| !token.is_empty()) {
            "presente no manifesto".to_string()
        } else {
            "não informado".to_string()
        },
    ));

    container(rows)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn libraries_block<'a>(detail: &'a StoreDetailState) -> Element<'a, Message> {
    let mut content = column![views::section_label("Bibliotecas Steam")].spacing(theme::SPACE_2);

    match detail.libraries_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            content = content.push(
                text("Consultando bibliotecas (store:libraries)...")
                    .size(12)
                    .color(theme::TEXT_2),
            );
        }
        LoadStatus::Error(_) => {
            content = content.push(views::action_error_banner(
                detail
                    .libraries_error
                    .as_deref()
                    .unwrap_or("O dispatcher não respondeu store:libraries."),
                Some(Message::StoreOpenDetail(detail.game.clone())),
            ));
        }
        LoadStatus::Ready if detail.libraries.is_empty() => {
            content = content.push(
                text(
                    "Nenhuma biblioteca Steam encontrada. Sem steamapps não há como \
                     enfileirar o store:install.",
                )
                .size(12)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
            );
        }
        LoadStatus::Ready => {
            for (index, library) in detail.libraries.iter().enumerate() {
                let selected = index == detail.selected_library;
                content = content.push(
                    button(
                        row![
                            column![
                                text(library.steam_dir.as_str())
                                    .size(12)
                                    .color(if selected {
                                        theme::TEXT_1
                                    } else {
                                        theme::TEXT_STRONG
                                    })
                                    .wrapping(Wrapping::WordOrGlyph),
                                text(format!(
                                    "{} livres · {}",
                                    format_bytes(library.free),
                                    ellipsize(&library.path, 64)
                                ))
                                .size(10)
                                .color(theme::TEXT_3)
                                .wrapping(Wrapping::WordOrGlyph),
                            ]
                            .spacing(2)
                            .width(Fill),
                            text(if selected { "Selecionada" } else { "Usar" })
                                .size(10)
                                .font(theme::FONT_MEDIUM)
                                .color(if selected { theme::ACCENT } else { theme::TEXT_3 }),
                        ]
                        .spacing(theme::SPACE_2)
                        .align_y(iced::Center),
                    )
                    .on_press(Message::StoreLibrarySelected(index))
                    .width(Fill)
                    .padding([7.0, 10.0])
                    .style(move |style_theme, status| {
                        theme::segment_button(style_theme, status, selected)
                    }),
                );
            }
        }
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn depot_picker<'a>(
    detail: &'a StoreDetailState,
    info: &'a StoreInstallInfo,
) -> Element<'a, Message> {
    let relevant = info.relevant_depots();
    let hidden = info.depots.len().saturating_sub(relevant.len());
    let selected_size: u64 = info
        .depots
        .iter()
        .filter(|depot| detail.selected_depots.contains(&depot.depot_id))
        .map(|depot| depot.size)
        .sum();

    let mut content = column![
        row![
            views::section_label("Depots"),
            Space::new().width(Fill),
            text(format!(
                "{} de {} · {}",
                detail
                    .selected_depots
                    .iter()
                    .filter(|id| info.depots.iter().any(|depot| &depot.depot_id == *id))
                    .count(),
                info.depots.len(),
                format_bytes(selected_size)
            ))
            .size(10)
            .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
        row![
            button(text("Recomendados").size(10).font(theme::FONT_MEDIUM))
                .on_press(Message::StoreRecommendedDepots)
                .padding([4.0, 9.0])
                .style(theme::ghost_button),
            button(text("Limpar").size(10).font(theme::FONT_MEDIUM))
                .on_press(Message::StoreClearDepots)
                .padding([4.0, 9.0])
                .style(theme::ghost_button),
        ]
        .spacing(6),
    ]
    .spacing(6);

    if relevant.is_empty() {
        content = content.push(
            text("O manifesto não listou depots utilizáveis para este jogo.")
                .size(12)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        );
    } else {
        for depot in relevant {
            let selected = detail.selected_depots.contains(&depot.depot_id);
            content = content.push(
                button(
                    row![
                        text(if selected { "✓" } else { "○" })
                            .size(12)
                            .font(theme::FONT_BOLD)
                            .color(if selected { theme::ACCENT } else { theme::TEXT_3 }),
                        text(depot_label(depot))
                            .size(12)
                            .color(if selected {
                                theme::TEXT_1
                            } else {
                                theme::TEXT_STRONG
                            })
                            .wrapping(Wrapping::None),
                        Space::new().width(Fill),
                        text(depot_meta(depot)).size(10).color(theme::TEXT_3),
                    ]
                    .spacing(theme::SPACE_2)
                    .align_y(iced::Center),
                )
                .on_press(Message::StoreDepotToggled(depot.depot_id.clone()))
                .width(Fill)
                .padding([6.0, 8.0])
                .style(move |style_theme, status| {
                    theme::game_item(style_theme, status, selected)
                }),
            );
        }
    }

    if hidden > 0 {
        content = content.push(
            text(format!(
                "{hidden} depot(s) sem metadados, compartilhado(s) ou de macOS fora da lista."
            ))
            .size(10)
            .color(theme::TEXT_3),
        );
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn install_actions<'a>(detail: &'a StoreDetailState) -> Element<'a, Message> {
    let blocked = install_block_reason(detail);
    let label = if detail.install_busy {
        "Enfileirando..."
    } else {
        "Instalar"
    };
    let mut install = button(text(label).size(12).font(theme::FONT_MEDIUM)).padding([9.0, 18.0]);
    if blocked.is_none() {
        install = install
            .on_press(Message::StoreInstallRequested)
            .style(theme::primary_button);
    } else {
        install = install.style(theme::ghost_button);
    }

    let mut actions = row![install].spacing(theme::SPACE_3).align_y(iced::Center);
    if let Some(reason) = blocked {
        actions = actions.push(
            text(reason)
                .size(11)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    actions.into()
}

fn install_block_reason(detail: &StoreDetailState) -> Option<String> {
    if detail.install_busy {
        return Some("Aguardando o dispatcher aceitar o pedido de instalação.".to_string());
    }
    match detail.install_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            return Some("Consultando store:installInfo...".to_string())
        }
        LoadStatus::Error(_) => {
            return Some(
                "Manifesto indisponível; o motivo do dispatcher aparece acima.".to_string(),
            )
        }
        LoadStatus::Ready => {}
    }

    match detail.libraries_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            return Some("Consultando store:libraries...".to_string())
        }
        LoadStatus::Error(_) => {
            return Some("Bibliotecas indisponíveis; o motivo do dispatcher aparece acima.".to_string())
        }
        LoadStatus::Ready => {}
    }
    if detail.libraries.is_empty() {
        return Some("Nenhuma biblioteca Steam em store:libraries.".to_string());
    }
    let Some(info) = detail.install.as_ref() else {
        return Some("Manifesto de instalação ausente.".to_string());
    };
    if info.depots.is_empty() {
        return Some("store:installInfo não retornou depots para este jogo.".to_string());
    }
    if !info
        .depots
        .iter()
        .any(|depot| detail.selected_depots.contains(&depot.depot_id))
    {
        return Some("Selecione ao menos um depot.".to_string());
    }
    None
}

fn depot_label(depot: &StoreDepot) -> String {
    if let Some(language) = depot.language.as_deref().filter(|language| !language.is_empty()) {
        let mut characters = language.chars();
        return match characters.next() {
            Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
            None => language.to_string(),
        };
    }
    if let Some(name) = depot.name.as_deref().filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    if let Some(dlc) = depot.dlc_appid.as_deref().filter(|dlc| !dlc.is_empty()) {
        return format!("DLC {dlc}");
    }
    format!("Depot {}", depot.depot_id)
}

fn depot_meta(depot: &StoreDepot) -> String {
    let mut parts = Vec::with_capacity(3);
    if let Some(os) = depot.os.as_deref().filter(|os| !os.is_empty()) {
        parts.push(os.to_uppercase());
    }
    parts.push(format_bytes(depot.size));
    if depot.optional {
        parts.push("opcional".to_string());
    }
    parts.join(" · ")
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let gib = bytes as f64 / GIB;
    if gib >= 1.0 {
        format!("{gib:.1} GiB")
    } else {
        format!("{:.0} MiB", bytes as f64 / MIB)
    }
}

fn info_panel<'a>(
    info: Option<&'a StoreInfo>,
    game: &'a StoreGame,
    loading: bool,
) -> Element<'a, Message> {
    let mut rows = column![].spacing(theme::SPACE_2);
    let mut has_row = false;

    if let Some(info) = info {
        if !info.developers.is_empty() {
            rows = rows.push(info_row("Desenvolvedora", info.developers.join(", ")));
            has_row = true;
        }
        if !info.publishers.is_empty() {
            rows = rows.push(info_row("Publicadora", info.publishers.join(", ")));
            has_row = true;
        }
        if let Some(release) = info.release_date.as_deref() {
            rows = rows.push(info_row("Lançamento", release.to_string()));
            has_row = true;
        }
    }
    if !game.genres.is_empty() {
        rows = rows.push(info_row("Gêneros", game.genres.join(", ")));
        has_row = true;
    }
    if !has_row {
        rows = rows.push(
            text(if loading {
                "Consultando metadados em game:sysinfo..."
            } else {
                "A loja não retornou metadados adicionais."
            })
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

// ── Helpers visuais ─────────────────────────────────────────────────────────

fn badge<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
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

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}
