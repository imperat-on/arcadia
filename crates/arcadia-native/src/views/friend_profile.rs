//! Perfil público do amigo: mesma composição de Meu perfil, somente leitura.

use iced::widget::{button, column, container, image, row, scrollable, text, Space, Stack};
use iced::{ContentFit, Element, Fill, Padding};

use super::{cards, profile};
use crate::app::{App, FriendAction, FriendProfileState, LoadStatus, Message};
use crate::backend::FriendProfile;
use crate::theme;

const BANNER_HEIGHT: f32 = 192.0;
const AVATAR_SIZE: f32 = 96.0;
const AVATAR_OVERLAP: f32 = 56.0;

pub fn view<'a>(
    app: &'a App,
    state: &'a FriendProfileState,
    friend: &'a FriendProfile,
) -> Element<'a, Message> {
    let mut content = column![header(app, state, friend)]
        .spacing(theme::SPACE_5)
        .width(Fill);
    if let Some(error) = app.friends.action_error.as_ref() {
        content = content.push(super::action_error_banner(error, None));
    }
    let body: Element<'a, Message> = if let LoadStatus::Error(error) = &state.status {
        super::action_error_banner(
            error,
            Some(Message::FriendProfileRequested(state.id.clone())),
        )
    } else if matches!(state.status, LoadStatus::Loading | LoadStatus::Idle) {
        container(
            text(app.i18n.t("common.carregando"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2),
        )
        .width(Fill)
        .padding(theme::SPACE_5)
        .into()
    } else if app.window_size.width - theme::SIDEBAR_WIDTH < 850.0 {
        column![games(app, friend), sidebar(app, friend)]
            .spacing(theme::SPACE_5)
            .width(Fill)
            .into()
    } else {
        row![
            games(app, friend),
            container(sidebar(app, friend)).width(340)
        ]
        .spacing(theme::SPACE_5)
        .width(Fill)
        .into()
    };
    content = content.push(body);
    let page = scrollable(
        container(content)
            .width(Fill)
            .max_width(1220)
            .padding(Padding {
                top: theme::SPACE_5,
                right: theme::PAGE_PADDING,
                bottom: theme::SPACE_6,
                left: theme::PAGE_PADDING,
            }),
    )
    .width(Fill)
    .height(Fill)
    .style(theme::scrollable_bare);
    if let Some(background) = friend
        .background_url
        .as_deref()
        .and_then(|url| profile_image(app, url))
    {
        Stack::new()
            .width(Fill)
            .height(Fill)
            .push(
                image(background.clone())
                    .width(Fill)
                    .height(Fill)
                    .content_fit(ContentFit::Cover),
            )
            .push(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(theme::profile_scrim),
            )
            .push(page)
            .into()
    } else {
        page.into()
    }
}

fn profile_image<'a>(app: &'a App, url: &str) -> Option<&'a iced::widget::image::Handle> {
    app.friend_media
        .texture(url)
        .or_else(|| app.animated_cards.texture(url))
        .or_else(|| app.covers.ready(Some(url)))
}

fn header<'a>(
    app: &'a App,
    state: &'a FriendProfileState,
    friend: &'a FriendProfile,
) -> Element<'a, Message> {
    let banner: Element<'a, Message> = match friend
        .banner_url
        .as_deref()
        .and_then(|url| profile_image(app, url))
    {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(BANNER_HEIGHT)
            .content_fit(ContentFit::Cover)
            .border_radius(12)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(BANNER_HEIGHT)
            .style(theme::banner_placeholder)
            .into(),
    };
    let avatar: Element<'a, Message> = match friend
        .avatar_url
        .as_deref()
        .and_then(|url| profile_image(app, url))
    {
        Some(handle) => image(handle.clone())
            .width(AVATAR_SIZE)
            .height(AVATAR_SIZE)
            .content_fit(ContentFit::Cover)
            .border_radius(8)
            .into(),
        None => container(
            text(friend.initial())
                .size(theme::TEXT_DISPLAY)
                .font(theme::FONT_BOLD),
        )
        .width(AVATAR_SIZE)
        .height(AVATAR_SIZE)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(theme::avatar)
        .into(),
    };
    let mut identity = column![text(friend.display())
        .size(theme::TEXT_DISPLAY)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)]
    .spacing(theme::SPACE_1)
    .width(Fill);
    if let Some(location) = friend.location() {
        identity = identity.push(
            text(location)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2),
        );
    }
    let identity = row![avatar, identity]
        .spacing(theme::SPACE_5)
        .align_y(iced::Bottom);
    let actions = row![
        super::back_button::labeled(
            app.i18n.t("amigos.voltar"),
            Message::FriendProfileClosed,
            theme::ghost_button
        ),
        Space::new().width(Fill),
        remove_action(app, state)
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);
    let mut header = column![Stack::new()
        .width(Fill)
        .height(BANNER_HEIGHT + AVATAR_SIZE - AVATAR_OVERLAP)
        .push(banner)
        .push(
            container(identity)
                .width(Fill)
                .height(Fill)
                .align_y(iced::Bottom)
                .padding([0.0, theme::SPACE_4])
        )
        .push(container(actions).width(Fill).padding(theme::SPACE_2)),]
    .spacing(theme::SPACE_3)
    .width(Fill);
    if let Some(summary) = friend
        .summary
        .as_deref()
        .filter(|summary| !summary.trim().is_empty())
    {
        header = header.push(
            text(summary)
                .size(theme::TEXT_READ)
                .color(theme::TEXT_2)
                .width(Fill),
        );
    }
    header.into()
}

