//! Componentes: lista `plugins:list` e ações reais do registro.
//!
//! Nada aqui baixa plugins: `install` só ativa um arquivo já detectado e
//! `remove` só desliga a flag — o arquivo do usuário é preservado. Erros do
//! dispatcher aparecem no card e no banner, e a lista é recarregada após cada
//! mutação.

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill};

use crate::app::{
    App, ComponentsState, LoadStatus, Message, PluginDetailsPanel, PluginNotice, Section,
};
use crate::backend::{PluginAction, PluginDetails, PluginItem};
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
        LoadStatus::Ready => ready_body(state),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body(state: &ComponentsState) -> Element<'_, Message> {
    let mut content = column![
        text(
            "Componentes opcionais que você já tem no sistema. O Arcadia apenas \
             detecta e carrega — não baixa nem hospeda nada."
        )
        .size(12)
        .color(theme::TEXT_2)
        .width(Fill),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    if let Some(banner) = state.banner.as_ref() {
        content = content.push(notice_banner(banner));
    }

    content = content.push(register_panel(state));

    if state.plugins.is_empty() {
        content = content.push(
            container(
                column![
                    text("Nenhum componente registrado")
                        .size(14)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_STRONG),
                    text(
                        "O registro respondeu vazio. Componentes opcionais aparecem aqui \
                         quando detectados pelo Arcadia."
                    )
                    .size(12)
                    .color(theme::TEXT_2),
                ]
                .spacing(4),
            )
            .width(Fill)
            .padding(14)
            .style(theme::empty_panel),
        );
    } else {
        for plugin in &state.plugins {
            content = content.push(plugin_card(state, plugin));
        }
    }

    content = content.push(
        text(
            "Instalar só ativa um componente já presente no sistema e falha com \
             “não detectado” quando o arquivo não existe. Remover apenas desativa a \
             flag; o arquivo do usuário nunca é apagado.",
        )
        .size(11)
        .color(theme::TEXT_3)
        .width(Fill),
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

fn register_panel(state: &ComponentsState) -> Element<'_, Message> {
    let can_register =
        !state.registering && state.busy.is_none() && !state.path_draft.trim().is_empty();
    let mut register_button = button(
        text(if state.registering {
            "Registrando..."
        } else {
            "Registrar componente"
        })
        .size(12)
        .font(theme::FONT_MEDIUM),
    )
    .padding([10.0, 16.0])
    .style(theme::primary_button);
    if can_register {
        register_button = register_button.on_press(Message::PluginRegisterRequested);
    }

    container(
        column![
            text("Adicionar componente manual")
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
            text(
                "Informe a pasta do componente ou o caminho do manifesto. O registro \
                 lê os metadados locais; nada é baixado."
            )
            .size(11)
            .color(theme::TEXT_3),
            row![
                text_input("Caminho da pasta ou do manifesto", &state.path_draft)
                    .on_input(Message::PluginPathChanged)
                    .on_submit(Message::PluginRegisterRequested)
                    .padding([10.0, 14.0])
                    .size(12)
                    .width(Fill)
                    .style(theme::search_input),
                register_button,
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill),
    )
    .width(Fill)
    .padding(14)
    .style(theme::empty_panel)
    .into()
}

fn plugin_card<'a>(state: &'a ComponentsState, plugin: &'a PluginItem) -> Element<'a, Message> {
    let busy = state.busy.as_deref() == Some(plugin.id.as_str());
    let can_act = state.busy.is_none() && !state.registering;
    let details_open = state
        .details
        .as_ref()
        .is_some_and(|details| details.id == plugin.id);

    let mut statuses = row![
        if plugin.enabled {
            badge("Ativo", theme::ACCENT)
        } else {
            badge("Desativado", theme::TEXT_3)
        },
        if plugin.installed {
            badge("Arquivo encontrado", theme::SUCCESS)
        } else {
            badge("Arquivo não encontrado", theme::WARNING)
        },
    ]
    .spacing(6)
    .align_y(iced::Center);
    if busy {
        statuses = statuses.push(text("ação em andamento...").size(10).color(theme::TEXT_3));
    } else if plugin.enabled {
        statuses = statuses.push(
            text("carregado no próximo boot")
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    let mut content = column![
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
            .color(theme::TEXT_2)
            .width(Fill),
        statuses,
    ]
    .spacing(6)
    .width(Fill);

    let mut actions = row![].spacing(8).align_y(iced::Center);
    if plugin.enabled {
        actions = actions.push(action_button(
            "Desativar",
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Disable,
            },
            can_act,
            false,
        ));
        actions = actions.push(action_button(
            "Remover",
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Remove,
            },
            can_act,
            false,
        ));
    } else {
        actions = actions.push(action_button(
            "Ativar",
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Enable,
            },
            can_act,
            true,
        ));
        actions = actions.push(action_button(
            "Instalar",
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Install,
            },
            can_act,
            false,
        ));
    }
    actions = actions.push(action_button(
        "Verificar",
        Message::PluginVerifyRequested(plugin.id.clone()),
        can_act,
        false,
    ));
    actions = actions.push(action_button(
        if details_open { "Fechar" } else { "Detalhes" },
        if details_open {
            Message::PluginDetailsClose
        } else {
            Message::PluginDetailsRequested(plugin.id.clone())
        },
        !busy,
        false,
    ));
    actions = actions.push(action_button(
        "Desregistrar",
        Message::PluginActionRequested {
            id: plugin.id.clone(),
            action: PluginAction::Unregister,
        },
        can_act,
        false,
    ));
    content = content.push(actions);

    if let Some(notice) = state.notices.get(&plugin.id) {
        content = content.push(
            text(notice.message.as_str())
                .size(11)
                .color(if notice.ok {
                    theme::SUCCESS
                } else {
                    theme::DANGER
                })
                .width(Fill),
        );
    }

    if details_open {
        if let Some(details) = state.details.as_ref() {
            content = content.push(details_panel(details));
        }
    }

    container(content)
        .width(Fill)
        .padding(14)
        .style(theme::stub_panel)
        .into()
}

