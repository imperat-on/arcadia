//! Página do jogo: hero/backdrop, descrição rica, galeria de screenshots,
//! trailer GStreamer e painéis de conquistas, ProtonDB e stats/reviews.

use iced::widget::text::{Rich, Span, Wrapping};
use iced::widget::{
    button, column, container, image, opaque, pick_list, progress_bar, row, scrollable, slider,
    text, text_input, Space, Stack,
};
use iced::{Color, ContentFit, Element, Fill};

use crate::app::{
    GameLaunchState, GamePanels, LaunchOptionsState, LoadStatus, Message, TrailerPickerState,
};
use crate::backend::{
    Achievement, EnvVarSetting, Game, GameMedia, GameSettingsView, GameStats, LaunchToggle,
    Screenshot, TrailerResult,
};
use crate::covers::CoverCache;
use crate::i18n::I18n;
use crate::input;
use crate::media::{self, MediaPlayer, MediaStatus};
use crate::picker::PickPurpose;
use crate::rich_text::{parse_description, RichBlock};
use crate::theme;

pub fn view<'a>(
    game: &'a Game,
    covers: &'a CoverCache,
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
    panels: Option<&'a GamePanels>,
    community: Option<&'a super::community::CommunityState>,
    selected_screenshot: usize,
    launch: &'a GameLaunchState,
    trailer_picker: &'a TrailerPickerState,
    launch_options: &'a LaunchOptionsState,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    let hero_url = game.hero_url.as_deref().or(game.cover_url.as_deref());

    let hero: Element<'a, Message> = match covers.ready(hero_url) {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let back = button(
        row![
            text("‹").size(22).color(theme::TEXT_1),
            text(i18n.t("sidebar.biblioteca"))
                .size(12)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::BackToLibrary)
    .padding([6.0, 12.0])
    .style(theme::ghost_button);

    let mut meta = column![
        text(game.title.as_str())
            .size(40)
            .font(theme::FONT_BOLD)
            .color(theme::TEXT_1),
        text(game.metadata_line())
            .size(14)
            .color(theme::TEXT_STRONG),
        row![
            text(game.playtime_label_i18n(i18n)).size(12).color(theme::TEXT_2),
            text("·").size(12).color(theme::TEXT_3),
            text(if game.installed {
                i18n.t("native.game.installed")
            } else {
                i18n.t("native.game.not_installed")
            })
            .size(12)
            .color(if game.installed {
                theme::SUCCESS
            } else {
                theme::WARNING
            }),
        ]
        .spacing(8),
    ]
    .spacing(6);

    meta = meta.push(launch_controls(game, launch, i18n));
    if let Some(error) = launch.error.as_deref() {
        meta = meta.push(super::action_error_banner(error, None));
    }
    let warnings = launch.warnings_for(&game.id);
    if !warnings.is_empty() {
        meta = meta.push(launch_warning_banner(warnings.join("  ·  ")));
    }

    let overlay = container(
        column![back, Space::new().height(Fill), meta,]
            .height(Fill)
            .width(Fill),
    )
    .width(Fill)
    .height(Fill)
    .padding(theme::PAGE_PADDING)
    .style(theme::hero_overlay);

    let hero = Stack::new()
        .width(Fill)
        .height(theme::HERO_HEIGHT)
        .push(hero)
        .push(overlay);

    let description = description_panel(i18n, game, game_media);
    let gallery = gallery_panel(i18n, game_media, covers, selected_screenshot);

    let mut left = column![description, gallery]
        .spacing(theme::SPACE_4)
        .width(iced::FillPortion(2));
    if let Some(community) = community {
        left = left.push(super::community::panel(i18n, game.title.as_str(), community));
    }

    let mut gamepad = input::GamepadInput::new();
    let gamepad_state = gamepad.poll();
    let gamepad_body = if gamepad_state.connected {
        i18n.t("native.game.controller_connected")
    } else {
        i18n.t("native.game.controller_placeholder")
    };

    let right = column![
        trailer_card(i18n, media, game_media),
        achievements_panel(i18n, panels, covers),
        protondb_panel(i18n, panels),
        stats_reviews_panel(i18n, panels),
        placeholder_card(i18n.t("native.game.controller"), gamepad_body),
    ]
    .spacing(theme::SPACE_4)
    .width(iced::FillPortion(1));

    let body = scrollable(
        row![left, right]
            .spacing(theme::SPACE_4)
            .padding([theme::SPACE_4, theme::PAGE_PADDING])
            .width(Fill)
            .align_y(iced::Top),
    )
    .width(Fill)
    .height(Fill)
    .style(theme::scrollable);

    let page = column![hero, body].width(Fill).height(Fill);

    let mut layers = Stack::new()
        .width(Fill)
        .height(Fill)
        .push(page);
    if trailer_picker.open {
        layers = layers.push(trailer_picker_overlay(i18n, trailer_picker, covers));
    }
    if launch_options.open && launch_options.game_id == game.id {
        layers = layers.push(launch_options_overlay(i18n, launch_options, game));
    }
    layers.into()
}

/// Botão Jogar/Parar da ficha, com o estado da sessão ao lado.
fn launch_controls<'a>(
    game: &'a Game,
    launch: &'a GameLaunchState,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    let running = launch.is_running(&game.id);
    let (label, message) = if !game.installed {
        (i18n.t("native.game.install_to_play").to_string(), None)
    } else if launch.is_launching(&game.id) {
        (i18n.t("common.abrindo").to_string(), None)
    } else if running {
        (
            format!("■  {}", i18n.t("hero.parar")),
            Some(Message::StopGame),
        )
    } else if launch.is_stopping(&game.id) {
        (i18n.t("native.game.stopping").to_string(), None)
    } else if launch.busy_with_other(&game.id) {
        (i18n.t("hero.jogar").to_string(), None)
    } else {
        (
            format!("▶  {}", i18n.t("hero.jogar")),
            Some(Message::LaunchGame(game.id.clone())),
        )
    };

    let play_style: fn(&iced::Theme, button::Status) -> button::Style = if running {
        theme::ghost_button
    } else {
        theme::primary_button
    };

    let play = button(text(label).size(13).font(theme::FONT_SEMIBOLD))
        .on_press_maybe(message)
        .padding([10.0, 22.0])
        .style(play_style);

    let options = button(
        text(i18n.t("native.game.launch_options"))
            .size(11)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::OpenLaunchOptions(game.id.clone()))
    .padding([10.0, 14.0])
    .style(theme::ghost_button);

    let status: Element<'a, Message> = if running {
        text(i18n.t("native.game.running_note"))
            .size(11)
            .color(theme::SUCCESS)
            .into()
    } else if launch.is_launching(&game.id) {
        text(i18n.t("native.game.awaiting_process"))
            .size(11)
            .color(theme::TEXT_2)
            .into()
    } else if launch.is_stopping(&game.id) {
        text(i18n.t("native.game.stopping_process"))
            .size(11)
            .color(theme::TEXT_2)
            .into()
    } else if launch.busy_with_other(&game.id) {
        text(i18n.t("native.game.close_other"))
            .size(11)
            .color(theme::TEXT_3)
            .into()
    } else {
        Space::new().width(0).into()
    };

    row![play, status, Space::new().width(Fill), options]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center)
        .into()
}


// ── Opções de inicialização (overlay) ──────────────────────────────────────

/// Banner de aviso com texto próprio: `warning_banner` recebe `&str` e o texto
/// é montado em runtime (`warnings.join`), então a versão local guarda a
/// `String` dentro do widget.
pub(crate) fn launch_warning_banner(message: String) -> Element<'static, Message> {
    container(text(message).size(12).color(theme::WARNING).width(Fill))
        .width(Fill)
        .padding([10.0, 14.0])
        .style(theme::stub_panel)
        .into()
}

