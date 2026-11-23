//! Loja do Big Picture: composição da referência, identidade ciano Arcadia.
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, Space, Stack,
};
use iced::{
    mouse, Color, ContentFit, Element, Fill, Length, Point, Rectangle, Renderer, Task, Theme,
};

use super::icons::{self, Icon};
use crate::app::{App, LoadStatus, Message};
use crate::backend::{self, StoreGame, StoreInfo};
use crate::big_picture::{ConsoleZone, STORE_GRID_COLS};
use crate::theme;

pub const SCROLL_ID: &str = "arcadia-console-storefront";

#[derive(Debug, Clone, Default)]
pub struct StorefrontState {
    pub category: usize,
    pub hero: usize,
    pub catalog: bool,
    pub feed: backend::StorefrontFeed,
    pub status: LoadStatus,
    pub language: Option<crate::i18n::Lang>,
    pub genre_attempted: std::collections::BTreeSet<String>,
    pub loaded_at: Option<std::time::Instant>,
}

impl StorefrontState {
    pub fn step_hero(&mut self, delta: isize, count: usize) {
        self.hero = if count == 0 {
            0
        } else {
            (self.hero as isize + delta).rem_euclid(count as isize) as usize
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub inset: f32,
    pub gap: f32,
    pub sidebar: f32,
    pub hero: f32,
    pub art: f32,
}

pub fn metrics(width: f32) -> Metrics {
    Metrics {
        inset: theme::STOREFRONT_INSET,
        gap: theme::STOREFRONT_GAP,
        sidebar: if width >= 1060.0 {
            (width * 0.215).clamp(230.0, 310.0)
        } else {
            0.0
        },
        hero: theme::STOREFRONT_HERO * (width / 1150.0).clamp(1.0, 1.30),
        art: theme::STOREFRONT_ART_HEIGHT * (width / 1150.0).clamp(0.9, 1.35),
    }
}

pub fn info<'a>(app: &'a App, game: &StoreGame) -> Option<&'a StoreInfo> {
    app.cached_store_info(&game.appid).or_else(|| {
        app.home
            .hero_info
            .as_ref()
            .filter(|_| app.home.hero_appid.as_deref() == Some(game.appid.as_str()))
    })
}

pub fn hero_games(app: &App) -> Vec<&StoreGame> {
    let mut result: Vec<_> = app
        .console_storefront
        .feed
        .specials
        .iter()
        .take(3)
        .collect();
    for game in app.home.games.iter().chain(app.store.browse.games.iter()) {
        if result.len() >= 3 {
            break;
        }
        if !result.iter().any(|item| item.appid == game.appid) {
            result.push(game);
        }
    }
    result
}

