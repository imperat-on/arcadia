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
    button, column, container, image, row, scrollable, text, text_input, Sensor, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, LoadStatus, Message, Section, StoreDetailState, StoreSearchState, StoreTab};
use crate::backend::{StoreDepot, StoreGame, StoreInfo, StoreInstallInfo};
use crate::covers::CoverCache;
use crate::theme;
use crate::views;
use crate::views::icons::{self, Icon};

const STORE_ROW_ART_WIDTH: f32 = 128.0;
const STORE_ROW_ART_HEIGHT: f32 = 64.0;
/// Altura da linha de resultado: casa com `.store-catalog-row { min-height: 68px }`
/// da `ui/src/index.css`. A capa 2:1 do `.store-catalog-art` (h-16 w-32) ocupa
/// os 64 px e sobra 2 px de borda acima e abaixo.
const STORE_ROW_HEIGHT: f32 = 68.0;
const THUMBNAIL_LOOKAHEAD: f32 = 2.0 * (STORE_ROW_HEIGHT + 4.0);

pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(detail) = app.store.detail.as_ref() {
        return detail_view(app, detail);
    }

    let state = &app.store;
    let tabs = tab_strip(app);
    match state.tab {
        StoreTab::Retro => column![tabs, views::retro::view(app)]
            .width(Fill)
            .height(Fill)
            .into(),
        StoreTab::Steam => column![tabs, steam_body(app)]
            .width(Fill)
            .height(Fill)
            .into(),
    }
}

// ── Segmented control ───────────────────────────────────────────────────────

fn tab_strip(app: &App) -> Element<'_, Message> {
    let active = app.store.tab;
    let control = container(
        row![
            tab_button(app, StoreTab::Steam, active),
            tab_button(app, StoreTab::Retro, active),
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

fn tab_label<'a>(app: &'a App, tab: StoreTab) -> &'a str {
    tab.label(&app.i18n)
}

fn tab_button<'a>(app: &'a App, tab: StoreTab, active: StoreTab) -> Element<'a, Message> {
    let selected = tab == active;
    let label = tab_label(app, tab);
    button(
        text(label)
            .size(theme::TEXT_BODY)
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
    let i18n = &app.i18n;

    // Cabeçalho da `StoreView.tsx`: `ui-title` (1.5rem / 650) seguido do
    // `ui-subtitle` a 14 px em branco 42%.
    let heading = column![
        text(i18n.t("native.store.title"))
            .size(24)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(i18n.t("native.store.description"))
            .size(theme::TEXT_READ)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.42)),
    ]
    .spacing(2);

    let mut content = column![heading, search_section(app)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if !state.suggestions.is_empty() {
        content = content.push(suggestions_panel(app));
    }

    content = content.push(results_heading(app));

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

fn search_section(app: &App) -> Element<'_, Message> {
    let state = &app.store;
    let i18n = &app.i18n;
    let input = Stack::new()
        .width(Fill)
        .push(
            text_input(i18n.t("native.store.search_placeholder"), &state.query)
                .on_input(Message::StoreQueryChanged)
                .on_submit(Message::StoreSearchSubmitted)
                .padding(Padding {
                    top: 10.0,
                    right: 14.0,
                    bottom: 10.0,
                    left: 38.0,
                })
                .size(theme::TEXT_READ)
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
        text(if busy {
            i18n.t("native.store.searching")
        } else {
            i18n.t("native.store.search_action")
        })
        .size(theme::TEXT_BODY)
        .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 18.0]);
    if !busy && !state.query.trim().is_empty() {
        search_button = search_button.on_press(Message::StoreSearchSubmitted);
    }

    let mut clear_button = button(
        text(i18n.t("native.console.store.clear"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 14.0])
    .style(theme::ghost_button);
    if !state.query.is_empty() || state.search.is_some() {
        clear_button = clear_button.on_press(Message::StoreClearSearch);
    }

    // `desktop-fluid-search ... max-w-[860px]` da `StoreView.tsx`: a busca não
    // estica pela largura toda da janela.
    container(
        row![
            input,
            search_button.style(theme::primary_button),
            clear_button
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    )
    .width(Fill)
    .max_width(860.0)
    .into()
}

fn suggestions_panel(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(1).width(Fill);
    for game in &app.store.suggestions {
        list = list.push(suggestion_row(&app.i18n, game, &app.store_thumbnails));
    }

    container(list)
        .width(Fill)
        .padding(4)
        .style(theme::stub_panel)
        .into()
}

fn suggestion_row<'a>(
    i18n: &'a crate::i18n::I18n,
    game: &'a StoreGame,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let thumb = store_thumbnail(game.cover_url.as_deref(), covers, |handle| match handle {
        Some(handle) => image(handle)
            .width(48)
            .height(27)
            .content_fit(ContentFit::Cover)
            .border_radius(2)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2),
        )
        .width(48)
        .height(27)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::game_icon_placeholder)
        .into(),
    });

    button(
        row![
            thumb,
            text(game.title.as_str())
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_STRONG)
                .wrapping(Wrapping::None),
            Space::new().width(Fill),
            text(i18n.t("native.store.search_action"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
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

fn results_heading(app: &App) -> Element<'_, Message> {
    let state = &app.store;
    let i18n = &app.i18n;

    // Total real do catálogo (`browse.total`), como `store.resultados_count`
    // da web; a busca mostra a contagem devolvida por `store:search`.
    let count = match &state.search {
        Some(search) => search.result.as_ref().map(|result| result.games.len()),
        None => state
            .browse
            .total
            .map(|total| total as usize)
            .or(Some(state.browse.games.len())),
    };

    let mut row = row![text(i18n.t_args(
        "native.store.results_count",
        &[("count", &count.unwrap_or(0).to_string())],
    ))
    .size(theme::TEXT_READ)
    .font(theme::FONT_MEDIUM)
    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.65)),]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    if let Some(search) = state.search.as_ref() {
        row = row.push(
            text(i18n.t_args("native.store.results_for", &[("query", &search.query)]))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3),
        );
        if let Some(source) = search
            .result
            .as_ref()
            .and_then(|result| result.source.as_deref())
        {
            row = row.push(badge(source, theme::TEXT_2));
        }
    }

    let mut header = row.width(Fill).push(Space::new().width(Fill));

    if count.unwrap_or(0) > 0 {
        header = header.push(
            text(i18n.t("native.store.scroll_hint"))
                .size(theme::TEXT_MICRO)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.35)),
        );
    }

    header.into()
}