pub(super) fn remove_action<'a>(
    app: &'a App,
    state: &'a FriendProfileState,
) -> Element<'a, Message> {
    if !state.is_friend {
        return Space::new().width(0).into();
    }
    if state.confirming_remove {
        row![
            text(app.i18n.t("amigos.confirmar_remover"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_2),
            button(text(app.i18n.t("amigos.remover")).size(theme::TEXT_MICRO))
                .on_press_maybe(app.friends.action_busy.is_none().then_some(
                    Message::FriendActionRequested {
                        id: state.id.clone(),
                        action: FriendAction::Remove
                    }
                ))
                .padding([8.0, 12.0])
                .style(|theme, status| theme::profile_menu_item(theme, status, true)),
            button(text(app.i18n.t("amigos.cancelar")).size(theme::TEXT_MICRO))
                .on_press(Message::FriendRemoveConfirmation(false))
                .padding([8.0, 12.0])
                .style(theme::ghost_button)
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center)
        .into()
    } else {
        button(text(app.i18n.t("amigos.remover")).size(theme::TEXT_CAPTION))
            .on_press(Message::FriendRemoveConfirmation(true))
            .padding([8.0, 12.0])
            .style(theme::ghost_button)
            .into()
    }
}

fn games<'a>(app: &'a App, friend: &'a FriendProfile) -> Element<'a, Message> {
    let heading = row![
        text(app.i18n.t("profile.todos_jogos"))
            .size(theme::TEXT_TITLE)
            .font(theme::FONT_BOLD),
        Space::new().width(Fill),
        container(
            text(profile::games_count_label(&app.i18n, friend.library.len()))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2)
        )
        .padding([4.0, 10.0])
        .style(theme::badge)
    ]
    .align_y(iced::Center)
    .spacing(theme::SPACE_2);
    let grid = if friend.library.is_empty() {
        container(
            text(app.i18n.t("profile.nenhum_jogado"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2),
        )
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::empty_panel)
        .into()
    } else {
        cards::grid(
            friend
                .library
                .iter()
                .map(|game| profile::profile_tile(game, &app.covers, None))
                .collect(),
            (2.0, 3.0),
        )
    };
    column![heading, grid]
        .spacing(theme::SPACE_4)
        .width(Fill)
        .into()
}

fn sidebar<'a>(app: &'a App, friend: &'a FriendProfile) -> Element<'a, Message> {
    let games = friend.games.unwrap_or(friend.library.len() as u64);
    let hours = friend
        .playtime_hours
        .map(|hours| {
            app.i18n
                .t("profile.estatisticas.horas_display")
                .replace("{h}", &hours.to_string())
        })
        .unwrap_or_else(|| app.i18n.t("profile.estatisticas.fallback").to_string());
    let stats = profile::profile_card(
        app.i18n.t("profile.estatisticas"),
        column![
            profile::stat_row(app.i18n.t("profile.estatisticas.jogos"), games.to_string()),
            profile::stat_row(app.i18n.t("profile.estatisticas.horas"), hours),
            profile::stat_row(
                app.i18n.t("amigos.titulo"),
                friend
                    .friends
                    .unwrap_or(friend.friend_list.len() as u64)
                    .to_string()
            ),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill)
        .into(),
    );
    let friends = if friend.friend_list.is_empty() {
        text(app.i18n.t("profile.vitrine_vazia"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill)
            .into()
    } else {
        iced::widget::Grid::with_children(friend.friend_list.iter().take(8).map(|person| {
            let avatar: Element<'a, Message> = match person
                .avatar_url
                .as_deref()
                .and_then(|url| app.friend_avatar_texture(url))
            {
                Some(handle) => image(handle.clone())
                    .width(Fill)
                    .height(Fill)
                    .content_fit(ContentFit::Cover)
                    .border_radius(8)
                    .into(),
                None => container(
                    text(person.initial())
                        .size(theme::TEXT_HEADING)
                        .font(theme::FONT_BOLD),
                )
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center)
                .style(theme::avatar)
                .into(),
            };
            avatar
        }))
        .columns(4)
        .spacing(theme::SPACE_2)
        .height(iced::widget::grid::aspect_ratio(1.0, 1.0))
        .into()
    };
    column![
        stats,
        profile::profile_card(app.i18n.t("amigos.titulo"), friends)
    ]
    .width(Fill)
    .spacing(theme::SPACE_4)
    .into()
}