pub fn games(app: &App) -> Vec<&StoreGame> {
    if let Some(search) = &app.store.search {
        return search
            .result
            .as_ref()
            .map(|result| result.games.iter().collect())
            .unwrap_or_default();
    }
    let state = &app.console_storefront;
    let mut all: Vec<_> = if state.catalog {
        app.store.browse.games.iter().collect()
    } else if state.category >= 3 {
        state
            .feed
            .specials
            .iter()
            .chain(&state.feed.releases)
            .chain(&state.feed.popular)
            .chain(&app.store.browse.games)
            .collect()
    } else if state.category == 0 && !state.catalog && !state.feed.specials.is_empty() {
        state
            .feed
            .specials
            .iter()
            .take(5)
            .chain(state.feed.releases.iter().filter(|game| {
                !state
                    .feed
                    .specials
                    .iter()
                    .take(5)
                    .any(|other| other.appid == game.appid)
            }))
            .collect()
    } else if state.category == 1 && !state.catalog && !state.feed.releases.is_empty() {
        state.feed.releases.iter().collect()
    } else if state.category == 2 && !state.feed.popular.is_empty() {
        state.feed.popular.iter().collect()
    } else if state.category == 2 && !app.home.games.is_empty() {
        app.home.games.iter().collect()
    } else {
        app.store.browse.games.iter().collect()
    };
    let genre = match if state.catalog { 0 } else { state.category } {
        3 => Some(["action", "ação", "acao", "acción"]),
        4 => Some(["adventure", "aventura", "adventure", "aventura"]),
        5 => Some(["rpg", "role-playing", "role playing", "rpg"]),
        6 => Some(["indie", "independente", "indie", "indie"]),
        7 => Some(["simulation", "simulação", "simulacao", "simulación"]),
        8 => Some(["sports", "esportes", "sport", "deportes"]),
        _ => None,
    };
    if let Some(labels) = genre {
        all.retain(|game| {
            game.genres
                .iter()
                .chain(
                    info(app, game)
                        .into_iter()
                        .flat_map(|info| info.genres.iter()),
                )
                .any(|genre| {
                    labels
                        .iter()
                        .any(|label| genre.to_lowercase().contains(label))
                })
                || app.library.games.iter().any(|item| {
                    item.steam_appid() == Some(game.appid.as_str())
                        && item.genre.as_deref().is_some_and(|genre| {
                            labels
                                .iter()
                                .any(|label| genre.to_lowercase().contains(label))
                        })
                })
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    all.retain(|game| seen.insert(game.appid.as_str()));
    all
}

pub fn scroll_task(index: usize, width: f32, top: bool) -> Task<Message> {
    let m = metrics(width);
    let line = index / STORE_GRID_COLS;
    let y = if top || line == 0 {
        0.0
    } else {
        m.hero + theme::STOREFRONT_CATEGORY_HEIGHT + line as f32 * (m.art + 70.0) - 80.0
    };
    iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(SCROLL_ID),
        iced::widget::scrollable::AbsoluteOffset {
            x: Some(0.0),
            y: Some(y),
        },
    ))
}

const CATEGORIES: [(&str, Glyph); 9] = [
    ("specials", Glyph::Tag),
    ("releases", Glyph::Calendar),
    ("popular", Glyph::Flame),
    ("action", Glyph::Gamepad),
    ("adventure", Glyph::Compass),
    ("rpg", Glyph::Sword),
    ("indie", Glyph::Leaf),
    ("simulation", Glyph::Car),
    ("sports", Glyph::Ball),
];

fn label<'a>(app: &'a App, key: &str) -> &'a str {
    app.i18n.t(match key {
        "featured" => "native.storefront.featured",
        "catalog" => "native.storefront.catalog",
        "popular" => "native.storefront.popular",
        "action" => "native.storefront.action",
        "adventure" => "native.storefront.adventure",
        "rpg" => "native.storefront.rpg",
        "indie" => "native.storefront.indie",
        "simulation" => "native.storefront.simulation",
        "sports" => "native.storefront.sports",
        "empty_category" => "native.storefront.empty_category",
        "view_all" => "native.storefront.view_all",
        "hero_badge" => "native.storefront.hero_badge",
        "open" => "native.storefront.open",
        "results" => "native.storefront.results",
        "more_games" => "native.storefront.more_games",
        "details" => "native.storefront.details",
        "explore" => "native.storefront.explore",
        "real_catalog" => "native.storefront.real_catalog",
        "weekly" => "native.storefront.weekly",
        "select" => "native.storefront.select",
        "back" => "native.storefront.back",
        "specials" => "native.storefront.specials",
        "releases" => "native.storefront.releases",
        "offers" => "native.storefront.offers",
        "limited" => "native.storefront.limited",
        "expires" => "native.storefront.expires",
        "top_sellers" => "native.storefront.top_sellers",
        _ => "native.storefront.tabs",
    })
}

