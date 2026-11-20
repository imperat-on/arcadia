//! Amigos: busca, pedidos recebidos/enviados, lista e cartão de perfil.
//!
//! Todas as ações usam `friends:*` de verdade; falha aparece em banner e nunca
//! vira sucesso simulado.

use iced::widget::{image, button, column, container, row, scrollable, text, text_input, Space};
use iced::{Color, ContentFit, Element, Fill};

use crate::app::{App, FriendAction, FriendsState, LoadStatus, Message, Section};
use crate::backend::{FriendAchievements, FriendProfile, FriendsData, FriendsLoad};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.friends;
    let i18n = &app.i18n;
    let count = match state.data.as_ref() {
        Some(FriendsLoad::Ready(data)) => Some(i18n.t_args(
            "native.friends.count",
            &[("count", &data.friends.len().to_string())],
        )),
        _ => None,
    };
    let header = views::page_header_labeled(
        i18n.t("amigos.titulo"),
        i18n.t("native.friends.subtitle"),
        count,
        app.account
            .is_signed_in()
            .then_some(Message::Refresh(Section::Friends)),
        i18n.t("native.refresh"),
    );

    let body: Element<'_, Message> = if !app.account.is_signed_in() {
        not_logged_in(app)
    } else if state.profile.is_some() {
        friend_profile_card(app)
    } else {
        match &state.status {
            LoadStatus::Idle | LoadStatus::Loading => {
                views::loading_state(i18n.t("native.friends.loading"))
            }
            LoadStatus::Error(error) => container(views::error_state_labeled(
                i18n.t("native.friends.load_error"),
                error,
                Message::Refresh(Section::Friends),
                i18n.t("downloads.tentar_novamente"),
            ))
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING])
            .into(),
            LoadStatus::Ready => ready_body(app, state),
        }
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn not_logged_in(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    container(
        column![
            text(i18n.t("amigos.nao_logado"))
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t("native.friends.not_logged_desc"))
                .size(13)
                .color(theme::TEXT_2)
                .width(Fill),
            button(
                text(i18n.t("account.entrar_btn"))
                    .size(13)
                    .font(theme::FONT_SEMIBOLD)
            )
            .on_press(Message::OpenAuth { required: false })
            .padding([10.0, 18.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_3)
        .max_width(520),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .padding(theme::PAGE_PADDING * 2.0)
    .into()
}

fn ready_body<'a>(app: &'a App, state: &'a FriendsState) -> Element<'a, Message> {
    let mut content = column![search_bar(app)].spacing(theme::SPACE_4).width(Fill);

    if let Some(error) = state.action_error.as_ref() {
        content = content.push(views::action_error_banner(error, None));
    }
    if let Some(notice) = state.notice.as_ref() {
        content = content.push(success_banner(notice));
    }

    if let Some(results) = state.search_results.as_ref() {
        content = content.push(search_results(app, results));
    } else if state.search_status == LoadStatus::Loading {
        content = content.push(
            text(app.i18n.t("native.friends.searching"))
                .size(12)
                .color(theme::TEXT_2),
        );
    }

    match state.data.as_ref() {
        Some(FriendsLoad::Ready(data)) => content = content.push(friends_body(app, data)),
        Some(FriendsLoad::NotLoggedIn) => content = content.push(not_logged_in_hint(app)),
        None => {}
    }

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn not_logged_in_hint(app: &App) -> Element<'_, Message> {
    views::warning_banner(app.i18n.t("native.friends.session_unavailable"))
}

fn search_bar(app: &App) -> Element<'_, Message> {
    let state = &app.friends;
    let i18n = &app.i18n;
    let busy = state.action_busy.is_some();
    row![
        text_input(i18n.t("amigos.busca_placeholder"), &state.search_query)
            .on_input(Message::FriendSearchChanged)
            .on_submit(Message::FriendSearchSubmitted)
            .padding([9.0, 12.0])
            .size(13)
            .width(Fill)
            .style(theme::search_input),
        button(
            text(if state.search_status == LoadStatus::Loading {
                i18n.t("common.pesquisando")
            } else {
                i18n.t("store.buscar")
            })
            .size(12)
            .font(theme::FONT_MEDIUM)
        )
        .on_press_maybe((!busy).then_some(Message::FriendSearchSubmitted))
        .padding([9.0, 16.0])
        .style(theme::primary_button),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center)
    .into()
}

fn search_results<'a>(
    app: &'a App,
    results: &'a [crate::backend::FriendSearchResult],
) -> Element<'a, Message> {
    let mut group = column![views::section_label(app.i18n.t("amigos.resultados"))]
        .spacing(theme::SPACE_2)
        .width(Fill);

    if results.is_empty() {
        group = group.push(
            text(app.i18n.t("amigos.sem_resultados"))
                .size(12)
                .color(theme::TEXT_2),
        );
    }

    for result in results {
        let status = result.status.as_deref();
        let actions: Element<'a, Message> = match status {
            Some("accepted") => action_button(
                app.i18n.t("amigos.ver_perfil"),
                Message::FriendProfileRequested(result.id.clone()),
                true,
                app.friends.action_busy.is_none(),
            ),
            Some("pending") if result.incoming => action_button(
                app.i18n.t("amigos.aceitar"),
                Message::FriendActionRequested {
                    id: result.id.clone(),
                    action: FriendAction::Accept,
                },
                true,
                app.friends.action_busy.is_none(),
            ),
            Some("pending") => text(app.i18n.t("amigos.pendente"))
                .size(11)
                .color(theme::WARNING)
                .into(),
            _ => action_button(
                app.i18n.t("amigos.adicionar"),
                Message::FriendActionRequested {
                    id: result.id.clone(),
                    action: FriendAction::Send,
                },
                true,
                app.friends.action_busy.is_none(),
            ),
        };

        group = group.push(person_card(
            &app.i18n,
            &result.username,
            result.display(),
            result.avatar_url.as_deref(),
            None,
            actions,
        ));
    }

    group.into()
}

