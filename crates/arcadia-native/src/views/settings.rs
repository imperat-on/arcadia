//! Configurações: `config:get` + `config:set` para chaves simples.
//!
//! Segredos chegam mascarados: `config:get` nunca devolve a chave em claro.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Border, Color, Element, Fill, Padding};

#[path = "settings_expand.rs"]
mod settings_expand;

use crate::app::{App, DebridProvider, LoadStatus, Message, Section, SettingsTab};
use crate::backend::SettingsConfig;
use crate::i18n::{I18n, Lang};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let state = &app.settings;
    // Sem botão "Atualizar": as configurações são lidas no boot e regravadas a
    // cada mudança, então o botão só recarregava o que já estava na tela. O
    // caminho de erro continua com "Tentar novamente", que é onde ele serve.
    let header = views::page_header_labeled(
        i18n.t("settings.title"),
        i18n.t("native.settings.subtitle"),
        None,
        None,
        "",
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => views::loading_state(i18n.t("common.carregando")),
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

    let disabled = state.saving.is_some();
    // A sub-navegação vive na sidebar (como o `Sidebar.tsx`): aqui só a seção
    // ativa é desenhada, sem strip de abas no topo.
    let content = match state.tab {
        SettingsTab::General => general_tab(app, config, disabled),
        SettingsTab::Integrations => integrations_tab(app, config),
        SettingsTab::Metadata => metadata_tab(app),
        SettingsTab::Accessibility => accessibility_tab(app, config, &state.accent_draft, disabled),
        SettingsTab::Controller => controller_tab(app),
        SettingsTab::Emulation => views::emulation::view(app),
        SettingsTab::About => about_tab(app, config),
    };

    let mut page = column![].spacing(theme::SPACE_3);
    if let Some(saving) = state.saving.as_deref() {
        page = page.push(
            text(i18n.t_args("native.settings.saving", &[("key", saving)]))
                .size(theme::TEXT_BODY)
                .color(theme::ACCENT),
        );
    }
    if let Some(error) = state.save_error.as_deref() {
        page = page.push(views::action_error_banner(error, None));
    }
    page = page.push(content);

    scrollable(
        container(page)
            .width(Fill)
            .padding([0.0, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn general_tab<'a>(
    app: &'a App,
    config: &'a SettingsConfig,
    disabled: bool,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    // A web usa um `<select>`; o `pick_list` é o equivalente nativo. Os
    // botões lado a lado que havia aqui ocupavam a linha inteira e não
    // pareciam um seletor.
    let current_lang = i18n.lang();
    let mut language = iced::widget::pick_list(Lang::ALL, Some(current_lang), |lang| {
        Message::LanguageChanged(lang.code().to_string())
    })
    .padding(Padding {
        top: 9.0,
        right: 14.0,
        bottom: 9.0,
        left: 14.0,
    })
    .text_size(theme::TEXT_BODY)
    .width(iced::Length::Fixed(260.0))
    .style(theme::pick_list)
    .menu_style(theme::pick_list_menu);
    // `disabled` só desabilita a seleção: o `pick_list` nao tem estado
    // disabled, e trocar o handler injetaria um no-op global que confundiria
    // outros controles.
    if disabled {
        language = language.width(iced::Length::Fixed(260.0));
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

    // Caminhos resolvidos: quando a config está vazia o campo mostrava
    // "Padrão", que não diz onde a pasta fica. `DefaultPaths` usa os mesmos
    // resolvedores do resto do app, então o que aparece aqui é para onde os
    // downloads realmente vão.
    let defaults = crate::backend::DefaultPaths::resolve(config);
    let steam_path = defaults.steam;
    let install_path = defaults.install;
    let prefix_path = defaults.wine_prefix;
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
                    path_field(&install_path),
                ),
                setting_row(
                    i18n.t("settings.prefix_path.label"),
                    i18n.t("settings.prefix_path.desc"),
                    path_field(&prefix_path),
                ),
                setting_row(
                    i18n.t("settings.steam_path.label"),
                    i18n.t("settings.steam_path.desc"),
                    path_field(&steam_path),
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

/// Acessibilidade: aparência e movimento. Reúne o que a web separa em
/// `AccessibilityView` mais o acento do tema, que nesta shell não tem aba
/// própria.
fn accessibility_tab<'a>(
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
                .size(theme::TEXT_CAPTION)
                .color(theme::WARNING)
                .into()
        });

    let accent = row![
        text_input("#22d3ee", accent_draft)
            .on_input(Message::AccentChanged)
            .padding([8.0, 12.0])
            .size(theme::TEXT_BODY)
            .width(170)
            .style(theme::search_input),
        swatch,
        {
            let mut save: iced::widget::Button<'_, Message> = button(
                text(i18n.t("common.salvar"))
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM),
            )
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
            i18n.t("native.settings.accent_group"),
            column![setting_row(
                i18n.t("native.settings.accent"),
                i18n.t("native.settings.accent_desc"),
                accent.into(),
            )]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        group(
            i18n.t("accessibility.aparencia"),
            column![
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

/// Metadados: o que a `MetadataSection` da web baixa por jogo. Os itens são
/// declarativos; a coleta real continua por conta do dispatcher.
/// Aba Metadados — porte das seções do `SettingsView.tsx`: SteamGridDB,
/// Trailers, cookies do YouTube e IGDB com a lista do que é coletado.
fn metadata_tab<'a>(app: &'a App) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let config = app.settings.config.as_ref();

    let mut content = column![].spacing(theme::SPACE_5).width(Fill);

    // ── SteamGridDB ─────────────────────────────────────────────────────
    let key_saved = config.is_some_and(|config| !config.steamgriddb_api_key.trim().is_empty());
    let key_label = if app.settings.steamgriddb_saved {
        i18n.t("common.salvo")
    } else {
        i18n.t("settings.steamgriddb.salvar")
    };
    let sgdb = column![
        text(i18n.t("settings.steamgriddb"))
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(i18n.t("settings.steamgriddb.desc"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
        column![
            iced::widget::text_input(
                i18n.t("settings.steamgriddb.placeholder"),
                &app.settings.steamgriddb_draft
            )
            .on_input(Message::SteamGridDbKeyChanged)
            .padding(Padding {
                top: 9.0,
                right: 12.0,
                bottom: 9.0,
                left: 12.0,
            })
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::search_input),
            button(
                text(key_label)
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_SEMIBOLD)
            )
            .on_press(Message::SteamGridDbKeySave)
            .padding([9.0, 20.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);
    content = content.push(card(sgdb.into()));

    // ── Trailers ────────────────────────────────────────────────────────
    let auto = config.is_some_and(|config| config.trailer_auto);
    let mut trailers = column![
        text(i18n.t("settings.trailers"))
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(i18n.t("settings.trailers.desc"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
        row![
            text(i18n.t("settings.trailers.auto_tocar"))
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_1)
                .width(Fill),
            toggle("trailer_auto", auto, false, i18n),
        ]
        .align_y(iced::Center)
        .width(Fill),
        button(
            text(i18n.t("settings.trailers.baixar_todos"))
                .size(theme::TEXT_BODY)
                .font(theme::FONT_SEMIBOLD)
        )
        .on_press(Message::TrailerDownloadAll)
        .padding([9.0, 20.0])
        .style(theme::primary_button),
        note(i18n.t("settings.trailers.hint")),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    if let Some(status) = app.settings.trailer_download_all.as_ref() {
        trailers = trailers.push(
            text(status.as_str())
                .size(theme::TEXT_CAPTION)
                .color(theme::SUCCESS),
        );
    }
    content = content.push(card(trailers.into()));

    // ── Cookies do YouTube ──────────────────────────────────────────────
    let cookies_saved = config.is_some_and(|config| !config.youtube_cookies.trim().is_empty());
    let cookies_status = if cookies_saved {
        i18n.t("common.ativado")
    } else {
        i18n.t("native.settings.cookies_not_used")
    };
    let cookies_label = if app.settings.youtube_cookies_saved {
        i18n.t("common.salvo")
    } else {
        i18n.t("settings.salvar")
    };

    let steps = column![
        text(i18n.t("settings.trailers.cookies_instrucoes"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
        note(i18n.t("settings.trailers.cookies_passo1")),
        note(i18n.t("settings.trailers.cookies_passo2")),
        note(i18n.t("settings.trailers.cookies_passo3")),
    ]
    .spacing(theme::SPACE_2)
    .width(Fill);

    let cookies = column![
        row![
            text(i18n.t("settings.trailers.cookies_label"))
                .size(theme::TEXT_LABEL)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1)
                .width(Fill),
            pill(cookies_status, cookies_saved),
        ]
        .align_y(iced::Center)
        .width(Fill),
        text(i18n.t("settings.trailers.cookies_desc"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
        container(steps)
            .width(Fill)
            .padding([12.0, 14.0])
            .style(theme::stub_panel),
        column![
            iced::widget::text_input(
                i18n.t("settings.trailers.escolher_arquivo"),
                &app.settings.youtube_cookies_draft
            )
            .on_input(Message::YoutubeCookiesChanged)
            .padding(Padding {
                top: 9.0,
                right: 12.0,
                bottom: 9.0,
                left: 12.0,
            })
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::search_input),
            button(
                text(cookies_label)
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_SEMIBOLD)
            )
            .on_press(Message::YoutubeCookiesSave)
            .padding([9.0, 20.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_2)
        .width(Fill),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);
    content = content.push(card(cookies.into()));

    // ── IGDB + o que é coletado ─────────────────────────────────────────
    let items = [
        "settings.metadados.item_capa_vertical",
        "settings.metadados.item_banner_hero",
        "settings.metadados.item_logo",
        "settings.metadados.item_descricao_curta",
        "settings.metadados.item_nota_metacritic",
        "settings.metadados.item_genero_ano",
        "settings.metadados.item_trailers",
    ];

    let mut list = column![].spacing(theme::SPACE_2).width(Fill);
    for key in items {
        list = list.push(
            row![
                text(i18n.t(key))
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_1)
                    .width(Fill),
                text(i18n.t("common.ativado"))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::SUCCESS),
            ]
            .align_y(iced::Center)
            .width(Fill),
        );
    }

    let igdb = column![
        row![
            text(i18n.t("settings.igdb"))
                .size(theme::TEXT_LABEL)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1)
                .width(Fill),
            pill(i18n.t("native.settings.no_key_needed"), true),
        ]
        .align_y(iced::Center)
        .width(Fill),
        text(i18n.t("settings.igdb.desc"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
        text(i18n.t("settings.igdb.o_que_buscado"))
            .size(theme::TEXT_BODY)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_1),
        note(i18n.t("settings.igdb.coletados_desc")),
        list,
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);
    content = content.push(card(igdb.into()));

    content.into()
}

/// Card de seção da aba Metadados.
fn card<'a>(body: Element<'a, Message>) -> Element<'a, Message> {
    container(body)
        .width(Fill)
        .padding([16.0, 18.0])
        .style(theme::stub_panel)
        .into()
}

/// Pílula de estado: verde quando ativo, cinza quando não.
fn pill(text_value: &str, active: bool) -> Element<'_, Message> {
    let color = if active {
        theme::SUCCESS
    } else {
        theme::TEXT_3
    };
    container(
        text(text_value.to_string())
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM)
            .color(color),
    )
    .padding([3.0, 10.0])
    .style(theme::badge)
    .into()
}

/// Controle: diagnóstico do gamepad que o shell nativo já usa na navegação.
fn controller_tab<'a>(app: &'a App) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let gamepad = crate::input::GamepadInput::new().poll();

    let status = if gamepad.connected {
        i18n.t("native.game.controller_connected")
    } else {
        i18n.t("native.game.controller_placeholder")
    };

    column![
        group(
            i18n.t("controller.titulo"),
            column![
                note(i18n.t("native.settings.controller_note")),
                setting_row(
                    i18n.t("native.settings.controller_status"),
                    status,
                    value_text(if gamepad.connected {
                        i18n.t("common.ativado")
                    } else {
                        i18n.t("common.desativado")
                    }),
                ),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ),
        note(i18n.t("native.settings.controller_nav_note")),
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
        crate::views::components::hubcap_key_card(app),
        group(
            i18n.t("native.settings.keys_tokens"),
            column![note(i18n.t("native.settings.keys_note")), note(discord),]
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
        debrid_section(app),
        note(i18n.t("native.settings.integrations_note")),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

/// Seção de debrid: título, descrição e uma linha expansível por provedor.
///
/// Porte do bloco `#servicos-debrid` do `SettingsPanel.tsx`: a linha fechada
/// mostra o nome e o estado (`Conectado` / `Não conectado`); aberta, revela a
/// descrição e o campo do token com o botão de salvar. O botão alterna entre
/// `Salvar`, `Salvo!` e `Desconectar` exatamente como na web.
fn debrid_section(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let config = match app.settings.config.as_ref() {
        Some(config) => config,
        None => return note(i18n.t("common.carregando")),
    };

    let mut rows = column![].spacing(0).width(Fill);
    for provider in DebridProvider::ALL {
        rows = rows.push(debrid_row(app, config, provider));
    }

    container(
        column![
            text(i18n.t("settings.debrid.titulo"))
                .size(theme::TEXT_LABEL)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t("settings.debrid.desc"))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2)
                .width(Fill),
            rows,
        ]
        .spacing(theme::SPACE_3)
        .width(Fill),
    )
    .width(Fill)
    .padding([16.0, 18.0])
    .style(theme::stub_panel)
    .into()
}

/// Uma linha de provedor. Fechada: nome + estado. Aberta: descrição + token.
fn debrid_row<'a>(
    app: &'a App,
    config: &'a SettingsConfig,
    provider: DebridProvider,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let open = app.settings.debrid_open.contains(&provider);
    let connected = is_debrid_connected(config, provider);
    let draft = app.settings.debrid_drafts.get(provider);

    let status = if connected {
        i18n.t("common.conectado")
    } else {
        i18n.t("common.nao_conectado")
    };
    let status_color = if connected {
        theme::SUCCESS
    } else {
        theme::TEXT_3
    };

    let mut header = row![
        text(if open { "v" } else { ">" })
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_3),
        text(provider.name())
            .size(theme::TEXT_BODY)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_1),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    if connected {
        header = header.push(
            text("✓")
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_BOLD)
                .color(theme::SUCCESS),
        );
    }
    header = header
        .push(Space::new().width(Fill))
        .push(text(status).size(theme::TEXT_CAPTION).color(status_color));

    let mut body = column![
        views::cards::hover_motion::wrap(
            button(header)
                .on_press(Message::DebridToggled(provider))
                .padding([10.0, 0.0])
                .width(Fill)
                .style(theme::row_button),
            views::cards::hover_motion::Kind::Navigation,
        ),
    ]
    .spacing(0)
    .width(Fill);

    {
        // O botão troca de rótulo conforme o estado, como na web: com token
        // salvo e campo vazio ele desconecta; com texto novo, salva.
        let label = if app.settings.debrid_saved == Some(provider) {
            i18n.t("common.salvo")
        } else if draft.trim().is_empty() && connected {
            i18n.t("settings.desconectar")
        } else {
            i18n.t("settings.salvar")
        };

        let placeholder = i18n
            .t("settings.debrid.token_placeholder")
            .replace("{nome}", provider.name());

        body = body.push(settings_expand::wrap(
            column![
                text(i18n.t(provider.description_key()))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_2)
                    .width(Fill),
                row![
                    iced::widget::text_input(&placeholder, draft)
                        .on_input(move |value| Message::DebridTokenChanged {
                            provider,
                            value
                        })
                        .padding(Padding {
                            top: 9.0,
                            right: 12.0,
                            bottom: 9.0,
                            left: 12.0,
                        })
                        .size(theme::TEXT_BODY)
                        .width(Fill)
                        .style(theme::search_input),
                    button(
                        text(label)
                            .size(theme::TEXT_BODY)
                            .font(theme::FONT_SEMIBOLD)
                    )
                    .on_press(Message::DebridTokenSave(provider))
                    .padding([9.0, 18.0])
                    .style(theme::primary_button),
                ]
                .spacing(theme::SPACE_2)
                .align_y(iced::Center),
            ]
            .spacing(theme::SPACE_2)
            .width(Fill)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 12.0,
                left: 18.0,
            }),
            open,
        ));
    }

    // Separador entre provedores, como `divide-y` da web.
    column![body, rule()]
        .spacing(0)
        .width(Fill)
        .into()
}

