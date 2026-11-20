//! Home nativa: vitrine `store:recent` (`top100in2weeks`) no mesmo desenho da
//! `HomeView.tsx` — herói largo, “Trending now”, “Roll the dice” e grade de
//! três colunas. A ficha da loja abre embutida, como na UI web.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, LoadStatus, Message, Section};
use crate::backend::{store_hero_url, StoreGame};
use crate::theme;
use crate::views;

const GRID_GAP: f32 = 16.0;
const GRID_COLUMNS: usize = 3;

pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(detail) = app.store.detail.as_ref() {
        return views::store::detail_view(app, detail);
    }

    let state = &app.home;
    let page: Element<'_, Message> = match state.status {
        LoadStatus::Idle | LoadStatus::Loading if state.games.is_empty() => {
            views::loading_state(app.i18n.t("native.store.catalog_loading"))
        }
        LoadStatus::Error(_) if state.games.is_empty() => container(views::error_state_labeled(
            app.i18n.t("native.store.catalog_error"),
            state
                .error
                .as_deref()
                .unwrap_or(app.i18n.t("native.store.catalog_error_detail")),
            Message::Refresh(Section::Home),
            app.i18n.t("downloads.tentar_novamente"),
        ))
        .width(Fill)
        .into(),
        _ if state.games.is_empty() => views::empty_state(
            app.i18n.t("native.store.catalog_empty_title"),
            app.i18n.t("native.store.catalog_empty_desc"),
        ),
        _ => ready(app),
    };

    scrollable(container(page).width(Fill))
        .height(Fill)
        .style(theme::scrollable)
        .into()
}

fn ready(app: &App) -> Element<'_, Message> {
    let hero_height = (app.window_size.height * 0.44).clamp(300.0, 430.0);
    let (_, cover_height) = card_metrics(app);

    let mut page = column![].width(Fill);
    if let Some(hero_game) = app.home.games.first() {
        page = page.push(hero(app, hero_game, hero_height));
    }

    let mut grid = column![].spacing(GRID_GAP).width(Fill);
    let games: Vec<&StoreGame> = app.home.games.iter().take(9).collect();
    for chunk in games.chunks(GRID_COLUMNS) {
        let mut line = row![].spacing(GRID_GAP).width(Fill);
        for game in chunk {
            line = line.push(container(store_card(app, game, cover_height)).width(Fill));
        }
        for _ in chunk.len()..GRID_COLUMNS {
            line = line.push(Space::new().width(Fill));
        }
        grid = grid.push(line);
    }

    page = page.push(
        container(
            column![
                trending_controls(app),
                text(app.i18n.t("store.mais_baixados_semana"))
                    .size(22)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_1),
                grid,
            ]
            .spacing(theme::SPACE_3)
            .width(Fill),
        )
        .width(Fill)
        .padding(Padding {
            top: theme::SPACE_5,
            right: theme::PAGE_PADDING,
            bottom: theme::PAGE_PADDING,
            left: theme::PAGE_PADDING,
        }),
    );

    page.into()
}

fn hero<'a>(app: &'a App, game: &'a StoreGame, height: f32) -> Element<'a, Message> {
    let art_url = store_hero_url(game)
        .or_else(|| game.cover_url.clone())
        .or_else(|| game.portrait_url.clone());

    let art: Element<'a, Message> = match app.covers.ready(art_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(96)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.06)),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
        .into(),
    };

    let description = app
        .home
        .hero_info
        .as_ref()
        .filter(|_| app.home.hero_appid.as_deref() == Some(game.appid.as_str()))
        .and_then(|info| info.description.as_deref())
        .map(|description| shorten(description, 320));

    let mut copy = column![text(game.title.to_uppercase())
        .size(42)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)
        .wrapping(Wrapping::WordOrGlyph)]
    .spacing(12)
    .width(Fill);

    if let Some(description) = description.filter(|description| !description.is_empty()) {
        copy = copy.push(
            text(description)
                .size(15)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.80))
                .wrapping(Wrapping::WordOrGlyph),
        );
    }

    let content = container(copy)
        .width(Fill)
        .height(Fill)
        .max_width(760)
        .align_x(iced::Left)
        .align_y(iced::Bottom)
        .padding(Padding {
            top: 0.0,
            right: theme::PAGE_PADDING,
            bottom: 36.0,
            left: theme::PAGE_PADDING,
        });

    let hero = Stack::new()
        .width(Fill)
        .height(Length::Fixed(height))
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::hero_overlay),
        )
        .push(content);

    button(hero)
        .on_press(Message::StoreOpenDetail(game.clone()))
        .width(Fill)
        .height(Length::Fixed(height))
        .padding(0)
        .style(theme::plain_button)
        .into()
}