fn friends_body<'a>(app: &'a App, data: &'a FriendsData) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let busy = app.friends.action_busy.is_none();
    let summary = row![
        counter(i18n.t("amigos.lista"), data.friends.len(), theme::ACCENT),
        counter(
            i18n.t("native.friends.received"),
            data.incoming.len(),
            theme::WARNING
        ),
        counter(
            i18n.t("native.friends.sent"),
            data.outgoing.len(),
            theme::TEXT_2
        ),
    ]
    .spacing(theme::SPACE_2);

    let mut content = column![summary].spacing(theme::SPACE_5).width(Fill);

    if !data.incoming.is_empty() {
        let mut group = column![views::section_label(i18n.t("amigos.pedidos_recebidos"))]
            .spacing(theme::SPACE_2)
            .width(Fill);
        for friend in &data.incoming {
            let actions: Element<'a, Message> = row![
                action_button(
                    i18n.t("amigos.aceitar"),
                    Message::FriendActionRequested {
                        id: friend.id.clone(),
                        action: FriendAction::Accept,
                    },
                    true,
                    busy,
                ),
                action_button(
                    i18n.t("amigos.recusar"),
                    Message::FriendActionRequested {
                        id: friend.id.clone(),
                        action: FriendAction::Cancel,
                    },
                    false,
                    busy,
                ),
            ]
            .spacing(theme::SPACE_2)
            .into();
            group = group.push(person_card(
                i18n,
                &friend.username,
                friend.display(),
                friend.avatar_url.as_deref(),
                friend.since_label(),
                actions,
            ));
        }
        content = content.push(group);
    }

    if !data.outgoing.is_empty() {
        let mut group = column![views::section_label(i18n.t("amigos.pedidos_enviados"))]
            .spacing(theme::SPACE_2)
            .width(Fill);
        for friend in &data.outgoing {
            let actions: Element<'a, Message> = action_button(
                i18n.t("common.cancelar"),
                Message::FriendActionRequested {
                    id: friend.id.clone(),
                    action: FriendAction::Cancel,
                },
                false,
                busy,
            );
            group = group.push(person_card(
                i18n,
                &friend.username,
                friend.display(),
                friend.avatar_url.as_deref(),
                friend.since_label(),
                actions,
            ));
        }
        content = content.push(group);
    }

    let mut friends = column![views::section_label(i18n.t("amigos.lista"))]
        .spacing(theme::SPACE_2)
        .width(Fill);
    if data.friends.is_empty() {
        friends = friends.push(
            container(
                text(i18n.t("amigos.vazio"))
                    .size(12)
                    .color(theme::TEXT_2),
            )
            .width(Fill)
            .padding(theme::SPACE_4)
            .style(theme::empty_panel),
        );
    } else {
        for friend in &data.friends {
            let actions: Element<'a, Message> = row![
                action_button(
                    i18n.t("amigos.ver_perfil"),
                    Message::FriendProfileRequested(friend.id.clone()),
                    false,
                    busy,
                ),
                action_button(
                    i18n.t("common.remover"),
                    Message::FriendActionRequested {
                        id: friend.id.clone(),
                        action: FriendAction::Remove,
                    },
                    false,
                    busy,
                ),
            ]
            .spacing(theme::SPACE_2)
            .into();
            friends = friends.push(person_card(
                i18n,
                &friend.username,
                friend.display(),
                friend.avatar_url.as_deref(),
                friend.since_label(),
                actions,
            ));
        }
    }
    content = content.push(friends);

    content.into()
}