/// `true` quando o provedor tem token salvo.
///
/// O `config:get` devolve o token mascarado quando existe e string vazia
/// quando não existe — nunca `null` para os campos de `SettingsConfig`. A
/// presença de qualquer texto é o sinal de "conectado", igual à web.
fn is_debrid_connected(config: &SettingsConfig, provider: DebridProvider) -> bool {
    let token = match provider {
        DebridProvider::RealDebrid => &config.realdebrid_token,
        DebridProvider::TorBox => &config.torbox_token,
        DebridProvider::AllDebrid => &config.alldebrid_token,
        DebridProvider::Premiumize => &config.premiumize_token,
    };
    !token.trim().is_empty()
}

/// Linha divisória de 1 px entre itens de uma lista.
fn rule() -> Element<'static, Message> {
    container(Space::new().height(1))
        .width(Fill)
        .style(theme::divider)
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
        updates_group(app),
        note(i18n.t("native.settings.about_note")),
    ]
    .spacing(theme::SPACE_5)
    .into()
}

/// Bloco "Atualizações" da aba Sobre: reusa o mesmo `UpdateUiState` do
/// diálogo global e do check automático do boot.
fn updates_group(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let channel = app
        .update
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.channel())
        .unwrap_or("—")
        .to_string();
    let last_check = app
        .update
        .last_check
        .clone()
        .unwrap_or_else(|| i18n.t("native.settings.update_never").to_string());

    let mut rows = column![
        setting_row(
            i18n.t("native.settings.update_channel"),
            i18n.t("native.settings.update_channel_desc"),
            value_text(&channel),
        ),
        setting_row(
            i18n.t("native.settings.update_last"),
            i18n.t("native.settings.update_last_desc"),
            value_text(&last_check),
        ),
    ]
    .spacing(theme::SPACE_2);

    if let Some(version) = app.update.available_version.as_deref() {
        rows = rows.push(setting_row(
            i18n.t("native.settings.update_available"),
            i18n.t("native.settings.update_available_desc"),
            value_text(version),
        ));
    } else if app.update.last_check.is_some()
        && app.update.last_error.is_none()
        && app
            .update
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.is_supported())
    {
        rows = rows.push(note(i18n.t("native.settings.update_current")));
    }

    if let Some(error) = app.update.last_error.as_deref() {
        rows = rows.push(
            text(i18n.t_args("native.settings.update_error", &[("error", error)]))
                .size(theme::TEXT_CAPTION)
                .color(theme::DANGER)
                .width(Fill),
        );
    }
    if app.update.last_motivo.as_deref() == Some("jogo_rodando") {
        rows = rows.push(note(i18n.t("native.update.jogo_rodando")));
    }
    if app
        .update
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_supported())
    {
        rows = rows.push(note(
            i18n.t_args("native.update.sem_suporte", &[("canal", &channel)]),
        ));
    }

    let label = if app.update.checking {
        i18n.t("native.settings.update_checking")
    } else {
        i18n.t("native.settings.update_check")
    };
    let mut control: iced::widget::Button<'_, Message> = button(
        text(label)
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
    .padding([8.0, 14.0]);
    if !app.update.checking {
        control = control.on_press(Message::CheckUpdates(true));
    }
    rows = rows.push(row![control.style(theme::primary_button)].padding([4.0, 14.0]));

    group(i18n.t("native.settings.updates"), rows.into())
}