/// Diálogo de opções de inicialização: overlay desenhado sobre a ficha e
/// reutilizado pelo Big Picture. Só envia `gamesettings:set`; o próximo
/// `game:launch` lê o arquivo no `RuntimeDeps` do IPC.
pub(crate) fn launch_options_overlay<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
    game: &'a Game,
) -> Element<'a, Message> {
    let header = row![
        column![
            text(i18n.t("native.game.launch_options"))
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t_args(
                "native.game.options_subtitle",
                &[("title", &state.game_title)],
            ))
            .size(11)
            .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        button(
            text(i18n.t("common.fechar"))
                .size(12)
                .font(theme::FONT_MEDIUM),
        )
            .on_press(Message::CloseLaunchOptions)
            .padding([7.0, 14.0])
            .style(theme::ghost_button),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    let mut content = column![header].spacing(theme::SPACE_4);
    if let Some(error) = state.error.as_deref() {
        content = content.push(super::action_error_banner(error, None));
    }

    content = match (&state.status, state.settings.as_ref()) {
        (LoadStatus::Idle | LoadStatus::Loading, _) => content.push(placeholder_panel(
            i18n.t("native.game.options_loading"),
            theme::TEXT_2,
        )),
        (LoadStatus::Error(error), _) => {
            content.push(launch_options_load_error(i18n, error, &game.id))
        }
        (LoadStatus::Ready, Some(settings)) => content
            .push(launch_options_form(i18n, state, settings, game))
            .push(launch_options_footer(i18n, state)),
        (LoadStatus::Ready, None) => content.push(placeholder_panel(
            i18n.t("native.game.options_unavailable"),
            theme::TEXT_3,
        )),
    };

    let card = container(content)
        .width(Fill)
        .max_width(860.0)
        .padding(theme::SPACE_4)
        .style(theme::panel);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::scrim),
        )
        .push(opaque(
            container(card)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .padding(theme::SPACE_3),
        ))
        .into()
}