fn person_card<'a>(
    i18n: &'a crate::i18n::I18n,
    username: &'a str,
    display: &'a str,
    _avatar_url: Option<&'a str>,
    since: Option<String>,
    actions: Element<'a, Message>,
) -> Element<'a, Message> {
    let avatar = container(
        text(initial(display))
            .size(16)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(40)
    .height(40)
    .center_x(Fill)
    .center_y(Fill)
    .style(move |_| container::Style {
        background: Some(avatar_color(username).into()),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    });

    let mut name = row![text(display)
        .size(14)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1)]
    .spacing(8)
    .align_y(iced::Center);
    if display != username {
        name = name.push(text(format!("@{username}")).size(10).color(theme::TEXT_3));
    }

    let since = since
        .map(|date| i18n.t_args("native.friends.since", &[("date", &date)]))
        .unwrap_or_else(|| i18n.t("native.friends.no_since").to_string());

    container(
        row![
            avatar,
            column![name, text(since).size(11).color(theme::TEXT_3)]
                .spacing(2)
                .width(Fill),
            actions,
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(12)
    .style(theme::stub_panel)
    .into()
}

fn friend_profile_card(app: &App) -> Element<'_, Message> {
    let Some(state) = app.friends.profile.as_ref() else {
        return views::empty_state(
            app.i18n.t("native.friends.no_profile"),
            app.i18n.t("native.friends.back_to_list"),
        );
    };
    let is_friend = state.is_friend
        || matches!(
            app.friends.data.as_ref(),
            Some(FriendsLoad::Ready(data)) if data.friends.iter().any(|friend| friend.id == state.id)
        );

    let mut content = column![row![
        button(
            text(format!("← {}", app.i18n.t("common.voltar")))
                .size(12)
                .font(theme::FONT_MEDIUM),
        )
            .on_press(Message::FriendProfileClosed)
            .padding([7.0, 12.0])
            .style(theme::ghost_button),
        Space::new().width(Fill),
        if is_friend {
            action_button(
                app.i18n.t("amigos.remover"),
                Message::FriendActionRequested {
                    id: state.id.clone(),
                    action: FriendAction::Remove,
                },
                false,
                app.friends.action_busy.is_none(),
            )
        } else {
            Space::new().width(0).into()
        },
    ]
    .align_y(iced::Center),]
    .spacing(theme::SPACE_3)
    .width(Fill);

    match state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            content = content.push(views::loading_state(
                app.i18n.t("native.friends.loading_profile"),
            ));
        }
        LoadStatus::Error(ref error) => {
            content = content.push(views::action_error_banner(error, None));
        }
        LoadStatus::Ready => {
            if let Some(profile) = state.profile.as_ref() {
                content = content.push(profile_details(app, profile));
            }
        }
    }
    content = content.push(achievements_section(app, state, is_friend));

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn achievements_section<'a>(
    app: &'a App,
    state: &'a crate::app::FriendProfileState,
    is_friend: bool,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![].spacing(theme::SPACE_2).width(Fill);
    let count = state
        .achievements
        .as_ref()
        .map(|achievements| format!(" ({})", achievements.items.len()))
        .unwrap_or_default();
    content = content.push(
        text(format!(
            "{}{count}",
            i18n.t("amigos.conquistas_recentes")
        ))
        .size(13)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1),
    );

    if !is_friend {
        return content
            .push(hint(
                i18n.t("native.friends.achievements_private"),
                theme::TEXT_3,
            ))
            .into();
    }

    match state.achievements_status {
        LoadStatus::Idle | LoadStatus::Loading => content = content.push(hint(
            i18n.t("native.friends.achievements_loading"),
            theme::TEXT_3,
        )),
        LoadStatus::Error(ref error) => {
            content = content.push(views::action_error_banner(error, None));
        }
        LoadStatus::Ready => {
            let Some(achievements) = state.achievements.as_ref() else {
                content = content.push(hint(i18n.t("amigos.sem_conquistas"), theme::TEXT_3));
                return content.into();
            };
            if achievements.items.is_empty() {
                content = content.push(hint(i18n.t("amigos.sem_conquistas"), theme::TEXT_3));
            } else {
                content = content.push(achievements_summary(i18n, achievements));
                content = content.push(achievement_grid(app, achievements));
            }
        }
    }

    content.into()
}

