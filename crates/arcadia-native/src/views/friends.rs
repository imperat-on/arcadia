//! Amigos: lista de `friends:list`, com pedidos recebidos/enviados.
//!
//! Ações de busca/aceitar/remover ficam para a próxima onda; esta tela mostra
//! o estado real (inclusive "não logado") sem inventar dados.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Color, Element, Fill};

use crate::app::{App, FriendsState, LoadStatus, Message, Section};
use crate::backend::{FriendItem, FriendsData, FriendsLoad};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.friends;
    let count = match state.data.as_ref() {
        Some(FriendsLoad::Ready(data)) => Some(format!("{} amigo(s)", data.friends.len())),
        _ => None,
    };
    let header = views::page_header(
        "Amigos",
        "Lista e pedidos lidos do backend; presença em tempo real entra depois.",
        count,
        Some(Message::Refresh(Section::Friends)),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => views::loading_state("Carregando amigos..."),
        LoadStatus::Error(error) => container(views::error_state(
            "Não foi possível carregar os amigos",
            error,
            Message::Refresh(Section::Friends),
        ))
        .width(Fill)
        .padding([theme::SPACE_2, theme::PAGE_PADDING])
        .into(),
        LoadStatus::Ready => ready_body(state),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body(state: &FriendsState) -> Element<'_, Message> {
    match state.data.as_ref() {
        Some(FriendsLoad::NotLoggedIn) => views::empty_state(
            "Entre na sua conta para ver amigos",
            "Esta tela usa a sessão restaurada do Arcadia. Faça login na UI web/desktop \
             e reabra o shell nativo; nenhum dado é inventado sem sessão.",
        ),
        Some(FriendsLoad::Ready(data)) => friends_body(data),
        None => views::empty_state(
            "Nenhuma resposta de amigos",
            "O dispatcher respondeu sem dados; tente atualizar.",
        ),
    }
}

fn friends_body(data: &FriendsData) -> Element<'_, Message> {
    let summary = row![
        counter("Amigos", data.friends.len(), theme::ACCENT),
        counter("Recebidos", data.incoming.len(), theme::WARNING),
        counter("Enviados", data.outgoing.len(), theme::TEXT_2),
    ]
    .spacing(theme::SPACE_2);

    let mut content = column![summary].spacing(theme::SPACE_5).width(Fill);

    if !data.incoming.is_empty() {
        content = content.push(friend_group(
            "Pedidos recebidos",
            &data.incoming,
            "Aceitar/recusar ainda não têm botão nativo.",
        ));
    }
    if !data.outgoing.is_empty() {
        content = content.push(friend_group(
            "Pedidos enviados",
            &data.outgoing,
            "Aguardando resposta do outro lado.",
        ));
    }
    content = content.push(friend_group(
        "Amigos",
        &data.friends,
        "Nenhum amigo aceito ainda. Busca e convites entram na próxima onda.",
    ));

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn friend_group<'a>(
    title: &'a str,
    friends: &'a [FriendItem],
    empty_hint: &'a str,
) -> Element<'a, Message> {
    let mut group = column![row![
        views::section_label(title),
        text(format!("{}", friends.len()))
            .size(10)
            .color(theme::TEXT_3),
    ]
    .spacing(6)
    .align_y(iced::Center),]
    .spacing(theme::SPACE_2)
    .width(Fill);

    if friends.is_empty() {
        group = group.push(inline_empty("Ninguém por aqui ainda", empty_hint));
    } else {
        for friend in friends {
            group = group.push(friend_card(friend));
        }
    }

    group.into()
}

fn friend_card(friend: &FriendItem) -> Element<'_, Message> {
    let avatar = container(
        text(friend.initial())
            .size(16)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(40)
    .height(40)
    .center_x(Fill)
    .center_y(Fill)
    .style(move |_| container::Style {
        background: Some(avatar_color(&friend.username).into()),
        border: iced::Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    });

    let mut name = row![text(friend.display())
        .size(14)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1),]
    .spacing(8)
    .align_y(iced::Center);

    if friend.display_name.is_some() && friend.display() != friend.username {
        name = name.push(
            text(format!("@{}", friend.username))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    let since = friend
        .since_label()
        .map(|date| format!("Desde {date}"))
        .unwrap_or_else(|| "Sem data de amizade".to_string());

    container(
        row![
            avatar,
            column![name, text(since).size(11).color(theme::TEXT_3),]
                .spacing(2)
                .width(Fill),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(12)
    .style(theme::stub_panel)
    .into()
}

fn counter(label: &str, value: usize, color: Color) -> Element<'_, Message> {
    container(
        row![
            text(format!("{value}"))
                .size(18)
                .font(theme::FONT_BOLD)
                .color(color),
            text(label).size(11).color(theme::TEXT_2),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .padding([8.0, 12.0])
    .style(theme::badge)
    .into()
}

fn inline_empty<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(14)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
            text(message).size(11).color(theme::TEXT_2),
        ]
        .spacing(2),
    )
    .width(Fill)
    .padding(theme::SPACE_4)
    .style(theme::stub_panel)
    .into()
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
