//! Home nativa: vitrine `store:recent` (`top100in2weeks`) com herói, grade
//! adaptável e ficha da loja embutida.

use std::time::Duration;

use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::advanced::{layout, mouse, renderer, widget::Tree, Layout, Widget};
use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{Color, ContentFit, Element, Fill, Length, Padding, Rectangle};

use crate::app::{App, LoadStatus, Message, Section};
use crate::backend::{store_hero_url, StoreGame};
use crate::theme;
use crate::views;

#[path = "home_motion.rs"]
mod home_motion;
pub(crate) use home_motion::{animation_interval, carousel_interval, HERO_ZOOM_DURATION};

const GRID_GAP: f32 = 16.0;
const GRID_LIMIT: usize = 9;
const ART_REVEAL: Duration = Duration::from_millis(280);
const CARD_ENTRANCE: Duration = Duration::from_millis(360);
const CARD_STAGGER: Duration = Duration::from_millis(80);
const CARD_LIFT: f32 = 12.0;

fn reveal(age: Duration) -> f32 {
    let t = (age.as_secs_f32() / ART_REVEAL.as_secs_f32()).min(1.0);
    1.0 - (1.0 - t).powi(2)
}

fn card_entrance(age: Duration, index: usize) -> f32 {
    let delay = CARD_STAGGER * index as u32;
    let t = (age.saturating_sub(delay).as_secs_f32() / CARD_ENTRANCE.as_secs_f32()).min(1.0);
    1.0 - (1.0 - t).powi(3)
}

fn hero_art<'a>(app: &'a App, game: &StoreGame) -> Option<(&'a Handle, Duration)> {
    let hero = store_hero_url(game);
    app.covers
        .ready_with_age(hero.as_deref())
        .or_else(|| app.covers.ready_with_age(game.cover_url.as_deref()))
        .or_else(|| app.covers.ready_with_age(game.portrait_url.as_deref()))
}

pub fn art_is_animating(app: &App) -> bool {
    let recent = |art: Option<(&Handle, Duration)>| art.is_some_and(|(_, age)| age < ART_REVEAL);
    !app.reduce_motion()
        && (app.home_entered.elapsed() < CARD_ENTRANCE + CARD_STAGGER * GRID_LIMIT as u32
            || app.home_hero_started.elapsed() < HERO_ZOOM_DURATION
            || recent(app.home.games.get(app.home_hero_index).and_then(|game| hero_art(app, game)))
            || app
                .home
                .games
                .iter()
                .take(GRID_LIMIT)
                .enumerate()
                .any(|(index, game)| {
                    app.covers
                        .ready_with_age(game.cover_url.as_deref())
                        .is_some_and(|(_, age)| age < CARD_ENTRANCE + CARD_STAGGER * index as u32)
                }))
}

