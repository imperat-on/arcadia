//! Componentes: lista `plugins:list` e ações reais do registro.
//!
//! Nada aqui baixa plugins: `install` só ativa um arquivo já detectado e
//! `remove` só desliga a flag — o arquivo do usuário é preservado. Erros do
//! dispatcher aparecem no card e no banner, e a lista é recarregada após cada
//! mutação.

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill};

use super::icons::{icon, Icon};
use crate::app::{
    App, ComponentsState, LoadStatus, Message, PluginDetailsPanel, PluginNotice, Section,
};
use crate::backend::{PluginAction, PluginDetails, PluginItem};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.components;
    let i18n = &app.i18n;
    let header = views::page_header(
        i18n.t("sidebar.plugins"),
        i18n.t("native.components.subtitle"),
        Some(i18n.t_args(
            "native.components.count",
            &[("count", &state.plugins.len().to_string())],
        )),
        None,
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Loading | LoadStatus::Error(_) if !state.plugins.is_empty() => {
            ready_body(app, state)
        }
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

/// Campo da chave Hubcap, o mesmo bloco da `StoreSetup.tsx`: senha, Salvar
/// ciano e Validar com borda. Sem essa chave o LuaTools não baixa o
/// manifesto e a injeção na Steam fica sem as chaves de depot.
///
/// Fica em Configurações → Integrações, que é onde a web coloca o
/// `StoreSetup`.
pub(crate) fn hubcap_key_card<'a>(app: &'a App) -> Element<'a, Message> {
    let state = &app.components;
    let draft = if state.hubcap_draft.is_empty() {
        app.settings
            .config
            .as_ref()
            .map(|config| config.hubcap_api_key.as_str())
            .unwrap_or("")
    } else {
        state.hubcap_draft.as_str()
    };
    let i18n = &app.i18n;
    let busy = state.hubcap_saving || state.hubcap_validating;
    let save_label = if state.hubcap_saving {
        i18n.t("store_setup.validando")
    } else {
        i18n.t("common.salvar")
    };
    let validate_label = if state.hubcap_validating {
        i18n.t("store_setup.validando")
    } else {
        i18n.t("store_setup.validar")
    };

    let mut save = button(
        text(save_label)
            .size(12.0)
            .font(theme::FONT_BOLD)
            .color(iced::Color::BLACK),
    )
    .padding([10.0, 16.0])
    .style(theme::hubcap_save_button);
    if !busy {
        save = save.on_press(Message::HubcapKeySave);
    }

    let mut validate = button(
        text(validate_label)
            .size(12.0)
            .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.80)),
    )
    .padding([10.0, 14.0])
    .style(theme::hubcap_validate_button);
    if !busy {
        validate = validate.on_press(Message::HubcapKeyValidate);
    }

    let mut body = column![
        text(i18n.t("store_setup.chave_hubcap"))
            .size(13.0)
            .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.70)),
        row![
            text_input(i18n.t("store_setup.hubcap_placeholder"), draft)
                .on_input(Message::HubcapKeyChanged)
                .secure(true)
                .padding([10.0, 14.0])
                .size(13.0)
                .width(Fill)
                .style(theme::search_input),
            save,
            validate,
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(i18n.t("store_setup.hubcap_dica"))
            .size(11.0)
            .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.35)),
    ]
    .spacing(6)
    .width(Fill);

    if let Some(stats) = state.hubcap_stats.as_ref() {
        if stats.error.is_none() {
            let usage = stats.daily_usage.unwrap_or(0);
            let limit = stats
                .daily_limit
                .map(|limit| limit.to_string())
                .unwrap_or_else(|| "—".to_string());
            let line = match stats.username.as_deref() {
                Some(name) => format!("{name} · {usage}/{limit}"),
                None => format!("{usage}/{limit}"),
            };
            body = body.push(text(line).size(12.0).color(theme::TEXT_2));
        }
    }

    if let Some(test) = state.hubcap_test.as_deref() {
        let key = match test {
            "ok" => "store_setup.teste_ok",
            "invalida" | "chave_invalida" => "store_setup.teste_invalida",
            "sem_chave" => "store_setup.teste_sem_chave",
            _ => "store_setup.teste_indisponivel",
        };
        let color = if test == "ok" {
            theme::SUCCESS
        } else {
            theme::WARNING
        };
        body = body.push(text(i18n.t(key)).size(11.0).color(color));
    } else if let Some(notice) = state.hubcap_notice.as_deref() {
        body = body.push(text(notice).size(12.0).color(theme::TEXT_2));
    }

    container(body)
        .width(Fill)
        .padding(16)
        .style(theme::panel)
        .into()
}