fn results_body(app: &App) -> Element<'_, Message> {
    let state = &app.store;

    if let Some(search) = state.search.as_ref() {
        return search_results(app, search);
    }

    let browse = &state.browse;
    match browse.status {
        LoadStatus::Idle | LoadStatus::Loading if browse.games.is_empty() => {
            views::loading_state(app.i18n.t("native.store.catalog_loading"))
        }
        LoadStatus::Error(_) if browse.games.is_empty() => container(views::error_state_labeled(
            app.i18n.t("native.store.catalog_error"),
            browse
                .error
                .as_deref()
                .unwrap_or(app.i18n.t("native.store.catalog_error_detail")),
            Message::Refresh(Section::Stores),
            app.i18n.t("native.store.try_again"),
        ))
        .width(Fill)
        .into(),
        _ if browse.games.is_empty() => inline_empty(
            app.i18n.t("native.store.catalog_empty_title"),
            app.i18n.t("native.store.catalog_empty_desc"),
        ),
        _ => browse_grid(app),
    }
}

fn search_results<'a>(app: &'a App, search: &'a StoreSearchState) -> Element<'a, Message> {
    match search.status {
        LoadStatus::Loading => views::loading_state(app.i18n.t("native.store.searching")),
        LoadStatus::Error(_) => container(views::error_state_labeled(
            app.i18n.t("native.store.search_error"),
            search
                .error
                .as_deref()
                .unwrap_or(app.i18n.t("native.store.search_error_detail")),
            Message::StoreSearchSubmitted,
            app.i18n.t("native.store.try_again"),
        ))
        .width(Fill)
        .into(),
        LoadStatus::Idle => views::loading_state(app.i18n.t("native.store.preparing_search")),
        LoadStatus::Ready => {
            let games = search
                .result
                .as_ref()
                .map(|result| result.games.as_slice())
                .unwrap_or_default();
            if games.is_empty() {
                inline_empty(
                    app.i18n.t("native.store.empty_title"),
                    app.i18n.t("native.store.empty_desc"),
                )
            } else {
                store_grid(games, &app.store_thumbnails)
            }
        }
    }
}