pub fn view(app: &App) -> Element<'_, Message> {
    let m = metrics(app.window_size.width);
    let games = games(app);
    let mut page = column![].spacing(m.gap).width(Fill);
    if app.big_picture.store_editing() || !app.store.query.is_empty() || app.store.search.is_some()
    {
        page = page.push(super::big_picture::store_search_row(app));
    }
    if app.store.search.is_none() {
        page = page.push(hero(app, m));
    }
    page = page.push(categories(app));
    if matches!(app.console_storefront.status, LoadStatus::Error(_)) {
        page = page.push(
            container(
                row![
                    text(app.i18n.t("native.console.store.error_title"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_2),
                    Space::new().width(Fill),
                    button(text(app.i18n.t("downloads.tentar_novamente")).size(theme::TEXT_MICRO))
                        .on_press(Message::ConsoleStorefrontRetry)
                        .style(theme::ghost_button)
                ]
                .align_y(iced::Center),
            )
            .padding(theme::SPACE_2)
            .width(Fill)
            .style(theme::storefront_panel),
        );
    }
    let status = app
        .store
        .search
        .as_ref()
        .map(|s| &s.status)
        .unwrap_or(&app.store.browse.status);
    let catalog: Element<'_, Message> = if games.is_empty() {
        let (message, retry) = match status {
            _ if app.storefront_genres_loading() => {
                (app.i18n.t("common.carregando").to_string(), None)
            }
            LoadStatus::Idle | LoadStatus::Loading => {
                (app.i18n.t("native.console.store.loading").to_string(), None)
            }
            LoadStatus::Error(error) => (
                error.clone(),
                Some(if app.store.search.is_some() {
                    Message::StoreSearchSubmitted
                } else {
                    Message::Refresh(crate::app::Section::Stores)
                }),
            ),
            _ => (
                label(app, "empty_category").to_string(),
                Some(Message::ConsoleStoreCatalog),
            ),
        };
        let mut content = column![text(message)
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_STRONG)]
        .spacing(m.gap);
        if let Some(retry) = retry {
            content = content.push(
                button(text(label(app, "view_all")))
                    .on_press(retry)
                    .style(theme::primary_button),
            );
        }
        container(content)
            .width(Fill)
            .padding(theme::SPACE_5)
            .style(theme::storefront_panel)
            .into()
    } else {
        catalog(app, &games, m)
    };
    let body: Element<'_, Message> = if m.sidebar > 0.0 && app.store.search.is_none() {
        row![
            container(catalog).width(Fill),
            sidebar(app).width(m.sidebar)
        ]
        .spacing(m.gap)
        .align_y(iced::Top)
        .into()
    } else {
        catalog
    };
    page = page.push(body);
    let scroller = scrollable(container(page).padding([0.0, m.inset]))
        .id(iced::widget::Id::new(SCROLL_ID))
        .width(Fill)
        .height(Fill)
        .style(theme::scrollable_bare);
    container(column![scroller, footer(app)].spacing(m.gap))
        .width(Fill)
        .height(Fill)
        .style(theme::app_background)
        .into()
}