fn launch_options_load_error<'a>(
    i18n: &'a I18n,
    error: &'a str,
    game_id: &'a str,
) -> Element<'a, Message> {
    container(
        column![
            text(i18n.t("native.game.options_read_error"))
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::DANGER),
            text(shorten(error, 280)).size(11).color(theme::TEXT_2),
            button(
                text(i18n.t("downloads.tentar_novamente"))
                    .size(12)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::OpenLaunchOptions(game_id.to_string()))
                .padding([7.0, 13.0])
                .style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_2)
        .align_x(iced::Center),
    )
    .width(Fill)
    .height(240)
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn launch_options_form<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
    settings: &'a GameSettingsView,
    game: &'a Game,
) -> Element<'a, Message> {
    let mode = state.mode.as_deref();
    let steam_available = game.launcher == "steam";
    let exe_available = !settings.exe_path.trim().is_empty();

    let mode_section = option_section(
        i18n.t("native.game.launch_mode"),
        column![
            row![
                mode_button(
                    i18n.t("native.game.mode_auto"),
                    mode.is_none(),
                    Some(Message::LaunchOptionsModeChanged(None))
                ),
                mode_button(
                    i18n.t("gamecard.steam"),
                    mode == Some("steam"),
                    steam_available.then_some(Message::LaunchOptionsModeChanged(Some(
                        "steam".to_string()
                    ))),
                ),
                mode_button(
                    i18n.t("gamesettings.executavel"),
                    mode == Some("exe"),
                    exe_available.then_some(Message::LaunchOptionsModeChanged(Some(
                        "exe".to_string()
                    ))),
                ),
            ]
            .spacing(8),
            text(if steam_available {
                i18n.t("native.game.mode_hint")
            } else {
                i18n.t("native.game.mode_hint_steam")
            })
            .size(10)
            .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .into(),
    );

    let default_prefix = if state.default_prefix.is_empty() {
        i18n.t("native.game.default_prefix").to_string()
    } else {
        state.default_prefix.clone()
    };
    let wine_section = option_section(
        i18n.t("native.game.wine_section"),
        column![
            field_label(i18n.t("native.game.prefix_label")),
            text_input(
                i18n.t("native.game.prefix_placeholder"),
                settings.prefix_path.as_str(),
            )
            .on_input(Message::LaunchOptionsPrefixChanged)
            .padding([8.0, 10.0])
            .size(12)
            .style(theme::search_input),
            text(i18n.t_args(
                "native.game.prefix_hint",
                &[("prefix", &default_prefix)],
            ))
            .size(9)
            .color(theme::TEXT_3),
            disabled_option(
                i18n.t("native.game.wine_version"),
                settings
                    .wine_version
                    .clone()
                    .unwrap_or_else(|| i18n.t("native.game.system_wine").to_string()),
                i18n.t("native.game.wine_disabled"),
            ),
        ]
        .spacing(theme::SPACE_2)
        .into(),
    );

    let paths_section = option_section(
        i18n.t("native.game.exe_section"),
        column![
            field_label(i18n.t("native.game.exe_label")),
            text_input(
                i18n.t("native.game.exe_placeholder"),
                settings.exe_path.as_str(),
            )
            .on_input(Message::LaunchOptionsExeChanged)
            .padding([8.0, 10.0])
            .size(12)
            .style(theme::search_input),
            field_label(i18n.t("native.game.extra_args")),
            text_input(
                i18n.t("native.game.args_placeholder"),
                settings.game_args.as_str(),
            )
            .on_input(Message::LaunchOptionsArgsChanged)
            .padding([8.0, 10.0])
            .size(12)
            .style(theme::search_input),
            text(i18n.t("native.game.args_hint"))
                .size(9)
                .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .into(),
    );

    let compatibility = option_section(
        i18n.t("native.game.compat_section"),
        column![
            toggle_row(&[
                ("Esync", LaunchToggle::Esync, settings.esync),
                ("Fsync", LaunchToggle::Fsync, settings.fsync),
                ("Wayland", LaunchToggle::WineWayland, settings.wine_wayland),
            ]),
            toggle_row(&[
                ("Wow64", LaunchToggle::Wow64, settings.wow64),
                ("FSR hack", LaunchToggle::FsrHack, settings.fsr_hack),
                ("NVAPI", LaunchToggle::AutoNvapi, settings.auto_nvapi),
            ]),
        ]
        .spacing(6)
        .into(),
    );

    let performance = option_section(
        i18n.t("native.game.performance_section"),
        column![
            toggle_row(&[
                ("MangoHud", LaunchToggle::Mangohud, settings.mangohud),
                ("GameMode", LaunchToggle::Gamemode, settings.gamemode),
                ("Gamescope", LaunchToggle::Gamescope, settings.gamescope),
            ]),
            toggle_row(&[
                ("HDR (gamescope)", LaunchToggle::GsHdr, settings.gs_hdr),
                (i18n.t("native.game.verbose"), LaunchToggle::VerboseLogs, settings.verbose_logs),
            ]),
            field_label(i18n.t("native.game.dxvk_hud")),
            text_input(
                i18n.t("native.game.hud_placeholder"),
                settings.dxvk_hud.as_str(),
            )
            .on_input(Message::LaunchOptionsHudChanged)
            .padding([8.0, 10.0])
            .size(12)
            .style(theme::search_input),
            text(i18n.t("native.game.performance_note"))
                .size(9)
                .color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_2)
        .into(),
    );

    let env_section = launch_env_section(i18n, state, settings);

    let emulator_section = launch_emulator_section(i18n, state, settings);

    let disabled_section = option_section(
        i18n.t("native.game.disabled_section"),
        column![
            disabled_option(
                "autoDXVK / autoVKD3D",
                format!(
                    "DXVK {} · VKD3D {}",
                    sim_nao(i18n, settings.auto_dxvk),
                    sim_nao(i18n, settings.auto_vkd3d)
                ),
                i18n.t("native.game.auto_graphics_note"),
            ),
            disabled_option(
                i18n.t("native.game.scripts"),
                if settings.script_pre.is_empty() && settings.script_post.is_empty() {
                    i18n.t("native.game.not_configured_plural").to_string()
                } else {
                    i18n.t("native.game.configured_plural").to_string()
                },
                i18n.t("native.game.scripts_note"),
            ),
            disabled_option(
                i18n.t("native.game.wrappers"),
                i18n.t_args(
                    "native.game.wrappers_count",
                    &[("count", &settings.wrapper_count.to_string())],
                ),
                i18n.t("native.game.wrappers_note"),
            ),
        ]
        .spacing(6)
        .into(),
    );

    let form = column![
        mode_section,
        wine_section,
        paths_section,
        compatibility,
        performance,
        env_section,
        emulator_section,
        disabled_section,
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    scrollable(form)
        .height(360.0)
        .style(theme::scrollable)
        .into()
}

fn launch_emulator_section<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
    settings: &'a GameSettingsView,
) -> Element<'a, Message> {
    let mut body = column![].spacing(theme::SPACE_2);

    let mut choices = Vec::with_capacity(state.emulators.len() + 1);
    choices.push(EmulatorChoice {
        id: String::new(),
        label: i18n.t("native.game.emulator_none").to_string(),
    });
    choices.extend(state.emulators.iter().map(|item| EmulatorChoice {
        id: item.id.clone(),
        label: if item.available {
            format!("{} · {}", item.name, item.systems.join(" / "))
        } else {
            format!(
                "{} ({})",
                item.name,
                i18n.t("native.game.emulator_unavailable")
            )
        },
    }));

    let selected_id = settings.emulator_id.clone().unwrap_or_default();
    let selected = choices.iter().find(|choice| choice.id == selected_id).cloned();
    body = body.push(field_label(i18n.t("native.game.emulator_selector")));
    body = body.push(
        pick_list(choices, selected, |choice: EmulatorChoice| {
            Message::LaunchOptionsEmulatorChanged(choice.id)
        })
        .width(Fill)
        .padding([8.0, 10.0])
        .text_size(12)
        .style(theme::pick_list)
        .menu_style(theme::pick_list_menu),
    );
    if let Some(error) = state.emulators_error.as_deref() {
        body = body.push(super::action_error_banner(error, None));
    }

    if let Some(item) = state.selected_emulator() {
        let rom_purpose = PickPurpose::GameRom {
            game_id: state.game_id.clone(),
            emulator_id: item.id.clone(),
        };
        body = body.push(field_label(i18n.t("native.game.emulator_rom")));
        body = body.push(pick_control_row(
            text_input(
                i18n.t("native.game.emulator_rom_placeholder"),
                settings.rom_path.as_str(),
            )
            .on_input(Message::LaunchOptionsRomChanged)
            .padding([8.0, 10.0])
            .size(12)
            .width(Fill)
            .style(theme::search_input)
            .into(),
            rom_purpose,
            state.pick_busy.as_ref(),
            i18n,
            i18n.t("native.game.emulator_pick_rom"),
        ));

        if item.requires_core || item.id == "retroarch" {
            let core_purpose = PickPurpose::GameCore {
                game_id: state.game_id.clone(),
                emulator_id: item.id.clone(),
            };
            body = body.push(field_label(i18n.t("native.game.emulator_core")));
            body = body.push(pick_control_row(
                text_input(
                    i18n.t("native.game.emulator_core_placeholder"),
                    settings.emulator_core_path.as_str(),
                )
                .on_input(Message::LaunchOptionsCoreChanged)
                .padding([8.0, 10.0])
                .size(12)
                .width(Fill)
                .style(theme::search_input)
                .into(),
                core_purpose,
                state.pick_busy.as_ref(),
                i18n,
                i18n.t("native.game.emulator_pick_core"),
            ));
            body = body.push(
                text(i18n.t("native.game.emulator_core_hint"))
                    .size(9)
                    .color(theme::TEXT_3),
            );
            if settings.emulator_core_path.trim().is_empty() {
                body = body.push(
                    text(i18n.t("native.game.emulator_core_missing"))
                        .size(10)
                        .color(theme::WARNING),
                );
            }
        }

        if let Some(status) = state.selected_emulator_status() {
            if status.required && !status.installed {
                let detail = status
                    .error
                    .clone()
                    .unwrap_or_else(|| i18n.t("native.game.emulator_bios_missing").to_string());
                body = body.push(
                    text(i18n.t_args(
                        "native.game.emulator_bios_warning",
                        &[("detail", &detail)],
                    ))
                    .size(10)
                    .color(theme::WARNING),
                );
            }
        }

        body = body.push(field_label(i18n.t("native.game.emulator_args")));
        body = body.push(
            text_input(
                i18n.t("native.game.emulator_args_placeholder"),
                state.emulator_args_text.as_str(),
            )
            .on_input(Message::LaunchOptionsEmulatorArgsChanged)
            .padding([8.0, 10.0])
            .size(12)
            .style(theme::search_input),
        );
        body = body.push(
            text(i18n.t("native.game.emulator_args_hint"))
                .size(9)
                .color(theme::TEXT_3),
        );
        body = body.push(
            text(i18n.t_args(
                "native.game.emulator_launch_note",
                &[("name", &item.name)],
            ))
            .size(10)
            .color(theme::TEXT_3),
        );
    } else if settings.emulator_id.is_some() {
        body = body.push(
            text(i18n.t("native.game.emulator_unknown"))
                .size(10)
                .color(theme::WARNING),
        );
    } else {
        body = body.push(
            text(i18n.t("native.game.emulator_none_hint"))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    option_section(i18n.t("native.game.emulator_section"), body.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmulatorChoice {
    id: String,
    label: String,
}

impl std::fmt::Display for EmulatorChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn pick_control_row<'a>(
    control: Element<'a, Message>,
    purpose: PickPurpose,
    pick_busy: Option<&PickPurpose>,
    i18n: &'a I18n,
    idle_label: &'a str,
) -> Element<'a, Message> {
    let picking = pick_busy == Some(&purpose);
    let label = if picking {
        i18n.t("native.picker.opening")
    } else {
        idle_label
    };
    let mut pick = button(text(label).size(11).font(theme::FONT_MEDIUM))
        .padding([8.0, 12.0])
        .style(theme::ghost_button);
    if pick_busy.is_none() {
        pick = pick.on_press(Message::PickRequested(purpose));
    }
    row![container(control).width(Fill), pick]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

fn launch_env_section<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
    settings: &'a GameSettingsView,
) -> Element<'a, Message> {
    let mut list = column![].spacing(4);
    if settings.env_vars.is_empty() {
        list = list.push(
            text(i18n.t("native.game.env_empty"))
                .size(10)
                .color(theme::TEXT_3),
        );
    }
    for (index, variable) in settings.env_vars.iter().enumerate() {
        list = list.push(env_var_row(i18n, variable, index));
    }

    let add = row![
        text_input(i18n.t("gamesettings.env_placeholder_nome"), state.env_name.as_str())
            .on_input(Message::LaunchOptionsEnvNameChanged)
            .padding([8.0, 10.0])
            .size(11)
            .width(Fill)
            .style(theme::search_input),
        text_input(
            i18n.t("native.game.env_value"),
            state.env_value.as_str()
        )
            .on_input(Message::LaunchOptionsEnvValueChanged)
            .padding([8.0, 10.0])
            .size(11)
            .width(Fill)
            .style(theme::search_input),
        button(
            text(i18n.t("fontes.adicionar"))
                .size(11)
                .font(theme::FONT_MEDIUM),
        )
        .on_press(Message::LaunchOptionsEnvAdd)
            .padding([8.0, 12.0])
            .style(theme::ghost_button),
    ]
    .spacing(8)
    .align_y(iced::Center);

    option_section(
        i18n.t("gamesettings.env_vars"),
        column![
            list,
            add,
            text(i18n.t("native.game.env_hint"))
                .size(9)
                .color(theme::TEXT_3)
        ]
        .spacing(theme::SPACE_2)
        .into(),
    )
}

fn env_var_row<'a>(
    i18n: &'a I18n,
    variable: &'a EnvVarSetting,
    index: usize,
) -> Element<'a, Message> {
    container(
        row![
            text(variable.name.as_str())
                .size(11)
                .color(theme::TEXT_1)
                .width(Fill),
            text(if variable.value.is_empty() {
                i18n.t("common.fallback")
            } else {
                variable.value.as_str()
            })
            .size(11)
            .color(theme::TEXT_2)
            .width(Fill),
            button(
                text(i18n.t("common.remover"))
                    .size(10)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::LaunchOptionsEnvRemove(index))
                .padding([4.0, 9.0])
                .style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding([5.0, 9.0])
    .style(theme::stub_panel)
    .into()
}

fn launch_options_footer<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
) -> Element<'a, Message> {
    let mut save = button(
        text(if state.saving {
            i18n.t("common.salvando")
        } else {
            i18n.t("common.salvar")
        })
        .size(12)
        .font(theme::FONT_SEMIBOLD),
    )
    .padding([9.0, 18.0])
    .style(theme::primary_button);
    if !state.saving {
        save = save.on_press(Message::LaunchOptionsSave);
    }

    let note = if state.saving {
        i18n.t("native.game.save_saving_note")
    } else if state.dirty {
        i18n.t("native.game.save_dirty")
    } else if state.saved {
        i18n.t("native.game.save_done")
    } else {
        i18n.t("native.game.save_idle")
    };

    row![
        text(note)
            .size(10)
            .color(if state.dirty {
                theme::WARNING
            } else if state.saved {
                theme::SUCCESS
            } else {
                theme::TEXT_3
            }),
        Space::new().width(Fill),
        button(
            text(i18n.t("common.fechar"))
                .size(12)
                .font(theme::FONT_MEDIUM),
        )
        .on_press(Message::CloseLaunchOptions)
            .padding([9.0, 14.0])
            .style(theme::ghost_button),
        save,
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center)
    .into()
}