fn achievements_summary<'a>(
    i18n: &'a crate::i18n::I18n,
    achievements: &'a FriendAchievements,
) -> Element<'a, Message> {
    let summary = achievements.summary();
    let platinum = summary
        .platinum_percent
        .map(|percent| format!("{percent}%"))
        .unwrap_or_else(|| "—".to_string());
    row![
        achievement_stat(i18n.t("amigos.total_conquistas"), summary.total.to_string(), theme::ACCENT),
        achievement_stat(i18n.t("amigos.platinadas"), platinum, theme::SUCCESS),
        achievement_stat(i18n.t("amigos.jogos"), summary.games.to_string(), theme::WARNING),
    ]
    .spacing(theme::SPACE_2)
    .width(Fill)
    .into()
}

fn achievement_stat<'a>(
    label: &'a str,
    value: String,
    color: Color,
) -> Element<'a, Message> {
    container(
        column![
            text(value)
                .size(18)
                .font(theme::FONT_BOLD)
                .color(color),
            text(label).size(10).color(theme::TEXT_3),
        ]
        .spacing(1)
        .align_x(iced::Center),
    )
    .width(Fill)
    .padding([8.0, 12.0])
    .style(theme::badge)
    .into()
}

fn achievement_grid<'a>(
    app: &'a App,
    achievements: &'a FriendAchievements,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(theme::SPACE_2).width(Fill);
    for chunk in achievements.items.chunks(3) {
        let mut line = row![].spacing(theme::SPACE_2).width(Fill);
        for item in chunk {
            line = line.push(achievement_card(app, item));
        }
        grid = grid.push(line);
    }
    grid.into()
}

fn achievement_card<'a>(
    app: &'a App,
    item: &'a crate::backend::FriendAchievement,
) -> Element<'a, Message> {
    let icon: Element<'a, Message> = match app.covers.ready(item.icon.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(40)
            .height(40)
            .content_fit(ContentFit::Cover)
            .border_radius(6)
            .into(),
        None => container(text("🏆").size(18))
            .width(40)
            .height(40)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let percent: Element<'a, Message> = match item.percent {
        Some(percent) if percent.is_finite() => badge(
            format!("{:.0}%", percent),
            if percent >= 100.0 {
                theme::SUCCESS
            } else {
                theme::TEXT_2
            },
        ),
        _ => Space::new().width(0).into(),
    };

    container(
        column![
            row![
                icon,
                column![
                    text(item.title.as_str())
                        .size(11)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_1),
                    text(item.appid.as_str()).size(9).color(theme::TEXT_3),
                ]
                .spacing(2)
                .width(Fill),
                percent,
            ]
            .spacing(theme::SPACE_2)
            .align_y(iced::Center),
            text(if item.unlocked {
                app.i18n.t("native.game.achievement_unlocked")
            } else {
                app.i18n.t("native.game.achievement_locked")
            })
            .size(9)
            .color(if item.unlocked {
                theme::SUCCESS
            } else {
                theme::TEXT_3
            }),
        ]
        .spacing(4),
    )
    .width(Fill)
    .padding(10)
    .style(theme::empty_panel)
    .into()
}

