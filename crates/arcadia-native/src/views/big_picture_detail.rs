//! Ficha do Big Picture baseada na referência: hero, estatísticas, abas e galeria.
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    button, canvas as canvas_widget, column, container, image, row, scrollable, text, Space, Stack,
};
use iced::{mouse, Color, ContentFit, Element, Fill, Length, Point, Rectangle, Renderer, Theme};

use super::{
    back_button, game_detail,
    icons::{self, Icon},
};
use crate::app::{App, Message};
use crate::backend::{strip_html, Game};
use crate::big_picture::{
    OverviewRow, OverviewTab, ACTION_FAVORITE, ACTION_MORE, ACTION_PLAY, ACTION_TRAILER,
};
use crate::theme;

#[derive(Debug, Clone, Copy)]
struct Metrics {
    hero: f32,
    inset: f32,
    copy: f32,
    thumb: f32,
}

fn metrics(width: f32, height: f32) -> Metrics {
    Metrics {
        hero: (height * theme::CONSOLE_DETAIL_HERO).clamp(380.0, 590.0),
        inset: (width * 0.009).clamp(10.0, 24.0),
        copy: (width * 0.46).clamp(300.0, 700.0),
        thumb: (width * theme::CONSOLE_DETAIL_THUMB / 1121.0).clamp(200.0, 360.0),
    }
}

pub fn gallery_scroll_task(index: usize, viewport_width: f32) -> iced::Task<Message> {
    let width = metrics(viewport_width, 756.0).thumb;
    let x = (index as f32 * (width + theme::SPACE_3) - viewport_width * 0.35).max(0.0);
    iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new("console-detail-gallery"),
        iced::widget::scrollable::AbsoluteOffset {
            x: Some(x),
            y: Some(0.0),
        },
    ))
}