fn details_panel(panel: &PluginDetailsPanel) -> Element<'_, Message> {
    let body: Element<'_, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => text("Carregando detalhes...")
            .size(11)
            .color(theme::TEXT_3)
            .into(),
        LoadStatus::Error(error) => text(error.as_str())
            .size(11)
            .color(theme::DANGER)
            .width(Fill)
            .into(),
        LoadStatus::Ready => match panel.details.as_ref() {
            Some(details) => details_body(details),
            None => text("Sem detalhes para este componente.")
                .size(11)
                .color(theme::TEXT_3)
                .into(),
        },
    };

    container(
        column![
            text("Detalhes")
                .size(11)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_3),
            body,
        ]
        .spacing(6)
        .width(Fill),
    )
    .width(Fill)
    .padding(10)
    .style(theme::empty_panel)
    .into()
}

fn details_body(details: &PluginDetails) -> Element<'_, Message> {
    let mut content = column![
        row![
            badge(if details.valid { "Válido" } else { "Inválido" },
                if details.valid { theme::SUCCESS } else { theme::DANGER }),
            badge(if details.installed { "Instalado" } else { "Não detectado" },
                if details.installed { theme::SUCCESS } else { theme::WARNING }),
            badge(if details.enabled { "Ativo" } else { "Desativado" }, theme::TEXT_2),
        ]
        .spacing(6),
        text(format!(
            "{} · {} · {}",
            details.id,
            if details.source.is_empty() {
                "origem desconhecida"
            } else {
                details.source.as_str()
            },
            if details.signed {
                "assinado"
            } else {
                "sem assinatura"
            }
        ))
        .size(10)
        .color(theme::TEXT_3),
        text(details.description.as_str())
            .size(11)
            .color(theme::TEXT_2)
            .width(Fill),
    ]
    .spacing(5)
    .width(Fill);

    if let Some(version) = details.version.as_deref() {
        content = content.push(text(format!("Versão: {version}")).size(10).color(theme::TEXT_3));
    }
    if !details.permissions.is_empty() {
        content = content.push(
            text(format!("Permissões: {}", details.permissions.join(", ")))
                .size(10)
                .color(theme::TEXT_3)
                .width(Fill),
        );
    }
    if !details.error.is_empty() {
        content = content.push(
            text(format!("Erro do registro: {}", details.error))
                .size(10)
                .color(theme::DANGER)
                .width(Fill),
        );
    }

    content.into()
}

fn action_button(
    label: &'static str,
    message: Message,
    enabled: bool,
    primary: bool,
) -> Element<'static, Message> {
    let mut control = button(text(label).size(11).font(theme::FONT_MEDIUM))
        .padding([6.0, 12.0])
        .style(if primary {
            theme::primary_button
        } else {
            theme::ghost_button
        });
    if enabled {
        control = control.on_press(message);
    }
    control.into()
}

fn notice_banner(notice: &PluginNotice) -> Element<'_, Message> {
    let style: fn(&iced::Theme) -> iced::widget::container::Style = if notice.ok {
        theme::stub_panel
    } else {
        theme::error_panel
    };
    container(
        text(notice.message.as_str())
            .size(12)
            .color(if notice.ok {
                theme::SUCCESS
            } else {
                theme::DANGER
            })
            .width(Fill),
    )
    .width(Fill)
    .padding([10.0, 14.0])
    .style(style)
    .into()
}

fn badge<'a>(label: &'a str, color: iced::Color) -> Element<'a, Message> {
    container(text(label).size(10).font(theme::FONT_MEDIUM).color(color))
        .padding([3.0, 8.0])
        .style(theme::badge)
        .into()
}