fn hero(app: &App, m: Metrics) -> Element<'_, Message> {
    let candidates = hero_games(app);
    let Some(game) = candidates
        .get(
            app.console_storefront
                .hero
                .min(candidates.len().saturating_sub(1)),
        )
        .copied()
    else {
        return container(
            column![
                text(label(app, "featured"))
                    .size(theme::TEXT_TITLE)
                    .font(theme::FONT_BOLD),
                text(app.i18n.t("native.console.store.loading"))
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_2)
            ]
            .spacing(theme::SPACE_3),
        )
        .padding(theme::SPACE_6)
        .width(Fill)
        .height(m.hero)
        .align_y(iced::Center)
        .style(theme::storefront_panel)
        .into();
    };
    let details = info(app, game);
    let hero_url = backend::store_hero_url(game);
    let handle = app
        .covers
        .ready(hero_url.as_deref())
        .or_else(|| {
            app.covers
                .ready(details.and_then(|d| d.background_url.as_deref()))
        })
        .or_else(|| app.covers.ready(game.cover_url.as_deref()));
    let art: Element<'_, Message> = handle
        .map(|h| {
            image(h.clone())
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Cover)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Fill).height(Fill).into());
    let logo = backend::steam_logo_url(&game.appid);
    let title: Element<'_, Message> = match app.covers.ready(logo.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(62)
            .content_fit(ContentFit::Contain)
            .into(),
        None => container(
            text(shorten(&game.title, 52))
                .size(theme::TEXT_DISPLAY)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1)
                .line_height(1.1),
        )
        .width(Fill)
        .height(66)
        .clip(true)
        .into(),
    };
    let badge = container(
        row![
            glyph(Glyph::Star, 13.0, theme::ACCENT),
            text(label(app, "hero_badge"))
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::ACCENT)
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .padding([4.0, 8.0])
    .style(theme::storefront_offer);
    let mut copy = column![badge, title].spacing(theme::SPACE_2);
    if let Some(description) = details.and_then(|d| d.description.as_deref()) {
        copy = copy.push(
            container(
                text(shorten(description, 150))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_STRONG),
            )
            .height(36)
            .clip(true),
        );
    }
    if game.price.is_some() {
        copy = copy.push(price_line(app, game, true));
    }
    let focused = app.big_picture.zone() == ConsoleZone::StoreHero;
    let open = button(
        container(
            row![
                icons::icon(Icon::Store, 16.0, 1.0),
                text(label(app, "open"))
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_SEMIBOLD)
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .height(theme::CONTROL_HEIGHT)
    .padding([0.0, 16.0])
    .on_press(Message::ConsoleStoreOpenDetail(game.clone()))
    .style(move |t, s| theme::storefront_control(t, s, true, focused));
    let mut actions = row![open].spacing(theme::SPACE_2).align_y(iced::Center);
    if let Some(library_game) = app
        .library
        .games
        .iter()
        .find(|item| item.steam_appid() == Some(game.appid.as_str()))
    {
        actions = actions.push(
            button(icons::icon(
                Icon::Heart,
                18.0,
                if library_game.favorite { 1.0 } else { 0.6 },
            ))
            .on_press(Message::ConsoleStoreFavorite(library_game.id.clone()))
            .padding(9)
            .style(move |t, s| theme::storefront_control(t, s, library_game.favorite, false)),
        );
    }
    copy = copy.push(actions);
    let copy = container(copy)
        .width((app.window_size.width * 0.43).clamp(300.0, 650.0))
        .height(Fill)
        .align_y(iced::Center)
        .padding([12.0, 48.0]);
    let arrows = row![
        circle(Glyph::Prev, Message::ConsoleStoreHeroStep(-1)),
        Space::new().width(Fill),
        circle(Glyph::Next, Message::ConsoleStoreHeroStep(1))
    ]
    .align_y(iced::Center);
    let mut dots = row![].spacing(6).align_y(iced::Center);
    for index in 0..candidates.len() {
        let current = index == app.console_storefront.hero;
        let delta = index as isize - app.console_storefront.hero as isize;
        dots = dots.push(
            button(
                container(Space::new())
                    .width(if current { 22 } else { 6 })
                    .height(6)
                    .style(move |_| container::Style {
                        background: Some(
                            if current {
                                theme::ACCENT
                            } else {
                                theme::TEXT_3
                            }
                            .into(),
                        ),
                        border: iced::Border::default().rounded(6),
                        ..Default::default()
                    }),
            )
            .padding([6.0, 0.0])
            .on_press(Message::ConsoleStoreHeroStep(delta))
            .style(theme::plain_button),
        );
    }
    mouse_area(
        container(
            Stack::new()
                .width(Fill)
                .height(m.hero)
                .clip(true)
                .push(art)
                .push(
                    container(Space::new())
                        .width(Fill)
                        .height(Fill)
                        .style(theme::storefront_scrim),
                )
                .push(copy)
                .push(
                    container(arrows)
                        .width(Fill)
                        .height(Fill)
                        .align_y(iced::Center)
                        .padding(theme::SPACE_2),
                )
                .push(
                    container(dots)
                        .width(Fill)
                        .height(Fill)
                        .align_x(iced::Center)
                        .align_y(iced::Bottom)
                        .padding(theme::SPACE_2),
                ),
        )
        .style(theme::storefront_panel),
    )
    .on_enter(Message::PrefetchStore(game.clone()))
    .into()
}

fn categories(app: &App) -> Element<'_, Message> {
    let mut items = row![].spacing(theme::SPACE_2).width(Fill);
    for (index, (key, icon)) in CATEGORIES.iter().enumerate() {
        let active = app.console_storefront.category == index
            && app.store.search.is_none()
            && !app.console_storefront.catalog;
        let focused = app.big_picture.zone() == ConsoleZone::StoreCategories
            && app.big_picture.store_category_index() == index;
        items = items.push(
            button(
                container(
                    column![
                        glyph(
                            *icon,
                            21.0,
                            if active {
                                theme::ACCENT
                            } else {
                                theme::TEXT_STRONG
                            }
                        ),
                        text(label(app, key))
                            .size(theme::TEXT_MICRO)
                            .font(theme::FONT_MEDIUM)
                            .width(Fill)
                            .align_x(iced::Center)
                            .wrapping(Wrapping::WordOrGlyph)
                    ]
                    .spacing(6)
                    .align_x(iced::Center),
                )
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
            )
            .on_press(Message::ConsoleStoreCategory(index))
            .width(Fill)
            .height(theme::STOREFRONT_CATEGORY_HEIGHT)
            .padding(0)
            .style(move |t, s| theme::storefront_control(t, s, active, focused)),
        );
    }
    items.into()
}