pub fn view<'a>(app: &'a App, game: &'a Game) -> Element<'a, Message> {
    let m = metrics(app.window_size.width, app.window_size.height);
    let i18n = &app.i18n;
    let bp = &app.big_picture;
    let data = app.game_media.as_ref();
    let meta = data.map(|data| &data.overview);
    let panels = app.game_panels.get(&game.id);
    let achievements = panels.map(|p| &p.achievements);
    let total = achievements.map(|p| p.items.len());
    let unlocked = achievements
        .map(|p| p.items.iter().filter(|a| a.unlocked).count())
        .unwrap_or(0);
    let progress = total
        .filter(|n| *n > 0)
        .map(|n| (100.0 * unlocked as f32 / n as f32).round() as u32);
    let size = meta.and_then(|m| {
        if game.installed {
            m.disk_size.or(m.download_size)
        } else {
            m.download_size.or(m.disk_size)
        }
    });
    let size_label = size.map(format_bytes).unwrap_or_else(|| "—".to_string());

    let backdrop = meta.and_then(|m| m.background.as_deref());
    let art: Element<'a, Message> = match app
        .covers
        .ready(backdrop)
        .or_else(|| app.covers.ready(game.hero_url.as_deref()))
        .or_else(|| app.covers.ready(game.cover_url.as_deref()))
    {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let logo = crate::backend::game_logo_url(game);
    let title: Element<'a, Message> = match app.covers.ready(logo.as_deref()) {
        Some(handle) => image(handle.clone())
            .width((m.copy * 0.56).clamp(245.0, 380.0))
            .height(112)
            .content_fit(ContentFit::Contain)
            .into(),
        None => text(game.title.clone())
            .size(theme::TEXT_DISPLAY * 1.4)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1)
            .into(),
    };
    let publisher = meta
        .and_then(|m| m.publisher.as_deref())
        .or(game.publisher.as_deref())
        .or(game.developer.as_deref());
    let release = meta
        .and_then(|m| m.release_date.as_deref())
        .or(game.year.as_deref());
    let mut metadata = row![].spacing(theme::SPACE_3).align_y(iced::Center);
    for (glyph, label) in [
        (Glyph::Publisher, publisher),
        (Glyph::Calendar, release),
        (Glyph::Gamepad, game.genre.as_deref()),
    ] {
        if let Some(label) = label.filter(|s| !s.trim().is_empty()) {
            metadata = metadata.push(
                row![
                    icon(glyph, 16.0, theme::TEXT_STRONG),
                    text(label.to_string())
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_STRONG)
                ]
                .spacing(6)
                .align_y(iced::Center),
            );
        }
    }
    let description = meta
        .and_then(|m| m.short_description.as_deref())
        .or(game.description.as_deref())
        .or(data.and_then(|m| m.about.as_deref()))
        .map(strip_html)
        .unwrap_or_else(|| i18n.t("gameoverview.sem_descricao").to_string());
    let description = shorten(&description, 265);
    let focused = |index| bp.overview_row() == OverviewRow::Actions && bp.action_index() == index;
    let (label, glyph, message) = if app.launch.is_running(&game.id) {
        (i18n.t("hero.parar"), Glyph::Stop, Some(Message::StopGame))
    } else if app.launch.is_launching(&game.id) {
        (i18n.t("common.abrindo"), Glyph::Play, None)
    } else if app.launch.is_stopping(&game.id) {
        (i18n.t("native.game.stopping"), Glyph::Stop, None)
    } else if game.installed {
        (
            i18n.t("hero.jogar"),
            Glyph::Play,
            Some(Message::ConsolePlay),
        )
    } else {
        // Mantém a ação já implementada para jogos não instalados, sem iniciar download fictício.
        (
            i18n.t("hero.instalar"),
            Glyph::Download,
            Some(Message::ConsolePlay),
        )
    };
    let mut primary_content = row![
        icon(glyph, 18.0, theme::CONSOLE_DETAIL_BG),
        text(label)
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_SEMIBOLD)
    ]
    .spacing(10)
    .align_y(iced::Center);
    if size.is_some() {
        primary_content = primary_content.push(
            text(size_label.clone())
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    let primary = button(
        container(primary_content)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press_maybe(message)
    .width(218)
    .height(theme::CONSOLE_DETAIL_ACTION)
    .padding(0)
    .style(move |t, s| theme::console_detail_action(t, s, true, focused(ACTION_PLAY)));
    let favorite = action(
        icon(
            Glyph::Heart,
            20.0,
            if game.favorite {
                theme::CONSOLE_DETAIL_ACCENT
            } else {
                theme::TEXT_1
            },
        ),
        Message::ConsoleFavorite,
        focused(ACTION_FAVORITE),
    );
    let more = action(
        icon(Glyph::More, 20.0, theme::TEXT_1),
        Message::ConsoleMore,
        focused(ACTION_MORE),
    );
    let copy = column![
        title,
        metadata,
        text(description)
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_STRONG)
            .line_height(1.5),
        Space::new().height(6),
        row![primary, favorite, more]
            .spacing(theme::CONSOLE_DETAIL_GAP)
            .align_y(iced::Center)
    ]
    .spacing(theme::SPACE_3)
    .width(m.copy);
    let header = row![
        back_button::circle(Some(Message::ConsoleCloseOverview)),
        text(format!("{} / {}", i18n.t("sidebar.biblioteca"), game.title))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_STRONG),
        Space::new().width(Fill),
        action(
            icon(Glyph::Minimize, 16.0, theme::TEXT_1),
            Message::WindowMinimize,
            false
        ),
        action(
            icon(Glyph::Maximize, 16.0, theme::TEXT_1),
            Message::WindowToggleMaximize,
            false
        ),
        action(
            icon(Glyph::Close, 16.0, theme::TEXT_1),
            Message::WindowClose,
            false
        )
    ]
    .spacing(8)
    .align_y(iced::Center);
    let hero = Stack::new()
        .width(Fill)
        .height(m.hero)
        .push(art)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::console_detail_scrim(false)),
        )
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::console_detail_scrim(true)),
        )
        .push(
            container(
                column![header, Space::new().height(Fill), copy]
                    .height(Fill)
                    .width(Fill),
            )
            .padding([12.0, m.inset])
            .width(Fill)
            .height(Fill),
        );

    let progress_label = progress
        .map(|n| format!("{n}%"))
        .unwrap_or_else(|| "—".to_string());
    let progress_body = row![
        text(progress_label)
            .size(theme::TEXT_BODY)
            .font(theme::FONT_BOLD),
        iced::widget::progress_bar(0.0..=100.0, progress.unwrap_or(0) as f32)
            .girth(4)
            .length(Fill)
    ]
    .spacing(12)
    .align_y(iced::Center);
    let stats = row![
        stat(
            Glyph::Progress,
            i18n.t("native.console.detail.progress"),
            progress_body.into()
        ),
        stat(
            Glyph::Clock,
            i18n.t("native.console.detail.playtime"),
            text(if game.playtime_minutes == 0 {
                i18n.t("native.console.detail.not_played").to_string()
            } else {
                game.playtime_label_i18n(i18n)
            })
            .size(theme::TEXT_BODY)
            .into()
        ),
        stat(
            Glyph::Trophy,
            i18n.t("conquistas.titulo"),
            text(
                total
                    .map(|n| format!("{unlocked} / {n}"))
                    .unwrap_or_else(|| "— / —".into())
            )
            .size(theme::TEXT_BODY)
            .into()
        ),
        stat(
            Glyph::Drive,
            i18n.t("native.console.detail.size"),
            text(size_label).size(theme::TEXT_BODY).into()
        ),
    ]
    .spacing(theme::CONSOLE_DETAIL_GAP)
    .width(Fill);

    let mut tabs = row![].spacing(theme::SPACE_1).width(Fill);
    for (tab, glyph, key) in [
        (
            OverviewTab::Media,
            Glyph::Video,
            "native.console.detail.media",
        ),
        (
            OverviewTab::Information,
            Glyph::Info,
            "native.console.detail.information",
        ),
        (
            OverviewTab::Requirements,
            Glyph::Requirements,
            "native.console.detail.requirements",
        ),
        (
            OverviewTab::Achievements,
            Glyph::Trophy,
            "conquistas.titulo",
        ),
    ] {
        let active = bp.overview_tab() == tab;
        let has_focus = active && bp.overview_row() == OverviewRow::Tabs;
        tabs = tabs.push(
            column![
                button(
                    row![
                        icon(glyph, 16.0, theme::TEXT_STRONG),
                        text(i18n.t(key)).size(theme::TEXT_CAPTION).font(if active {
                            theme::FONT_SEMIBOLD
                        } else {
                            theme::FONT
                        })
                    ]
                    .spacing(8)
                    .align_y(iced::Center)
                )
                .on_press(Message::ConsoleOverviewTab(tab))
                .padding([12.0, 14.0])
                .style(move |t, s| theme::console_detail_tab(t, s, active, has_focus)),
                container(Space::new())
                    .width(Fill)
                    .height(2)
                    .style(theme::console_detail_underline(active))
            ]
            .width(Length::Shrink),
        );
    }
    let content: Element<'a, Message> = match bp.overview_tab() {
        OverviewTab::Media => gallery(app, game, m.thumb),
        OverviewTab::Information => {
            let description = data
                .and_then(|m| m.about.as_deref())
                .or(game.description.as_deref())
                .map(strip_html)
                .unwrap_or_else(|| i18n.t("gameoverview.sem_descricao").into());
            column![
                text(i18n.t("native.console.detail.information"))
                    .size(theme::TEXT_HEADING)
                    .font(theme::FONT_SEMIBOLD),
                text(description)
                    .size(theme::TEXT_READ)
                    .color(theme::TEXT_STRONG)
                    .line_height(1.6),
                button(text(i18n.t("native.game.launch_options")))
                    .on_press(Message::OpenLaunchOptions(game.id.clone()))
                    .style(theme::ghost_button)
            ]
            .spacing(16)
            .into()
        }
        OverviewTab::Requirements => game_detail::requirements_panel(
            i18n,
            data.and_then(|m| m.req_min.as_deref()),
            data.and_then(|m| m.req_rec.as_deref()),
            app.requirements_tab,
        )
        .unwrap_or_else(|| {
            text(i18n.t("gamepage.requisitos_indisponiveis"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2)
                .into()
        }),
        OverviewTab::Achievements => super::game::achievements_panel(i18n, panels, &app.covers),
    };
    let mut body = column![stats, tabs, content]
        .spacing(theme::SPACE_4)
        .width(Fill);
    if let Some(notice) = bp.notice() {
        body = body.push(super::warning_banner(notice));
    }
    if let Some(error) = app.launch.error.as_deref() {
        body = body.push(super::action_error_banner(error, None));
    }
    let warnings = app.launch.warnings_for(&game.id);
    if !warnings.is_empty() {
        body = body.push(super::game::launch_warning_banner(warnings.join(" · ")));
    }
    let page = container(
        scrollable(
            column![hero, container(body).padding([0.0, m.inset])]
                .spacing(theme::SPACE_2)
                .width(Fill),
        )
        .style(theme::scrollable_bare),
    )
    .width(Fill)
    .height(Fill)
    .style(theme::console_detail_background);
    let mut layers = Stack::new().width(Fill).height(Fill).push(page);
    if bp.overview_preview() {
        if let Some(data) = data {
            let gallery = game_detail::gallery_panel(
                i18n,
                &app.media,
                &app.covers,
                &data.movies,
                &data.screenshots,
                app.selected_screenshot,
                app.gallery_trailer,
                Message::SelectTrailer,
                Message::SelectScreenshot,
                Vec::new(),
            );
            layers = layers.push(
                container(
                    column![
                        row![
                            Space::new().width(Fill),
                            back_button::circle(Some(Message::ConsoleOverviewClosePreview))
                        ],
                        gallery
                    ]
                    .spacing(12),
                )
                .padding(24)
                .width(Fill)
                .height(Fill)
                .style(theme::console_detail_background),
            );
        }
    }
    layers.into()
}