fn mode_button<'a>(
    label: &'a str,
    active: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let style: fn(&iced::Theme, button::Status) -> button::Style = if active {
        theme::primary_button
    } else {
        theme::ghost_button
    };
    let mut control = button(text(label).size(11).font(theme::FONT_MEDIUM))
        .padding([7.0, 12.0])
        .style(style);
    if let Some(message) = message {
        control = control.on_press(message);
    } else {
        control = control.style(theme::ghost_button);
    }
    control.into()
}

fn toggle_row<'a>(items: &[(&'a str, LaunchToggle, bool)]) -> Element<'a, Message> {
    let mut controls = row![].spacing(6);
    for (label, key, value) in items {
        controls = controls.push(toggle_button(label, *key, *value));
    }
    controls.into()
}

fn toggle_button<'a>(label: &'a str, key: LaunchToggle, value: bool) -> Element<'a, Message> {
    let glyph = if value { "✓" } else { "○" };
    let style: fn(&iced::Theme, button::Status) -> button::Style = if value {
        theme::primary_button
    } else {
        theme::ghost_button
    };
    button(
        text(format!("{glyph} {label}"))
            .size(11)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::LaunchOptionsToggle { key, value: !value })
    .padding([7.0, 12.0])
    .style(style)
    .into()
}

fn option_section<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            crate::views::section_label(title),
            body,
        ]
        .spacing(theme::SPACE_2),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

fn field_label<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(11).color(theme::TEXT_2).into()
}

fn disabled_option<'a>(
    label: &'a str,
    value: String,
    reason: &'a str,
) -> Element<'a, Message> {
    container(
        column![
            row![
                text(label)
                    .size(11)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_2),
                Space::new().width(Fill),
                text(value).size(10).color(theme::TEXT_3),
            ]
            .align_y(iced::Center),
            text(reason)
                .size(10)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(3),
    )
    .width(Fill)
    .padding([7.0, 10.0])
    .style(theme::stub_panel)
    .into()
}

fn sim_nao<'a>(i18n: &'a I18n, value: bool) -> &'a str {
    if value {
        i18n.t("common.ligado")
    } else {
        i18n.t("common.desligado")
    }
}

// ── Descrição rica ─────────────────────────────────────────────────────────

/// Painel "Sobre o jogo": usa o HTML de `about` do `game:sysinfo`; sem ele,
/// renderiza a descrição já limpa da biblioteca. Nada de tag crua na tela.
fn description_panel<'a>(
    i18n: &'a I18n,
    game: &'a Game,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
    let about = game_media.and_then(|media| media.about.as_deref());
    let raw = about.or(game.description.as_deref());
    let note = if about.is_some() {
        i18n.t("native.game.html_note")
    } else {
        i18n.t("native.game.library_desc_note")
    };

    let body: Element<'a, Message> = match (game_media, raw) {
        (None, None) => hint(i18n.t("native.game.description_loading"), theme::TEXT_2),
        (_, Some(raw)) => {
            let blocks = parse_description(raw);
            if blocks.is_empty() {
                hint(
                    i18n.t("native.game.description_unreadable"),
                    theme::TEXT_3,
                )
            } else {
                description_blocks(blocks)
            }
        }
        (Some(media), None) => {
            let failed = media
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Falha") || notice.contains("info:null"));
            if failed {
                hint(
                    media
                        .notice
                        .as_deref()
                        .unwrap_or(i18n.t("native.game.store_query_failed")),
                    theme::WARNING,
                )
            } else {
                hint(
                    i18n.t("native.console.store.description_empty"),
                    theme::TEXT_3,
                )
            }
        }
    };

    panel_card(
        i18n.t("native.game.about"),
        None,
        column![text(note).size(9).color(theme::TEXT_3), body]
            .spacing(theme::SPACE_2)
            .into(),
    )
}

fn description_blocks(blocks: Vec<RichBlock>) -> Element<'static, Message> {
    let mut content = column![].spacing(theme::SPACE_3);
    for block in blocks {
        content = content.push(rich_block(block));
    }
    content.into()
}

fn rich_block(block: RichBlock) -> Element<'static, Message> {
    let spans: Vec<Span<'static, (), iced::Font>> = block
        .spans
        .into_iter()
        .map(|span| {
            let mut out = Span::new(span.text);
            if span.bold {
                out = out.font(theme::FONT_BOLD);
            }
            if span.link {
                out = out.color(theme::ACCENT).underline(true);
            }
            out
        })
        .collect();

    Rich::<(), Message>::with_spans(spans)
        .size(13)
        .line_height(1.35)
        .width(Fill)
        .into()
}

// ── Galeria de screenshots ─────────────────────────────────────────────────