fn browse_grid(app: &App) -> Element<'_, Message> {
    let browse = &app.store.browse;
    let mut content = column![store_grid(&browse.games, &app.store_thumbnails)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if browse.has_more {
        let label = if browse.loading_more {
            app.i18n.t("native.store.loading_more")
        } else {
            app.i18n.t("native.store.load_more")
        };
        let mut more = button(text(label).size(theme::TEXT_BODY).font(theme::FONT_MEDIUM))
            .padding([9.0, 18.0]);
        if !browse.loading_more {
            more = more.on_press(Message::StoreLoadMore);
        }
        content = content.push(
            Sensor::new(container(more.style(theme::ghost_button)).width(Fill).center_x(Fill))
                .key(browse.next_offset)
                .anticipate(4.0 * (STORE_ROW_HEIGHT + 4.0))
                .on_show(|_| Message::StorePrefetchNext),
        );
    } else {
        content = content.push(
            container(
                text(app.i18n.t("native.store.end_catalog"))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_3),
            )
            .width(Fill)
            .center_x(Fill),
        );
    }

    content.into()
}

fn store_grid<'a>(games: &'a [StoreGame], covers: &'a CoverCache) -> Element<'a, Message> {
    // `store-catalog-list flex flex-col gap-1`: uma coluna de linhas densas,
    // não uma grade de cartões. A busca e o catálogo usam a mesma lista.
    let mut list = column![].width(Fill).spacing(4);
    for game in games {
        list = list.push(store_row(game, covers));
    }
    list.into()
}

/// Uma linha do catálogo, porte do `LinhaLoja` de `storeShared.tsx`:
/// capa 2:1 (128×64) à esquerda, título e gêneros empilhados no meio, fonte
/// em etiqueta e check de biblioteca à direita.
fn store_row<'a>(game: &'a StoreGame, covers: &'a CoverCache) -> Element<'a, Message> {
    let art = store_thumbnail(game.cover_url.as_deref(), covers, |handle| match handle {
        Some(handle) => image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(theme::TEXT_TITLE)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.14)),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::cover_placeholder)
        .into(),
    });

    // `.store-catalog-art`: 128×64 com linha à direita separando do texto.
    let art = container(art)
        .width(Length::Fixed(STORE_ROW_ART_WIDTH))
        .height(Length::Fixed(STORE_ROW_ART_HEIGHT))
        .clip(true)
        .style(theme::store_row_art);

    let mut copy = column![text(game.title.as_str())
        .size(theme::TEXT_READ)
        .font(theme::FONT_MEDIUM)
        .color(Color::from_rgba(1.0, 1.0, 1.0, 0.90))
        .wrapping(Wrapping::None)]
    .spacing(3)
    .width(Fill);

    let genres = game.genres.join(", ");
    if !genres.trim().is_empty() {
        copy = copy.push(
            text(ellipsize(&genres, 48))
                .size(theme::TEXT_MICRO)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.50))
                .wrapping(Wrapping::None),
        );
    }

    if let Some(source) = game
        .source
        .as_deref()
        .filter(|source| !source.trim().is_empty())
    {
        copy = copy.push(
            container(
                text(ellipsize(source, 28))
                    .size(theme::TEXT_MICRO)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.65))
                    .wrapping(Wrapping::None),
            )
            .padding([2.0, 6.0])
            .style(theme::store_row_tag),
        );
    }

    // Coluna final de 40 px com o check quando o jogo já está na biblioteca.
    let trailer: Element<'a, Message> = if game.manifest {
        container(
            text("✓")
                .size(theme::TEXT_READ)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.55)),
        )
        .width(Length::Fixed(40.0))
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .into()
    } else {
        Space::new().width(Length::Fixed(40.0)).into()
    };

    iced::widget::mouse_area(button(
        container(
            row![art, copy, trailer]
                .spacing(theme::SPACE_3)
                .align_y(iced::Center),
        )
        .width(Fill)
        .height(Length::Fixed(STORE_ROW_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: theme::SPACE_3,
            bottom: 0.0,
            left: 0.0,
        }),
    )
    .on_press(Message::StoreOpenDetail(game.clone()))
    .width(Fill)
    .padding(0)
    .style(theme::store_row))
    .on_enter(Message::PrefetchStore(game.clone()))
    .into()
}