fn action<'a>(
    content: Element<'a, Message>,
    message: Message,
    focused: bool,
) -> Element<'a, Message> {
    button(
        container(content)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(message)
    .padding(0)
    .width(theme::CONSOLE_DETAIL_ACTION)
    .height(theme::CONSOLE_DETAIL_ACTION)
    .style(move |t, s| theme::console_detail_action(t, s, false, focused))
    .into()
}

fn stat<'a>(glyph: Glyph, label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            row![
                icon(glyph, 16.0, theme::TEXT_STRONG),
                text(label).size(theme::TEXT_CAPTION)
            ]
            .spacing(8)
            .align_y(iced::Center),
            value
        ]
        .spacing(7),
    )
    .width(Fill)
    .height(theme::CONSOLE_DETAIL_STAT)
    .padding([10.0, 20.0])
    .style(theme::console_detail_stat)
    .into()
}

fn gallery<'a>(app: &'a App, game: &'a Game, width: f32) -> Element<'a, Message> {
    let mut items = row![].spacing(theme::SPACE_3);
    if let Some(data) = &app.game_media {
        let offset = usize::from(!data.movies.is_empty());
        if let Some(movie) = data.movies.first() {
            items = items.push(thumb(
                app,
                movie.thumb.as_deref().or(game.cover_url.as_deref()),
                0,
                width * 1.10,
                true,
            ));
        }
        for (i, shot) in data.screenshots.iter().enumerate() {
            items = items.push(thumb(app, shot.thumb_or_full(), i + offset, width, false));
        }
    }
    let strip: Element<'a, Message> = if app.game_media.is_none() {
        container(
            text(app.i18n.t("common.carregando"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2),
        )
        .height(theme::CONSOLE_DETAIL_THUMB_HEIGHT)
        .into()
    } else if app
        .game_media
        .as_ref()
        .is_some_and(|m| m.movies.is_empty() && m.screenshots.is_empty())
    {
        text(app.i18n.t("native.game.screenshots_missing"))
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_2)
            .into()
    } else {
        scrollable(items)
            .id(iced::widget::Id::new("console-detail-gallery"))
            .direction(Direction::Horizontal(
                Scrollbar::new().width(0).scroller_width(0),
            ))
            .style(theme::scrollable_bare)
            .into()
    };
    let strip = Stack::new()
        .width(Fill)
        .height(width * 9.0 / 16.0)
        .push(strip)
        .push(
            container(action(
                icon(Glyph::Next, 18.0, theme::TEXT_1),
                Message::ConsoleOverviewAdvance,
                false,
            ))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Right)
            .align_y(iced::Center),
        );
    column![
        text(app.i18n.t("native.console.detail.gallery"))
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_SEMIBOLD),
        strip
    ]
    .spacing(theme::SPACE_4)
    .into()
}