fn section_heading<'a>(app: &'a App, title: &'a str) -> Element<'a, Message> {
    row![
        text(title).size(theme::TEXT_LABEL).font(theme::FONT_BOLD),
        Space::new().width(Fill),
        button(
            row![
                text(label(app, "view_all")).size(theme::TEXT_MICRO),
                glyph(Glyph::Next, 14.0, theme::TEXT_2)
            ]
            .spacing(5)
            .align_y(iced::Center)
        )
        .on_press(Message::ConsoleStoreCatalog)
        .style(theme::ghost_button)
        .padding(0)
    ]
    .align_y(iced::Center)
    .into()
}

fn catalog<'a>(app: &'a App, games: &[&'a StoreGame], m: Metrics) -> Element<'a, Message> {
    let title = if app.store.search.is_some() {
        label(app, "results")
    } else if app.console_storefront.catalog {
        label(app, "catalog")
    } else if app.console_storefront.category == 0
        && app.console_storefront.feed.specials.is_empty()
    {
        label(app, "featured")
    } else {
        label(app, CATEGORIES[app.console_storefront.category.min(8)].0)
    };
    let mut content = column![section_heading(app, title)]
        .spacing(theme::SPACE_3)
        .width(Fill);
    for (line, chunk) in games.chunks(STORE_GRID_COLS).enumerate() {
        if line == 1
            && app.store.search.is_none()
            && !app.console_storefront.catalog
            && app.console_storefront.category == 0
        {
            content = content
                .push(Space::new().height(theme::SPACE_2))
                .push(section_heading(
                    app,
                    label(
                        app,
                        if app.console_storefront.feed.releases.is_empty() {
                            "more_games"
                        } else {
                            "releases"
                        },
                    ),
                ));
        }
        let mut cards = row![].spacing(theme::SPACE_2).width(Fill);
        for (col, game) in chunk.iter().enumerate() {
            cards = cards.push(card(app, game, line * STORE_GRID_COLS + col, m.art));
        }
        for _ in chunk.len()..STORE_GRID_COLS {
            cards = cards.push(Space::new().width(Fill));
        }
        content = content.push(cards);
    }
    if app.store.search.is_none() && app.store.browse.has_more && app.console_storefront.catalog {
        let focused = app.big_picture.zone() == ConsoleZone::StoreGrid
            && app.big_picture.store_index() >= games.len();
        content = content.push(
            button(
                container(
                    text(if app.store.browse.loading_more {
                        app.i18n.t("common.carregando")
                    } else {
                        app.i18n.t("native.console.store.load_more")
                    })
                    .size(theme::TEXT_CAPTION),
                )
                .width(Fill)
                .align_x(iced::Center),
            )
            .on_press_maybe((!app.store.browse.loading_more).then_some(Message::StoreLoadMore))
            .width(Fill)
            .padding(12)
            .style(move |t, s| theme::storefront_control(t, s, false, focused)),
        );
    }
    content.into()
}

fn card<'a>(
    app: &'a App,
    game: &'a StoreGame,
    index: usize,
    art_height: f32,
) -> Element<'a, Message> {
    let portrait = backend::store_portrait_url(game);
    let art: Element<'a, Message> = match app
        .covers
        .ready(portrait.as_deref())
        .or_else(|| app.covers.ready(game.cover_url.as_deref()))
    {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(art_height)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(shorten(&game.title, 24))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_2),
        )
        .width(Fill)
        .height(art_height)
        .padding(theme::SPACE_2)
        .align_y(iced::Center)
        .style(theme::storefront_panel)
        .into(),
    };
    let copy = column![price_line(app, game, false)].width(Fill);
    let focused =
        app.big_picture.zone() == ConsoleZone::StoreGrid && app.big_picture.store_index() == index;
    mouse_area(
        button(
            column![
                container(art).clip(true),
                container(copy).width(Fill).padding([7.0, 7.0]).height(48)
            ]
            .width(Fill),
        )
        .on_press(Message::ConsoleStoreOpenDetail(game.clone()))
        .width(Fill)
        .padding(2)
        .style(move |t, s| theme::storefront_control(t, s, false, focused)),
    )
    .on_enter(Message::PrefetchStore(game.clone()))
    .into()
}

