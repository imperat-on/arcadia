//! Perfil da conta logada: identidade, avatar e formulário de edição dos
//! campos aceitos por `account:updateProfile`.

use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{ContentFit, Element, Fill};

use crate::app::{App, Message, ProfileField};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let header = views::page_header(
        "Meu perfil",
        "Dados da conta online; a edição grava somente os campos aceitos pelo IPC.",
        None,
        Some(Message::ReloadAccount),
    );

    if !app.account.is_signed_in() {
        let body = container(
            column![
                text("Entre para ver seu perfil")
                    .size(20)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
                text("Com uma conta você edita nome, resumo, país/cidade e avatar. Sem sessão o launcher continua no perfil local.")
                    .size(13)
                    .color(theme::TEXT_2)
                    .width(Fill),
                button(
                    text("Entrar")
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
        .padding(theme::PAGE_PADDING * 2.0);

        return column![header, body].width(Fill).height(Fill).into();
    }

    let editor = &app.account.editor;
    let mut content = column![profile_card(app)]
        .spacing(theme::SPACE_4)
        .width(Fill);

    if let Some(error) = app.account.error.as_ref() {
        content = content.push(views::action_error_banner(error, None));
    }
    if let Some(notice) = app.account.notice.as_ref() {
        content = content.push(success_banner(notice));
    }
    if let Some(error) = editor.error.as_ref() {
        content = content.push(views::action_error_banner(error, None));
    }
    if let Some(notice) = editor.notice.as_ref() {
        content = content.push(success_banner(notice));
    }

    if editor.editing {
        content = content.push(edit_form(app));
    }

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING])
            .style(theme::app_background),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn profile_card(app: &App) -> Element<'_, Message> {
    let profile = app.account.profile.as_ref();
    let display = profile.map(|profile| profile.display()).unwrap_or("?");
    let username = profile
        .map(|profile| profile.username.as_str())
        .filter(|username| !username.trim().is_empty())
        .or_else(|| {
            app.account
                .session
                .as_ref()
                .and_then(|session| session.username.as_deref())
        })
        .unwrap_or("?");
    let location = profile.and_then(|profile| profile.location());
    let summary = profile
        .and_then(|profile| profile.summary.as_deref())
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or("Sem resumo público.");
    let email = app
        .account
        .session
        .as_ref()
        .and_then(|session| session.email.as_deref());

    let mut identity = column![
        text(display)
            .size(26)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(format!("@{username}")).size(12).color(theme::TEXT_3),
    ]
    .spacing(2);

    if let Some(location) = location {
        identity = identity.push(text(location).size(12).color(theme::TEXT_2));
    }
    if let Some(email) = email {
        identity = identity.push(text(email).size(11).color(theme::TEXT_3));
    }

    let mut actions = row![
        button(text("Editar perfil").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::ProfileEditToggled)
            .padding([8.0, 14.0])
            .style(theme::ghost_button),
        button(text("Sair da conta").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::SignOutRequested)
            .padding([8.0, 14.0])
            .style(theme::ghost_button),
    ]
    .spacing(theme::SPACE_2);

    actions = actions.push(Space::new().width(Fill)).push(
        button(text("Atualizar").size(12).font(theme::FONT_MEDIUM))
            .on_press(Message::ReloadAccount)
            .padding([8.0, 14.0])
            .style(theme::ghost_button),
    );

    container(
        column![
            row![avatar(app), identity.width(Fill)]
                .spacing(theme::SPACE_4)
                .align_y(iced::Center),
            text(summary).size(13).color(theme::TEXT_2).width(Fill),
            actions.width(Fill),
        ]
        .spacing(theme::SPACE_4),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::panel)
    .into()
}

fn avatar(app: &App) -> Element<'_, Message> {
    if let Some(handle) = app.account.avatar.as_ref() {
        return image(handle.clone())
            .width(88)
            .height(88)
            .content_fit(ContentFit::Cover)
            .border_radius(16)
            .into();
    }

    container(
        text(app.account.initial("?"))
            .size(30)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
    )
    .width(88)
    .height(88)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .style(theme::avatar)
    .into()
}

fn edit_form(app: &App) -> Element<'_, Message> {
    let editor = &app.account.editor;
    let mut form = column![
        text("Editar perfil")
            .size(18)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text("Campos abaixo são exatamente os aceitos por account:updateProfile.")
            .size(11)
            .color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    form = form.push(field_input(ProfileField::DisplayName, &editor.display_name));
    form = form.push(field_input(ProfileField::Summary, &editor.summary));
    form = form.push(
        row![
            field_input(ProfileField::Country, &editor.country),
            field_input(ProfileField::City, &editor.city),
        ]
        .spacing(theme::SPACE_3),
    );
    form = form.push(field_input(ProfileField::Showcase, &editor.showcase));
    form = form.push(
        text("A vitrine é uma lista de appids separada por vírgulas.")
            .size(10)
            .color(theme::TEXT_3),
    );
    form = form.push(field_input(
        ProfileField::BackgroundUrl,
        &editor.background_url,
    ));
    form = form.push(field_input(ProfileField::BannerUrl, &editor.banner_url));

    let avatar_row = row![
        text_input(
            "Caminho do arquivo (ex.: /home/você/foto.png)",
            &editor.avatar_path,
        )
        .on_input(Message::AvatarPathChanged)
        .padding([8.0, 10.0])
        .size(12)
        .width(Fill)
        .style(theme::search_input),
        button(
            text(if editor.uploading_avatar {
                "Enviando…"
            } else {
                "Usar este avatar"
            })
            .size(12)
            .font(theme::FONT_MEDIUM)
        )
        .on_press_maybe((!editor.uploading_avatar).then_some(Message::AvatarUpload))
        .padding([8.0, 14.0])
        .style(theme::ghost_button),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    form = form
        .push(avatar_row)
        .push(
            text("O seletor nativo de arquivos do shell Tauri (profile:pickImage) não existe no IPC; informe o caminho manualmente.")
                .size(10)
                .color(theme::TEXT_3),
        )
        .push(
            row![
                button(
                    text(if editor.saving {
                        "Salvando…"
                    } else {
                        "Salvar perfil"
                    })
                    .size(12)
                    .font(theme::FONT_SEMIBOLD)
                )
                .on_press_maybe((!editor.saving).then_some(Message::ProfileSave))
                .padding([9.0, 16.0])
                .style(theme::primary_button),
                button(text("Cancelar").size(12).font(theme::FONT_MEDIUM))
                    .on_press(Message::ProfileEditToggled)
                    .padding([9.0, 16.0])
                    .style(theme::ghost_button),
            ]
            .spacing(theme::SPACE_2),
        );

    container(form)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::stub_panel)
        .into()
}

fn field_input<'a>(field: ProfileField, value: &'a str) -> Element<'a, Message> {
    column![
        text(field.label()).size(11).color(theme::TEXT_2),
        text_input(field.label(), value)
            .on_input(move |value| Message::ProfileFieldChanged { field, value })
            .padding([8.0, 10.0])
            .size(12)
            .width(Fill)
            .style(theme::search_input),
    ]
    .spacing(4)
    .width(Fill)
    .into()
}

fn success_banner(message: &str) -> Element<'_, Message> {
    container(text(message).size(12).color(theme::SUCCESS).width(Fill))
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::stub_panel)
        .into()
}
