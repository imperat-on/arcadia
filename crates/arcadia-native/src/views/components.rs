//! Componentes: `plugins:list` (builtins e plugins registrados).
//!
//! Ativar/desativar fica para a próxima onda; aqui o estado é somente leitura,
//! sem esconder plugins desativados ou não detectados.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Fill};

use crate::app::{App, LoadStatus, Message, Section};
use crate::backend::PluginItem;
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.components;
    let header = views::page_header(
        "Componentes",
        "Plugins detectados pelo Arcadia; nada é baixado por esta tela.",
        Some(format!("{} componente(s)", state.plugins.len())),
        Some(Message::Refresh(Section::Components)),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => views::loading_state("Carregando componentes..."),
        LoadStatus::Error(error) => container(views::error_state(
            "Não foi possível carregar os componentes",
            error,
            Message::Refresh(Section::Components),
        ))
        .width(Fill)
        .padding([theme::SPACE_2, theme::PAGE_PADDING])
        .into(),
        LoadStatus::Ready if state.plugins.is_empty() => views::empty_state(
            "Nenhum componente registrado",
            "O registro de plugins respondeu vazio. Componentes opcionais aparecem \
             aqui quando detectados pelo Arcadia.",
        ),
        LoadStatus::Ready => ready_body(state),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body(state: &crate::app::ComponentsState) -> Element<'_, Message> {
    let mut content = column![text(
        "Componentes opcionais que você já tem no sistema. O Arcadia apenas \
             detecta e carrega — não baixa nem hospeda nada."
    )
    .size(12)
    .color(theme::TEXT_2)
    .width(Fill),]
    .spacing(theme::SPACE_3)
    .width(Fill);

    for plugin in &state.plugins {
        content = content.push(plugin_card(plugin));
    }

    content = content.push(
        text("Ativar/desativar e informar caminho manual entram na próxima onda.")
            .size(11)
            .color(theme::TEXT_3),
    );

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn plugin_card(plugin: &PluginItem) -> Element<'_, Message> {
    let status = if plugin.enabled {
        badge("Ativo", theme::ACCENT)
    } else {
        badge("Desativado", theme::TEXT_3)
    };
    let detection = if plugin.installed {
        badge("Arquivo encontrado", theme::SUCCESS)
    } else {
        badge("Arquivo não encontrado", theme::WARNING)
    };

    let mut statuses = row![status, detection].spacing(6).align_y(iced::Center);
    if plugin.enabled {
        statuses = statuses.push(
            text("carregado no próximo boot")
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    container(
        row![column![
            row![
                text(plugin.name.as_str())
                    .size(15)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text(format!("· {}", plugin.id))
                    .size(10)
                    .color(theme::TEXT_3),
            ]
            .spacing(6)
            .align_y(iced::Center),
            text(plugin.description.as_str())
                .size(12)
                .color(theme::TEXT_2),
            statuses,
        ]
        .spacing(5)
        .width(Fill),]
        .width(Fill),
    )
    .width(Fill)
    .padding(14)
    .style(theme::stub_panel)
    .into()
}

fn badge<'a>(label: &'a str, color: iced::Color) -> Element<'a, Message> {
    container(text(label).size(10).font(theme::FONT_MEDIUM).color(color))
        .padding([3.0, 8.0])
        .style(theme::badge)
        .into()
}