fn price_line<'a>(app: &'a App, game: &'a StoreGame, hero: bool) -> Element<'a, Message> {
    let mut line = row![].spacing(6).align_y(iced::Center).width(Fill);
    let price = game.price.as_deref().unwrap_or(label(app, "details"));
    if let Some(offer) = app.console_storefront.feed.offers.get(&game.appid) {
        line = line.push(
            container(
                text(format!("-{}%", offer.discount))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_BOLD)
                    .color(theme::BACKGROUND),
            )
            .padding([3.0, 5.0])
            .style(|_| container::Style {
                background: Some(theme::ACCENT.into()),
                border: iced::Border::default().rounded(4),
                ..Default::default()
            }),
        );

        let original =
            iced::widget::rich_text([iced::widget::text::Span::<()>::new(offer.original.clone())
                .strikethrough(true)
                .color(theme::TEXT_2)])
            .size(theme::TEXT_MICRO);
        if hero {
            line = line
                .push(original)
                .push(text(price).size(theme::TEXT_LABEL).font(theme::FONT_BOLD));
        } else {
            line = line.push(
                column![
                    original,
                    text(price).size(theme::TEXT_MICRO).font(theme::FONT_BOLD)
                ]
                .spacing(2),
            );
        }
    } else {
        line = line.push(
            text(price)
                .size(if hero {
                    theme::TEXT_LABEL
                } else {
                    theme::TEXT_MICRO
                })
                .font(theme::FONT_SEMIBOLD),
        );
    }
    if !hero {
        line = line
            .push(Space::new().width(Fill))
            .push(icons::icon(Icon::Store, 14.0, 0.7));
    }
    line.into()
}

fn sidebar(app: &App) -> iced::widget::Column<'_, Message> {
    let has_offers = !app.console_storefront.feed.specials.is_empty();
    let offer = container(
        row![
            glyph(Glyph::Tag, 30.0, theme::ACCENT),
            column![
                text(label(app, if has_offers { "offers" } else { "explore" }))
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_BOLD),
                text(label(
                    app,
                    if has_offers {
                        "limited"
                    } else {
                        "real_catalog"
                    }
                ))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_STRONG)
            ]
            .spacing(5)
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::storefront_offer);
    let list = if !app.console_storefront.feed.popular.is_empty() {
        &app.console_storefront.feed.popular
    } else if app.home.games.is_empty() {
        &app.store.browse.games
    } else {
        &app.home.games
    };
    let mut trending = column![text(label(
        app,
        if !app.console_storefront.feed.popular.is_empty() {
            "top_sellers"
        } else if app.home.games.is_empty() {
            "catalog"
        } else {
            "weekly"
        }
    ))
    .size(theme::TEXT_BODY)
    .font(theme::FONT_BOLD)]
    .spacing(theme::SPACE_3)
    .width(Fill);
    for (index, game) in list.iter().take(5).enumerate() {
        let cover: Element<'_, Message> = match app.covers.ready(game.cover_url.as_deref()) {
            Some(handle) => image(handle.clone())
                .width(48)
                .height(32)
                .content_fit(ContentFit::Cover)
                .border_radius(5)
                .into(),
            None => container(Space::new())
                .width(48)
                .height(32)
                .style(theme::storefront_offer)
                .into(),
        };
        trending = trending.push(
            button(
                row![
                    container(
                        text((index + 1).to_string())
                            .size(theme::TEXT_MICRO)
                            .color(theme::TEXT_2)
                    )
                    .width(18)
                    .align_x(iced::Center),
                    cover,
                    text(shorten(&game.title, 29))
                        .size(theme::TEXT_MICRO)
                        .wrapping(Wrapping::WordOrGlyph)
                ]
                .spacing(7)
                .align_y(iced::Center),
            )
            .on_press(Message::ConsoleStoreOpenDetail(game.clone()))
            .width(Fill)
            .padding(2)
            .style(theme::plain_button),
        );
    }
    let mut sidebar = column![offer].spacing(theme::STOREFRONT_GAP);
    if let Some(expires) = app
        .console_storefront
        .feed
        .specials
        .iter()
        .filter_map(|game| app.console_storefront.feed.offers.get(&game.appid)?.expires)
        .filter(|expires| *expires > chrono::Utc::now().timestamp())
        .min()
    {
        let left = expires.saturating_sub(chrono::Utc::now().timestamp());
        sidebar = sidebar.push(
            container(
                row![
                    text(label(app, "expires"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_2),
                    Space::new().width(Fill),
                    text(format!(
                        "{}d {}h {}m",
                        left / 86400,
                        (left % 86400) / 3600,
                        (left % 3600) / 60
                    ))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_SEMIBOLD)
                ]
                .align_y(iced::Center),
            )
            .width(Fill)
            .padding(theme::SPACE_2)
            .style(theme::storefront_panel),
        );
    }
    sidebar.push(
        container(trending)
            .width(Fill)
            .padding(theme::SPACE_3)
            .style(theme::storefront_panel),
    )
}