fn store_thumbnail<'a>(
    url: Option<&'a str>,
    covers: &CoverCache,
    content: impl FnOnce(Option<image::Handle>) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(url) = url else { return content(None) };
    if let Some(handle) = covers.ready_cached(url) {
        return content(Some(handle.clone()));
    }
    Sensor::new(content(None))
        .key_ref(url)
        .anticipate(THUMBNAIL_LOOKAHEAD)
        .on_show(move |_| Message::StoreThumbnailVisible(url.to_owned()))
        .on_hide(Message::StoreThumbnailHidden(url.to_owned()))
        .into()
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;
    use iced::advanced::layout::{Layout, Limits};
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::Tree;
    use iced::advanced::{clipboard, mouse, Shell};
    use iced::{Event, Font, Pixels, Rectangle, Renderer, Size};

    #[tokio::test]
    async fn rows_request_thumbnails_only_near_the_scroll_viewport() {
        let renderer = Renderer::new(Font::DEFAULT, Pixels(14.0), Some("tiny-skia"))
            .await
            .expect("software renderer");
        let covers = CoverCache::default();
        let games: Vec<_> = (1..=24)
            .map(|index| StoreGame {
                title: format!("Game {index}"),
                cover_url: Some(format!("https://example.test/{index}.jpg")),
                ..StoreGame::default()
            })
            .collect();
        let mut scroller: Element<'_, Message> =
            scrollable(store_grid(&games, &covers)).height(Fill).into();
        let mut tree = Tree::new(&scroller);
        let size = Size::new(780.0, 430.0);
        let layout = scroller
            .as_widget_mut()
            .layout(&mut tree, &renderer, &Limits::new(Size::ZERO, size));
        let mut messages = Vec::new();
        scroller.as_widget_mut().update(
            &mut tree,
            &Event::Window(iced::window::Event::RedrawRequested(std::time::Instant::now())),
            Layout::new(&layout),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(size),
        );
        let requested: Vec<_> = messages
            .into_iter()
            .filter_map(|message| match message {
                Message::StoreThumbnailVisible(url) => Some(url),
                _ => None,
            })
            .collect();
        assert_eq!(requested.len(), 8);
        assert_eq!(requested[0], "https://example.test/1.jpg");
        assert_eq!(requested[7], "https://example.test/8.jpg");
        assert!(covers.take_misses().is_empty());
    }
}

// ── Ficha da loja ───────────────────────────────────────────────────────────

