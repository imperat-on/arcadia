//! Telas do shell nativo. Cada tela recebe apenas o que precisa para desenhar.

pub mod cards;
pub mod chrome;
pub mod game;
pub mod home;
pub mod icons;
pub mod library;
pub mod sidebar;

use iced::widget::{button, container, text};
use iced::{Element, Fill};

use crate::app::{Message, Section};
use crate::theme;

/// Placeholder honesto para telas que ainda não foram portadas.
pub fn section_placeholder(section: Section) -> Element<'static, Message> {
    container(
        iced::widget::column![
            text(section.label())
                .size(30)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text("Ainda não portado")
                .size(11)
                .font(theme::FONT_MEDIUM)
                .color(theme::ACCENT),
            text(section.stub_message()).size(14).color(theme::TEXT_2),
            button(
                text("Voltar para a Biblioteca")
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

pub fn missing_game() -> Element<'static, Message> {
    container(
        iced::widget::column![
            text("Jogo não encontrado")
                .size(26)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text("O jogo pode ter saído da biblioteca ou o id não existe mais.")
                .size(14)
                .color(theme::TEXT_2),
            button(
                text("Voltar para a Biblioteca")
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