fn thumb<'a>(
    app: &'a App,
    url: Option<&str>,
    index: usize,
    width: f32,
    video: bool,
) -> Element<'a, Message> {
    let h = if video { width / 1.10 } else { width } * 9.0 / 16.0;
    let art: Element<'a, Message> = match app.covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(10)
            .into(),
        None => Space::new().width(Fill).height(Fill).into(),
    };
    let mut frame = Stack::new().width(Fill).height(Fill).push(art);
    if video {
        frame = frame.push(
            container(icon(Glyph::Play, 28.0, theme::TEXT_1))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        );
        if app.media.duration_seconds() > 0.5 {
            frame = frame.push(
                container(
                    container(
                        text(crate::media::format_seconds(app.media.duration_seconds()))
                            .size(theme::TEXT_MICRO),
                    )
                    .padding([3.0, 6.0])
                    .style(theme::console_detail_background),
                )
                .width(Fill)
                .height(Fill)
                .padding(8)
                .align_x(iced::Right)
                .align_y(iced::Bottom),
            );
        }
    }
    let selected = app.big_picture.overview_media() == index
        || (video
            && app.big_picture.overview_row() == OverviewRow::Actions
            && app.big_picture.action_index() == ACTION_TRAILER);
    button(frame)
        .on_press(Message::ConsoleOverviewMedia(index))
        .width(width)
        .height(h)
        .padding(2)
        .style(move |t, s| theme::console_detail_thumb(t, s, selected))
        .into()
}