pub(crate) fn detail_view<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let game = &detail.game;
    let info = detail.info.as_ref();
    let i18n = &app.i18n;

    let backdrop = info
        .and_then(|info| info.background_url.clone())
        .or_else(|| info.and_then(|info| info.header_url.clone()))
        .or_else(|| game.hero_url.clone())
        .or_else(|| game.cover_url.clone());
    // Portrait: o primeiro candidato que já estiver no cache vence. O caminho
    // por convenção (`library_600x900`) dá 404 em jogo novo, então tentar só
    // ele deixava a ficha na letra até o fallback terminar.
    let portrait = crate::backend::store_portrait_candidates(game)
        .into_iter()
        .find(|url| app.covers.ready(Some(url)).is_some())
        .or_else(|| crate::backend::store_portrait_url(game));

    let mut chips = Vec::new();
    if let Some(release) = info.and_then(|info| info.release_date.as_deref()) {
        chips.push(super::game_detail::chip(release.to_string(), false));
    }
    if let Some(developer) = info
        .and_then(|info| info.developers.first())
        .filter(|developer| !developer.trim().is_empty())
    {
        chips.push(super::game_detail::chip(developer.clone(), false));
    }
    if let Some(publisher) = info
        .and_then(|info| info.publishers.first())
        .filter(|publisher| {
            !publisher.trim().is_empty()
                && info
                    .and_then(|info| info.developers.first())
                    .is_none_or(|developer| developer != *publisher)
        })
    {
        chips.push(super::game_detail::chip(publisher.clone(), false));
    }
    if let Some(price) = game
        .price
        .as_deref()
        .filter(|price| !price.trim().is_empty())
    {
        chips.push(super::game_detail::chip(price.to_string(), true));
    }

    let mut left = Vec::new();
    if let Some(error) = detail.error.as_deref() {
        left.push(views::action_error_banner(
            error,
            Some(Message::StoreOpenDetail(game.clone())),
        ));
    }
    left.push(store_release_box(app, info, game));
    let movies = info.map(|info| info.movies.as_slice()).unwrap_or_default();
    let screenshots = info
        .map(|info| info.screenshots.as_slice())
        .unwrap_or_default();
    let gallery = super::game_detail::gallery_panel(
        i18n,
        &app.media,
        &app.covers,
        movies,
        screenshots,
        detail.selected_screenshot,
        app.gallery_trailer,
        Message::StoreSelectTrailer,
        Message::StoreScreenshotSelected,
        Vec::new(),
    );
    let showing_video = app.gallery_trailer && (!movies.is_empty() || app.media.is_active());
    let current = if showing_video { None } else {
        screenshots.get(detail.selected_screenshot.min(screenshots.len().saturating_sub(1)))
            .and_then(|screenshot| app.covers.ready(screenshot.full_or_thumb())).cloned()
    };
    left.push(super::game::gallery_motion::wrap(gallery, game.appid.to_string(), current));
    left.push(detail_description(app, detail, info));

    let community_appid = crate::backend::community_appid(&game.appid);
    if let Some(community) = app.community.get(&community_appid) {
        left.push(views::community::panel(
            i18n,
            game.title.as_str(),
            community,
        ));
    }

    let panels = app.game_panels.get(&crate::app::store_panels_id(game));
    let mut right = vec![
        super::store_downloads::summary(app, detail),
        super::game::protondb_panel(i18n, panels),
        super::game::stats_reviews_panel(i18n, panels),
        super::game::achievements_panel(i18n, panels, &app.covers),
    ];
    if let Some(requirements) = super::game_detail::requirements_panel(
        i18n,
        info.and_then(|info| info.req_min.as_deref()),
        info.and_then(|info| info.req_rec.as_deref()),
        app.requirements_tab,
    ) {
        right.push(requirements);
    }
    if let Some(languages) =
        super::game_detail::languages_panel(i18n, info.and_then(|info| info.languages.as_deref()))
    {
        right.push(languages);
    }
    // O painel de manifesto continua na coluna: é onde se escolhe biblioteca
    // e depot. Os botões da web ficam na barra do herói.
    right.push(install_panel(app, detail));

    super::game_detail::view(
        app.window_size.height * 0.46,
        &app.covers,
        backdrop,
        portrait,
        game.title.as_str(),
        chips,
        Message::StoreCloseDetail,
        Some(store_action_bar(app, detail)),
        left,
        right,
    )
}

/// Barra da `StoreGamePage.tsx`: "Adicionar à biblioteca" fantasma e
/// "Baixar" usa tanto as ofertas das fontes quanto os depots Steam.
fn store_action_bar<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let in_library = app.library.games.iter().any(|game| {
        game.steam_appid() == Some(detail.game.appid.as_str())
    });
    let can_download = super::store_downloads::available(detail)
        || install_block_reason(i18n, detail).is_none();
    let searching = matches!(detail.downloads.status, LoadStatus::Idle | LoadStatus::Loading);

    let mut actions = row![].spacing(8).align_y(iced::Center);

    if !can_download {
        actions = actions.push(
            text(i18n.t(if searching {
                "native.store.sources.searching"
            } else if matches!(detail.downloads.status, LoadStatus::Error(_)) {
                "native.store.sources.failed"
            } else {
                "store.nenhum_download"
            }))
                .size(12.0)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.45)),
        );
    }

    if !in_library {
        let message = (!detail.install_busy).then_some(Message::StoreAddToLibrary);
        actions = actions.push(store_ghost_button(
            i18n.t("store.adicionar_biblioteca"),
            "+",
            message,
        ));
    }

    if can_download {
        let message = (!detail.install_busy).then_some(Message::StoreDownloadRequested);
        actions = actions.push(store_primary_button(
            if detail.install_busy {
                i18n.t("native.store.enqueueing")
            } else {
                i18n.t("store.baixar")
            },
            "↓",
            message,
        ));
    }

    if in_library {
        let message = (!detail.install_busy).then_some(Message::StoreRemoveFromLibrary);
        actions = actions.push(store_danger_button(i18n.t("common.remover"), message));
    }

    actions.into()
}

fn store_danger_button<'a>(label: &'a str, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(12.5).font(theme::FONT_SEMIBOLD))
        .on_press_maybe(message)
        .padding([8.0, 16.0])
        .style(theme::store_danger_pill)
        .into()
}