fn gallery_panel<'a>(
    i18n: &'a I18n,
    game_media: Option<&'a GameMedia>,
    covers: &'a CoverCache,
    selected: usize,
) -> Element<'a, Message> {
    let right: Option<Element<'a, Message>> = Some(
        button(text(i18n.t("native.refresh")).size(9).font(theme::FONT_MEDIUM))
            .on_press(Message::ReloadGameData)
            .padding([5.0, 10.0])
            .style(theme::ghost_button)
            .into(),
    );

    let body: Element<'a, Message> = match game_media {
        None => hint(i18n.t("native.game.screenshots_loading"), theme::TEXT_2),
        Some(media) if media.screenshots.is_empty() => {
            let has_failure = media
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Falha") || notice.contains("info:null"));
            if has_failure {
                hint(
                    media
                        .notice
                        .as_deref()
                        .unwrap_or(i18n.t("native.game.store_query_failed")),
                    theme::WARNING,
                )
            } else {
                hint(
                    i18n.t("native.game.screenshots_missing"),
                    theme::TEXT_3,
                )
            }
        }
        Some(media) => {
            let index = selected.min(media.screenshots.len() - 1);
            let screenshot = &media.screenshots[index];
            let full = screenshot.full_or_thumb();
            let large: Element<'a, Message> = match covers.ready(full) {
                Some(handle) => image(handle.clone())
                    .width(Fill)
                    .height(320)
                    .content_fit(ContentFit::Contain)
                    .into(),
                None => container(
                    text(i18n.t("native.game.screenshot_loading"))
                        .size(10)
                        .color(theme::TEXT_3),
                )
                .width(Fill)
                .height(320)
                .center_x(Fill)
                .center_y(Fill)
                .style(theme::cover_placeholder)
                .into(),
            };

            column![
                large,
                screenshot_strip(&media.screenshots, covers, index),
                text(i18n.t_args(
                    "native.game.screenshot_counter",
                    &[
                        ("index", &(index + 1).to_string()),
                        ("total", &media.screenshots.len().to_string()),
                    ],
                ))
                .size(9)
                .color(theme::TEXT_3),
            ]
            .spacing(theme::SPACE_2)
            .into()
        }
    };

    panel_card(i18n.t("native.game.screenshots"), right, body)
}

fn screenshot_strip<'a>(
    screenshots: &'a [Screenshot],
    covers: &'a CoverCache,
    selected: usize,
) -> Element<'a, Message> {
    let thumbs = row(
        screenshots
            .iter()
            .enumerate()
            .map(|(index, screenshot)| screenshot_thumb(screenshot, covers, index, selected == index))
            .collect::<Vec<_>>(),
    )
    .spacing(theme::SPACE_2);

    scrollable(thumbs)
        .horizontal()
        .width(Fill)
        .height(82)
        .style(theme::scrollable)
        .into()
}

fn screenshot_thumb<'a>(
    screenshot: &'a Screenshot,
    covers: &'a CoverCache,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let thumb = screenshot.thumb_or_full();
    let picture: Element<'a, Message> = match covers.ready(thumb) {
        Some(handle) => image(handle.clone())
            .width(128)
            .height(72)
            .content_fit(ContentFit::Cover)
            .border_radius(3)
            .into(),
        None => container(text("…").size(11).color(theme::TEXT_3))
            .width(128)
            .height(72)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let button = button(picture)
        .on_press(Message::SelectScreenshot(index))
        .padding(0)
        .style(theme::card);
    let marker = container(Space::new())
        .width(128)
        .height(2)
        .style(if selected {
            theme::accent_bar
        } else {
            theme::transparent_bar
        });

    column![button, marker]
        .spacing(3)
        .width(128)
        .into()
}

// ── Conquistas ─────────────────────────────────────────────────────────────

fn achievements_panel<'a>(
    i18n: &'a I18n,
    panels: Option<&'a GamePanels>,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let Some(panels) = panels else {
        return panel_card(
            i18n.t("conquistas.titulo"),
            None,
            hint(i18n.t("native.game.achievements_loading"), theme::TEXT_2),
        );
    };

    let panel = &panels.achievements;
    let done = panel.items.iter().filter(|item| item.unlocked).count();
    let total = panel.items.len();
    let percent = if total > 0 {
        (done as f32 / total as f32 * 100.0).round() as u32
    } else {
        0
    };
    let right: Option<Element<'a, Message>> = (total > 0).then(|| {
        text(format!("{done} / {total} · {percent}%"))
            .size(10)
            .color(theme::TEXT_2)
            .into()
    });

    let body: Element<'a, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            hint(i18n.t("native.game.achievements_loading"), theme::TEXT_2)
        }
        LoadStatus::Error(error) => {
            error_body(i18n, i18n.t("native.game.achievements_load_error"), error)
        }
        LoadStatus::Ready if panel.items.is_empty() => hint(
            panel
                .note
                .as_deref()
                .unwrap_or(i18n.t("native.game.achievements_empty")),
            theme::TEXT_3,
        ),
        LoadStatus::Ready => {
            let mut content = column![].spacing(theme::SPACE_2);
            if let Some(note) = panel.note.as_deref() {
                content = content.push(hint(note, theme::WARNING));
            }
            content = content.push(
                progress_bar(0.0..=1.0, done as f32 / total as f32)
                    .length(Fill)
                    .girth(6),
            );

            let mut list = column![].spacing(theme::SPACE_1);
            for item in &panel.items {
                list = list.push(achievement_row(i18n, item, covers));
            }
            content = content.push(
                scrollable(list)
                    .height(280)
                    .width(Fill)
                    .style(theme::scrollable),
            );
            content.into()
        }
    };

    panel_card(i18n.t("conquistas.titulo"), right, body)
}

