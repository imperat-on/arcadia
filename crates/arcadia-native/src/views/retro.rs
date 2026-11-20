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

use crate::app::{App, LoadStatus, Message, RetroDetailState, Section};
use crate::backend::{retro_system_filters, RetroGame, RetroSource, RetroSystemInfo};
use crate::theme;
use crate::views;
use crate::views::cards;
use crate::views::icons::{self, Icon};

pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(detail) = app.retro.detail.as_ref() {
        return detail_view(app, detail);
    }

    let header = views::page_header_labeled(
        app.i18n.t("store.retro_tab"),
        app.i18n.t("native.retro.subtitle"),
        retro_count_label(app),
        Some(Message::Refresh(Section::Stores)),
        app.i18n.t("native.refresh"),
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

fn retro_count_label(app: &App) -> Option<String> {
    let state = &app.retro;
    let total = state.total?;
    Some(match state.total_offers {
        Some(offers) => app.i18n.t_args(
            "native.retro.count",
            &[
                ("games", &total.to_string()),
                ("offers", &offers.to_string()),
            ],
        ),
        None => app.i18n.t_args(
            "native.retro.count_no_offers",
            &[("games", &total.to_string())],
        ),
    })
}

fn body_content(app: &App) -> Element<'_, Message> {
    let state = &app.retro;
    let mut content = column![search_controls(app)]
        .width(Fill)
        .spacing(theme::SPACE_3);

    if !state.applied_query.is_empty() && state.status != LoadStatus::Loading {
        content = content.push(
            text(app.i18n.t_args(
                "native.retro.results_for",
                &[("query", &state.applied_query)],
            ))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemChoice {
    system: RetroSystemInfo,
    label: String,
}

impl std::fmt::Display for SystemChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn search_controls(app: &App) -> Element<'_, Message> {
    let state = &app.retro;
    let i18n = &app.i18n;
    let systems: Vec<SystemChoice> = retro_system_filters()
        .into_iter()
        .map(|system| SystemChoice {
            label: if system.id.is_empty() {
                i18n.t("native.retro.all_systems").to_string()
            } else {
                system.label.to_string()
            },
            system,
        })
        .collect();
    let selected = systems
        .iter()
        .find(|choice| choice.system == state.system)
        .cloned();
    let systems = pick_list(systems, selected, |choice: SystemChoice| {
        Message::RetroSystemChanged(choice.system)
    })
    .width(Length::Fixed(190.0))
    .padding([10.0, 12.0])
    .text_size(12)
    .style(theme::pick_list)
    .menu_style(theme::pick_list_menu);

    let input = Stack::new()
        .width(Fill)
        .push(
            text_input(i18n.t("native.retro.search_placeholder"), &state.query)
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
        text(if loading {
            i18n.t("native.retro.searching")
        } else {
            i18n.t("store.buscar")
        })
            .size(12)
            .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 18.0]);
    if !loading {
        search_button = search_button.on_press(Message::RetroSearchSubmitted);
    }

    let mut clear_button = button(
        text(i18n.t("native.console.store.clear"))
            .size(12)
            .font(theme::FONT_MEDIUM),
    )
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
                views::loading_state(app.i18n.t("native.retro.loading"))
            }
            LoadStatus::Error(_) => container(views::error_state_labeled(
                app.i18n.t("native.retro.load_error"),
                state
                    .error
                    .as_deref()
                    .unwrap_or(app.i18n.t("native.retro.load_error_detail")),
                Message::RetroSearchSubmitted,
                app.i18n.t("downloads.tentar_novamente"),
            ))
            .width(Fill)
            .into(),
            LoadStatus::Ready => {
                if state.applied_query.is_empty() {
                    inline_empty(
                        app.i18n.t("native.retro.empty_title"),
                        app.i18n.t("native.retro.empty_desc"),
                    )
                } else {
                    inline_empty(
                        app.i18n.t("native.retro.no_results_title"),
                        app.i18n.t("native.retro.no_results_desc"),
                    )
                }
            }
        };
    }

    let cards: Vec<Element<'_, Message>> = state
        .games
        .iter()
        .map(|game| retro_card(app, game))
        .collect();

    let mut content = column![].width(Fill).spacing(theme::SPACE_3);
    if state.status == LoadStatus::Loading {
        content = content.push(
            text(app.i18n.t("native.retro.loading_page"))
                .size(11)
                .color(theme::TEXT_3),
        );
    }
    content = content.push(cards::grid(cards, (2.0, 3.55)));
    content = content.push(pagination(app));

    content.into()
}