pub(crate) fn carousel_is_active(app: &App) -> bool {
    matches!(app.screen, crate::app::Screen::Section(Section::Home))
        && app.store.detail.is_none()
        && app.home.games.len() > 1
}

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
    let (columns, cover_height) = card_metrics(app.window_size.width);

    let mut page = column![].width(Fill);
    if let Some(hero_game) = app.home.games.get(app.home_hero_index) {
        page = page.push(hero(app, hero_game, hero_height));
    }

    let mut grid = column![].spacing(GRID_GAP).width(Fill);
    let games = &app.home.games[..app.home.games.len().min(GRID_LIMIT)];
    for (row_index, chunk) in games.chunks(columns).enumerate() {
        let mut line = row![].spacing(GRID_GAP).width(Fill);
        for (column_index, game) in chunk.iter().enumerate() {
            let index = row_index * columns + column_index;
            let progress = if !app.reduce_motion() {
                card_entrance(app.home_entered.elapsed(), index)
            } else {
                1.0
            };
            let offset = CARD_LIFT * (1.0 - progress);
            line = line.push(
                container(store_card(app, game, cover_height, progress))
                    .width(Fill)
                    .padding(Padding {
                        top: offset,
                        bottom: CARD_LIFT - offset,
                        left: 0.0,
                        right: 0.0,
                    }),
            );
        }
        for _ in chunk.len()..columns {
            line = line.push(Space::new().width(Fill));
        }
        grid = grid.push(line);
    }

    page = page.push(
        container(
            column![trending_header(app), grid]
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
    let art_age = hero_art(app, game);
    let progress = if !app.reduce_motion() {
        reveal(app.home_hero_started.elapsed()).min(art_age.map_or(0.0, |(_, age)| reveal(age)))
    } else {
        1.0
    };
    let zoom = if app.reduce_motion() {
        1.0
    } else {
        1.0 + 0.04
            * (app.home_hero_started.elapsed().as_secs_f32()
                / HERO_ZOOM_DURATION.as_secs_f32())
            .clamp(0.0, 1.0)
    };
    // The title switches immediately, so an old game's artwork must not remain
    // underneath it while the selected image loads or fails.
    let base = showcase_art(None, &game.title, true, 1.0, app.reduce_motion());
    let art: Element<'_, Message> = if let Some((handle, _)) = art_age {
        Stack::new().width(Fill).height(Fill).clip(true).push(base)
            .push(Element::new(HeroArt {
                handle: handle.clone(), opacity: progress, zoom,
                reduced: app.reduce_motion(),
            })).into()
    } else { base };

    let description = app
        .home
        .hero_info
        .as_ref()
        .filter(|_| app.home.hero_appid.as_deref() == Some(game.appid.as_str()))
        .and_then(|info| info.description.as_deref())
        .map(|description| shorten(description, 320));

    let mut copy = column![text(game.title.as_str())
        .size(if card_metrics(app.window_size.width).0 == 2 {
            theme::TEXT_DISPLAY
        } else {
            42.0
        })
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)
        .wrapping(Wrapping::WordOrGlyph)]
    .spacing(12)
    .width(Fill);

    if let Some(description) = description.filter(|description| !description.is_empty()) {
        copy = copy.push(
            text(description)
                .size(theme::TEXT_READ)
                .color(theme::TEXT_STRONG)
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
            bottom: 36.0 - 12.0 * (1.0 - progress),
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
                .style(theme::home_hero_scrim),
        )
        .push(content);

    iced::widget::mouse_area(button(hero)
        .on_press(Message::StoreOpenDetail(game.clone()))
        .width(Fill)
        .height(Length::Fixed(height))
        .padding(0)
        .style(theme::plain_button))
        .on_enter(Message::PrefetchStore(game.clone()))
        .into()
}

/// Draw-only background; the surrounding button retains all input handling.
struct HeroArt {
    handle: Handle,
    opacity: f32,
    zoom: f32,
    reduced: bool,
}

impl Widget<Message, iced::Theme, iced::Renderer> for HeroArt {
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Fill, Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.resolve(Fill, Fill, iced::Size::ZERO))
    }

    fn draw(&self, _tree: &Tree, renderer: &mut iced::Renderer, _theme: &iced::Theme,
        _style: &renderer::Style, layout: Layout<'_>, _cursor: mouse::Cursor, viewport: &Rectangle) {
        use iced::advanced::image::Renderer as _;
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else { return };
        let Some(dimensions) = renderer.measure_image(&self.handle) else { return };
        if dimensions.width == 0 || dimensions.height == 0 || bounds.width <= 0.0 || bounds.height <= 0.0 { return; }
        let overscan = if self.reduced { 1.0 } else { 1.3 };
        let scale = (bounds.width / dimensions.width as f32)
            .max(bounds.height * overscan / dimensions.height as f32) * self.zoom;
        let width = dimensions.width as f32 * scale;
        let height = dimensions.height as f32 * scale;
        let shift = home_motion::parallax(viewport.y - bounds.y, bounds.height, self.reduced);
        let rect = Rectangle { x: bounds.center_x() - width / 2.0,
            y: bounds.center_y() - height / 2.0 + shift, width, height };
        renderer.draw_image(iced::advanced::image::Image::new(self.handle.clone()).opacity(self.opacity), rect, clip);
    }
}

fn trending_header(app: &App) -> Element<'_, Message> {
    let heading = text(app.i18n.t("store.mais_baixados_semana"))
        .size(theme::TEXT_TITLE)
        .font(theme::FONT_SEMIBOLD)
        .color(theme::TEXT_1)
        .width(Fill);
    let dice = button(
        text(app.i18n.t("store.surpreenda_me"))
            .size(theme::TEXT_READ)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::HomeRollDice)
    .padding([theme::SPACE_2, theme::SPACE_4])
    .style(theme::card_action_button);

    row![heading, dice]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center)
        .into()
}

