//! Telas do shell nativo. Cada tela recebe apenas o que precisa para desenhar.

pub mod components;
pub mod downloads;
pub mod emulation;
pub mod friends;
pub mod big_picture;
pub mod cards;
pub mod chrome;
pub mod community;
pub mod game;
pub mod home;
pub mod icons;
pub mod library;
pub mod overlays;
pub mod retro;
pub mod settings;
pub mod sidebar;
pub mod sources;
pub mod store;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Fill};

use crate::app::{Message, Section};
use crate::i18n::I18n;
use crate::theme;

/// Cabeçalho comum: título, subtítulo, contador e ação de atualizar.
pub(crate) fn page_header<'a>(
    title: &'a str,
    subtitle: &'a str,
    count: Option<String>,
    refresh: Option<Message>,
) -> Element<'a, Message> {
    page_header_labeled(title, subtitle, count, refresh, "Atualizar")
}

/// Igual a [`page_header`], mas com o rótulo da ação vindo do i18n. As telas
/// ainda não migradas continuam usando o wrapper acima.
pub(crate) fn page_header_labeled<'a>(
    title: &'a str,
    subtitle: &'a str,
    count: Option<String>,
    refresh: Option<Message>,
    refresh_label: &'a str,
) -> Element<'a, Message> {
    let mut title_row = row![text(title)
        .size(30)
        .font(theme::FONT_BOLD)
        .color(theme::TEXT_1)]
    .spacing(10)
    .align_y(iced::Center);

    if let Some(count) = count {
        title_row = title_row.push(
            container(
                text(count)
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_2),
            )
            .padding([3.0, 8.0])
            .style(theme::badge),
        );
    }

    let heading = column![title_row, text(subtitle).size(13).color(theme::TEXT_2),].spacing(4);

    let right: Element<'a, Message> = match refresh {
        Some(message) => button(text(refresh_label).size(12).font(theme::FONT_MEDIUM))
            .on_press(message)
            .padding([8.0, 14.0])
            .style(theme::ghost_button)
            .into(),
        None => Space::new().width(0).into(),
    };

    container(
        row![heading, Space::new().width(Fill), right]
            .spacing(theme::SPACE_4)
            .align_y(iced::Center),
    )
    .width(Fill)
    .padding([24.0, theme::PAGE_PADDING])
    .into()
}

pub(crate) fn section_label(title: &str) -> Element<'_, Message> {
    text(title.to_uppercase())
        .size(10)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_3)
        .into()
}

pub(crate) fn loading_state(message: &str) -> Element<'_, Message> {
    centered_message(message)
}

pub(crate) fn empty_state<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(18)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_STRONG),
            text(message).size(13).color(theme::TEXT_2).width(Fill),
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

pub(crate) fn error_state<'a>(
    title: &'a str,
    error: &'a str,
    retry: Message,
) -> Element<'a, Message> {
    error_state_labeled(title, error, retry, "Tentar novamente")
}

/// Igual a [`error_state`], com o rótulo de retry vindo do i18n.
pub(crate) fn error_state_labeled<'a>(
    title: &'a str,
    error: &'a str,
    retry: Message,
    retry_label: &'a str,
) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::DANGER),
            text(error).size(13).color(theme::TEXT_2).width(Fill),
            button(text(retry_label).size(13).font(theme::FONT_MEDIUM))
                .on_press(retry)
                .padding([10.0, 18.0])
                .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_3),
    )
    .width(Fill)
    .max_width(720)
    .padding(theme::SPACE_5)
    .style(theme::error_panel)
    .into()
}

pub(crate) fn warning_banner(message: &str) -> Element<'_, Message> {
    container(text(message).size(12).color(theme::WARNING).width(Fill))
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::stub_panel)
        .into()
}

pub(crate) fn action_error_banner(message: &str, retry: Option<Message>) -> Element<'_, Message> {
    let mut content =
        column![text(message).size(12).color(theme::DANGER).width(Fill),].spacing(theme::SPACE_2);

    if let Some(retry) = retry {
        content = content.push(
            button(text("Tentar novamente").size(12).font(theme::FONT_MEDIUM))
                .on_press(retry)
                .padding([6.0, 12.0])
                .style(theme::ghost_button),
        );
    }

    container(content)
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::error_panel)
        .into()
}

fn centered_message(message: &str) -> Element<'_, Message> {
    container(text(message).size(14).color(theme::TEXT_2))
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .padding(theme::PAGE_PADDING)
        .into()
}

/// Placeholder honesto para telas que ainda não foram portadas.
pub fn section_placeholder<'a>(section: Section, i18n: &'a I18n) -> Element<'a, Message> {
    container(
        iced::widget::column![
            text(section.label(i18n))
                .size(30)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t("native.section.not_ported"))
                .size(11)
                .font(theme::FONT_MEDIUM)
                .color(theme::ACCENT),
            text(section.stub_message(i18n))
                .size(14)
                .color(theme::TEXT_2),
            button(
                text(i18n.t("native.section.back_library"))
                    .size(13)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press(Message::BackToLibrary)
            .padding([10, 18])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_4)
        .max_width(560),
    )
    .width(Fill)
    .height(Fill)
    .padding(theme::PAGE_PADDING + theme::SPACE_6)
    .into()
}

pub fn missing_game<'a>(i18n: &'a I18n) -> Element<'a, Message> {
    container(
        iced::widget::column![
            text(i18n.t("native.game.not_found"))
                .size(26)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t("native.game.not_found_desc"))
                .size(14)
                .color(theme::TEXT_2),
            button(
                text(i18n.t("native.section.back_library"))
                    .size(13)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press(Message::BackToLibrary)
            .padding([10, 18])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_4),
    )
    .width(Fill)
    .height(Fill)
    .padding(theme::PAGE_PADDING + theme::SPACE_6)
    .into()
}
