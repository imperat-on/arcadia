//! Home nativa: o que a `HomeView.tsx` faz com dados da biblioteca.
//!
//! A vitrine da loja ainda não tem backend no nativo, então a Home usa a
//! biblioteca local: um hero do jogo em destaque, a seção "Continuar jogando"
//! (horas registradas) e a grade da coleção. Onde falta dado, o estado é
//! explícito em vez de fingir conteúdo.

use iced::widget::text::Wrapping;
use iced::widget::Row;
use iced::widget::{button, column, container, image, scrollable, text, Space, Stack};
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{App, LibraryStatus, Message, Section};
use crate::backend::Game;
use crate::covers::CoverCache;
use crate::theme;
use crate::views::cards;

pub fn view(app: &App) -> Element<'_, Message> {
    let games = app.library.visible_all();
    let mut page = column![hero(app, &games)].width(Fill);
    page = page.push(body(app, &games));

    scrollable(container(page).width(Fill))
        .height(Fill)
        .style(theme::scrollable)
        .into()
}

fn hero<'a>(app: &'a App, games: &[&'a Game]) -> Element<'a, Message> {
    let Some(game) = pick_hero(games, &app.covers) else {
        return empty_hero(app);
    };

    let url = game
        .hero_url
        .as_deref()
        .or(game.cover_url.as_deref())
        .filter(|url| !url.is_empty());

    let art: Element<'a, Message> = match app.covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(
                game.title
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_string()),
            )
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

    let mut copy = column![text(game.title.to_uppercase())
        .size(40)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)
        .wrapping(Wrapping::WordOrGlyph)]
    .spacing(10);

    if let Some(description) = game.description.as_deref() {
        copy = copy.push(
            text(shorten(description, 240))
                .size(14)
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
        .padding([40.0, theme::PAGE_PADDING]);

    let hero = Stack::new()
        .width(Fill)
        .height(theme::HERO_HEIGHT)
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::hero_overlay),
        )
        .push(content);

    button(hero)
        .on_press(Message::OpenGame(game.id.clone()))
        .width(Fill)
        .height(Length::Fixed(theme::HERO_HEIGHT))
        .padding(0)
        .style(theme::plain_button)
        .into()
}

fn empty_hero(app: &App) -> Element<'static, Message> {
    let message = match &app.library.status {
        LibraryStatus::Loading => app.i18n.t("common.carregando"),
        LibraryStatus::Error(_) => app.i18n.t("native.home.library_error"),
        LibraryStatus::Ready => app.i18n.t("ps5.biblioteca.vazia"),
    };
    let message = message.to_string();

    container(
        column![
            text("ARCADIA")
                .size(46)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(message)
                .size(14)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.55)),
        ]
        .spacing(10)
        .align_x(iced::Center),
    )
    .width(Fill)
    .height(theme::HERO_HEIGHT)
    .center_x(Fill)
    .center_y(Fill)
    .style(theme::empty_panel)
    .into()
}

fn body<'a>(app: &'a App, games: &[&'a Game]) -> Element<'a, Message> {
    let mut playing: Vec<&'a Game> = games
        .iter()
        .copied()
        .filter(|game| game.playtime_minutes > 0)
        .collect();
    playing.sort_by(|left, right| right.playtime_minutes.cmp(&left.playtime_minutes));
    playing.truncate(5);

    let mut content = column![].spacing(16);

    content = content.push(cards::section_heading(
        app.i18n.t("native.home.continue_playing"),
        Some(playing.len()),
        None,
    ));

    if playing.is_empty() {
        content = content.push(
            container(
                text(match &app.library.status {
                    LibraryStatus::Loading => app.i18n.t("native.home.history_loading"),
                    LibraryStatus::Error(_) => app.i18n.t("native.home.history_error"),
                    LibraryStatus::Ready => app.i18n.t("profile.nenhum_jogado"),
                })
                .size(13)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.42)),
            )
            .width(Fill)
            .padding(theme::SPACE_5)
            .style(theme::empty_panel),
        );
    } else {
        let rail: Vec<Element<'a, Message>> = playing
            .into_iter()
            .map(|game| cards::compact_card(&app.i18n, game, &app.covers))
            .collect();
        content = content.push(Row::with_children(rail).spacing(theme::SPACE_3));
    }

    content = content.push(Space::new().height(12));

    let library_action = button(
        text(app.i18n.t("native.home.view_library"))
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.78)),
    )
    .on_press(Message::Navigate(Section::Library))
    .padding([7.0, 12.0])
    .style(theme::ghost_button);

    content = content.push(cards::section_heading(
        app.i18n.t("native.home.in_library"),
        Some(games.len()),
        Some(library_action.into()),
    ));

    if games.is_empty() {
        content = content.push(
            container(
                text(app.i18n.t("native.home.grid_empty"))
                    .size(13)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.42)),
            )
            .width(Fill)
            .padding(theme::SPACE_5)
            .style(theme::empty_panel),
        );
    } else {
        let cards: Vec<Element<'a, Message>> = games
            .iter()
            .take(7)
            .map(|game| cards::game_card(&app.i18n, game, &app.covers))
            .collect();
        content = content.push(cards::grid(cards, (2.0, 3.55)));
    }

    container(content)
        .width(Fill)
        .padding(Padding {
            top: theme::SPACE_5,
            right: theme::PAGE_PADDING,
            bottom: theme::PAGE_PADDING + theme::SPACE_2,
            left: theme::PAGE_PADDING,
        })
        .into()
}

fn pick_hero<'a>(games: &[&'a Game], covers: &CoverCache) -> Option<&'a Game> {
    let ready = |game: &Game| covers.ready(art_url(game)).is_some();

    games
        .iter()
        .copied()
        .find(|game| game.favorite && ready(game))
        .or_else(|| games.iter().copied().find(|game| ready(game)))
        .or_else(|| {
            games
                .iter()
                .copied()
                .find(|game| game.hero_url.is_some() || game.cover_url.is_some())
        })
        .or_else(|| games.first().copied())
}

fn art_url(game: &Game) -> Option<&str> {
    game.hero_url
        .as_deref()
        .or(game.cover_url.as_deref())
        .filter(|url| !url.is_empty())
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