fn language_button(lang: Lang, active_lang: Lang, disabled: bool) -> Element<'static, Message> {
    let active = active_lang == lang;
    let mut control: iced::widget::Button<'_, Message> = button(
        text(lang.label())
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
    .padding([7.0, 12.0]);
    if !active && !disabled {
        control = control.on_press(Message::LanguageChanged(lang.code().to_string()));
    }
    if active {
        control.style(theme::primary_button).into()
    } else {
        control.style(theme::ghost_button).into()
    }
}

/// Interruptor de uma configuração booleana.
///
/// A web desenha um switch pill; o botão de texto "Ativado/Desativado" que
/// havia aqui não dizia nada de relance e destoava do resto da UI. O
/// `Toggler` do iced é o mesmo controle, com o trilho ciano de `theme`.
fn toggle<'a>(
    key: &'static str,
    value: bool,
    disabled: bool,
    _i18n: &'a I18n,
) -> Element<'a, Message> {
    let mut control = iced::widget::toggler(value)
        .size(theme::TOGGLE_SIZE)
        .style(theme::toggler_switch);
    if !disabled {
        control = control.on_toggle(move |now| Message::SettingToggled { key, value: now });
    }
    control.into()
}

fn setting_row<'a>(
    label: &'a str,
    description: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(label)
                    .size(theme::TEXT_LABEL)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text(description)
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_2),
            ]
            .spacing(3)
            .width(Fill),
            control,
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([14.0, 18.0])
    .style(theme::stub_panel)
    .into()
}

/// Campo de caminho somente leitura com o ícone de pasta à esquerda — o mesmo
/// `input + folder` do `SettingsView.tsx`. Não é editável no nativo ainda: o
/// caminho é escolhido por diálogo na web, e um campo de texto livre aceitaria
/// qualquer coisa sem validar diretório.
fn path_field(value: &str) -> Element<'static, Message> {
    container(
        text(value.to_string())
            .size(theme::TEXT_BODY)
            .color(theme::TEXT_STRONG)
            .width(Fill),
    )
    .width(iced::Length::Fixed(340.0))
    .padding(Padding {
        top: 8.0,
        right: 12.0,
        bottom: 8.0,
        left: 12.0,
    })
    .style(theme::path_input)
    .into()
}

fn value_text(value: impl AsRef<str>) -> Element<'static, Message> {
    text(value.as_ref().to_string())
        .size(theme::TEXT_CAPTION)
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
        .size(theme::TEXT_CAPTION)
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
