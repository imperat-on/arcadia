//! Configurações: `config:get` + `config:set` para chaves simples.
//!
//! Segredos nunca entram neste view-model: `config:get` já os mascara e o
//! parser de [`SettingsConfig`] simplesmente não os lê.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Border, Color, Element, Fill};

use crate::app::{App, LoadStatus, Message, Section, SettingsTab};
use crate::backend::SettingsConfig;
use crate::i18n::{I18n, Lang};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let state = &app.settings;
    let header = views::page_header_labeled(
        i18n.t("settings.title"),
        i18n.t("native.settings.subtitle"),
        None,
        Some(Message::Refresh(Section::Settings)),
        i18n.t("native.refresh"),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            views::loading_state(i18n.t("common.carregando"))
        }
        LoadStatus::Error(error) => container(views::error_state_labeled(
            i18n.t("native.settings.load_error"),
            error,
            Message::Refresh(Section::Settings),
            i18n.t("downloads.tentar_novamente"),
        ))
        .width(Fill)
        .padding([theme::SPACE_2, theme::PAGE_PADDING])
        .into(),
        LoadStatus::Ready => ready_body(app),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let state = &app.settings;
    let Some(config) = state.config.as_ref() else {
        return views::empty_state(
            i18n.t("native.settings.empty_title"),
            i18n.t("native.settings.empty_desc"),
        );
    };

    let tabs = container(
        row(SettingsTab::ALL
            .iter()
            .map(|tab| tab_button(*tab, state.tab == *tab, i18n))
            .collect::<Vec<_>>())
        .spacing(4),
    )
    .width(Fill)
    .padding([0.0, theme::PAGE_PADDING]);

    let disabled = state.saving.is_some();
    let content = match state.tab {
        SettingsTab::General => general_tab(app, config, disabled),
        SettingsTab::Appearance => appearance_tab(app, config, &state.accent_draft, disabled),
        SettingsTab::Downloads => downloads_tab(app, config, disabled),
        SettingsTab::Playback => playback_tab(app, config, disabled),
        SettingsTab::Integrations => integrations_tab(app, config),
        SettingsTab::About => about_tab(app, config),
    };

    let mut page = column![].spacing(theme::SPACE_3);
    if let Some(saving) = state.saving.as_deref() {
        page = page.push(
            text(i18n.t_args("native.settings.saving", &[("key", saving)]))
                .size(12)
                .color(theme::ACCENT),
        );
    }
    if let Some(error) = state.save_error.as_deref() {
        page = page.push(views::action_error_banner(error, None));
    }
    page = page.push(content);

    let body = scrollable(
        container(page)
            .width(Fill)
            .padding([0.0, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable);

    column![tabs, body].width(Fill).height(Fill).into()
}

fn tab_button(tab: SettingsTab, active: bool, i18n: &I18n) -> Element<'_, Message> {
    button(
        text(tab.label(i18n))
            .size(12)
            .font(if active {
                theme::FONT_MEDIUM
            } else {
                theme::FONT
            })
            .color(if active { theme::TEXT_1 } else { theme::TEXT_2 }),
    )
    .on_press(Message::SettingsTabChanged(tab))
    .padding([7.0, 12.0])
    .style(move |style_theme, status| theme::sidebar_item(style_theme, status, active))
    .into()
}

