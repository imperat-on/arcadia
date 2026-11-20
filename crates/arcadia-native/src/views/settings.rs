//! Configurações: `config:get` + `config:set` para chaves simples.
//!
//! Segredos nunca entram neste view-model: `config:get` já os mascara e o
//! parser de [`SettingsConfig`] simplesmente não os lê.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Border, Color, Element, Fill};

use crate::app::{App, LoadStatus, Message, Section, SettingsTab};
use crate::backend::SettingsConfig;
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.settings;
    let header = views::page_header(
        "Configurações",
        "Preferências de config.json; alterações simples gravam via config:set.",
        None,
        Some(Message::Refresh(Section::Settings)),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            views::loading_state("Carregando configurações...")
        }
        LoadStatus::Error(error) => container(views::error_state(
            "Não foi possível carregar as configurações",
            error,
            Message::Refresh(Section::Settings),
        ))
        .width(Fill)
        .padding([theme::SPACE_2, theme::PAGE_PADDING])
        .into(),
        LoadStatus::Ready => ready_body(app),
    };

    column![header, body].width(Fill).height(Fill).into()
}

fn ready_body(app: &App) -> Element<'_, Message> {
    let state = &app.settings;
    let Some(config) = state.config.as_ref() else {
        return views::empty_state(
            "Configuração vazia",
            "O dispatcher respondeu sem objeto de config; tente atualizar.",
        );
    };

    let tabs = container(
        row(SettingsTab::ALL
            .iter()
            .map(|tab| tab_button(*tab, state.tab == *tab))
            .collect::<Vec<_>>())
        .spacing(4),
    )
    .width(Fill)
    .padding([0.0, theme::PAGE_PADDING]);

    let disabled = state.saving.is_some();
    let content = match state.tab {
        SettingsTab::General => general_tab(config, disabled),
        SettingsTab::Appearance => appearance_tab(config, &state.accent_draft, disabled),
        SettingsTab::Downloads => downloads_tab(config, disabled),
        SettingsTab::Playback => playback_tab(config, disabled),
        SettingsTab::Integrations => integrations_tab(config),
        SettingsTab::About => about_tab(app, config),
    };

    let mut page = column![].spacing(theme::SPACE_3);
    if let Some(saving) = state.saving.as_deref() {
        page = page.push(
            text(format!("Salvando {saving}..."))
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

fn tab_button(tab: SettingsTab, active: bool) -> Element<'static, Message> {
    button(
        text(tab.label())
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

fn general_tab<'a>(config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    let language = row![]
        .spacing(6)
        .push(language_button(
            "pt-BR",
            "Português (Brasil)",
            config,
            disabled,
        ))
        .push(language_button("en-US", "English (US)", config, disabled))
        .push(language_button("es-ES", "Español", config, disabled));

    let behavior = column![
        setting_row(
            "Verificar atualizações ao iniciar",
            "Consulta o canal configurado sem bloquear o boot.",
            toggle(
                "check_updates_on_start",
                config.check_updates_on_start,
                disabled
            ),
        ),
        setting_row(
            "Iniciar em modo console",
            "Abre o shell no modo console quando disponível.",
            toggle(
                "start_in_console_mode",
                config.start_in_console_mode,
                disabled
            ),
        ),
        setting_row(
            "Esconder changelog ao iniciar",
            "Pula a janela de novidades das primeiras execuções.",
            toggle(
                "hide_changelog_on_start",
                config.hide_changelog_on_start,
                disabled,
            ),
        ),
        setting_row(
            "Minimizar ao abrir um jogo",
            "Libera a janela principal durante o jogo.",
            toggle(
                "minimize_on_game_launch",
                config.minimize_on_game_launch,
                disabled,
            ),
        ),
        setting_row(
            "Desativar registro de tempo",
            "Não conta horas de jogo enquanto esta opção estiver ligada.",
            toggle(
                "disable_playtime_tracking",
                config.disable_playtime_tracking,
                disabled,
            ),
        ),
        setting_row(
            "Discord Rich Presence",
            "Publica o jogo aberto no Discord, quando configurado.",
            toggle(
                "discord_rich_presence",
                config.discord_rich_presence,
                disabled,
            ),
        ),
    ]
    .spacing(theme::SPACE_2);

    let steam_path = path_text(&config.steam_path);
    let install_path = path_text(&config.default_install_path);
    let client_id = if config.discord_client_id.trim().is_empty() {
        "Padrão do aplicativo".to_string()
    } else {
        config.discord_client_id.clone()
    };

    column![
        group("Idioma", language.into()),
        group(
            "Caminhos",
            column![
                setting_row(
                    "Pasta de instalação padrão",
                    "Usada por lojas e downloads que não escolhem destino.",
                    value_text(&install_path),
                ),
                setting_row(
                    "Pasta da Steam",
                    "Detectada pelo sistema quando não há override.",
                    value_text(&steam_path),
                ),
                setting_row(
                    "Discord Client ID",
                    "Application ID público; tokens e chaves nunca aparecem aqui.",
                    value_text(&client_id),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group("Comportamento", behavior.into()),
        note(
            "Caminhos e pastas ainda são somente leitura nesta passada; \
             o seletor nativo de diretórios entra numa onda própria.",
        ),
        note(&format!(
            "Idioma ativo: {}. Tema ativo: {}.",
            config.language_label(),
            config.theme_name
        )),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn appearance_tab<'a>(
    config: &'a SettingsConfig,
    accent_draft: &'a str,
    disabled: bool,
) -> Element<'a, Message> {
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
        .unwrap_or_else(|| text("cor inválida").size(11).color(theme::WARNING).into());

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
                button(text("Salvar").size(11).font(theme::FONT_MEDIUM)).padding([8.0, 14.0]);
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
            "Tema",
            column![setting_row(
                "Tema",
                "O shell nativo usa os tokens de theme.rs; este é o mesmo tema da UI web.",
                value_text("Midnight Mirage (midnight)"),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            "Cor de destaque",
            column![
                setting_row(
                    "Accent",
                    "Gravado em config.accent. Aceita hexadecimal (#rrggbb).",
                    accent.into(),
                ),
                setting_row(
                    "Títulos sempre visíveis",
                    "Mantém o título dos cards visível mesmo sem hover.",
                    toggle("always_titles", config.always_titles, disabled),
                ),
                setting_row(
                    "Reduzir animações",
                    "Desliga transições decorativas no cliente web; o nativo já é estático.",
                    toggle("no_anim", config.no_anim, disabled),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn downloads_tab<'a>(config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    let install_path = path_text(&config.default_install_path);
    let cores = if config.download_cpu_cores == 0 {
        "Automático (máximo)".to_string()
    } else {
        format!("{} núcleo(s)", config.download_cpu_cores)
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
            "Destino",
            column![setting_row(
                "Pasta de instalação padrão",
                "Também usada pela fila Epic/Steam.",
                value_text(&install_path),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            "Desempenho",
            column![
                setting_row(
                    "Núcleos de CPU para downloads",
                    "0 usa o máximo disponível; o limite é aplicado pelo serviço de download.",
                    stepper.into(),
                ),
                note(
                    "Limite de banda e downloads simultâneos existem no contrato \
                     (download_limit/max_concurrent_downloads) mas ainda não têm \
                     controle nativo nesta passada.",
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn playback_tab<'a>(config: &'a SettingsConfig, disabled: bool) -> Element<'a, Message> {
    column![
        group(
            "Trailers",
            column![setting_row(
                "Reproduzir trailer automaticamente",
                "Abre o trailer ao visitar a página de um jogo.",
                toggle("trailer_auto", config.trailer_auto, disabled),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            "Notificações e música",
            column![
                setting_row(
                    "Notificações",
                    "Avisos do launcher, downloads e conquistas.",
                    toggle(
                        "notifications_enabled",
                        config.notifications_enabled,
                        disabled,
                    ),
                ),
                setting_row(
                    "Música de fundo",
                    "Trilha do launcher nas telas de navegação.",
                    toggle("music_enabled", config.music_enabled, disabled),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note("Volume, playlist e trailer automático por jogo ainda dependem da UI web."),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn integrations_tab<'a>(config: &'a SettingsConfig) -> Element<'a, Message> {
    let discord = if config.discord_rich_presence {
        "Rich Presence ligado (config.discord_rich_presence)."
    } else {
        "Rich Presence desligado (config.discord_rich_presence)."
    };

    column![
        group(
            "Chaves e tokens",
            column![
                note(
                    "config:get devolve tokens mascarados/omitidos e esta tela não os \
                     lê. Steam, SteamGridDB, Hubcap e serviços debrid continuam sendo \
                     configurados na UI web por enquanto.",
                ),
                note(discord),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            "Contas",
            column![
                setting_row(
                    "Idioma das lojas",
                    "Também define o país usado nas consultas de preço.",
                    value_text(config.language_label()),
                ),
                setting_row(
                    "Pasta da Steam",
                    "Override opcional para a biblioteca Steam detectada.",
                    value_text(&path_text(&config.steam_path)),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note("Login, sync e emuladores ainda não têm tela nativa."),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn about_tab<'a>(app: &'a App, config: &'a SettingsConfig) -> Element<'a, Message> {
    column![
        group(
            "Arcadia Native",
            column![
                setting_row(
                    "Versão",
                    "Crate arcadia-native (shell iced, sem webview).",
                    value_text(env!("CARGO_PKG_VERSION")),
                ),
                setting_row(
                    "Data dir",
                    "Mesmo diretório usado pelo dispatcher e pela UI web.",
                    value_text(&app.backend.data_dir().display().to_string()),
                ),
                setting_row(
                    "Tema",
                    "Tokens visuais portados de ui/src/index.css.",
                    value_text(&config.theme_name),
                ),
                setting_row(
                    "Idioma",
                    "Preferência salva em config.language.",
                    value_text(config.language_label()),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note(
            "Primeira passada das telas de seção: Biblioteca, Downloads, \
             Configurações, Fontes, Amigos e Componentes usam IPC real. \
             Home, Lojas e Big Picture seguem em placeholder.",
        ),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

fn language_button(
    id: &'static str,
    label: &'static str,
    config: &SettingsConfig,
    disabled: bool,
) -> Element<'static, Message> {
    let active = config.language == id;
    let mut control: iced::widget::Button<'_, Message> =
        button(text(label).size(11).font(theme::FONT_MEDIUM)).padding([7.0, 12.0]);
    if !active && !disabled {
        control = control.on_press(Message::LanguageChanged(id.to_string()));
    }
    if active {
        control.style(theme::primary_button).into()
    } else {
        control.style(theme::ghost_button).into()
    }
}

fn toggle<'a>(key: &'static str, value: bool, disabled: bool) -> Element<'a, Message> {
    let label = if value { "Ativado" } else { "Desativado" };
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

fn path_text(value: &str) -> String {
    if value.trim().is_empty() {
        "Padrão do sistema".to_string()
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
