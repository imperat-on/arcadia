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
    let i18n = &app.i18n;
    let header = views::page_header_labeled(
        i18n.t("sidebar.plugins"),
        i18n.t("native.components.subtitle"),
        Some(i18n.t_args(
            "native.components.count",
            &[("count", &state.plugins.len().to_string())],
        )),
        Some(Message::Refresh(Section::Components)),
        i18n.t("native.refresh"),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            views::loading_state(i18n.t("native.components.loading"))
        }
        LoadStatus::Error(error) => container(views::error_state_labeled(
            i18n.t("native.components.load_error"),
            error,
            Message::Refresh(Section::Components),
            i18n.t("downloads.tentar_novamente"),
        ))
        .width(Fill)
        .padding([theme::SPACE_2, theme::PAGE_PADDING])
        .into(),
        LoadStatus::Ready => ready_body(app, state),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body<'a>(app: &'a App, state: &'a ComponentsState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![
        text(i18n.t("native.components.intro"))
            .size(12)
            .color(theme::TEXT_2)
            .width(Fill),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    if let Some(banner) = state.banner.as_ref() {
        content = content.push(notice_banner(banner));
    }

    content = content.push(register_panel(app, state));

    if state.plugins.is_empty() {
        content = content.push(
            container(
                column![
                    text(i18n.t("native.components.empty_title"))
                        .size(14)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_STRONG),
                    text(i18n.t("native.components.empty_desc"))
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
            content = content.push(plugin_card(app, state, plugin));
        }
    }

    content = content.push(
        text(i18n.t("native.components.footer_note"))
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

fn register_panel<'a>(app: &'a App, state: &'a ComponentsState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let can_register =
        !state.registering && state.busy.is_none() && !state.path_draft.trim().is_empty();
    let mut register_button = button(
        text(if state.registering {
            i18n.t("native.components.registering")
        } else {
            i18n.t("native.components.register")
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
            text(i18n.t("native.components.add_title"))
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
            text(i18n.t("native.components.add_desc"))
                .size(11)
                .color(theme::TEXT_3),
            row![
                text_input(
                    i18n.t("native.components.path_placeholder"),
                    &state.path_draft
                )
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

fn plugin_card<'a>(
    app: &'a App,
    state: &'a ComponentsState,
    plugin: &'a PluginItem,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let busy = state.busy.as_deref() == Some(plugin.id.as_str());
    let can_act = state.busy.is_none() && !state.registering;
    let details_open = state
        .details
        .as_ref()
        .is_some_and(|details| details.id == plugin.id);

    let mut statuses = row![
        if plugin.enabled {
            badge(i18n.t("common.ativo"), theme::ACCENT)
        } else {
            badge(i18n.t("common.desativado"), theme::TEXT_3)
        },
        if plugin.installed {
            badge(i18n.t("native.components.file_found"), theme::SUCCESS)
        } else {
            badge(i18n.t("native.components.file_missing"), theme::WARNING)
        },
    ]
    .spacing(6)
    .align_y(iced::Center);
    if busy {
        statuses = statuses.push(
            text(i18n.t("native.components.busy"))
                .size(10)
                .color(theme::TEXT_3),
        );
    } else if plugin.enabled {
        statuses = statuses.push(
            text(i18n.t("native.components.load_next_boot"))
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
            i18n.t("native.components.disable"),
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Disable,
            },
            can_act,
            false,
        ));
        actions = actions.push(action_button(
            i18n.t("common.remover"),
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Remove,
            },
            can_act,
            false,
        ));
    } else {
        actions = actions.push(action_button(
            i18n.t("native.components.enable"),
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Enable,
            },
            can_act,
            true,
        ));
        actions = actions.push(action_button(
            i18n.t("native.components.install"),
            Message::PluginActionRequested {
                id: plugin.id.clone(),
                action: PluginAction::Install,
            },
            can_act,
            false,
        ));
    }
    actions = actions.push(action_button(
        i18n.t("native.components.verify"),
        Message::PluginVerifyRequested(plugin.id.clone()),
        can_act,
        false,
    ));
    actions = actions.push(action_button(
        if details_open { i18n.t("common.fechar") } else { i18n.t("gameoverview.detalhes") },
        if details_open {
            Message::PluginDetailsClose
        } else {
            Message::PluginDetailsRequested(plugin.id.clone())
        },
        !busy,
        false,
    ));
    actions = actions.push(action_button(
        i18n.t("native.components.unregister"),
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
            content = content.push(details_panel(app, details));
        }
    }

    container(content)
        .width(Fill)
        .padding(14)
        .style(theme::stub_panel)
        .into()
}

fn details_panel<'a>(app: &'a App, panel: &'a PluginDetailsPanel) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let body: Element<'a, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => text(i18n.t("native.components.details_loading"))
            .size(11)
            .color(theme::TEXT_3)
            .into(),
        LoadStatus::Error(error) => text(error.as_str())
            .size(11)
            .color(theme::DANGER)
            .width(Fill)
            .into(),
        LoadStatus::Ready => match panel.details.as_ref() {
            Some(details) => details_body(app, details),
            None => text(i18n.t("native.components.no_details"))
                .size(11)
                .color(theme::TEXT_3)
                .into(),
        },
    };

    container(
        column![
            text(i18n.t("gameoverview.detalhes"))
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

fn details_body<'a>(app: &'a App, details: &'a PluginDetails) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![
        row![
            badge(
                if details.valid {
                    i18n.t("native.components.valid")
                } else {
                    i18n.t("native.components.invalid")
                },
                if details.valid { theme::SUCCESS } else { theme::DANGER }
            ),
            badge(
                if details.installed {
                    i18n.t("native.components.installed")
                } else {
                    i18n.t("native.components.not_detected")
                },
                if details.installed { theme::SUCCESS } else { theme::WARNING }
            ),
            badge(
                if details.enabled {
                    i18n.t("common.ativo")
                } else {
                    i18n.t("common.desativado")
                },
                theme::TEXT_2
            ),
        ]
        .spacing(6),
        text(format!(
            "{} · {} · {}",
            details.id,
            if details.source.is_empty() {
                i18n.t("native.components.unknown_source")
            } else {
                details.source.as_str()
            },
            if details.signed {
                i18n.t("native.components.signed")
            } else {
                i18n.t("native.components.unsigned")
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
        content = content.push(
            text(i18n.t_args("native.components.version", &[("version", version)]))
                .size(10)
                .color(theme::TEXT_3),
        );
    }
    if !details.permissions.is_empty() {
        content = content.push(
            text(i18n.t_args(
                "native.components.permissions",
                &[("list", &details.permissions.join(", "))],
            ))
            .size(10)
                .color(theme::TEXT_3)
                .width(Fill),
        );
    }
    if !details.error.is_empty() {
        content = content.push(
            text(i18n.t_args(
                "native.components.registry_error",
                &[("error", &details.error)],
            ))
            .size(10)
                .color(theme::DANGER)
                .width(Fill),
        );
    }

    content.into()
}

fn action_button<'a>(
    label: &'a str,
    message: Message,
    enabled: bool,
    primary: bool,
) -> Element<'a, Message> {
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
