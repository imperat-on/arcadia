//! Tela de autenticação nativa — o mesmo empilhamento do `AuthDialog.tsx`:
//! fundo quase preto com dois brilhos azuis, marca ARCADIA centralizada e um
//! cartão único com abas, campos com ícone e botão azul de largura total.

#[path = "auth_art.rs"]
mod auth_art;

use iced::widget::{
    button, canvas, column, container, row, scrollable, text, text_input, Space, Stack,
};
use iced::Padding;
use iced::{Element, Fill, Length};

use crate::app::{App, AuthMode, Message};
use crate::theme;
use crate::views::chrome;
use auth_art::Glyph;

pub const EMAIL_ID: &str = "arcadia-auth-email";
pub const USERNAME_ID: &str = "arcadia-auth-username";
pub const PASSWORD_ID: &str = "arcadia-auth-password";

pub fn view(app: &App) -> Element<'_, Message> {
    let auth = &app.account.auth;
    let i18n = &app.i18n;
    let signup = auth.mode == AuthMode::SignUp;

    let card_width = theme::AUTH_FORM_WIDTH.min(app.window_size.width - 2.0 * theme::SPACE_4);

    let mut form = column![].spacing(theme::SPACE_3).width(Fill);
    if signup {
        form = form.push(field(
            Glyph::Mail,
            text_input(i18n.t("account.email"), &auth.email)
                .id(iced::widget::Id::new(EMAIL_ID))
                .on_input(Message::AuthEmailChanged)
                .on_submit(Message::AuthSubmit),
        ));
    }
    form = form.push(field(
        Glyph::User,
        text_input(i18n.t("native.auth.username_placeholder"), &auth.username)
            .id(iced::widget::Id::new(USERNAME_ID))
            .on_input(Message::AuthUsernameChanged)
            .on_submit(Message::AuthSubmit),
    ));
    form = form.push(field(
        Glyph::Lock,
        text_input(i18n.t("native.auth.password_placeholder"), &auth.password)
            .id(iced::widget::Id::new(PASSWORD_ID))
            .on_input(Message::AuthPasswordChanged)
            .on_submit(Message::AuthSubmit)
            .secure(true),
    ));
    if signup {
        form = form.push(
            text(i18n.t("account.username_hint"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    if let Some(error) = auth.error.as_deref() {
        form = form.push(
            container(
                text(error)
                    .size(theme::TEXT_CAPTION)
                    .color(theme::DANGER)
                    .width(Fill),
            )
            .width(Fill)
            .padding([theme::SPACE_2, theme::SPACE_3])
            .style(theme::error_panel),
        );
    }

    let populated = if signup {
        !auth.email.trim().is_empty()
            && !auth.username.trim().is_empty()
            && !auth.password.is_empty()
    } else {
        !auth.username.trim().is_empty() && !auth.password.is_empty()
    };
    let submit_label = if auth.sending {
        i18n.t("account.enviando")
    } else if signup {
        i18n.t("account.criar_conta_btn")
    } else {
        i18n.t("account.entrar_btn")
    };
    let label_color = if populated {
        theme::TEXT_1
    } else {
        theme::TEXT_2
    };
    let submit = button(
        // `button::layout` posiciona o conteúdo no topo-esquerda da área com
        // padding; sem este container `Fill` + `align_*`, o rótulo fica colado
        // à esquerda em vez de centralizado no botão.
        container(
            row![
                canvas::Canvas::new(auth_art::Icon {
                    glyph: Glyph::Arrow,
                    color: label_color,
                })
                .width(17)
                .height(17),
                text(submit_label)
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_BOLD)
                    .color(label_color),
            ]
            .spacing(theme::SPACE_2)
            .align_y(iced::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(theme::AUTH_CONTROL_HEIGHT)
    .padding(0)
    .on_press_maybe((!auth.sending).then_some(Message::AuthSubmit))
    .style(move |style_theme, status| theme::auth_submit(style_theme, status, populated));

    let card = container(
        column![
            tabs(app),
            text(if signup {
                i18n.t("account.descricao_criar")
            } else {
                i18n.t("account.descricao")
            })
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
            form,
            submit,
        ]
        .spacing(theme::SPACE_3)
        .width(Fill),
    )
    .width(card_width)
    .padding(theme::SPACE_5)
    .style(theme::auth_card);

    // O `scrollable` entrega altura infinita ao conteúdo, e `Fill` dentro de
    // limite infinito colapsa (a marca e o cartão sumiam). A área é calculada
    // a partir da janela, com piso para telas baixas continuarem roláveis.
    let top_height = theme::TOPBAR_HEIGHT + theme::SPACE_4;
    let area_height = (app.window_size.height - top_height).max(640.0);
    let center = column![
        Space::new().height(Length::FillPortion(2)),
        brand(),
        Space::new().height(theme::SPACE_5),
        card,
        Space::new().height(Length::FillPortion(3)),
    ]
    .align_x(iced::Center)
    .width(Fill)
    .height(area_height);

    let body = column![
        top_bar(auth.required),
        container(center).width(Fill).height(area_height),
    ]
    .width(Fill)
    .height(top_height + area_height);

    container(
        Stack::new()
            .width(Fill)
            .height(Fill)
            .push(
                canvas::Canvas::new(auth_art::Backdrop)
                    .width(Fill)
                    .height(Fill),
            )
            .push(
                scrollable(body)
                    .width(Fill)
                    .height(Fill)
                    .style(theme::scrollable_bare),
            ),
    )
    .width(Fill)
    .height(Fill)
    .style(theme::auth_background)
    .into()
}

/// Faixa superior arrastável: controles de janela à esquerda e o "fechar" do
/// diálogo à direita (ausente quando o login é obrigatório, como no web).
fn top_bar(required: bool) -> Element<'static, Message> {
    let mut bar = row![
        chrome::dot(
            theme::CONTROL_CLOSE,
            theme::CONTROL_CLOSE_RING,
            Message::WindowClose,
        ),
        chrome::dot(
            theme::CONTROL_MINIMIZE,
            theme::CONTROL_MINIMIZE_RING,
            Message::WindowMinimize,
        ),
        chrome::dot(
            theme::CONTROL_MAXIMIZE,
            theme::CONTROL_MAXIMIZE_RING,
            Message::WindowToggleMaximize,
        ),
        Space::new().width(Fill),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    if !required {
        bar = bar.push(
            button(
                canvas::Canvas::new(auth_art::Icon {
                    glyph: Glyph::Close,
                    color: theme::TEXT_2,
                })
                .width(14)
                .height(14),
            )
            .on_press(Message::AuthClosed)
            .width(Length::Fixed(34.0))
            .height(Length::Fixed(34.0))
            .padding(0)
            .style(theme::auth_close),
        );
    }

    container(bar)
        .width(Fill)
        .height(theme::TOPBAR_HEIGHT + theme::SPACE_4)
        .padding([0.0, theme::SPACE_4])
        .align_y(iced::Center)
        .into()
}

/// Marca ARCADIA com o "IA" em ciano, como no `AuthDialog.tsx`.
fn brand() -> Element<'static, Message> {
    column![
        row![
            text("ARCAD")
                .size(34)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text("IA")
                .size(34)
                .font(theme::FONT_BOLD)
                .color(theme::ACCENT),
        ]
        .spacing(0)
        .align_y(iced::Center),
        text("LAUNCHER")
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_1)
    .align_x(iced::Center)
    .into()
}

fn field<'a>(glyph: Glyph, input: iced::widget::TextInput<'a, Message>) -> Element<'a, Message> {
    // O ícone é sobreposto à faixa esquerda do próprio campo: assim o anel de
    // foco (desenhado pelo `text_input`) envolve ícone e texto, como no web.
    let icon = container(
        canvas::Canvas::new(auth_art::Icon {
            glyph,
            color: theme::AUTH_ICON,
        })
        .width(16)
        .height(16),
    )
    .height(Fill)
    .padding([0.0, theme::SPACE_3])
    .align_y(iced::Center);

    Stack::new()
        .width(Fill)
        .height(theme::AUTH_INPUT_HEIGHT)
        .push(
            input
                .padding(Padding {
                    top: theme::SPACE_3,
                    right: theme::SPACE_4,
                    bottom: theme::SPACE_3,
                    left: 42.0,
                })
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::auth_input),
        )
        .push(icon)
        .into()
}

fn tabs(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let mode = app.account.auth.mode;
    container(
        row![
            tab(i18n.t("account.modo_criar"), AuthMode::SignUp, mode),
            tab(i18n.t("account.modo_entrar"), AuthMode::SignIn, mode),
        ]
        .spacing(theme::SPACE_1),
    )
    .width(Fill)
    .height(theme::AUTH_CONTROL_HEIGHT)
    .padding(theme::SPACE_1)
    .style(theme::auth_tab_bar)
    .into()
}

fn tab(label: &str, target: AuthMode, current: AuthMode) -> Element<'_, Message> {
    let active = current == target;
    button(
        container(
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_SEMIBOLD)
                .color(if active { theme::TEXT_1 } else { theme::TEXT_3 }),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center),
    )
    .on_press(Message::AuthModeChanged(target))
    .width(Fill)
    .height(Fill)
    .padding(0)
    .style(move |style_theme, status| theme::auth_tab(style_theme, status, active))
    .into()
}