fn showcase_art(
    art: Option<(&Handle, Duration)>,
    title: &str,
    featured: bool,
    entrance: f32,
    reduced: bool,
) -> Element<'static, Message> {
    let placeholder = || {
        container(
            text(initial(title))
                .size(if featured { 96.0 } else { theme::TEXT_DISPLAY })
                .font(theme::FONT_BOLD)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.12)),
        )
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
    };
    let Some((handle, age)) = art else {
        return placeholder().into();
    };
    let progress = if !reduced {
        reveal(age).min(entrance)
    } else {
        1.0
    };
    let image = image(handle.clone())
        .width(Fill)
        .height(Fill)
        .content_fit(ContentFit::Cover)
        .opacity(progress)
        .scale(if featured {
            1.0 + 0.035 * (1.0 - progress)
        } else {
            1.0 + 0.025 * (1.0 - progress)
        });

    if progress < 1.0 {
        Stack::new()
            .push(placeholder())
            .push(image)
            .clip(true)
            .into()
    } else {
        image.into()
    }
}

fn store_card<'a>(
    app: &'a App,
    game: &'a StoreGame,
    cover_height: f32,
    progress: f32,
) -> Element<'a, Message> {
    let cover = showcase_art(
        app.covers.ready_with_age(game.cover_url.as_deref()),
        &game.title,
        false,
        progress,
        app.reduce_motion(),
    );

    let in_library = library_has(app, game);
    let action_label = if in_library {
        app.i18n.t("store.na_biblioteca")
    } else {
        app.i18n.t("store.ver_detalhes")
    };

    let mut action_content = row![].spacing(theme::SPACE_2).align_y(iced::Center);
    if in_library {
        action_content = action_content.push(
            text("✓")
                .size(theme::TEXT_BODY)
                .font(theme::FONT_MEDIUM),
        );
    }
    action_content = action_content.push(
        text(action_label)
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
    );

    let action = button(
        container(action_content)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::StoreOpenDetail(game.clone()))
    .width(Fill)
    .height(Length::Fixed(theme::HOME_ACTION_HEIGHT))
    .padding([0.0, theme::SPACE_3])
    .style(if in_library {
        theme::library_button
    } else {
        theme::home_detail_button
    });

    iced::widget::mouse_area(container(
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
                            .size(theme::TEXT_CAPTION)
                            .font(theme::FONT_SEMIBOLD)
                            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.94))
                            .wrapping(Wrapping::WordOrGlyph),
                    )
                    .width(Fill)
                    .height(Length::Fixed(2.6 * theme::TEXT_CAPTION))
                    .clip(true),
                    action,
                ]
                .spacing(10)
                .width(Fill),
            )
            .width(Fill)
            .padding([12.0, 12.0]),
        ]
        .width(Fill),
    )
    .width(Fill)
    .style(theme::home_store_card))
    .on_enter(Message::PrefetchStore(game.clone()))
    .into()
}

fn card_metrics(window_width: f32) -> (usize, f32) {
    let content_width = window_width - theme::SIDEBAR_WIDTH - 2.0 * theme::PAGE_PADDING;
    let columns = if content_width >= 3.0 * 250.0 + 2.0 * GRID_GAP {
        3
    } else {
        2
    };
    let card_width = (content_width - (columns - 1) as f32 * GRID_GAP) / columns as f32;
    (columns, (card_width / (460.0 / 215.0)).clamp(112.0, 210.0))
}

fn library_has(app: &App, game: &StoreGame) -> bool {
    let appid = game.appid.trim();
    if appid.is_empty() {
        return false;
    }
    app.library.games.iter().any(|library_game| {
        library_game.steam_appid() == Some(appid) || library_game.id == format!("steam:{appid}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_grid_adapts_to_window_width() {
        assert_eq!(card_metrics(1024.0).0, 2);
        assert_eq!(card_metrics(1280.0).0, 3);
    }

    #[test]
    fn card_entrance_is_staggered_and_settles() {
        assert_eq!(card_entrance(Duration::ZERO, 0), 0.0);
        assert_eq!(card_entrance(Duration::from_millis(80), 1), 0.0);
        assert!(
            card_entrance(Duration::from_millis(200), 0)
                > card_entrance(Duration::from_millis(200), 1)
        );
        assert_eq!(card_entrance(Duration::from_secs(2), 8), 1.0);
    }

    #[test]
    fn art_reveal_eases_out_and_settles() {
        assert_eq!(reveal(Duration::ZERO), 0.0);
        assert_eq!(reveal(ART_REVEAL / 2), 0.75);
        assert_eq!(reveal(ART_REVEAL * 2), 1.0);
    }
}
