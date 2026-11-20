//! Overlay fullscreen de login/cadastro, espelhando o `AuthDialog.tsx`.
//!
//! O dispatcher valida os campos de verdade; aqui só ficam checagens básicas
//! que evitam uma ida à rede. A senha nunca é logada nem sai do estado do
//! formulário.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Color, Element, Fill};

use crate::app::{App, AuthMode, Message};
use crate::theme;

pub const EMAIL_ID: &str = "arcadia-auth-email";
pub const USERNAME_ID: &str = "arcadia-auth-username";
pub const PASSWORD_ID: &str = "arcadia-auth-password";

pub fn view(app: &App) -> Element<'_, Message> {
    let auth = &app.account.auth;
    let i18n = &app.i18n;
    let signup = auth.mode == AuthMode::SignUp;

    let mut header = row![
        text("ARCAD")
            .size(26)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text("IA")
            .size(26)
            .font(theme::FONT_BOLD)
            .color(theme::ACCENT),
    ]
    .spacing(0)
    .align_y(iced::Center);

    if !auth.required {
        header = header.push(Space::new().width(Fill)).push(
            button(text("✕").size(16).color(theme::TEXT_2))
                .on_press(Message::AuthClosed)
                .padding([4.0, 8.0])
                .style(theme::plain_button),
        );
    }

    let mut card = column![
        header,
        text("LAUNCHER")
            .size(10)
            .font(theme::FONT_MEDIUM)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.30)),
        Space::new().height(20),
        auth_tabs(app),
        Space::new().height(4),
        text(if signup {
            i18n.t("account.descricao_criar")
        } else {
            i18n.t("account.descricao")
        })
        .size(12)
        .color(theme::TEXT_2)
        .width(Fill),
        Space::new().height(8),
    ]
    .spacing(4)
    .width(Fill);

    if signup {
        card = card.push(
            text_input(i18n.t("account.email"), &auth.email)
                .id(iced::widget::Id::new(EMAIL_ID))
                .on_input(Message::AuthEmailChanged)
                .on_submit(Message::AuthSubmit)
                .padding([10.0, 12.0])
                .size(13)
                .width(Fill)
                .style(theme::search_input),
        );
    }

    card = card
        .push(
            text_input(i18n.t("native.auth.username_placeholder"), &auth.username)
                .id(iced::widget::Id::new(USERNAME_ID))
                .on_input(Message::AuthUsernameChanged)
                .on_submit(Message::AuthSubmit)
                .padding([10.0, 12.0])
                .size(13)
                .width(Fill)
                .style(theme::search_input),
        )
        .push(
            text_input(i18n.t("native.auth.password_placeholder"), &auth.password)
                .id(iced::widget::Id::new(PASSWORD_ID))
                .on_input(Message::AuthPasswordChanged)
                .on_submit(Message::AuthSubmit)
                .secure(true)
                .padding([10.0, 12.0])
                .size(13)
                .width(Fill)
                .style(theme::search_input),
        );

    if signup {
        card = card.push(
            text(i18n.t("account.username_hint"))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    if let Some(error) = auth.error.as_ref() {
        card = card.push(
            container(text(error).size(11).color(theme::DANGER).width(Fill))
                .width(Fill)
                .padding([8.0, 10.0])
                .style(theme::error_panel),
        );
    }

    let can_send = !auth.sending;
    card = card.push(
        button(
            text(if auth.sending {
                i18n.t("account.enviando")
            } else if signup {
                i18n.t("account.criar_conta_btn")
            } else {
                i18n.t("account.entrar_btn")
            })
            .size(13)
            .font(theme::FONT_SEMIBOLD),
        )
        .on_press_maybe(can_send.then_some(Message::AuthSubmit))
        .width(Fill)
        .padding([11.0, 16.0])
        .style(theme::primary_button),
    );

    let card = container(card.spacing(10))
        .width(Fill)
        .max_width(440)
        .padding(28)
        .style(theme::panel);

    container(container(card).center_x(Fill).center_y(Fill).padding(24))
        .width(Fill)
        .height(Fill)
        .style(theme::app_background)
        .into()
}

fn auth_tabs<'a>(app: &'a App) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mode = app.account.auth.mode;

    row![
        auth_tab(i18n.t("account.modo_entrar"), AuthMode::SignIn, mode),
        auth_tab(i18n.t("account.modo_criar"), AuthMode::SignUp, mode),
    ]
    .spacing(6)
    .width(Fill)
    .into()
}

fn auth_tab<'a>(label: &'a str, target: AuthMode, active: AuthMode) -> Element<'a, Message> {
    let active = active == target;
    button(text(label).size(12).font(theme::FONT_SEMIBOLD))
        .on_press(Message::AuthModeChanged(target))
        .width(Fill)
        .padding([8.0, 10.0])
        .style(move |style_theme, status| {
            if active {
                theme::primary_button(style_theme, status)
            } else {
                theme::ghost_button(style_theme, status)
            }
        })
        .into()
}