fn footer(app: &App) -> Element<'_, Message> {
    let cap = |name: &str| {
        container(
            text(name.to_string())
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD),
        )
        .padding([3.0, 6.0])
        .style(theme::console_key_cap)
    };
    container(
        row![
            cap("A"),
            text(label(app, "select"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_STRONG),
            cap("B"),
            text(label(app, "back"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_STRONG),
            Space::new().width(Fill),
            cap("LT / RT"),
            text(label(app, "tabs"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_STRONG)
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(theme::STOREFRONT_FOOTER)
    .padding([0.0, theme::STOREFRONT_INSET])
    .align_y(iced::Center)
    .style(theme::console_prompt_bar)
    .into()
}

fn circle(icon: Glyph, message: Message) -> Element<'static, Message> {
    button(
        container(glyph(icon, 17.0, theme::TEXT_1))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .width(30)
    .height(30)
    .padding(0)
    .on_press(message)
    .style(|t, s| {
        let mut style = theme::storefront_control(t, s, false, false);
        style.border.radius = 999.0.into();
        style
    })
    .into()
}

fn shorten(value: &str, limit: usize) -> String {
    let mut out: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        out.push('…');
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub enum Glyph {
    Tag,
    Calendar,
    Flame,
    Gamepad,
    Compass,
    Sword,
    Leaf,
    Car,
    Ball,
    Star,
    Prev,
    Next,
    Minimize,
    Maximize,
    Close,
}
pub fn glyph(icon: Glyph, size: f32, color: Color) -> Element<'static, Message> {
    iced::widget::canvas(LineIcon { icon, color })
        .width(size)
        .height(size)
        .into()
}
struct LineIcon {
    icon: Glyph,
    color: Color,
}
impl canvas::Program<Message> for LineIcon {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let u = bounds.width / 24.0;
        let p = |x: f32, y: f32| Point::new(x * u, y * u);
        let path = Path::new(|b| match self.icon {
            Glyph::Tag => {
                b.move_to(p(3., 3.));
                b.line_to(p(12., 3.));
                b.line_to(p(22., 13.));
                b.line_to(p(13., 22.));
                b.line_to(p(3., 12.));
                b.close();
                b.circle(p(8., 8.), 1.2 * u);
            }
            Glyph::Calendar => {
                b.rectangle(p(3., 5.), iced::Size::new(18. * u, 16. * u));
                for x in [7., 17.] {
                    b.move_to(p(x, 2.));
                    b.line_to(p(x, 8.));
                }
                b.move_to(p(3., 11.));
                b.line_to(p(21., 11.));
            }
            Glyph::Flame => {
                b.move_to(p(13., 2.));
                b.bezier_curve_to(p(18., 10.), p(25., 12.), p(19., 20.));
                b.bezier_curve_to(p(6., 27.), p(-1., 14.), p(8., 7.));
                b.line_to(p(8., 13.));
                b.line_to(p(13., 2.));
            }
            Glyph::Gamepad => {
                b.move_to(p(5., 7.));
                b.line_to(p(19., 7.));
                b.line_to(p(23., 19.));
                b.line_to(p(18., 19.));
                b.line_to(p(15., 15.));
                b.line_to(p(9., 15.));
                b.line_to(p(6., 19.));
                b.line_to(p(1., 19.));
                b.close();
                b.move_to(p(5., 11.));
                b.line_to(p(10., 11.));
                b.move_to(p(7.5, 9.));
                b.line_to(p(7.5, 14.));
                b.circle(p(17., 11.), u);
            }
            Glyph::Compass => {
                b.circle(p(12., 12.), 10. * u);
                b.move_to(p(16., 6.));
                b.line_to(p(14., 14.));
                b.line_to(p(7., 17.));
                b.close();
            }
            Glyph::Ball => {
                b.circle(p(12., 12.), 10. * u);
                b.move_to(p(12., 7.));
                b.line_to(p(17., 11.));
                b.line_to(p(15., 17.));
                b.line_to(p(9., 17.));
                b.line_to(p(7., 11.));
                b.close();
                for (x, y, ex, ey) in [
                    (12., 7., 12., 2.),
                    (17., 11., 21., 8.),
                    (15., 17., 18., 21.),
                    (9., 17., 6., 21.),
                    (7., 11., 3., 8.),
                ] {
                    b.move_to(p(x, y));
                    b.line_to(p(ex, ey));
                }
            }
            Glyph::Sword => {
                b.move_to(p(4., 21.));
                b.line_to(p(18., 7.));
                b.line_to(p(22., 2.));
                b.line_to(p(17., 3.));
                b.line_to(p(3., 17.));
                b.move_to(p(4., 13.));
                b.line_to(p(11., 20.));
            }
            Glyph::Leaf => {
                b.move_to(p(4., 20.));
                b.bezier_curve_to(p(1., 7.), p(14., 3.), p(22., 3.));
                b.bezier_curve_to(p(22., 16.), p(16., 24.), p(4., 20.));
                b.line_to(p(16., 9.));
            }
            Glyph::Car => {
                b.rectangle(p(2., 10.), iced::Size::new(20. * u, 8. * u));
                b.move_to(p(5., 10.));
                b.line_to(p(8., 5.));
                b.line_to(p(17., 5.));
                b.line_to(p(20., 10.));
                b.circle(p(6., 19.), 2. * u);
                b.circle(p(18., 19.), 2. * u);
            }
            Glyph::Star => {
                for i in 0..10 {
                    let a = i as f32 * std::f32::consts::PI / 5. - std::f32::consts::FRAC_PI_2;
                    let r = if i % 2 == 0 { 10. } else { 4.5 };
                    let point = p(12. + r * a.cos(), 12. + r * a.sin());
                    if i == 0 {
                        b.move_to(point)
                    } else {
                        b.line_to(point)
                    }
                }
                b.close();
            }
            Glyph::Prev => {
                b.move_to(p(15., 5.));
                b.line_to(p(8., 12.));
                b.line_to(p(15., 19.));
            }
            Glyph::Next => {
                b.move_to(p(9., 5.));
                b.line_to(p(16., 12.));
                b.line_to(p(9., 19.));
            }
            Glyph::Minimize => {
                b.move_to(p(5., 16.));
                b.line_to(p(19., 16.));
            }
            Glyph::Maximize => b.rectangle(p(5., 5.), iced::Size::new(14. * u, 14. * u)),
            Glyph::Close => {
                b.move_to(p(6., 6.));
                b.line_to(p(18., 18.));
                b.move_to(p(18., 6.));
                b.line_to(p(6., 18.));
            }
        });
        frame.stroke(
            &path,
            Stroke::default().with_color(self.color).with_width(1.5 * u),
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carousel_wraps_and_handles_empty_catalog() {
        let mut state = StorefrontState::default();
        state.step_hero(-1, 3);
        assert_eq!(state.hero, 2);
        state.step_hero(1, 3);
        assert_eq!(state.hero, 0);
        state.step_hero(5, 0);
        assert_eq!(state.hero, 0);
    }
    #[test]
    fn layout_fits_reference_and_compact_windows() {
        for width in [800., 1024., 1150., 1920.] {
            let m = metrics(width);
            let main = width
                - 2. * m.inset
                - if m.sidebar > 0. {
                    m.sidebar + m.gap
                } else {
                    0.
                };
            assert!(main > 600.);
            assert!((main - 4. * theme::SPACE_2) / 5. > 110.);
            assert!(m.hero >= 230. && m.hero < 320.);
        }
        assert!(metrics(1150.).sidebar > 0.);
        assert_eq!(metrics(800.).sidebar, 0.);
    }
}