fn trending_controls(app: &App) -> Element<'_, Message> {
    let active = button(
        text(app.i18n.t("store.mais_baixados_semana"))
            .size(14)
            .font(theme::FONT_MEDIUM)
            .color(theme::BACKGROUND),
    )
    .padding([8.0, 16.0])
    .style(theme::home_pill);

    let dice = button(
        text(app.i18n.t("store.surpreenda_me"))
            .size(14)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::HomeRollDice)
    .padding([8.0, 16.0])
    .style(theme::ghost_button);

    row![active, Space::new().width(Fill), dice]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center)
        .into()
}

fn store_card<'a>(app: &'a App, game: &'a StoreGame, cover_height: f32) -> Element<'a, Message> {
    let cover: Element<'a, Message> = match app.covers.ready(game.cover_url.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(initial(&game.title))
                .size(30)
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.16)),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
        .into(),
    };

    let in_library = library_has(app, game);
    let action_label = if in_library {
        app.i18n.t("store.na_biblioteca")
    } else {
        app.i18n.t("store.ver_detalhes")
    };

    let action = button(
        text(action_label)
            .size(13)
            .font(theme::FONT_MEDIUM)
            .color(if in_library {
                theme::ACCENT
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.72)
            }),
    )
    .on_press(Message::StoreOpenDetail(game.clone()))
    .width(Fill)
    .height(Length::Fixed(34.0))
    .padding(0)
    .style(if in_library {
        theme::library_button
    } else {
        theme::ghost_button
    });

    container(
        column![
            button(
                container(cover)
                    .width(Fill)
                    .height(Length::Fixed(cover_height))
                    .clip(true)
            )
            .on_press(Message::StoreOpenDetail(game.clone()))
            .width(Fill)
            .padding(0)
            .style(theme::plain_button),
            container(
                column![
                    container(
                        text(game.title.as_str())
                            .size(15)
                            .font(theme::FONT_MEDIUM)
                            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.90))
                            .wrapping(Wrapping::None),
                    )
                    .width(Fill)
                    .height(22)
                    .clip(true),
                    action,
                ]
                .spacing(8)
                .width(Fill),
            )
            .width(Fill)
            .padding([10.0, 12.0]),
        ]
        .width(Fill),
    )
    .width(Fill)
    .style(theme::home_store_card)
    .into()
}

fn card_metrics(app: &App) -> (f32, f32) {
    let content_width = (app.window_size.width - theme::SIDEBAR_WIDTH - 2.0 * theme::PAGE_PADDING)
        .max(620.0);
    let card_width = (content_width - (GRID_COLUMNS as f32 - 1.0) * GRID_GAP)
        / GRID_COLUMNS as f32;
    let cover_height = (card_width / (460.0 / 215.0)).clamp(132.0, 210.0);
    (card_width, cover_height)
}

fn library_has(app: &App, game: &StoreGame) -> bool {
    let appid = game.appid.trim();
    if appid.is_empty() {
        return false;
    }
    app.library.games.iter().any(|library_game| {
        library_game.steam_appid() == Some(appid)
            || library_game.id == format!("steam:{appid}")
    })
}

fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn shorten(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= limit {
        return flattened;
    }

    let mut short: String = flattened.chars().take(limit).collect();
    short.push('…');
    short
}
