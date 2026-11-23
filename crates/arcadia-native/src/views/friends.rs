//! Amigos: busca, pedidos recebidos/enviados, lista e cartão de perfil.
//!
//! Todas as ações usam `friends:*` de verdade; falha aparece em banner e nunca
//! vira sucesso simulado.

use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, text_input, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill, Padding};

use crate::app::{App, FriendAction, FriendsState, LoadStatus, Message, Section};
use crate::backend::{FriendsData, FriendsLoad};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.friends;
    let i18n = &app.i18n;
    if app.account.is_signed_in() && state.profile.is_some() {
        return friend_profile_card(app);
    }
    let header = container(
        text(i18n.t("amigos.titulo"))
            .size(theme::TEXT_TITLE)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(Fill)
    .padding(Padding {
        top: theme::SPACE_5,
        right: theme::PAGE_PADDING,
        bottom: theme::SPACE_4,
        left: theme::PAGE_PADDING,
    });
    let body = if !app.account.is_signed_in() {
        not_logged_in(app)
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
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .width(Fill),
            button(
                text(i18n.t("account.entrar_btn"))
                    .size(theme::TEXT_READ)
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
                .size(theme::TEXT_BODY)
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
    let input = text_input(
        app.i18n.t("amigos.busca_placeholder"),
        &app.friends.search_query,
    )
    .on_input(Message::FriendSearchChanged)
    .on_submit(Message::FriendSearchSubmitted)
    .padding(Padding {
        top: 14.0,
        right: 16.0,
        bottom: 14.0,
        left: 48.0,
    })
    .size(theme::TEXT_READ)
    .width(Fill)
    .style(theme::search_input);
    Stack::new()
        .width(Fill)
        .height(50)
        .push(input)
        .push(
            container(views::icons::icon(views::icons::Icon::Search, 16.0, 0.4))
                .height(Fill)
                .padding([0.0, 18.0])
                .align_y(iced::Center),
        )
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
                .size(theme::TEXT_BODY)
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
                .size(theme::TEXT_CAPTION)
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
            app,
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
    let enabled = app.friends.action_busy.is_none();
    let mut content = column![].spacing(theme::SPACE_5).width(Fill);
    for (items, key, incoming) in [
        (&data.incoming, "amigos.pedidos_recebidos", true),
        (&data.outgoing, "amigos.pedidos_enviados", false),
    ] {
        if items.is_empty() {
            continue;
        }
        let mut group = column![group_heading(i18n.t(key), items.len())]
            .spacing(theme::SPACE_2)
            .width(Fill);
        for friend in items {
            let cancel = action_button(
                i18n.t(if incoming {
                    "amigos.recusar"
                } else {
                    "amigos.cancelar"
                }),
                Message::FriendActionRequested {
                    id: friend.id.clone(),
                    action: FriendAction::Cancel,
                },
                false,
                enabled,
            );
            let actions = if incoming {
                row![
                    action_button(
                        i18n.t("amigos.aceitar"),
                        Message::FriendActionRequested {
                            id: friend.id.clone(),
                            action: FriendAction::Accept
                        },
                        true,
                        enabled
                    ),
                    cancel
                ]
                .spacing(theme::SPACE_2)
                .into()
            } else {
                cancel
            };
            group = group.push(person_card(
                app,
                &friend.username,
                friend.display(),
                friend.avatar_url.as_deref(),
                friend.since_label(),
                actions,
            ));
        }
        content = content.push(group);
    }
    let mut list = column![group_heading(i18n.t("amigos.lista"), data.friends.len())]
        .spacing(theme::SPACE_2)
        .width(Fill);
    if data.friends.is_empty() {
        list = list.push(
            container(
                text(i18n.t("amigos.vazio"))
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_2),
            )
            .width(Fill)
            .padding(theme::SPACE_5)
            .style(theme::empty_panel),
        );
    }
    for friend in &data.friends {
        let action = action_button(
            i18n.t("amigos.ver_perfil"),
            Message::FriendProfileRequested(friend.id.clone()),
            false,
            enabled,
        );
        list = list.push(
            mouse_area(person_card(
                app,
                &friend.username,
                friend.display(),
                friend.avatar_url.as_deref(),
                friend.since_label(),
                action,
            ))
            .on_press(Message::FriendProfileRequested(friend.id.clone())),
        );
    }
    content.push(list).into()
}

fn group_heading(label: &str, count: usize) -> Element<'_, Message> {
    row![
        views::section_label(label),
        container(
            text(count.to_string())
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_2)
        )
        .padding([2.0, 6.0])
        .style(theme::badge)
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center)
    .into()
}

fn person_card<'a>(
    app: &'a App,
    username: &'a str,
    display: &'a str,
    avatar_url: Option<&'a str>,
    since: Option<String>,
    actions: Element<'a, Message>,
) -> Element<'a, Message> {
    let avatar: Element<'a, Message> =
        match avatar_url.and_then(|url| app.friend_avatar_texture(url)) {
            Some(handle) => image(handle.clone())
                .width(48)
                .height(48)
                .content_fit(ContentFit::Cover)
                .border_radius(4)
                .into(),
            None => container(
                text(initial(display))
                    .size(theme::TEXT_LABEL)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
            )
            .width(48)
            .height(48)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(move |_| container::Style {
                background: Some(avatar_color(username).into()),
                border: iced::Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into(),
        };
    let mut name = row![text(display)
        .size(theme::TEXT_READ)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1)]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);
    if display != username {
        name = name.push(
            text(format!("@{username}"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    let mut details = column![name].spacing(3).width(Fill);
    if let Some(date) = since {
        details = details.push(
            text(app.i18n.t_args("native.friends.since", &[("date", &date)]))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    container(
        row![avatar, details, actions]
            .spacing(theme::SPACE_3)
            .align_y(iced::Center),
    )
    .width(Fill)
    .padding([15.0, 20.0])
    .style(theme::stub_panel)
    .into()
}

fn friend_profile_card(app: &App) -> Element<'_, Message> {
    let Some(state) = app.friends.profile.as_ref() else {
        return views::loading_state(app.i18n.t("common.carregando"));
    };
    if let Some(profile) = state.profile.as_ref() {
        return super::friend_profile::view(app, state, profile);
    }
    let body = match &state.status {
        LoadStatus::Error(error) => views::action_error_banner(
            error,
            Some(Message::FriendProfileRequested(state.id.clone())),
        ),
        _ => views::loading_state(app.i18n.t("common.carregando")),
    };
    column![
        super::back_button::labeled(
            app.i18n.t("common.voltar"),
            Message::FriendProfileClosed,
            theme::ghost_button
        ),
        body
    ]
    .spacing(theme::SPACE_3)
    .width(Fill)
    .height(Fill)
    .padding(theme::PAGE_PADDING)
    .into()
}

fn action_button<'a>(
    label: &'a str,
    message: Message,
    primary: bool,
    enabled: bool,
) -> Element<'a, Message> {
    button(
        container(
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .on_press_maybe(enabled.then_some(message))
    .height(theme::CONTROL_HEIGHT)
    .padding([0.0, 12.0])
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
    container(
        text(message)
            .size(theme::TEXT_BODY)
            .color(theme::SUCCESS)
            .width(Fill),
    )
    .width(Fill)
    .padding([10.0, 14.0])
    .style(theme::stub_panel)
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