fn retro_card<'a>(app: &'a App, game: &'a RetroGame) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let covers = &app.covers;
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
                text(initial(i18n, game))
                    .size(30)
                    .font(theme::FONT_BOLD)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30)),
                text(game.system_label_i18n(i18n))
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
        text(game.system_label_i18n(i18n))
            .size(10)
            .color(theme::TEXT_3)
            .wrapping(Wrapping::None),
    ]
    .spacing(6)
    .align_y(iced::Center);

    if let Some(offers) = game.offer_count.filter(|count| *count > 0) {
        meta = meta.push(
            text(i18n.t_args(
                "native.retro.offers_count",
                &[("count", &offers.to_string())],
            ))
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

fn pagination(app: &App) -> Element<'_, Message> {
    let state = &app.retro;
    let loading = state.status == LoadStatus::Loading;
    let page = state.offset / crate::app::RETRO_PAGE_SIZE + 1;
    let total_pages = state.total.map(|total| {
        ((total + crate::app::RETRO_PAGE_SIZE - 1) / crate::app::RETRO_PAGE_SIZE).max(1)
    });

    let page_label = match total_pages {
        Some(total) => app.i18n.t_args(
            "native.retro.page_of",
            &[("page", &page.to_string()), ("total", &total.to_string())],
        ),
        None => app.i18n.t_args("native.retro.page", &[("page", &page.to_string())]),
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
        text(game.system_label_i18n(&app.i18n))
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
            retro_portrait(app, game),
            column![
                text(game.title.as_str())
                    .size(34)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(app.i18n.t_args(
                    "native.retro.id_line",
                    &[("id", game.id.as_str())],
                ))
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
                    text(app.i18n.t("native.retro.back"))
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
                text(app.i18n.t("native.retro.loading_detail"))
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
                retro_description(app, detail, game),
                retro_offers_panel(app, detail)
            ]
            .spacing(theme::SPACE_4)
            .width(Fill),
            retro_info_panel(app, detail, game),
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

fn retro_portrait<'a>(app: &'a App, game: &'a RetroGame) -> Element<'a, Message> {
    let url = game.cover_url.as_deref().or(game.fallback_cover_url.as_deref());
    match app.covers.ready(url) {
        Some(handle) => image(handle.clone())
            .width(116)
            .height(164)
            .content_fit(ContentFit::Cover)
            .border_radius(3)
            .into(),
        None => container(
            text(initial(&app.i18n, game))
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
    app: &'a App,
    detail: &'a RetroDetailState,
    game: &'a RetroGame,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
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
        None if detail.status == LoadStatus::Loading => {
            text(i18n.t("native.retro.description_loading"))
                .size(12)
                .color(theme::TEXT_2)
                .into()
        }
        None => text(i18n.t("native.retro.description_empty"))
            .size(12)
            .color(theme::TEXT_3)
            .into(),
    };

    container(
        column![
            views::section_label(i18n.t("native.game.about")),
            body,
        ]
        .spacing(theme::SPACE_3),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel)
    .into()
}

fn retro_info_panel<'a>(
    app: &'a App,
    detail: &'a RetroDetailState,
    game: &'a RetroGame,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let page_sources: &'a [RetroSource] = &app.retro.sources;
    let mut rows = column![].spacing(theme::SPACE_2);
    let mut has_row = true;

    let system_label = game.system_label_i18n(i18n);
    rows = rows.push(info_row(i18n.t("native.retro.system"), system_label.clone()));
    if let Some(platform) = game
        .platform
        .as_deref()
        .filter(|platform| *platform != system_label.as_str())
    {
        rows = rows.push(info_row(i18n.t("native.retro.platform"), platform.to_string()));
        has_row = true;
    }
    if let Some(year) = game.release_year {
        rows = rows.push(info_row(i18n.t("native.retro.release"), year.to_string()));
        has_row = true;
    }
    if let Some(developer) = game.developer.as_deref() {
        rows = rows.push(info_row(i18n.t("native.retro.developer"), developer.to_string()));
        has_row = true;
    }
    if let Some(publisher) = game.publisher.as_deref() {
        rows = rows.push(info_row(i18n.t("native.retro.publisher"), publisher.to_string()));
        has_row = true;
    }
    if !game.genres.is_empty() {
        rows = rows.push(info_row(i18n.t("native.retro.genres"), game.genres.join(", ")));
        has_row = true;
    }
    if let Some(size) = game.file_size.as_deref() {
        rows = rows.push(info_row(i18n.t("native.retro.size"), size.to_string()));
        has_row = true;
    }
    let offer_count = detail
        .detail
        .as_ref()
        .map(|full| full.offers.len() as u64)
        .filter(|count| *count > 0)
        .or(game.offer_count);
    if let Some(count) = offer_count {
        rows = rows.push(info_row(i18n.t("native.retro.offers"), count.to_string()));
        has_row = true;
    }
    let sources = detail_sources(detail, page_sources);
    if !sources.is_empty() {
        rows = rows.push(info_row(i18n.t("native.retro.sources"), sources.join(", ")));
        has_row = true;
    }
    if !has_row {
        rows = rows.push(
            text(i18n.t("native.retro.no_metadata"))
                .size(12)
                .color(theme::TEXT_3),
        );
    }

    container(
        column![
            views::section_label(i18n.t("native.store.info_title")),
            rows,
        ]
        .spacing(theme::SPACE_3),
    )
        .width(Length::Fixed(320.0))
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn retro_offers_panel<'a>(app: &'a App, detail: &'a RetroDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let offers = detail
        .detail
        .as_ref()
        .map(|full| full.offers.as_slice())
        .unwrap_or_default();
    let mut content = column![views::section_label(i18n.t("native.retro.offers_title"))]
        .spacing(theme::SPACE_2);

    if offers.is_empty() {
        content = content.push(
            text(if detail.status == LoadStatus::Loading {
                i18n.t("native.retro.offers_loading")
            } else {
                i18n.t("native.retro.offers_empty")
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
                .unwrap_or_else(|| i18n.t("native.retro.source_unknown"));
            let mut meta = Vec::new();
            if let Some(size) = offer.file_size.as_deref() {
                meta.push(size.to_string());
            }
            if let Some(date) = offer.upload_date.as_deref() {
                meta.push(date.to_string());
            }
            if offer.uri_count > 0 {
                meta.push(i18n.t_args(
                    "native.retro.links_count",
                    &[("count", &offer.uri_count.to_string())],
                ));
            } else if offer.has_uris {
                meta.push(i18n.t("native.retro.links_available").to_string());
            } else {
                meta.push(i18n.t("native.retro.links_none").to_string());
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
                            text(i18n.t_args(
                                "native.retro.source_line",
                                &[("source", source)],
                            ))
                                .size(10)
                                .color(theme::TEXT_3)
                                .wrapping(Wrapping::WordOrGlyph),
                            text(meta.join(" · ")).size(10).color(theme::TEXT_3),
                        ]
                        .spacing(2)
                        .width(Fill),
                        button(
                            text(i18n.t("native.retro.view_links"))
                                .size(10)
                                .font(theme::FONT_MEDIUM),
                        )
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
        content = content.push(offer_links_panel(app, detail));
    }

    container(content)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn offer_links_panel<'a>(app: &'a App, detail: &'a RetroDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![].spacing(theme::SPACE_2);
    match detail.offer_status {
        LoadStatus::Idle => {
            content = content.push(
                text(i18n.t("native.retro.select_offer"))
                    .size(11)
                    .color(theme::TEXT_3),
            );
        }
        LoadStatus::Loading => {
            content = content.push(
                text(i18n.t("native.retro.offer_loading"))
                    .size(11)
                    .color(theme::TEXT_2),
            );
        }
        LoadStatus::Error(_) => {
            content = content.push(views::action_error_banner_labeled(
                detail
                    .offer_error
                    .as_deref()
                    .unwrap_or(i18n.t("native.retro.offer_error_detail")),
                detail
                    .selected_offer
                    .clone()
                    .map(Message::RetroOfferRequested),
                i18n.t("downloads.tentar_novamente"),
            ));
        }
        LoadStatus::Ready => match detail.offer_detail.as_ref() {
            Some(offer) if offer.uris.is_empty() => {
                content = content.push(
                    text(i18n.t("native.retro.offer_no_uris"))
                        .size(11)
                        .color(theme::TEXT_3),
                );
            }
            Some(offer) => {
                content = content.push(
                    text(i18n.t_args(
                        "native.retro.offer_links_title",
                        &[("id", offer.id.as_str())],
                    ))
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
                    text(i18n.t("native.retro.offer_links_note"))
                    .size(10)
                    .color(theme::TEXT_3)
                    .wrapping(Wrapping::WordOrGlyph),
                );
            }
            None => {
                content = content.push(
                    text(i18n.t("native.retro.offer_no_detail"))
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

fn initial(i18n: &crate::i18n::I18n, game: &RetroGame) -> String {
    let platform = game
        .system_label_i18n(i18n)
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