fn general_tab<'a>(app: &'a App, config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut language = row![].spacing(6);
    for lang in Lang::ALL {
        language = language.push(language_button(lang, i18n.lang(), disabled));
    }

    let behavior = column![
        setting_row(
            i18n.t("settings.check_updates.label"),
            i18n.t("settings.check_updates.desc"),
            toggle(
                "check_updates_on_start",
                config.check_updates_on_start,
                disabled,
                i18n,
            ),
        ),
        setting_row(
            i18n.t("settings.console_mode.label"),
            i18n.t("settings.console_mode.desc"),
            toggle(
                "start_in_console_mode",
                config.start_in_console_mode,
                disabled,
                i18n,
            ),
        ),
        setting_row(
            i18n.t("settings.hide_changelog.label"),
            i18n.t("settings.hide_changelog.desc"),
            toggle(
                "hide_changelog_on_start",
                config.hide_changelog_on_start,
                disabled,
                i18n,
            ),
        ),
        setting_row(
            i18n.t("settings.minimize_on_launch.label"),
            i18n.t("settings.minimize_on_launch.desc"),
            toggle(
                "minimize_on_game_launch",
                config.minimize_on_game_launch,
                disabled,
                i18n,
            ),
        ),
        setting_row(
            i18n.t("settings.disable_playtime.label"),
            i18n.t("settings.disable_playtime.desc"),
            toggle(
                "disable_playtime_tracking",
                config.disable_playtime_tracking,
                disabled,
                i18n,
            ),
        ),
        setting_row(
            i18n.t("settings.discord_rpc.label"),
            i18n.t("settings.discord_rpc.desc"),
            toggle(
                "discord_rich_presence",
                config.discord_rich_presence,
                disabled,
                i18n,
            ),
        ),
    ]
    .spacing(theme::SPACE_2);

    let steam_path = path_text(&config.steam_path, i18n);
    let install_path = path_text(&config.default_install_path, i18n);
    let client_id = if config.discord_client_id.trim().is_empty() {
        i18n.t("common.padrao").to_string()
    } else {
        config.discord_client_id.clone()
    };

    column![
        group(i18n.t("idioma.idioma"), language.into()),
        group(
            i18n.t("settings.paths"),
            column![
                setting_row(
                    i18n.t("settings.install_path.label"),
                    i18n.t("settings.install_path.desc"),
                    value_text(&install_path),
                ),
                setting_row(
                    i18n.t("settings.steam_path.label"),
                    i18n.t("settings.steam_path.desc"),
                    value_text(&steam_path),
                ),
                setting_row(
                    "Discord Client ID",
                    i18n.t("native.settings.discord_client_id_desc"),
                    value_text(&client_id),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(i18n.t("settings.behavior"), behavior.into()),
        note(i18n.t("native.settings.paths_note")),
        note(&i18n.t_args(
            "native.settings.active_note",
            &[
                ("language", config.language_label()),
                ("theme", &config.theme_name),
            ],
        )),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn appearance_tab<'a>(
    app: &'a App,
    config: &'a SettingsConfig,
    accent_draft: &'a str,
    disabled: bool,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let swatch: Element<'a, Message> = parse_hex_color(accent_draft)
        .map(|color| {
            container(Space::new().width(18).height(18))
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(color.into()),
                    border: Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..container::Style::default()
                })
                .into()
        })
        .unwrap_or_else(|| {
            text(i18n.t("native.settings.invalid_color"))
                .size(11)
                .color(theme::WARNING)
                .into()
        });

    let accent = row![
        text_input("#22d3ee", accent_draft)
            .on_input(Message::AccentChanged)
            .padding([8.0, 12.0])
            .size(12)
            .width(170)
            .style(theme::search_input),
        swatch,
        {
            let mut save: iced::widget::Button<'_, Message> =
                button(text(i18n.t("common.salvar")).size(11).font(theme::FONT_MEDIUM))
                    .padding([8.0, 14.0]);
            if !disabled {
                save = save.on_press(Message::AccentSave);
            }
            let rendered: Element<'_, Message> = save.style(theme::primary_button).into();
            rendered
        },
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![
        group(
            i18n.t("native.settings.theme_group"),
            column![setting_row(
                i18n.t("settings.theme.label"),
                i18n.t("settings.theme.desc"),
                value_text("Midnight Mirage (midnight)"),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            i18n.t("native.settings.accent_group"),
            column![
                setting_row(
                    i18n.t("native.settings.accent"),
                    i18n.t("native.settings.accent_desc"),
                    accent.into(),
                ),
                setting_row(
                    i18n.t("accessibility.sempre_titulos"),
                    i18n.t("native.settings.always_titles_desc"),
                    toggle("always_titles", config.always_titles, disabled, i18n),
                ),
                setting_row(
                    i18n.t("accessibility.desativar_animacoes"),
                    i18n.t("native.settings.no_anim_desc"),
                    toggle("no_anim", config.no_anim, disabled, i18n),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn downloads_tab<'a>(app: &'a App, config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let install_path = path_text(&config.default_install_path, i18n);
    let cores = if config.download_cpu_cores == 0 {
        i18n.t("native.settings.auto_max").to_string()
    } else {
        i18n.t_args(
            "native.settings.cores",
            &[("count", &config.download_cpu_cores.to_string())],
        )
    };

    let stepper = row![
        step_button("-", Message::CpuCoresChanged(-1), disabled),
        text(cores).size(12).color(theme::TEXT_1).width(130),
        step_button("+", Message::CpuCoresChanged(1), disabled),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![
        group(
            i18n.t("native.settings.destination"),
            column![setting_row(
                i18n.t("settings.install_path.label"),
                i18n.t("settings.install_path.desc"),
                value_text(&install_path),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            i18n.t("native.settings.performance"),
            column![
                setting_row(
                    i18n.t("settings.download_cores.label"),
                    i18n.t("settings.download_cores.desc"),
                    stepper.into(),
                ),
                note(i18n.t("native.settings.downloads_note")),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn playback_tab<'a>(app: &'a App, config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    let i18n = &app.i18n;
    column![
        group(
            i18n.t("settings.trailers"),
            column![setting_row(
                i18n.t("settings.trailers.auto_tocar"),
                i18n.t("native.settings.trailer_auto_desc"),
                toggle("trailer_auto", config.trailer_auto, disabled, i18n),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            i18n.t("native.settings.notifications_music"),
            column![
                setting_row(
                    i18n.t("native.settings.notifications"),
                    i18n.t("native.settings.notifications_desc"),
                    toggle(
                        "notifications_enabled",
                        config.notifications_enabled,
                        disabled,
                        i18n,
                    ),
                ),
                setting_row(
                    i18n.t("native.settings.music"),
                    i18n.t("native.settings.music_desc"),
                    toggle("music_enabled", config.music_enabled, disabled, i18n),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note(i18n.t("native.settings.playback_note")),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn integrations_tab<'a>(app: &'a App, config: &'a SettingsConfig) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let discord = if config.discord_rich_presence {
        i18n.t("native.settings.discord_on")
    } else {
        i18n.t("native.settings.discord_off")
    };

    column![
        group(
            i18n.t("native.settings.keys_tokens"),
            column![
                note(i18n.t("native.settings.keys_note")),
                note(discord),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            i18n.t("native.settings.accounts"),
            column![
                setting_row(
                    i18n.t("native.settings.store_language"),
                    i18n.t("native.settings.store_language_desc"),
                    value_text(config.language_label()),
                ),
                setting_row(
                    i18n.t("settings.steam_path.label"),
                    i18n.t("settings.steam_path.desc"),
                    value_text(&path_text(&config.steam_path, i18n)),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note(i18n.t("native.settings.integrations_note")),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn about_tab<'a>(app: &'a App, config: &'a SettingsConfig) -> Element<'a, Message> {
    let i18n = &app.i18n;
    column![
        group(
            "Arcadia Native",
            column![
                setting_row(
                    i18n.t("native.settings.version"),
                    i18n.t("native.settings.version_desc"),
                    value_text(env!("CARGO_PKG_VERSION")),
                ),
                setting_row(
                    i18n.t("native.settings.data_dir"),
                    i18n.t("native.settings.data_dir_desc"),
                    value_text(&app.backend.data_dir().display().to_string()),
                ),
                setting_row(
                    i18n.t("settings.theme.label"),
                    i18n.t("native.settings.about_theme_desc"),
                    value_text(&config.theme_name),
                ),
                setting_row(
                    i18n.t("idioma.idioma"),
                    i18n.t("native.settings.language_desc"),
                    value_text(config.language_label()),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note(i18n.t("native.settings.about_note")),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn language_button(lang: Lang, active_lang: Lang, disabled: bool) -> Element<'static, Message> {
    let active = active_lang == lang;
    let mut control: iced::widget::Button<'_, Message> =
        button(text(lang.label()).size(11).font(theme::FONT_MEDIUM)).padding([7.0, 12.0]);
    if !active && !disabled {
        control = control.on_press(Message::LanguageChanged(lang.code().to_string()));
    }
    if active {
        control.style(theme::primary_button).into()
    } else {
        control.style(theme::ghost_button).into()
    }
}

fn toggle<'a>(
    key: &'static str,
    value: bool,
    disabled: bool,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    let label = if value {
        i18n.t("common.ativado")
    } else {
        i18n.t("common.desativado")
    };
    let mut control: iced::widget::Button<'_, Message> =
        button(text(label).size(11).font(theme::FONT_MEDIUM)).padding([6.0, 12.0]);
    if !disabled {
        control = control.on_press(Message::SettingToggled { key, value: !value });
    }
    if value {
        control.style(theme::primary_button).into()
    } else {
        control.style(theme::ghost_button).into()
    }
}

fn step_button<'a>(label: &'static str, message: Message, disabled: bool) -> Element<'a, Message> {
    let mut control: iced::widget::Button<'_, Message> =
        button(text(label).size(14).font(theme::FONT_MEDIUM)).padding([4.0, 12.0]);
    if !disabled {
        control = control.on_press(message);
    }
    control.style(theme::ghost_button).into()
}

fn setting_row<'a>(
    label: &'a str,
    description: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(label).size(13).color(theme::TEXT_1),
                text(description).size(11).color(theme::TEXT_2),
            ]
            .spacing(2)
            .width(Fill),
            control,
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([10.0, 14.0])
    .style(theme::stub_panel)
    .into()
}

fn value_text(value: impl AsRef<str>) -> Element<'static, Message> {
    text(value.as_ref().to_string())
        .size(11)
        .color(theme::TEXT_STRONG)
        .into()
}

fn group<'a>(title: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![views::section_label(title), content]
        .spacing(theme::SPACE_2)
        .width(Fill)
        .into()
}

fn note(message: impl AsRef<str>) -> Element<'static, Message> {
    text(message.as_ref().to_string())
        .size(11)
        .color(theme::TEXT_3)
        .width(Fill)
        .into()
}

fn path_text(value: &str, i18n: &I18n) -> String {
    if value.trim().is_empty() {
        i18n.t("common.padrao").to_string()
    } else {
        value.to_string()
    }
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::from_rgb8(red, green, blue))
}