fn ready_body<'a>(app: &'a App, state: &'a ComponentsState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![].spacing(theme::SPACE_3).width(Fill);

    if let Some(banner) = state.banner.as_ref() {
        content = content.push(notice_banner(banner));
    }

    if state.plugins.is_empty() {
        content = content.push(
            container(
                column![
                    text(i18n.t("native.components.empty_title"))
                        .size(theme::TEXT_READ)
                        .font(theme::FONT_MEDIUM)
                        .color(theme::TEXT_STRONG),
                    text(i18n.t("native.components.empty_desc"))
                        .size(theme::TEXT_BODY)
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

    content = content.push(register_panel(app, state));

    content = content.push(
        text(i18n.t("native.components.footer_note"))
            .size(theme::TEXT_CAPTION)
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
        container(
            text(if state.registering {
                i18n.t("native.components.registering")
            } else {
                i18n.t("native.components.register")
            })
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .height(theme::CONTROL_HEIGHT)
    .padding([0.0, theme::SPACE_4])
    .style(theme::component_primary_button);
    if can_register {
        register_button = register_button.on_press(Message::PluginRegisterRequested);
    }

    let field: Element<'a, Message> = text_input(
        i18n.t("native.components.path_placeholder"),
        &state.path_draft,
    )
    .on_input(Message::PluginPathChanged)
    .on_submit(Message::PluginRegisterRequested)
    .padding([theme::SPACE_2, theme::SPACE_3])
    .size(theme::TEXT_BODY)
    .width(Fill)
    .style(theme::search_input)
    .into();
    let form: Element<'a, Message> =
        if app.window_size.width - theme::SIDEBAR_WIDTH < theme::COMPONENT_COMPACT_WIDTH {
            column![field, register_button]
                .spacing(theme::SPACE_2)
                .into()
        } else {
            row![field, register_button]
                .spacing(theme::SPACE_2)
                .align_y(iced::Center)
                .into()
        };
    container(
        column![
            text(i18n.t("native.components.add_title"))
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
            text(i18n.t("native.components.add_desc"))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3),
            form,
        ]
        .spacing(theme::SPACE_2)
        .width(Fill),
    )
    .width(Fill)
    .padding(theme::SPACE_4)
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
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }

    let (name, description) = match plugin.id.as_str() {
        "slssteam" => (
            i18n.t("plugins.slssteam_nome"),
            i18n.t("plugins.slssteam_desc"),
        ),
        "luatools-fixes" => (
            i18n.t("plugins.luatools_nome"),
            i18n.t("plugins.luatools_desc"),
        ),
        _ => (plugin.name.as_str(), plugin.description.as_str()),
    };
    let identity = column![
        text(name)
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_1),
        text(plugin.id.as_str())
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_1)
    .width(Fill);
    let header = row![
        container(icon(
            if plugin.id == "slssteam" {
                Icon::Grid
            } else {
                Icon::Plugin
            },
            theme::SPACE_5,
            0.85
        ))
        .padding(theme::SPACE_3)
        .style(theme::badge),
        identity,
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);
    let heading: Element<'a, Message> =
        if app.window_size.width - theme::SIDEBAR_WIDTH < theme::COMPONENT_COMPACT_WIDTH {
            column![header, statuses].spacing(theme::SPACE_2).into()
        } else {
            row![header, statuses]
                .spacing(theme::SPACE_3)
                .align_y(iced::Center)
                .into()
        };
    let mut content = column![
        heading,
        text(description)
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_2)
            .width(Fill),
    ]
    .spacing(theme::SPACE_3)
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
            can_act && plugin.installed,
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
        if details_open {
            i18n.t("common.fechar")
        } else {
            i18n.t("gameoverview.detalhes")
        },
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
    content = content.push(actions.wrap());

    if let Some(notice) = state.notices.get(&plugin.id) {
        content = content.push(
            text(notice.message.as_str())
                .size(theme::TEXT_CAPTION)
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
        .padding(theme::SPACE_4)
        .style(move |theme| theme::component_panel(theme, plugin.enabled))
        .into()
}

fn details_panel<'a>(app: &'a App, panel: &'a PluginDetailsPanel) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let body: Element<'a, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => text(i18n.t("native.components.details_loading"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_3)
            .into(),
        LoadStatus::Error(error) => text(error.as_str())
            .size(theme::TEXT_CAPTION)
            .color(theme::DANGER)
            .width(Fill)
            .into(),
        LoadStatus::Ready => match panel.details.as_ref() {
            Some(details) => details_body(app, details),
            None => text(i18n.t("native.components.no_details"))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_3)
                .into(),
        },
    };

    container(
        column![
            text(i18n.t("gameoverview.detalhes"))
                .size(theme::TEXT_CAPTION)
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
                    if details.source == "builtin" {
                        i18n.t("native.components.metadata_valid")
                    } else {
                        i18n.t("native.components.valid")
                    }
                } else {
                    i18n.t("native.components.invalid")
                },
                if details.valid {
                    theme::SUCCESS
                } else {
                    theme::DANGER
                }
            ),
            badge(
                if details.installed {
                    i18n.t("native.components.installed")
                } else {
                    i18n.t("native.components.not_detected")
                },
                if details.installed {
                    theme::SUCCESS
                } else {
                    theme::WARNING
                }
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
        .size(theme::TEXT_MICRO)
        .color(theme::TEXT_3),
        text(details.description.as_str())
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
    ]
    .spacing(5)
    .width(Fill);

    if let Some(version) = details.version.as_deref() {
        content = content.push(
            text(i18n.t_args("native.components.version", &[("version", version)]))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    if !details.permissions.is_empty() {
        content = content.push(
            text(i18n.t_args(
                "native.components.permissions",
                &[("list", &details.permissions.join(", "))],
            ))
            .size(theme::TEXT_MICRO)
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
            .size(theme::TEXT_MICRO)
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
    let mut control = button(
        container(
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM),
        )
        .height(Fill)
        .align_y(iced::Center),
    )
    .height(theme::CONTROL_HEIGHT)
    .padding([0.0, theme::SPACE_3])
    .style(if primary {
        theme::component_primary_button
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
            .size(theme::TEXT_BODY)
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
    container(
        text(label)
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_MEDIUM)
            .color(color),
    )
    .padding([3.0, 8.0])
    .style(theme::badge)
    .into()
}