fn store_ghost_button<'a>(
    label: &'a str,
    glyph: &'a str,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(store_button_label(label, glyph, Color::from_rgba(1.0, 1.0, 1.0, 0.85)))
        .on_press_maybe(message)
        .padding([8.0, 16.0])
        .style(theme::store_ghost_pill)
        .into()
}

fn store_primary_button<'a>(
    label: &'a str,
    glyph: &'a str,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(store_button_label(label, glyph, Color::BLACK))
        .on_press_maybe(message)
        .padding([8.0, 20.0])
        .style(theme::store_primary_pill)
        .into()
}

fn store_button_label<'a>(label: &'a str, glyph: &'a str, color: Color) -> Element<'a, Message> {
    row![
        text(glyph).size(14.0).font(theme::FONT_BOLD).color(color),
        text(label).size(12.5).font(theme::FONT_BOLD).color(color),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn store_release_box<'a>(
    app: &'a App,
    info: Option<&'a StoreInfo>,
    game: &'a StoreGame,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![].spacing(4).width(Fill);

    if let Some(release) = info.and_then(|info| info.release_date.as_deref()) {
        content = content.push(
            text(i18n.t_args("native.store.released_on", &[("date", release)]))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_STRONG)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    if let Some(developer) = info
        .and_then(|info| info.developers.first())
        .filter(|developer| !developer.trim().is_empty())
    {
        content = content.push(
            text(i18n.t_args("native.store.developed_by", &[("name", developer.as_str())]))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    if let Some(publisher) = info
        .and_then(|info| info.publishers.first())
        .filter(|publisher| !publisher.trim().is_empty())
    {
        content = content.push(
            text(i18n.t_args("native.store.published_by", &[("name", publisher.as_str())]))
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    if !game.genres.is_empty() {
        content = content.push(
            text(game.genres.join(" · "))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn detail_description<'a>(
    app: &'a App,
    detail: &'a StoreDetailState,
    info: Option<&'a StoreInfo>,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let body: Element<'a, Message> = match (
        detail.status.clone(),
        info.and_then(|info| info.about.as_deref().or(info.description.as_deref())),
    ) {
        (LoadStatus::Loading, _) => text(i18n.t("native.console.store.description_loading"))
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_2)
            .into(),
        (LoadStatus::Error(_), _) => text(i18n.t("native.console.store.description_error"))
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_2)
            .into(),
        (_, Some(description)) => super::game::description_blocks(
            crate::rich_text::parse_description(description),
            &app.covers,
            &app.animated_cards,
        ),
        (_, None) => text(i18n.t("native.console.store.description_empty"))
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_3)
            .into(),
    };

    container(
        column![views::section_label(i18n.t("native.game.about")), body,].spacing(theme::SPACE_3),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel)
    .into()
}

/// O dispatcher responde "nenhum provedor disponível" quando a loja não tem
/// backend de instalação para o appid. Isso não é um erro de rede recuperável.
fn install_provider_missing(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("nenhum provedor")
        || lower.contains("no provider")
        || lower.contains("provider unavailable")
}

fn install_panel<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![views::section_label(i18n.t("native.store.install_section"))]
        .spacing(theme::SPACE_3);

    match detail.install_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            content = content.push(
                text(i18n.t("native.store.install_loading"))
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_2)
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }
        LoadStatus::Error(_) => {
            let error = detail
                .install_error
                .as_deref()
                .unwrap_or(i18n.t("native.store.install_error_detail"));
            if install_provider_missing(error) {
                // A ausência de provedor Steam é um estado normal da loja, não
                // uma falha para pintar de vermelho com "tentar novamente".
                content = content.push(
                    text(i18n.t("native.store.sources.steam_unavailable"))
                        .size(theme::TEXT_BODY)
                        .color(theme::TEXT_2)
                        .wrapping(Wrapping::WordOrGlyph),
                );
            } else {
                content = content.push(views::action_error_banner_labeled(
                    error,
                    Some(Message::StoreOpenDetail(detail.game.clone())),
                    i18n.t("native.store.try_again"),
                ));
            }
        }
        LoadStatus::Ready => {
            if let Some(info) = detail.install.as_ref() {
                content = content.push(install_summary(app, info));
                content = content.push(libraries_block(app, detail));
                content = content.push(depot_picker(app, detail, info));
                content = content.push(install_actions(app, detail));
            } else {
                content = content.push(
                    text(i18n.t("native.store.install_empty_manifest"))
                        .size(theme::TEXT_BODY)
                        .color(theme::TEXT_3),
                );
            }
        }
    }

    if let Some(message) = detail.install_message.as_deref() {
        content = content.push(
            container(
                text(message)
                    .size(theme::TEXT_BODY)
                    .color(theme::SUCCESS)
                    .width(Fill),
            )
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

fn install_summary<'a>(app: &'a App, info: &'a StoreInstallInfo) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut rows = column![].spacing(theme::SPACE_2);
    rows = rows.push(info_row(
        i18n.t("native.store.manifest_row"),
        i18n.t_args(
            "native.store.manifest_row_value",
            &[
                ("depots", &info.depots.len().to_string()),
                ("size", &format_bytes(info.total_size())),
            ],
        ),
    ));
    if let Some(source) = info.source.as_deref().filter(|source| !source.is_empty()) {
        rows = rows.push(info_row(i18n.t("native.store.source"), source.to_string()));
    }
    if let Some(dir) = info
        .install_dir
        .as_deref()
        .filter(|dir| !dir.trim().is_empty())
    {
        rows = rows.push(info_row(
            i18n.t("native.store.suggested_folder"),
            dir.to_string(),
        ));
    }
    rows = rows.push(info_row(
        i18n.t("native.store.token"),
        if info.token.as_deref().is_some_and(|token| !token.is_empty()) {
            i18n.t("native.store.token_present").to_string()
        } else {
            i18n.t("native.store.token_missing").to_string()
        },
    ));

    container(rows)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn libraries_block<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![views::section_label(i18n.t("native.store.libraries_title"))]
        .spacing(theme::SPACE_2);

    match detail.libraries_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            content = content.push(
                text(i18n.t("native.store.libraries_loading"))
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_2),
            );
        }
        LoadStatus::Error(_) => {
            content = content.push(views::action_error_banner_labeled(
                detail
                    .libraries_error
                    .as_deref()
                    .unwrap_or(i18n.t("native.store.libraries_error_detail")),
                Some(Message::StoreOpenDetail(detail.game.clone())),
                i18n.t("native.store.try_again"),
            ));
        }
        LoadStatus::Ready if detail.libraries.is_empty() => {
            content = content.push(
                text(i18n.t("native.store.libraries_empty"))
                    .size(theme::TEXT_BODY)
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
                                    .size(theme::TEXT_BODY)
                                    .color(if selected {
                                        theme::TEXT_1
                                    } else {
                                        theme::TEXT_STRONG
                                    })
                                    .wrapping(Wrapping::WordOrGlyph),
                                text(format!(
                                    "{} · {}",
                                    i18n.t_args(
                                        "native.store.free",
                                        &[("free", &format_bytes(library.free))],
                                    ),
                                    ellipsize(&library.path, 64)
                                ))
                                .size(theme::TEXT_MICRO)
                                .color(theme::TEXT_3)
                                .wrapping(Wrapping::WordOrGlyph),
                            ]
                            .spacing(2)
                            .width(Fill),
                            text(if selected {
                                i18n.t("native.store.selected")
                            } else {
                                i18n.t("native.store.use")
                            })
                            .size(theme::TEXT_MICRO)
                            .font(theme::FONT_MEDIUM)
                            .color(if selected {
                                theme::ACCENT
                            } else {
                                theme::TEXT_3
                            }),
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
    app: &'a App,
    detail: &'a StoreDetailState,
    info: &'a StoreInstallInfo,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
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
            views::section_label(i18n.t("native.store.depots_title")),
            Space::new().width(Fill),
            text(
                i18n.t_args(
                    "native.store.depots_selected",
                    &[
                        (
                            "selected",
                            &detail
                                .selected_depots
                                .iter()
                                .filter(|id| info.depots.iter().any(|depot| &depot.depot_id == *id))
                                .count()
                                .to_string(),
                        ),
                        ("total", &info.depots.len().to_string()),
                        ("size", &format_bytes(selected_size)),
                    ],
                )
            )
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
        row![
            button(
                text(i18n.t("native.store.depots_recommended"))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::StoreRecommendedDepots)
            .padding([4.0, 9.0])
            .style(theme::ghost_button),
            button(
                text(i18n.t("native.console.store.clear"))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::StoreClearDepots)
            .padding([4.0, 9.0])
            .style(theme::ghost_button),
        ]
        .spacing(6),
    ]
    .spacing(6);

    if relevant.is_empty() {
        content = content.push(
            text(i18n.t("native.store.depots_none"))
                .size(theme::TEXT_BODY)
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
                            .size(theme::TEXT_BODY)
                            .font(theme::FONT_BOLD)
                            .color(if selected {
                                theme::ACCENT
                            } else {
                                theme::TEXT_3
                            }),
                        text(depot_label(depot))
                            .size(theme::TEXT_BODY)
                            .color(if selected {
                                theme::TEXT_1
                            } else {
                                theme::TEXT_STRONG
                            })
                            .wrapping(Wrapping::None),
                        Space::new().width(Fill),
                        text(depot_meta(i18n, depot))
                            .size(theme::TEXT_MICRO)
                            .color(theme::TEXT_3),
                    ]
                    .spacing(theme::SPACE_2)
                    .align_y(iced::Center),
                )
                .on_press(Message::StoreDepotToggled(depot.depot_id.clone()))
                .width(Fill)
                .padding([6.0, 8.0])
                .style(move |style_theme, status| theme::game_item(style_theme, status, selected)),
            );
        }
    }

    if hidden > 0 {
        content = content.push(
            text(i18n.t_args(
                "native.store.depots_hidden",
                &[("count", &hidden.to_string())],
            ))
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
        );
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

fn install_actions<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let blocked = install_block_reason(i18n, detail);
    let label = if detail.install_busy {
        i18n.t("native.store.enqueueing")
    } else {
        i18n.t("native.console.store.install")
    };
    let mut install =
        button(text(label).size(theme::TEXT_BODY).font(theme::FONT_MEDIUM)).padding([9.0, 18.0]);
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
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    actions.into()
}