fn hint(message: impl Into<String>, color: Color) -> Element<'static, Message> {
    text(message.into()).size(11).color(color).into()
}

fn badge(label: String, color: Color) -> Element<'static, Message> {
    container(text(label).size(9).font(theme::FONT_MEDIUM).color(color))
        .padding([2.0, 6.0])
        .style(theme::badge)
        .into()
}

fn profile_details<'a>(app: &'a App, profile: &'a FriendProfile) -> Element<'a, Message> {
    let avatar = container(
        text(profile.initial())
            .size(28)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(76)
    .height(76)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(theme::avatar);

    let mut identity = column![
        text(profile.display())
            .size(24)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(format!("@{}", profile.username))
            .size(12)
            .color(theme::TEXT_3),
    ]
    .spacing(2);
    if let Some(location) = profile.location() {
        identity = identity.push(text(location).size(12).color(theme::TEXT_2));
    }

    let mut stats = row![].spacing(theme::SPACE_2);
    if let Some(games) = profile.games {
        stats = stats.push(counter(app.i18n.t("amigos.jogos"), games as usize, theme::ACCENT));
    }
    if let Some(hours) = profile.playtime_hours {
        stats = stats.push(counter(
            app.i18n.t("native.friends.hours"),
            hours as usize,
            theme::TEXT_2,
        ));
    }
    if let Some(friends) = profile.friends {
        stats = stats.push(counter(
            app.i18n.t("amigos.lista"),
            friends as usize,
            theme::TEXT_2,
        ));
    }

    container(
        column![
            row![avatar, identity]
                .spacing(theme::SPACE_4)
                .align_y(iced::Center),
            text(
                profile
                    .summary
                    .as_deref()
                    .filter(|summary| !summary.trim().is_empty())
                    .unwrap_or(app.i18n.t("native.profile.no_summary"))
            )
            .size(13)
            .color(theme::TEXT_2)
            .width(Fill),
            stats,
        ]
        .spacing(theme::SPACE_4),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel)
    .into()
}

fn action_button<'a>(
    label: &'a str,
    message: Message,
    primary: bool,
    enabled: bool,
) -> Element<'a, Message> {
    button(text(label).size(11).font(theme::FONT_MEDIUM))
        .on_press_maybe(enabled.then_some(message))
        .padding([6.0, 10.0])
        .style(move |style_theme, status| {
            if primary {
                theme::primary_button(style_theme, status)
            } else {
                theme::ghost_button(style_theme, status)
            }
        })
        .into()
}

fn success_banner(message: &str) -> Element<'_, Message> {
    container(text(message).size(12).color(theme::SUCCESS).width(Fill))
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::stub_panel)
        .into()
}

fn counter(label: &str, value: usize, color: Color) -> Element<'static, Message> {
    container(
        row![
            text(format!("{value}"))
                .size(18)
                .font(theme::FONT_BOLD)
                .color(color),
            text(label.to_string()).size(11).color(theme::TEXT_2),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .padding([8.0, 12.0])
    .style(theme::badge)
    .into()
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Cor estável por username, como `corDeUsername` da UI web.
fn avatar_color(username: &str) -> Color {
    const PALETTE: [Color; 6] = [
        Color::from_rgb8(0x1f, 0x3a, 0x5f),
        Color::from_rgb8(0x3b, 0x2a, 0x55),
        Color::from_rgb8(0x1d, 0x4a, 0x45),
        Color::from_rgb8(0x53, 0x32, 0x2a),
        Color::from_rgb8(0x4a, 0x3d, 0x1c),
        Color::from_rgb8(0x2f, 0x3d, 0x5c),
    ];
    let hash = username.chars().fold(0usize, |hash, character| {
        (hash.wrapping_mul(31).wrapping_add(character as usize)) % 360
    });
    PALETTE[hash % PALETTE.len()]
}