fn achievement_row<'a>(
    i18n: &'a I18n,
    item: &'a Achievement,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let icon: Element<'a, Message> = match covers.ready(item.icon.as_deref()) {
        Some(handle) => {
            let picture = image(handle.clone())
                .width(42)
                .height(42)
                .content_fit(ContentFit::Cover)
                .border_radius(3);
            if item.unlocked {
                picture.into()
            } else {
                picture.opacity(0.5_f32).into()
            }
        }
        None => container(text("?").size(14).color(theme::TEXT_3))
            .width(42)
            .height(42)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::cover_placeholder)
            .into(),
    };

    let status = if item.unlocked {
        text(i18n.t("native.game.achievement_unlocked"))
            .size(9)
            .color(theme::SUCCESS)
    } else {
        text(i18n.t("native.game.achievement_locked"))
            .size(9)
            .color(theme::TEXT_3)
    };
    let mut meta = row![status].spacing(theme::SPACE_2);
    if let Some(percent) = item.percent {
        meta = meta.push(
            text(i18n.t_args(
                "native.game.achievement_players",
                &[("percent", &percent.to_string())],
            ))
                .size(9)
                .color(theme::TEXT_3),
        );
    }
    if let Some(points) = item.points {
        meta = meta.push(
            text(i18n.t_args("native.game.points", &[("points", &points.to_string())]))
                .size(9)
                .color(theme::TEXT_3),
        );
    }

    let mut info = column![
        text(item.title.as_str())
            .size(12)
            .font(if item.unlocked {
                theme::FONT_SEMIBOLD
            } else {
                theme::FONT_MEDIUM
            })
            .color(if item.unlocked {
                theme::TEXT_1
            } else {
                theme::TEXT_2
            })
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(2)
    .width(Fill);

    if let Some(description) = item.description.as_deref() {
        info = info.push(
            text(description)
                .size(10)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    info = info.push(meta);

    row![icon, info]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center)
        .width(Fill)
        .padding([4.0, 2.0])
        .into()
}

// ── ProtonDB ───────────────────────────────────────────────────────────────

fn protondb_panel<'a>(i18n: &'a I18n, panels: Option<&'a GamePanels>) -> Element<'a, Message> {
    let Some(panels) = panels else {
        return panel_card(
            "ProtonDB",
            None,
            hint(i18n.t("native.game.protondb_loading"), theme::TEXT_2),
        );
    };
    let panel = &panels.proton;
    let badge = panel
        .info
        .as_ref()
        .and_then(|info| info.tier.as_deref())
        .map(|tier| colored_badge(tier_label(i18n, tier), tier_color(tier)));

    let body: Element<'a, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            hint(i18n.t("native.game.protondb_loading"), theme::TEXT_2)
        }
        LoadStatus::Error(error) => error_body(
            i18n,
            i18n.t("native.game.protondb_error"),
            error,
        ),
        LoadStatus::Ready => match panel.info.as_ref() {
            None => hint(
                panel
                    .note
                    .as_deref()
                    .unwrap_or(i18n.t("native.game.protondb_empty")),
                theme::TEXT_3,
            ),
            Some(info) => {
                let mut rows = column![].spacing(theme::SPACE_2);
                if let Some(tier) = info.tier.as_deref() {
                    rows = rows.push(proton_row(
                        i18n.t("protondb.nivel"),
                        colored_badge(tier_label(i18n, tier), tier_color(tier)),
                    ));
                }
                if let Some(deck) = info.deck.as_deref() {
                    rows = rows.push(proton_row(
                        i18n.t("protondb.steam_deck"),
                        text(deck_label(i18n, deck))
                            .size(11)
                            .color(theme::TEXT_STRONG)
                            .into(),
                    ));
                }
                if let Some(score) = info.score_pct {
                    rows = rows.push(proton_row(
                        i18n.t("protondb.pontuacao"),
                        text(format!("{score}%")).size(11).color(theme::TEXT_STRONG).into(),
                    ));
                }
                if let Some(total) = info.total {
                    rows = rows.push(proton_row(
                        i18n.t("native.game.reports"),
                        text(format_count(total))
                            .size(11)
                            .color(theme::TEXT_STRONG)
                            .into(),
                    ));
                }
                if let Some(url) = info.url.as_deref() {
                    rows = rows.push(
                        column![
                            text(i18n.t("native.game.link"))
                                .size(9)
                                .font(theme::FONT_MEDIUM)
                                .color(theme::TEXT_3),
                            text(url)
                                .size(9)
                                .color(theme::ACCENT)
                                .wrapping(Wrapping::WordOrGlyph),
                        ]
                        .spacing(2),
                    );
                    rows = rows.push(hint(
                        i18n.t("native.game.link_note"),
                        theme::TEXT_3,
                    ));
                }
                rows.into()
            }
        },
    };

    panel_card("ProtonDB", badge, body)
}

fn proton_row<'a>(label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label).size(11).color(theme::TEXT_2),
        Space::new().width(Fill),
        value,
    ]
    .align_y(iced::Center)
    .into()
}

fn tier_label<'a>(i18n: &'a I18n, tier: &'a str) -> &'a str {
    match tier {
        "platinum" => i18n.t("protondb.tier.platinum"),
        "gold" => i18n.t("protondb.tier.gold"),
        "silver" => i18n.t("protondb.tier.silver"),
        "bronze" => i18n.t("protondb.tier.bronze"),
        "borked" => i18n.t("protondb.tier.borked"),
        "pending" => i18n.t("native.game.tier_pending"),
        other => other,
    }
}

fn tier_color(tier: &str) -> Color {
    match tier {
        "platinum" => Color::from_rgb8(0xb4, 0xc7, 0xdc),
        "gold" => Color::from_rgb8(0xcf, 0xb5, 0x3b),
        "silver" => Color::from_rgb8(0xa7, 0xa7, 0xad),
        "bronze" => Color::from_rgb8(0xcd, 0x7f, 0x32),
        "borked" => theme::DANGER,
        _ => theme::TEXT_2,
    }
}

fn deck_label<'a>(i18n: &'a I18n, deck: &'a str) -> &'a str {
    match deck {
        "verified" => i18n.t("protondb.deck.verified"),
        "playable" => i18n.t("protondb.deck.playable"),
        "unsupported" => i18n.t("protondb.deck.unsupported"),
        "unknown" => i18n.t("protondb.deck.unknown"),
        other => other,
    }
}

fn colored_badge<'a>(label: &'a str, background: Color) -> Element<'a, Message> {
    container(
        text(label)
            .size(9)
            .font(theme::FONT_BOLD)
            .color(theme::BACKGROUND),
    )
    .padding([2.0, 8.0])
    .style(move |_theme| container::Style {
        background: Some(background.into()),
        border: iced::Border::default().rounded(3),
        text_color: Some(theme::BACKGROUND),
        ..container::Style::default()
    })
    .into()
}

// ── Stats e reviews ────────────────────────────────────────────────────────

fn stats_reviews_panel<'a>(
    i18n: &'a I18n,
    panels: Option<&'a GamePanels>,
) -> Element<'a, Message> {
    let Some(panels) = panels else {
        return panel_card(
            i18n.t("native.game.stats_title"),
            None,
            hint(i18n.t("native.game.stats_loading"), theme::TEXT_2),
        );
    };
    let panel = &panels.stats;
    let right: Option<Element<'a, Message>> = panel
        .stats
        .as_ref()
        .and_then(|stats| stats.total_reviews)
        .map(|total| {
            text(i18n.t_args(
                "native.game.reviews_count",
                &[("count", &format_count(total))],
            ))
                .size(9)
                .color(theme::TEXT_2)
                .into()
        });

    let body: Element<'a, Message> = match &panel.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            hint(i18n.t("native.game.stats_loading"), theme::TEXT_2)
        }
        LoadStatus::Error(error) => error_body(i18n, i18n.t("native.game.stats_error"), error),
        LoadStatus::Ready => match panel.stats.as_ref() {
            None => hint(
                panel
                    .note
                    .as_deref()
                    .unwrap_or(i18n.t("native.game.stats_empty")),
                theme::TEXT_3,
            ),
            Some(stats) => stats_body(i18n, stats),
        },
    };

    panel_card(i18n.t("native.game.stats_title"), right, body)
}

fn stats_body<'a>(i18n: &'a I18n, stats: &'a GameStats) -> Element<'a, Message> {
    let mut content = column![].spacing(theme::SPACE_2);
    if let Some(owners) = stats.owners.as_deref() {
        content = content.push(proton_row(
            i18n.t("native.game.owners"),
            text(owners).size(11).color(theme::TEXT_STRONG).into(),
        ));
    }
    if let Some(ccu) = stats.ccu.filter(|ccu| *ccu > 0) {
        content = content.push(proton_row(
            i18n.t("stats.jogadores_ativos"),
            text(format_count(ccu))
                .size(11)
                .color(theme::TEXT_STRONG)
                .into(),
        ));
    }
    if stats.positive_pct.is_some() || stats.review_desc.is_some() {
        let mut value = stats.review_desc.clone().unwrap_or_default();
        if let Some(percent) = stats.positive_pct {
            if !value.is_empty() {
                value.push_str(" · ");
            }
            value.push_str(&i18n.t_args(
                "native.game.positive_pct",
                &[("percent", &percent.to_string())],
            ));
        }
        content = content.push(proton_row(
            i18n.t("stats.avaliacao"),
            text(value).size(11).color(theme::TEXT_STRONG).into(),
        ));
    }

    if stats.reviews.is_empty() {
        content = content.push(hint(
            i18n.t("native.game.reviews_empty"),
            theme::TEXT_3,
        ));
    } else {
        content = content.push(
            text(i18n.t("native.game.recent_reviews"))
                .size(10)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_2),
        );
        for review in stats.reviews.iter().take(4) {
            content = content.push(review_card(i18n, review));
        }
        if stats.reviews.len() > 4 {
            content = content.push(hint(
                i18n.t_args(
                    "native.game.reviews_truncated",
                    &[("count", &stats.reviews.len().to_string())],
                ),
                theme::TEXT_3,
            ));
        }
    }

    content.into()
}