pub(super) fn install_block_reason(i18n: &crate::i18n::I18n, detail: &StoreDetailState) -> Option<String> {
    if detail.install_busy {
        return Some(i18n.t("native.store.install_pending").to_string());
    }
    match detail.install_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            return Some(i18n.t("native.store.install_loading").to_string())
        }
        LoadStatus::Error(_) => {
            return Some(
                i18n.t("native.store.install_manifest_unavailable")
                    .to_string(),
            )
        }
        LoadStatus::Ready => {}
    }

    match detail.libraries_status {
        LoadStatus::Idle | LoadStatus::Loading => {
            return Some(i18n.t("native.store.libraries_loading").to_string())
        }
        LoadStatus::Error(_) => {
            return Some(
                i18n.t("native.store.install_libraries_unavailable")
                    .to_string(),
            )
        }
        LoadStatus::Ready => {}
    }
    if detail.libraries.is_empty() {
        return Some(i18n.t("native.store.install_no_library").to_string());
    }
    let Some(info) = detail.install.as_ref() else {
        return Some(i18n.t("native.store.install_manifest_missing").to_string());
    };
    if info.depots.is_empty() {
        return Some(i18n.t("native.store.install_no_depots").to_string());
    }
    if !info
        .depots
        .iter()
        .any(|depot| detail.selected_depots.contains(&depot.depot_id))
    {
        return Some(i18n.t("native.store.install_select_depot").to_string());
    }
    None
}

fn depot_label(depot: &StoreDepot) -> String {
    if let Some(language) = depot
        .language
        .as_deref()
        .filter(|language| !language.is_empty())
    {
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

fn depot_meta(i18n: &crate::i18n::I18n, depot: &StoreDepot) -> String {
    let mut parts = Vec::with_capacity(3);
    if let Some(os) = depot.os.as_deref().filter(|os| !os.is_empty()) {
        parts.push(os.to_uppercase());
    }
    parts.push(format_bytes(depot.size));
    if depot.optional {
        parts.push(i18n.t("native.store.optional").to_string());
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

fn info_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    column![
        text(label).size(theme::TEXT_MICRO).color(theme::TEXT_3),
        text(value)
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_STRONG)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(2)
    .width(Fill)
    .into()
}

// ── Helpers visuais ─────────────────────────────────────────────────────────

fn badge<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
    container(
        text(label)
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_MEDIUM)
            .color(color),
    )
    .padding([2.0, 6.0])
    .style(theme::badge)
    .into()
}

fn inline_empty<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(17)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_STRONG),
            text(message).size(theme::TEXT_BODY).color(theme::TEXT_2),
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