fn format_bytes(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}
fn shorten(value: &str, max: usize) -> String {
    let mut s: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        s.push('…');
    }
    s
}

#[derive(Clone, Copy)]
enum Glyph {
    Play,
    Stop,
    Download,
    Heart,
    More,
    Clock,
    Trophy,
    Drive,
    Progress,
    Calendar,
    Gamepad,
    Publisher,
    Video,
    Info,
    Requirements,
    Minimize,
    Maximize,
    Close,
    Next,
}
fn icon(glyph: Glyph, size: f32, color: Color) -> Element<'static, Message> {
    // Os ícones da aplicação são reutilizados quando a tinta não precisa de cor específica.
    if matches!(glyph, Glyph::Gamepad) {
        return icons::icon(Icon::Gamepad, size, 0.85);
    }
    canvas_widget(LineIcon { glyph, color })
        .width(size)
        .height(size)
        .into()
}
struct LineIcon {
    glyph: Glyph,
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
        let mut f = Frame::new(renderer, bounds.size());
        let u = bounds.width / 24.0;
        let p = |x: f32, y: f32| Point::new(x * u, y * u);
        let path = Path::new(|b| match self.glyph {
            Glyph::Play => {
                b.move_to(p(8., 5.));
                b.line_to(p(19., 12.));
                b.line_to(p(8., 19.));
                b.close();
            }
            Glyph::Stop => {
                b.rectangle(p(6., 6.), iced::Size::new(12. * u, 12. * u));
            }
            Glyph::Clock | Glyph::Progress | Glyph::Info => {
                b.circle(p(12., 12.), 9. * u);
                if matches!(self.glyph, Glyph::Clock) {
                    b.move_to(p(12., 6.));
                    b.line_to(p(12., 12.));
                    b.line_to(p(16., 14.));
                } else {
                    b.move_to(p(12., 10.));
                    b.line_to(p(12., 17.));
                    b.move_to(p(12., 6.));
                    b.line_to(p(12., 7.));
                }
            }
            Glyph::Heart => {
                b.move_to(p(12., 20.));
                b.bezier_curve_to(p(-4., 10.), p(4., -1.), p(12., 7.));
                b.bezier_curve_to(p(20., -1.), p(28., 10.), p(12., 20.));
            }
            Glyph::More => {
                for x in [5., 12., 19.] {
                    b.circle(p(x, 12.), 1.2 * u);
                }
            }
            Glyph::Download => {
                b.move_to(p(12., 3.));
                b.line_to(p(12., 15.));
                b.move_to(p(7., 10.));
                b.line_to(p(12., 15.));
                b.line_to(p(17., 10.));
                b.move_to(p(4., 16.));
                b.line_to(p(4., 21.));
                b.line_to(p(20., 21.));
                b.line_to(p(20., 16.));
            }
            Glyph::Trophy => {
                b.move_to(p(7., 3.));
                b.line_to(p(17., 3.));
                b.line_to(p(17., 10.));
                b.bezier_curve_to(p(17., 17.), p(7., 17.), p(7., 10.));
                b.close();
                b.move_to(p(7., 5.));
                b.line_to(p(3., 5.));
                b.line_to(p(3., 10.));
                b.line_to(p(7., 12.));
                b.move_to(p(17., 5.));
                b.line_to(p(21., 5.));
                b.line_to(p(21., 10.));
                b.line_to(p(17., 12.));
                b.move_to(p(12., 16.));
                b.line_to(p(12., 21.));
                b.move_to(p(7., 21.));
                b.line_to(p(17., 21.));
            }
            Glyph::Calendar => {
                b.rectangle(p(3., 5.), iced::Size::new(18. * u, 16. * u));
                b.move_to(p(3., 10.));
                b.line_to(p(21., 10.));
                b.move_to(p(7., 2.));
                b.line_to(p(7., 7.));
                b.move_to(p(17., 2.));
                b.line_to(p(17., 7.));
            }
            Glyph::Drive
            | Glyph::Requirements
            | Glyph::Publisher
            | Glyph::Maximize
            | Glyph::Video => {
                b.rectangle(p(3., 5.), iced::Size::new(18. * u, 14. * u));
                b.move_to(p(7., 15.));
                b.line_to(p(10., 15.));
            }
            Glyph::Minimize => {
                b.move_to(p(5., 16.));
                b.line_to(p(19., 16.));
            }
            Glyph::Next => {
                b.move_to(p(9., 5.));
                b.line_to(p(16., 12.));
                b.line_to(p(9., 19.));
            }
            Glyph::Close => {
                b.move_to(p(6., 6.));
                b.line_to(p(18., 18.));
                b.move_to(p(18., 6.));
                b.line_to(p(6., 18.));
            }
            Glyph::Gamepad => {}
        });
        if matches!(self.glyph, Glyph::Play | Glyph::Stop | Glyph::More) {
            f.fill(&path, self.color);
        } else {
            f.stroke(
                &path,
                Stroke::default().with_width(1.6 * u).with_color(self.color),
            );
        }
        vec![f.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_geometry_stays_bounded() {
        for (w, h) in [(800., 600.), (1121., 756.), (1280., 800.), (1920., 1080.)] {
            let m = metrics(w, h);
            assert!(m.hero >= 380. && m.hero <= 590.);
            assert!(m.copy + 2. * m.inset < w);
            assert!(m.thumb >= 200. && m.thumb <= 360.);
        }
        let m = metrics(1121., 756.);
        assert!((m.hero - 400.68).abs() < 1.);
    }
}