fn review_card<'a>(
    i18n: &'a I18n,
    review: &'a crate::backend::GameReview,
) -> Element<'a, Message> {
    let mut body = column![
        row![
            text(review.author.as_str())
                .size(10)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_STRONG),
            Space::new().width(Fill),
            text(if review.positive {
                format!("★ {}", i18n.t("native.game.recommends"))
            } else {
                format!("☆ {}", i18n.t("native.game.not_recommends"))
            })
            .size(9)
            .color(if review.positive {
                theme::SUCCESS
            } else {
                theme::DANGER
            }),
        ]
        .align_y(iced::Center),
        text(review.text.as_str())
            .size(10)
            .color(theme::TEXT_2)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(3);

    let mut meta = row![].spacing(theme::SPACE_2);
    if review.hours > 0 {
        meta = meta.push(
            text(i18n.t_args(
                "community.horas_jogadas",
                &[("h", &review.hours.to_string())],
            ))
                .size(9)
                .color(theme::TEXT_3),
        );
    }
    if review.helpful > 0 {
        meta = meta.push(
            text(i18n.t_args(
                "native.game.helpful",
                &[("count", &review.helpful.to_string())],
            ))
                .size(9)
                .color(theme::TEXT_3),
        );
    }
    if review.hours > 0 || review.helpful > 0 {
        body = body.push(meta);
    }

    container(body)
        .width(Fill)
        .padding([6.0, 8.0])
        .style(theme::stub_panel)
        .into()
}

// ── Helpers dos painéis ────────────────────────────────────────────────────

fn panel_card<'a>(
    title: &'a str,
    right: Option<Element<'a, Message>>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let mut header = row![text(title)
        .size(13)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1)]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);
    if let Some(right) = right {
        header = header.push(Space::new().width(Fill)).push(right);
    }

    container(column![header, body].spacing(theme::SPACE_3))
        .width(Fill)
        .padding(theme::SPACE_4)
        .style(theme::panel)
        .into()
}

fn hint<'a>(message: impl Into<String>, color: Color) -> Element<'a, Message> {
    text(message.into())
        .size(11)
        .color(color)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn error_body<'a>(
    i18n: &'a I18n,
    title: &'a str,
    error: &'a str,
) -> Element<'a, Message> {
    container(
        column![
            text(title).size(11).color(theme::DANGER),
            text(shorten(error, 220))
                .size(10)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
            button(
                text(i18n.t("downloads.tentar_novamente"))
                    .size(10)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::ReloadGameData)
                .padding([5.0, 10.0])
                .style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_2),
    )
    .width(Fill)
    .into()
}

fn format_count(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index) % 3 == 0 {
            out.push('.');
        }
        out.push(character);
    }
    out
}

fn placeholder_card<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(message).size(11).color(theme::TEXT_3),
        ]
        .spacing(4),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

/// Card "Trailer": frame RGBA do GStreamer, play/pause, seek e estado.
fn trailer_card<'a>(
    i18n: &'a I18n,
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
    let surface: Element<'a, Message> = match media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Contain)
            .into(),
        None => missing_frame(i18n, media, game_media),
    };

    let surface = container(surface)
        .width(Fill)
        .height(180)
        .style(theme::cover_placeholder);

    let duration = media.duration_seconds();
    let position = media.position_seconds();
    let label = if media.is_playing() {
        i18n.t("native.game.pause")
    } else {
        i18n.t("native.game.play")
    };
    let mut toggle = button(text(label).size(11).font(theme::FONT_MEDIUM))
        .padding([6, 12])
        .style(theme::ghost_button);
    if media.can_control() {
        toggle = toggle.on_press(Message::MediaTogglePlayback);
    }

    let mut controls = row![toggle].spacing(theme::SPACE_2).align_y(iced::Center);
    if duration > 0.5 {
        controls = controls.push(
            slider(0.0..=duration as f32, position as f32, |seconds| {
                Message::MediaSeek(f64::from(seconds))
            })
            .step(0.1_f32)
            .width(Fill),
        );
    } else {
        controls = controls.push(
            text(i18n.t("native.game.awaiting_duration"))
                .size(9)
                .color(theme::TEXT_3),
        );
    }

    let status_color = if media.status().is_error() {
        theme::DANGER
    } else {
        theme::TEXT_3
    };

    let timing = media
        .first_frame_ms()
        .map(|elapsed| {
            i18n.t_args("native.game.first_frame", &[("ms", &elapsed.to_string())])
        })
        .unwrap_or_default();

    let body = column![
        row![
            text(i18n.t("gameoverview.trailer"))
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(source_label(i18n, media.uri()))
                .size(9)
                .color(theme::TEXT_3),
            Space::new().width(Fill),
            button(
                text(i18n.t("trailer.escolher"))
                    .size(10)
                    .font(theme::FONT_MEDIUM),
            )
                .on_press(Message::OpenTrailerPicker)
                .padding([5.0, 10.0])
                .style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
        surface,
        controls,
        row![
            text(format!(
                "{} / {}{timing}",
                media::format_seconds(position),
                media::format_seconds(duration)
            ))
            .size(10)
            .color(theme::TEXT_2),
            Space::new().width(Fill),
            text(short_status(i18n, media.status()))
                .size(10)
                .color(status_color),
        ]
        .align_y(iced::Center),
        text(i18n.t("native.game.audio_note"))
            .size(9)
            .color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_2);

    // Com o último frame ainda na tela, o detalhe do erro precisa de uma linha
    // própria (a área do vídeo só mostra o placeholder quando não há frame).
    let body = match (media.texture().is_some(), media.status()) {
        (true, MediaStatus::Error(message)) => body.push(
            text(shorten(message, 180))
                .size(9)
                .color(theme::DANGER),
        ),
        _ => body,
    };

    container(body)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::stub_panel)
        .into()
}

/// Área do vídeo antes do primeiro frame: carregando, erro, `info:null` etc.
fn missing_frame<'a>(
    i18n: &'a I18n,
    media: &'a MediaPlayer,
    game_media: Option<&'a GameMedia>,
) -> Element<'a, Message> {
    let (title, detail, color) = if let MediaStatus::Error(message) = media.status() {
        (
            shorten(message, 180),
            i18n.t("native.game.trailer_open_error").to_string(),
            theme::DANGER,
        )
    } else if let Some(notice) = game_media.and_then(|info| info.notice.as_deref()) {
        (
            shorten(notice, 140),
            i18n.t("native.game.no_trailer_played").to_string(),
            theme::WARNING,
        )
    } else if game_media.is_none() {
        (
            i18n.t("native.game.querying_trailer").to_string(),
            i18n.t("native.game.sysinfo_pipeline").to_string(),
            theme::TEXT_2,
        )
    } else {
        (
            short_status(i18n, media.status()).to_string(),
            i18n.t("native.game.awaiting_frame").to_string(),
            theme::TEXT_2,
        )
    };

    container(
        column![
            text(title).size(11).color(color),
            text(detail).size(9).color(theme::TEXT_3),
        ]
        .spacing(4)
        .align_x(iced::Center),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .padding(theme::SPACE_2)
    .into()
}

/// Rótulo curto na linha de controles; o erro completo fica na área do vídeo.
fn short_status<'a>(i18n: &'a I18n, status: &MediaStatus) -> &'a str {
    match status {
        MediaStatus::Idle => i18n.t("native.game.media.idle"),
        MediaStatus::Loading => i18n.t("native.game.media.loading"),
        MediaStatus::Playing => i18n.t("native.game.media.playing"),
        MediaStatus::Paused => i18n.t("native.game.media.paused"),
        MediaStatus::Ended => i18n.t("native.game.media.ended"),
        MediaStatus::Error(_) => i18n.t("native.game.media.error"),
    }
}

/// Origem escolhida pelo wrapper (`hls` tem prioridade sobre `mp4`/`webm`).
fn source_label<'a>(i18n: &'a I18n, uri: Option<&str>) -> &'a str {
    match uri {
        Some(uri) if uri.starts_with("file://") || !uri.contains("://") => {
            i18n.t("native.game.source.local")
        }
        Some(uri) if uri.to_ascii_lowercase().contains(".m3u8") => {
            i18n.t("native.game.source.hls")
        }
        Some(_) => i18n.t("native.game.source.file"),
        None => i18n.t("native.game.source.gstreamer"),
    }
}

// ── Seletor de trailers (overlay) ──────────────────────────────────────────

/// Véu + cartão central do seletor; o player da ficha continua montado atrás.
fn trailer_picker_overlay<'a>(
    i18n: &'a I18n,
    picker: &'a TrailerPickerState,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let header = row![
        column![
            text(i18n.t("trailer.escolher"))
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t_args(
                "native.game.youtube_subtitle",
                &[("title", &picker.game_title)],
            ))
            .size(11)
            .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        button(
            text(i18n.t("common.fechar"))
                .size(12)
                .font(theme::FONT_MEDIUM),
        )
            .on_press(Message::CloseTrailerPicker)
            .padding([7.0, 14.0])
            .style(theme::ghost_button),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    let search = row![
        text_input(
            i18n.t("native.game.trailer_search_placeholder"),
            picker.query.as_str(),
        )
            .on_input(Message::TrailerQueryChanged)
            .on_submit(Message::TrailerSearchSubmitted)
            .padding([9.0, 12.0])
            .size(12)
            .width(Fill)
            .style(theme::search_input),
        button(
            text(i18n.t("store.buscar"))
                .size(12)
                .font(theme::FONT_MEDIUM),
        )
        .on_press(Message::TrailerSearchSubmitted)
            .padding([9.0, 16.0])
            .style(theme::primary_button),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    let mut content = column![header, search].spacing(theme::SPACE_4);
    if let Some(path) = picker.existing_path.as_deref() {
        content = content.push(existing_trailer_panel(i18n, path));
    }
    if let Some(error) = picker.download_error.as_deref() {
        content = content.push(super::action_error_banner(error, None));
    }
    if let Some(video) = picker.downloading.as_ref() {
        content = content.push(downloading_panel(i18n, video, covers));
    } else {
        content = content.push(results_panel(i18n, picker, covers));
    }

    let card = container(content)
        .width(Fill)
        .max_width(840.0)
        .max_height(700.0)
        .padding(theme::SPACE_5)
        .style(theme::panel);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::scrim),
        )
        .push(opaque(
            container(card)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .padding(theme::SPACE_5),
        ))
        .into()
}

fn placeholder_panel<'a>(message: &'a str, color: iced::Color) -> Element<'a, Message> {
    container(text(message).size(12).color(color))
        .width(Fill)
        .height(320)
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn results_panel<'a>(
    i18n: &'a I18n,
    picker: &'a TrailerPickerState,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    match &picker.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            placeholder_panel(i18n.t("trailer.buscando_youtube"), theme::TEXT_2)
        }
        LoadStatus::Error(error) => container(
            column![
                text(i18n.t("trailer.falha_busca"))
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::DANGER),
                text(shorten(error, 260)).size(11).color(theme::TEXT_2),
                button(
                    text(i18n.t("downloads.tentar_novamente"))
                        .size(12)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::TrailerSearchSubmitted)
                    .padding([7.0, 13.0])
                    .style(theme::ghost_button),
            ]
            .spacing(theme::SPACE_2)
            .align_x(iced::Center),
        )
        .width(Fill)
        .height(320)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
        LoadStatus::Ready if picker.results.is_empty() => {
            placeholder_panel(i18n.t("trailer.nenhum_video"), theme::TEXT_2)
        }
        LoadStatus::Ready => scrollable(
            column(
                picker
                    .results
                    .iter()
                    .map(|result| trailer_result_row(i18n, result, covers))
                    .collect::<Vec<_>>(),
            )
            .spacing(theme::SPACE_2)
            .width(Fill),
        )
        .height(320)
        .style(theme::scrollable)
        .into(),
    }
}

fn trailer_thumbnail<'a>(
    i18n: &'a I18n,
    video: &'a TrailerResult,
    covers: &'a CoverCache,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    match covers.ready(video.thumbnail.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(width)
            .height(height)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(
            text(if video.thumbnail.is_some() {
                i18n.t("native.game.thumbnail_loading")
            } else {
                i18n.t("native.game.no_preview")
            })
            .size(9)
            .color(theme::TEXT_3),
        )
        .width(width)
        .height(height)
        .center_x(Fill)
        .center_y(Fill)
        .style(theme::cover_placeholder)
        .into(),
    }
}

fn trailer_result_row<'a>(
    i18n: &'a I18n,
    video: &'a TrailerResult,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let channel = if video.channel.trim().is_empty() {
        i18n.t("native.game.channel_unknown")
    } else {
        video.channel.as_str()
    };
    let mut meta = row![text(channel).size(10).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    button(
        row![
            trailer_thumbnail(i18n, video, covers, 160.0, 90.0),
            column![
                text(video.title.as_str())
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(i18n.t("native.game.trailer_click_note"))
                    .size(9)
                    .color(theme::TEXT_3),
            ]
            .spacing(5)
            .width(Fill),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .on_press(Message::TrailerDownloadRequested(video.clone()))
    .width(Fill)
    .padding(theme::SPACE_2)
    .style(theme::card)
    .into()
}

fn downloading_panel<'a>(
    i18n: &'a I18n,
    video: &'a TrailerResult,
    covers: &'a CoverCache,
) -> Element<'a, Message> {
    let channel = if video.channel.trim().is_empty() {
        i18n.t("native.game.channel_unknown")
    } else {
        video.channel.as_str()
    };
    let mut meta = row![text(channel).size(10).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(10)
                .color(theme::TEXT_3),
        );
    }

    container(
        row![
            trailer_thumbnail(i18n, video, covers, 200.0, 112.0),
            column![
                text(i18n.t("common.baixando"))
                    .size(14)
                    .font(theme::FONT_BOLD)
                    .color(theme::ACCENT),
                text(video.title.as_str())
                    .size(13)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(i18n.t("native.game.ytdlp_note"))
                    .size(9)
                    .color(theme::TEXT_3),
            ]
            .spacing(5)
            .width(Fill),
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

fn existing_trailer_panel<'a>(i18n: &'a I18n, path: &'a str) -> Element<'a, Message> {
    container(
        row![
            column![
                text(i18n.t("native.game.downloaded_trailer"))
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text(shorten(path, 110)).size(9).color(theme::TEXT_3),
            ]
            .spacing(3)
            .width(Fill),
            button(
                text(i18n.t("native.game.open_downloaded"))
                    .size(11)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::TrailerOpenDownloaded)
            .padding([7.0, 12.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

fn shorten(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}
