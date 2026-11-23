//! Página do jogo: hero/backdrop, descrição rica, galeria de screenshots,
//! trailer GStreamer e painéis de conquistas, ProtonDB e stats/reviews.

use iced::widget::text::{Rich, Span, Wrapping};
use iced::widget::{
    button, column, container, image, opaque, pick_list, progress_bar, row, scrollable, text,
    text_input, tooltip, Space, Stack,
};
use iced::widget::image::Handle;
use iced::{Color, ContentFit, Element, Fill, Length, Padding};

use crate::app::{
    GameLaunchState, GamePanels, LaunchOptionsState, LoadStatus, Message, RequirementsTab,
    TrailerPickerState,
};
use crate::backend::{
    Achievement, EnvVarSetting, Game, GameMedia, GameSettingsView, GameStats, LaunchToggle,
    TrailerResult,
};
use crate::covers::CoverCache;
use crate::i18n::I18n;
use crate::input;
use crate::media::{self, MediaPlayer};
use crate::picker::PickPurpose;
use crate::rich_text::{parse_description, RichBlock, RichSpan};
use crate::theme;

use super::icons::{self, Icon};

#[path = "gallery_motion.rs"]
pub(super) mod gallery_motion;

// ── Tipografia da descrição ────────────────────────────────────────────────
//
// A `about` é texto longo de leitura contínua: precisa de corpo maior e
// entrelinha mais aberta que o resto da UI (que é densa por natureza).
// Títulos de seção do HTML da loja (`<h1>`..`<h3>`) ganham peso e corpo
// próprios, e os itens de lista um recuo com marcador.

/// Corpo do texto corrido.
const DESCRIPTION_SIZE: f32 = 14.5;
const DESCRIPTION_LINE_HEIGHT: f32 = 1.62;
/// Título de seção dentro da descrição (`<h1>`-`<h3>` do `about`).
const DESCRIPTION_HEADING_SIZE: f32 = 17.0;
/// Espaçamento entre parágrafos.
const DESCRIPTION_BLOCK_GAP: f32 = 11.0;
/// Recuo dos itens de lista, alinhando o texto depois do marcador.
const DESCRIPTION_BULLET_INDENT: f32 = 14.0;
/// Cor do corpo: um degrau acima do `TEXT_STRONG` para leitura longa.
const DESCRIPTION_TEXT: Color = Color::from_rgb8(0xd6, 0xdd, 0xe8);

pub fn view<'a>(
    game: &'a Game,
    covers: &'a CoverCache,
    media: &'a MediaPlayer,
    animated: &'a crate::animated_cards::AnimatedCards,
    game_media: Option<&'a GameMedia>,
    panels: Option<&'a GamePanels>,
    community: Option<&'a super::community::CommunityState>,
    selected_screenshot: usize,
    gallery_trailer: bool,
    requirements_tab: RequirementsTab,
    launch: &'a GameLaunchState,
    trailer_picker: &'a TrailerPickerState,
    launch_options: &'a LaunchOptionsState,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    let mut chips = vec![super::game_detail::chip(
        game.playtime_label_i18n(i18n),
        false,
    )];
    if let Some(year) = game.year.as_deref().filter(|year| !year.trim().is_empty()) {
        chips.push(super::game_detail::chip(year.to_string(), false));
    }
    if let Some(developer) = game
        .developer
        .as_deref()
        .or(game.publisher.as_deref())
        .filter(|name| !name.trim().is_empty())
    {
        chips.push(super::game_detail::chip(developer.to_string(), false));
    }
    if game.installed {
        chips.push(super::game_detail::chip(
            i18n.t("native.game.installed").to_string(),
            true,
        ));
    }

    let description = description_panel(i18n, game, game_media, covers, animated);
    // A galeria não tem cabeçalho: nem rótulos técnicos, nem barra de accent,
    // nem botão de recarregar. A troca de trailer acontece pelas miniaturas.
    let gallery_actions: Vec<Element<'a, Message>> = Vec::new();

    let movies = game_media
        .map(|media| media.movies.as_slice())
        .unwrap_or_default();
    let screenshots = game_media
        .map(|media| media.screenshots.as_slice())
        .unwrap_or_default();
    let gallery = super::game_detail::gallery_panel(
        i18n,
        media,
        covers,
        movies,
        screenshots,
        selected_screenshot,
        gallery_trailer,
        Message::SelectTrailer,
        Message::SelectScreenshot,
        gallery_actions,
    );

    let showing_video = gallery_trailer && (!movies.is_empty() || media.is_active());
    let current = if showing_video { None } else {
        screenshots.get(selected_screenshot.min(screenshots.len().saturating_sub(1)))
            .and_then(|screenshot| covers.ready(screenshot.full_or_thumb())).cloned()
    };
    let gallery = gallery_motion::wrap(gallery, game.id.to_string(), current);

    let mut left = Vec::new();
    if let Some(meta) = library_meta_line(game) {
        left.push(
            container(text(meta).size(theme::TEXT_READ).color(theme::TEXT_STRONG))
                .width(Fill)
                .padding(theme::SPACE_4)
                .style(theme::panel)
                .into(),
        );
    }
    left.push(gallery);
    left.push(description);
    if let Some(community) = community {
        left.push(super::community::panel(
            i18n,
            game.title.as_str(),
            community,
        ));
    }

    let mut gamepad = input::GamepadInput::new();
    let gamepad_state = gamepad.poll();
    let controller_support = game_media.and_then(|media| media.controller_support.as_deref());
    let controller_body = match controller_support {
        Some(support) if support.eq_ignore_ascii_case("full") => {
            i18n.t("native.game.controller_full")
        }
        Some(support) if support.eq_ignore_ascii_case("partial") => {
            i18n.t("native.game.controller_partial")
        }
        _ if gamepad_state.connected => i18n.t("native.game.controller_connected"),
        _ => i18n.t("native.game.controller_placeholder"),
    };

    let mut right = vec![
        protondb_panel(i18n, panels),
        stats_reviews_panel(i18n, panels),
        achievements_panel(i18n, panels, covers),
    ];
    if let Some(requirements) = super::game_detail::requirements_panel(
        i18n,
        game_media.and_then(|media| media.req_min.as_deref()),
        game_media.and_then(|media| media.req_rec.as_deref()),
        requirements_tab,
    ) {
        right.push(requirements);
    }
    if let Some(languages) = super::game_detail::languages_panel(
        i18n,
        game_media.and_then(|media| media.languages.as_deref()),
    ) {
        right.push(languages);
    }
    right.push(placeholder_card(
        i18n.t("native.game.controller"),
        controller_body,
    ));

    let page = super::game_detail::view(
        380.0,
        covers,
        game.hero_url.clone().or_else(|| game.cover_url.clone()),
        game.cover_url.clone(),
        game.title.as_str(),
        chips,
        Message::BackToLibrary,
        Some(launch_controls(game, launch, i18n)),
        left,
        right,
    );
    let mut layers = Stack::new().width(Fill).height(Fill).push(page);
    if trailer_picker.open {
        layers = layers.push(trailer_picker_overlay(i18n, trailer_picker, covers));
    }
    if launch_options.open && launch_options.game_id == game.id {
        layers = layers.push(launch_options_overlay(i18n, launch_options, game));
    }
    layers.into()
}

fn library_meta_line(game: &Game) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(year) = game
        .year
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(year);
    }
    if let Some(studio) = game
        .developer
        .as_deref()
        .or(game.publisher.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(studio);
    }
    if let Some(genre) = game
        .genre
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(genre);
    }
    (!parts.is_empty()).then(|| parts.join("  ·  "))
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

    let play = button(
        text(label)
            .size(theme::TEXT_READ)
            .font(theme::FONT_SEMIBOLD),
    )
    .on_press_maybe(message)
    .padding([10.0, 22.0])
    .style(play_style);

    let options = button(
        text(i18n.t("native.game.launch_options"))
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::OpenLaunchOptions(game.id.clone()))
    .padding([10.0, 14.0])
    .style(theme::ghost_button);

    let status: Element<'a, Message> = if running {
        text(i18n.t("native.game.running_note"))
            .size(theme::TEXT_CAPTION)
            .color(theme::SUCCESS)
            .into()
    } else if launch.is_launching(&game.id) {
        text(i18n.t("native.game.awaiting_process"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .into()
    } else if launch.is_stopping(&game.id) {
        text(i18n.t("native.game.stopping_process"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .into()
    } else if launch.busy_with_other(&game.id) {
        text(i18n.t("native.game.close_other"))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_3)
            .into()
    } else {
        Space::new().width(0).into()
    };

    row![play, options, status]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center)
        .into()
}

// ── Opções de inicialização (overlay) ──────────────────────────────────────

/// Banner de aviso com texto próprio: `warning_banner` recebe `&str` e o texto
/// é montado em runtime (`warnings.join`), então a versão local guarda a
/// `String` dentro do widget.
pub(crate) fn launch_warning_banner(message: String) -> Element<'static, Message> {
    container(
        text(message)
            .size(theme::TEXT_BODY)
            .color(theme::WARNING)
            .width(Fill),
    )
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
            text(i18n.t("native.game.launch_how"))
                .size(theme::TEXT_HEADING)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(state.game_title.as_str())
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2),
        ]
        .spacing(2)
        .width(Fill),
        tooltip(
            button(
                text("✕")
                    .size(16)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_2),
            )
            .on_press(Message::CloseLaunchOptions)
            .padding([5.0, 9.0])
            .style(theme::launch_icon_button),
            container(
                text(i18n.t("native.game.close_options"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_1),
            )
            .padding([4.0, 8.0])
            .style(theme::launch_tooltip),
            tooltip::Position::Bottom,
        ),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    // O diálogo compacto mede o conteúdo em vez de esticar até 640 px: sem
    // campos extras, os cards não podem deixar um bloco vazio na janela alta.
    let compact = match (&state.status, state.settings.as_ref()) {
        (LoadStatus::Ready, Some(settings)) => {
            state.error.is_none() && launch_basic_is_compact(state, settings, game)
        }
        _ => false,
    };

    let mut content = column![
        header,
        container(Space::new().width(Fill).height(1)).style(theme::edit_hairline),
    ]
    .spacing(theme::SPACE_3)
    .height(if compact {
        Length::Shrink
    } else {
        Length::Fill
    });

    if let Some(error) = state.error.as_deref() {
        content = content.push(super::action_error_banner(error, None));
    }

    content = match (&state.status, state.settings.as_ref()) {
        (LoadStatus::Idle | LoadStatus::Loading, _) => content.push(
            launch_options_placeholder(i18n.t("native.game.options_loading"), theme::TEXT_2),
        ),
        (LoadStatus::Error(error), _) => {
            content.push(launch_options_load_error(i18n, error, &game.id))
        }
        (LoadStatus::Ready, Some(settings)) => content
            .push(launch_options_form(
                i18n, state, settings, game, compact,
            ))
            .push(launch_options_footer(i18n, state)),
        (LoadStatus::Ready, None) => content.push(launch_options_placeholder(
            i18n.t("native.game.options_unavailable"),
            theme::TEXT_3,
        )),
    };

    let card = container(content)
        .width(Fill)
        .height(if compact {
            Length::Shrink
        } else {
            Length::Fill
        })
        .max_width(820.0)
        .max_height(if compact { 440.0 } else { 640.0 })
        .padding(theme::SPACE_5)
        .style(theme::launch_dialog);

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

fn launch_options_placeholder<'a>(
    message: &'a str,
    color: Color,
) -> Element<'a, Message> {
    container(text(message).size(theme::TEXT_READ).color(color))
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
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
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::DANGER),
            text(shorten(error, 280))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2),
            button(
                text(i18n.t("downloads.tentar_novamente"))
                    .size(theme::TEXT_BODY)
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
    .height(Fill)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .into()
}

/// Básico sem emulador, caminho de exe ou argumentos visíveis: cabe em um
/// cartão compacto. Ajustes avançados abertos ou campos extras mantêm o
/// diálogo alto com rolagem interna.
fn launch_basic_is_compact(
    state: &LaunchOptionsState,
    settings: &GameSettingsView,
    game: &Game,
) -> bool {
    if state.advanced_open {
        return false;
    }
    let mode = state.mode.as_deref();
    let has_emulator = settings
        .emulator_id
        .as_deref()
        .is_some_and(|id| !id.trim().is_empty());
    let exe_in_basic = mode == Some("exe");
    let args_in_basic = mode != Some("steam") && (mode == Some("exe") || game.launcher != "steam");
    !has_emulator && !exe_in_basic && !args_in_basic
}

fn launch_options_form<'a>(
    i18n: &'a I18n,
    state: &'a LaunchOptionsState,
    settings: &'a GameSettingsView,
    game: &'a Game,
    compact: bool,
) -> Element<'a, Message> {
    let mode = state.mode.as_deref();
    let steam_available = game.launcher == "steam";
    let exe_available = !settings.exe_path.trim().is_empty();
    let emulator_in_basic = settings
        .emulator_id
        .as_deref()
        .is_some_and(|id| !id.trim().is_empty());
    let args_in_basic = mode != Some("steam") && (mode == Some("exe") || game.launcher != "steam");
    let exe_in_basic = mode == Some("exe");

    let mode_cards = row![
        launch_mode_card(
            i18n.t("native.game.mode_auto"),
            i18n.t("native.game.mode_auto_desc"),
            mode.is_none(),
            true,
            Some(i18n.t("native.game.mode_recommended")),
            Some(Message::LaunchOptionsModeChanged(None)),
        ),
        launch_mode_card(
            i18n.t("gamecard.steam"),
            if steam_available {
                i18n.t("native.game.mode_steam_desc")
            } else {
                i18n.t("native.game.mode_steam_locked")
            },
            mode == Some("steam"),
            steam_available,
            None,
            steam_available
                .then_some(Message::LaunchOptionsModeChanged(Some("steam".to_string()))),
        ),
        launch_mode_card(
            i18n.t("gamesettings.executavel"),
            if exe_available {
                i18n.t("native.game.mode_exe_desc")
            } else {
                i18n.t("native.game.mode_exe_locked")
            },
            mode == Some("exe"),
            exe_available,
            None,
            exe_available.then_some(Message::LaunchOptionsModeChanged(Some("exe".to_string()))),
        ),
    ]
    .spacing(theme::SPACE_3)
    .width(Fill);

    let summary_key = match mode {
        None => "native.game.mode_summary_auto",
        Some("steam") => "native.game.mode_summary_steam",
        _ => "native.game.mode_summary_exe",
    };
    let summary = row![
        text("●").size(10).color(theme::ACCENT),
        text(i18n.t(summary_key))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    let mut basic = column![mode_cards, summary].spacing(theme::SPACE_3);

    if exe_in_basic || args_in_basic {
        let exe_field = || {
            launch_field(
                i18n.t("native.game.exe_label"),
                i18n.t("native.game.exe_placeholder"),
                settings.exe_path.as_str(),
                Message::LaunchOptionsExeChanged,
            )
        };
        let args_field = || {
            launch_field(
                i18n.t("native.game.extra_args"),
                i18n.t("native.game.args_placeholder"),
                settings.game_args.as_str(),
                Message::LaunchOptionsArgsChanged,
            )
        };
        let fields: Element<'a, Message> = if exe_in_basic && args_in_basic {
            row![exe_field(), args_field()]
                .spacing(theme::SPACE_3)
                .width(Fill)
                .into()
        } else if exe_in_basic {
            exe_field()
        } else {
            args_field()
        };
        basic = basic.push(
            container(fields)
                .width(Fill)
                .padding(theme::SPACE_3)
                .style(theme::edit_subcard),
        );
    }

    if emulator_in_basic {
        basic = basic.push(launch_emulator_section(i18n, state, settings));
    }

    basic = basic.push(launch_advanced_toggle(i18n, state.advanced_open));

    let mut form = column![basic].spacing(theme::SPACE_3);

    if state.advanced_open {
        let mut advanced = column![].spacing(theme::SPACE_3);

        let mut path_fields = column![].spacing(theme::SPACE_2);
        if !exe_in_basic {
            path_fields = path_fields.push(launch_field(
                i18n.t("native.game.exe_label"),
                i18n.t("native.game.exe_placeholder"),
                settings.exe_path.as_str(),
                Message::LaunchOptionsExeChanged,
            ));
        }
        if !args_in_basic {
            path_fields = path_fields
                .push(launch_field(
                    i18n.t("native.game.extra_args"),
                    i18n.t("native.game.args_placeholder"),
                    settings.game_args.as_str(),
                    Message::LaunchOptionsArgsChanged,
                ))
                .push(
                    text(i18n.t("native.game.args_hint"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_3),
                );
        }
        if !exe_in_basic || !args_in_basic {
            advanced = advanced.push(option_section(
                i18n.t("native.game.exe_section"),
                path_fields.into(),
            ));
        }

        let default_prefix = if state.default_prefix.is_empty() {
            i18n.t("native.game.default_prefix").to_string()
        } else {
            state.default_prefix.clone()
        };
        advanced = advanced.push(option_section(
            i18n.t("native.game.wine_section"),
            column![
                field_label(i18n.t("native.game.prefix_label")),
                text_input(
                    i18n.t("native.game.prefix_placeholder"),
                    settings.prefix_path.as_str(),
                )
                .on_input(Message::LaunchOptionsPrefixChanged)
                .padding([9.0, 12.0])
                .size(theme::TEXT_BODY)
                .style(theme::launch_input),
                text(i18n.t_args("native.game.prefix_hint", &[("prefix", &default_prefix)],))
                    .size(theme::TEXT_MICRO)
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
        ));

        advanced = advanced.push(option_section(
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
        ));

        advanced = advanced.push(option_section(
            i18n.t("native.game.performance_section"),
            column![
                toggle_row(&[
                    ("MangoHud", LaunchToggle::Mangohud, settings.mangohud),
                    ("GameMode", LaunchToggle::Gamemode, settings.gamemode),
                    ("Gamescope", LaunchToggle::Gamescope, settings.gamescope),
                ]),
                toggle_row(&[
                    ("HDR (gamescope)", LaunchToggle::GsHdr, settings.gs_hdr),
                    (
                        i18n.t("native.game.verbose"),
                        LaunchToggle::VerboseLogs,
                        settings.verbose_logs
                    ),
                ]),
                field_label(i18n.t("native.game.dxvk_hud")),
                text_input(
                    i18n.t("native.game.hud_placeholder"),
                    settings.dxvk_hud.as_str(),
                )
                .on_input(Message::LaunchOptionsHudChanged)
                .padding([9.0, 12.0])
                .size(theme::TEXT_BODY)
                .style(theme::launch_input),
                text(i18n.t("native.game.performance_note"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            ]
            .spacing(theme::SPACE_2)
            .into(),
        ));

        advanced = advanced.push(launch_env_section(i18n, state, settings));

        if !emulator_in_basic {
            advanced = advanced.push(launch_emulator_section(i18n, state, settings));
        }

        advanced = advanced.push(option_section(
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
        ));

        form = form.push(advanced);
    }

    if compact {
        form.into()
    } else {
        scrollable(form)
            .height(Fill)
            .style(theme::scrollable)
            .into()
    }
}

fn launch_advanced_toggle<'a>(i18n: &'a I18n, open: bool) -> Element<'a, Message> {
    button(
        row![
            icons::icon(Icon::Gear, 16.0, 0.62),
            column![
                text(i18n.t("native.game.advanced_title"))
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_SEMIBOLD)
                    .color(theme::TEXT_1),
                text(i18n.t("native.game.advanced_hint"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            ]
            .spacing(1)
            .width(Fill),
            text(if open { "▴" } else { "▾" })
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_2),
        ]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center),
    )
    .on_press(Message::LaunchOptionsAdvancedToggled)
    .width(Fill)
    .padding([10.0, 14.0])
    .style(move |theme, status| theme::launch_advanced_toggle(theme, status, open))
    .into()
}

fn launch_mode_card<'a>(
    title: &'a str,
    description: &'a str,
    active: bool,
    enabled: bool,
    recommended: Option<&'a str>,
    message: Option<Message>,
) -> Element<'a, Message> {
    let title_color = if active {
        theme::TEXT_1
    } else if enabled {
        theme::TEXT_STRONG
    } else {
        theme::TEXT_3
    };
    let description_color = if enabled {
        theme::TEXT_2
    } else {
        theme::TEXT_3
    };

    let heading = row![
        text(if active { "●" } else { "○" })
            .size(10)
            .color(if active { theme::ACCENT } else { theme::TEXT_3 }),
        text(title)
            .size(theme::TEXT_LABEL)
            .font(theme::FONT_SEMIBOLD)
            .color(title_color),
    ]
    .spacing(theme::SPACE_2)
    .align_y(iced::Center);

    // O selo fica em linha própria: em 640 px ele não cabe ao lado do título
    // sem invadir o card vizinho.
    let mut body = column![heading].spacing(4).width(Fill);
    if let Some(label) = recommended {
        body = body.push(
            container(
                text(label)
                    .size(12)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::ACCENT),
            )
            .padding([2.0, 7.0])
            .style(theme::edit_badge),
        );
    }
    body = body.push(
        text(description)
            .size(theme::TEXT_MICRO)
            .color(description_color)
            .wrapping(Wrapping::WordOrGlyph),
    );

    let mut card = button(body)
        .width(Fill)
        .height(112.0)
        .padding([12.0, 14.0])
        .style(move |theme, status| {
            theme::launch_mode_card(theme, status, active, enabled)
        });
    if enabled {
        if let Some(message) = message {
            card = card.on_press(message);
        }
    }
    card.into()
}

fn launch_field<'a>(
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    column![
        field_label(label),
        text_input(placeholder, value)
            .on_input(on_input)
            .padding([9.0, 12.0])
            .size(theme::TEXT_BODY)
            .style(theme::launch_input),
    ]
    .spacing(theme::SPACE_2)
    .width(Fill)
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
    let selected = choices
        .iter()
        .find(|choice| choice.id == selected_id)
        .cloned();
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
            .padding([9.0, 12.0])
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::launch_input)
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
                .padding([9.0, 12.0])
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::launch_input)
                .into(),
                core_purpose,
                state.pick_busy.as_ref(),
                i18n,
                i18n.t("native.game.emulator_pick_core"),
            ));
            body = body.push(
                text(i18n.t("native.game.emulator_core_hint"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            );
            if settings.emulator_core_path.trim().is_empty() {
                body = body.push(
                    text(i18n.t("native.game.emulator_core_missing"))
                        .size(theme::TEXT_MICRO)
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
                    text(i18n.t_args("native.game.emulator_bios_warning", &[("detail", &detail)]))
                        .size(theme::TEXT_MICRO)
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
            .padding([9.0, 12.0])
            .size(theme::TEXT_BODY)
            .style(theme::launch_input),
        );
        body = body.push(
            text(i18n.t("native.game.emulator_args_hint"))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
        body = body.push(
            text(i18n.t_args("native.game.emulator_launch_note", &[("name", &item.name)]))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    } else if settings.emulator_id.is_some() {
        body = body.push(
            text(i18n.t("native.game.emulator_unknown"))
                .size(theme::TEXT_MICRO)
                .color(theme::WARNING),
        );
    } else {
        body = body.push(
            text(i18n.t("native.game.emulator_none_hint"))
                .size(theme::TEXT_MICRO)
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
    let mut pick = button(
        text(label)
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
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
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }
    for (index, variable) in settings.env_vars.iter().enumerate() {
        list = list.push(env_var_row(i18n, variable, index));
    }

    let add = row![
        text_input(
            i18n.t("gamesettings.env_placeholder_nome"),
            state.env_name.as_str()
        )
        .on_input(Message::LaunchOptionsEnvNameChanged)
        .padding([9.0, 12.0])
        .size(theme::TEXT_CAPTION)
        .width(Fill)
        .style(theme::launch_input),
        text_input(i18n.t("native.game.env_value"), state.env_value.as_str())
            .on_input(Message::LaunchOptionsEnvValueChanged)
            .padding([9.0, 12.0])
            .size(theme::TEXT_CAPTION)
            .width(Fill)
            .style(theme::launch_input),
        button(
            text(i18n.t("fontes.adicionar"))
                .size(theme::TEXT_CAPTION)
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
                .size(theme::TEXT_MICRO)
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
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_1)
                .width(Fill),
            text(if variable.value.is_empty() {
                i18n.t("common.fallback")
            } else {
                variable.value.as_str()
            })
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2)
            .width(Fill),
            button(
                text(i18n.t("common.remover"))
                    .size(theme::TEXT_MICRO)
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
    .padding([6.0, 9.0])
    .style(theme::edit_subcard)
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
            i18n.t("native.game.save_changes")
        })
        .size(theme::TEXT_BODY)
        .font(theme::FONT_SEMIBOLD),
    )
    .padding([9.0, 18.0])
    .style(theme::launch_primary_button);
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

    column![
        container(Space::new().width(Fill).height(1)).style(theme::edit_hairline),
        row![
            text(note)
                .size(theme::TEXT_MICRO)
                .width(Fill)
                .color(if state.dirty {
                    theme::WARNING
                } else if state.saved {
                    theme::SUCCESS
                } else {
                    theme::TEXT_3
                }),
            button(
                text(i18n.t("common.cancelar"))
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::CloseLaunchOptions)
            .padding([9.0, 16.0])
            .style(theme::launch_secondary_button),
            save,
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Center),
    ]
    .spacing(theme::SPACE_3)
    .into()
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
    button(
        text(format!("{glyph} {label}"))
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::LaunchOptionsToggle { key, value: !value })
    .padding([7.0, 12.0])
    .style(move |theme, status| {
        theme::launch_toggle_button(theme, status, value)
    })
    .into()
}

fn option_section<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_STRONG),
            body,
        ]
        .spacing(theme::SPACE_3),
    )
    .width(Fill)
    .padding(theme::SPACE_4)
    .style(theme::edit_card)
    .into()
}

fn field_label<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(theme::TEXT_CAPTION)
        .color(theme::TEXT_2)
        .into()
}

fn disabled_option<'a>(label: &'a str, value: String, reason: &'a str) -> Element<'a, Message> {
    container(
        column![
            row![
                text(label)
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_2),
                Space::new().width(Fill),
                text(value).size(theme::TEXT_MICRO).color(theme::TEXT_3),
            ]
            .align_y(iced::Center),
            text(reason)
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(3),
    )
    .width(Fill)
    .padding([8.0, 10.0])
    .style(theme::edit_subcard)
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
    covers: &'a CoverCache,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let about = game_media.and_then(|media| media.about.as_deref());
    let raw = about.or(game.description.as_deref());

    let body: Element<'a, Message> = match (game_media, raw) {
        (None, None) => hint(i18n.t("native.game.description_loading"), theme::TEXT_2),
        (_, Some(raw)) => {
            let blocks = parse_description(raw);
            if blocks.is_empty() {
                hint(i18n.t("native.game.description_unreadable"), theme::TEXT_3)
            } else {
                description_blocks(blocks, covers, animated)
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

    panel_card(i18n.t("native.game.about"), None, body)
}

pub(super) fn description_blocks<'a>(
    blocks: Vec<RichBlock>,
    covers: &'a CoverCache,
    animated: &'a crate::animated_cards::AnimatedCards,
) -> Element<'a, Message> {
    let mut content = column![].spacing(DESCRIPTION_BLOCK_GAP);
    for block in blocks.into_iter().flat_map(split_block_lines) {
        let mut block = block;

        // Card de seção da Steam: `<img>` e `<video poster>` do `about` viram
        // figura na largura do painel. Enquanto o handle não chega, o espaço
        // fica reservado para o texto não pular de lugar.
        if let Some(url) = block.image.clone() {
            // Card animado: o `about` da Steam toca o `<video>` em loop. Se o
            // pool já tem um frame, ele vence o poster estático — que em
            // vários jogos é preto.
            let frame = if block.video.is_empty() {
                None
            } else {
                animated.texture(&block.video)
            };
            content = content.push(description_figure(&url, covers, frame));
            continue;
        }

        let bullet = block
            .spans
            .first()
            .map(|span| {
                let trimmed = span.text.trim_start();
                trimmed.starts_with("- ") || trimmed.starts_with("• ") || trimmed.starts_with("* ")
            })
            .unwrap_or(false);

        if bullet {
            if let Some(first) = block.spans.first_mut() {
                let trimmed = first.text.trim_start();
                let rest = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("• "))
                    .or_else(|| trimmed.strip_prefix("* "))
                    .unwrap_or(trimmed);
                first.text = format!("•  {rest}");
            }
            content = content.push(container(rich_block(block)).width(Fill).padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: DESCRIPTION_BULLET_INDENT,
            }));
        } else {
            content = content.push(rich_block(block));
        }
    }
    content.into()
}

/// Figura do `about`: a arte de seção que a Steam mostra entre parágrafos.
///
/// A altura é fixa de propósito — e por isso NÃO se usa `center_x`/`center_y`
/// aqui: no iced esses atalhos chamam `.width(Fill)`/`.height(Fill)` e
/// sobrescrevem a altura fixa, devolvendo um container com altura ilimitada
/// que colapsa a coluna inteira da ficha. O alinhamento é feito com
/// `align_x`/`align_y`, que não mexem na dimensão.
/// Largura de referência para calcular a altura da figura pela proporção.
const DESCRIPTION_FIGURE_WIDTH: f32 = 640.0;
/// Proporção de fallback: banner 1170×450 da Steam (~2.6:1).
const DESCRIPTION_FIGURE_RATIO: f32 = 1170.0 / 450.0;
/// Altura da caixa enquanto a imagem não chegou.
const DESCRIPTION_FIGURE_RATIO_HEIGHT: f32 = DESCRIPTION_FIGURE_WIDTH / DESCRIPTION_FIGURE_RATIO;
/// Piso: evita que uma peça muito larga vire uma tira invisível.
///
/// Baixo de propósito. O piso alto (era 140) era o que deformava os cards
/// 2,67:1 da Steam: a 640 px de largura a altura natural é 240, mas num
/// painel mais estreito a altura correta cai para ~180 e o clamp forçava
/// 140, comprimindo o vídeo.
const DESCRIPTION_FIGURE_MIN_HEIGHT: f32 = 90.0;
/// Teto: uma peça vertical não pode empurrar o resto da ficha para fora.
const DESCRIPTION_FIGURE_MAX_HEIGHT: f32 = 520.0;

fn description_figure<'a>(
    url: &str,
    covers: &'a CoverCache,
    animated_frame: Option<&'a Handle>,
) -> Element<'a, Message> {
    // A altura sai da proporção real da imagem, não de um valor fixo: o
    // `about` mistura banners 1170×450 (cards do Cyberpunk) com peças
    // verticais 616×670 (arte de edição). Com altura fixa, a vertical virava
    // uma tira minúscula e o banner ganhava tarja preta.
    //
    // O frame animado tem prioridade sobre o poster: a página da loja toca o
    // `<video>` do card em loop, e o poster de vários jogos é preto.
    let (body, height): (Element<'a, Message>, f32) = match animated_frame {
        Some(frame) => {
            // A proporção vem do próprio frame animado.
            let (width, height_px) = match frame {
                Handle::Rgba { width, height, .. } => (*width, *height),
                _ => (0, 0),
            };
            let ratio = if width > 0 && height_px > 0 {
                width as f32 / height_px as f32
            } else {
                DESCRIPTION_FIGURE_RATIO
            };
            // A altura é derivada da proporção do frame. `ContentFit::Contain`
            // dentro de um container com altura calculada garante que o vídeo
            // caiba sem esticar, mesmo que a caixa não bata na proporção.
            let height = (DESCRIPTION_FIGURE_WIDTH / ratio)
                .clamp(DESCRIPTION_FIGURE_MIN_HEIGHT, DESCRIPTION_FIGURE_MAX_HEIGHT);
            (
                image(frame.clone())
                    .width(Fill)
                    .height(Fill)
                    .content_fit(ContentFit::Contain)
                    .into(),
                height,
            )
        }
        None => match covers.ready(Some(url)) {
            Some(handle) => {
                let (width, height_px) = covers.size(Some(url)).unwrap_or((0, 0));
                let ratio = if width > 0 && height_px > 0 {
                    width as f32 / height_px as f32
                } else {
                    DESCRIPTION_FIGURE_RATIO
                };
                // Largura de referência do painel de descrição (~640 px); a altura
                // nunca passa do teto para não empurrar o resto da ficha.
                let height = (DESCRIPTION_FIGURE_WIDTH / ratio)
                    .clamp(DESCRIPTION_FIGURE_MIN_HEIGHT, DESCRIPTION_FIGURE_MAX_HEIGHT);
                (
                    image(handle.clone())
                        .width(Fill)
                        .height(Fill)
                        .content_fit(ContentFit::Contain)
                        .into(),
                    height,
                )
            }
            // Sem handle: o download ainda não voltou, ou falhou. Caixa neutra com
            // a proporção de banner em vez de reticências — não parece texto solto.
            None => (
                container(hint("", theme::TEXT_3))
                    .width(Fill)
                    .height(Fill)
                    .style(theme::cover_placeholder)
                    .into(),
                DESCRIPTION_FIGURE_RATIO_HEIGHT,
            ),
        },
    };

    // A caixa do card tem a proporção do frame, e o `image` usa
    // `ContentFit::Contain`: mesmo que a caixa não bata exatamente, o vídeo
    // NUNCA é esticado — o iced letterboxa dentro dela.
    // Sem placa atrás do poster: nada de fundo cinza nem borda. A imagem (ou
    // o vídeo do card) fica solta no texto, com o respiro vertical do padding.
    container(body)
        .width(Fill)
        .height(Length::Fixed(height))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .padding(Padding {
            top: 6.0,
            right: 0.0,
            bottom: 6.0,
            left: 0.0,
        })
        .into()
}

/// O HTML da Steam usa `<br>` para listas e subtítulos. Cada linha vira um
/// bloco próprio para o painel dar ritmo e indentar os itens com “•”.
fn split_block_lines(block: RichBlock) -> Vec<RichBlock> {
    // Bloco de imagem não tem linhas de texto: passa intacto para a UI.
    if block.image.is_some() {
        return vec![block];
    }

    let mut lines: Vec<RichBlock> = Vec::new();
    let mut current = RichBlock::default();

    for span in block.spans {
        let parts: Vec<&str> = span.text.split('\n').collect();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 && !current.spans.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if part.is_empty() {
                continue;
            }
            current.spans.push(RichSpan {
                text: (*part).to_string(),
                bold: span.bold,
                link: span.link,
            });
        }
    }
    if !current.spans.is_empty() {
        lines.push(current);
    }
    lines
}

fn rich_block(block: RichBlock) -> Element<'static, Message> {
    // Cada `Span` precisa da família e do tamanho explícitos: o default do
    // `iced_core::text::Span` é a fonte do sistema (não a Inter carregada no
    // `main.rs`), e o texto sai com a métrica de fallback — era o que deixava
    // a descrição com aparência "genérica" ao lado do resto da UI.
    //
    // O bloco de título (`<h1>`-`<h6>` do `about`) recebe corpo maior e peso
    // semibold: a loja usa h1 para "About the Game" e para o nome da edição,
    // e sem hierarquia tudo virava um bloco de texto do mesmo tamanho.
    let heading = block.heading;
    let (size, family, color) = if heading {
        (
            DESCRIPTION_HEADING_SIZE,
            theme::FONT_SEMIBOLD,
            theme::TEXT_STRONG,
        )
    } else {
        (DESCRIPTION_SIZE, theme::FONT, DESCRIPTION_TEXT)
    };

    let spans: Vec<Span<'static, (), iced::Font>> = block
        .spans
        .into_iter()
        .map(|span| {
            let mut out = Span::new(span.text)
                .font(family)
                .size(size)
                .color(if span.link { theme::ACCENT } else { color });
            if span.bold && !heading {
                out = out.font(theme::FONT_MEDIUM);
            }
            if span.link {
                out = out.underline(true);
            }
            out
        })
        .collect();

    Rich::<(), Message>::with_spans(spans)
        .size(size)
        .line_height(DESCRIPTION_LINE_HEIGHT)
        .width(Fill)
        .into()
}

// ── Conquistas ─────────────────────────────────────────────────────────────

pub(crate) fn achievements_panel<'a>(
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
            .size(theme::TEXT_MICRO)
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
        None => container(text("?").size(theme::TEXT_READ).color(theme::TEXT_3))
            .width(42)
            .height(42)
            .align_x(iced::Center)
            .align_y(iced::Center)
            .style(theme::cover_placeholder)
            .into(),
    };

    let status = if item.unlocked {
        text(i18n.t("native.game.achievement_unlocked"))
            .size(theme::TEXT_MICRO)
            .color(theme::SUCCESS)
    } else {
        text(i18n.t("native.game.achievement_locked"))
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3)
    };
    let mut meta = row![status].spacing(theme::SPACE_2);
    if let Some(percent) = item.percent {
        meta = meta.push(
            text(i18n.t_args(
                "native.game.achievement_players",
                &[("percent", &percent.to_string())],
            ))
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
        );
    }
    if let Some(points) = item.points {
        meta = meta.push(
            text(i18n.t_args("native.game.points", &[("points", &points.to_string())]))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }

    let mut info = column![text(item.title.as_str())
        .size(theme::TEXT_BODY)
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
        .wrapping(Wrapping::WordOrGlyph),]
    .spacing(2)
    .width(Fill);

    if let Some(description) = item.description.as_deref() {
        info = info.push(
            text(description)
                .size(theme::TEXT_MICRO)
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

pub(crate) fn protondb_panel<'a>(
    i18n: &'a I18n,
    panels: Option<&'a GamePanels>,
) -> Element<'a, Message> {
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
        LoadStatus::Error(error) => error_body(i18n, i18n.t("native.game.protondb_error"), error),
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
                            .size(theme::TEXT_CAPTION)
                            .color(theme::TEXT_STRONG)
                            .into(),
                    ));
                }
                if let Some(score) = info.score_pct {
                    rows = rows.push(proton_row(
                        i18n.t("protondb.pontuacao"),
                        text(format!("{score}%"))
                            .size(theme::TEXT_CAPTION)
                            .color(theme::TEXT_STRONG)
                            .into(),
                    ));
                }
                if let Some(total) = info.total {
                    rows = rows.push(proton_row(
                        i18n.t("native.game.reports"),
                        text(format_count(total))
                            .size(theme::TEXT_CAPTION)
                            .color(theme::TEXT_STRONG)
                            .into(),
                    ));
                }
                if let Some(url) = info.url.as_deref() {
                    rows = rows.push(
                        column![
                            text(i18n.t("native.game.link"))
                                .size(theme::TEXT_MICRO)
                                .font(theme::FONT_MEDIUM)
                                .color(theme::TEXT_3),
                            text(url)
                                .size(theme::TEXT_MICRO)
                                .color(theme::ACCENT)
                                .wrapping(Wrapping::WordOrGlyph),
                        ]
                        .spacing(2),
                    );
                }
                rows.into()
            }
        },
    };

    panel_card("ProtonDB", badge, body)
}

fn proton_row<'a>(label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label).size(theme::TEXT_CAPTION).color(theme::TEXT_2),
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
            .size(theme::TEXT_MICRO)
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

pub(crate) fn stats_reviews_panel<'a>(
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
            .size(theme::TEXT_MICRO)
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
            text(owners)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_STRONG)
                .into(),
        ));
    }
    if let Some(ccu) = stats.ccu.filter(|ccu| *ccu > 0) {
        content = content.push(proton_row(
            i18n.t("stats.jogadores_ativos"),
            text(format_count(ccu))
                .size(theme::TEXT_CAPTION)
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
            text(value)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_STRONG)
                .into(),
        ));
    }

    if stats.reviews.is_empty() {
        content = content.push(hint(i18n.t("native.game.reviews_empty"), theme::TEXT_3));
    } else {
        content = content.push(
            text(i18n.t("native.game.recent_reviews"))
                .size(theme::TEXT_MICRO)
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

fn review_card<'a>(i18n: &'a I18n, review: &'a crate::backend::GameReview) -> Element<'a, Message> {
    let mut body = column![
        row![
            text(review.author.as_str())
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_STRONG),
            Space::new().width(Fill),
            text(if review.positive {
                format!("★ {}", i18n.t("native.game.recommends"))
            } else {
                format!("☆ {}", i18n.t("native.game.not_recommends"))
            })
            .size(theme::TEXT_MICRO)
            .color(if review.positive {
                theme::SUCCESS
            } else {
                theme::DANGER
            }),
        ]
        .align_y(iced::Center),
        text(review.text.as_str())
            .size(theme::TEXT_MICRO)
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
            .size(theme::TEXT_MICRO)
            .color(theme::TEXT_3),
        );
    }
    if review.helpful > 0 {
        meta = meta.push(
            text(i18n.t_args(
                "native.game.helpful",
                &[("count", &review.helpful.to_string())],
            ))
            .size(theme::TEXT_MICRO)
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
    let mut header = row![
        container(Space::new().width(3).height(14)).style(theme::accent_bar),
        text(title.to_uppercase())
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_2),
    ]
    .spacing(8)
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
        .size(theme::TEXT_CAPTION)
        .color(color)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn error_body<'a>(i18n: &'a I18n, title: &'a str, error: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title).size(theme::TEXT_CAPTION).color(theme::DANGER),
            text(shorten(error, 220))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_2)
                .wrapping(Wrapping::WordOrGlyph),
            button(
                text(i18n.t("downloads.tentar_novamente"))
                    .size(theme::TEXT_MICRO)
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
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_1),
            text(message).size(theme::TEXT_CAPTION).color(theme::TEXT_3),
        ]
        .spacing(4),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::stub_panel)
    .into()
}

/// Card "Trailer": frame RGBA do GStreamer, play/pause, seek e estado.
/// Área do vídeo antes do primeiro frame: carregando, erro, `info:null` etc.
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
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        button(
            text(i18n.t("common.fechar"))
                .size(theme::TEXT_BODY)
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
        .size(theme::TEXT_BODY)
        .width(Fill)
        .style(theme::search_input),
        button(
            text(i18n.t("store.buscar"))
                .size(theme::TEXT_BODY)
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
    container(text(message).size(theme::TEXT_BODY).color(color))
        .width(Fill)
        .height(320)
        .align_x(iced::Center)
        .align_y(iced::Center)
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
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::DANGER),
                text(shorten(error, 260))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_2),
                button(
                    text(i18n.t("downloads.tentar_novamente"))
                        .size(theme::TEXT_BODY)
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
        .align_x(iced::Center)
        .align_y(iced::Center)
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
            .size(theme::TEXT_MICRO)
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
    let mut meta =
        row![text(channel).size(theme::TEXT_MICRO).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }

    button(
        row![
            trailer_thumbnail(i18n, video, covers, 160.0, 90.0),
            column![
                text(video.title.as_str())
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(i18n.t("native.game.trailer_click_note"))
                    .size(theme::TEXT_MICRO)
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
    let mut meta =
        row![text(channel).size(theme::TEXT_MICRO).color(theme::TEXT_2)].spacing(theme::SPACE_2);
    if video.duration > 0 {
        meta = meta.push(
            text(media::format_seconds(video.duration as f64))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3),
        );
    }

    container(
        row![
            trailer_thumbnail(i18n, video, covers, 200.0, 112.0),
            column![
                text(i18n.t("common.baixando"))
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_BOLD)
                    .color(theme::ACCENT),
                text(video.title.as_str())
                    .size(theme::TEXT_READ)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1)
                    .wrapping(Wrapping::WordOrGlyph),
                meta,
                text(i18n.t("native.game.ytdlp_note"))
                    .size(theme::TEXT_MICRO)
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
                    .size(theme::TEXT_BODY)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                text(shorten(path, 110))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
            ]
            .spacing(3)
            .width(Fill),
            button(
                text(i18n.t("native.game.open_downloaded"))
                    .size(theme::TEXT_CAPTION)
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

#[cfg(test)]
mod launch_options_ui_tests {
    use super::*;
    use crate::backend::{EmulatorInfo, EmulatorStatus};
    use crate::i18n::Lang;

    fn game(launcher: &str) -> Game {
        Game {
            id: "custom:demo".into(),
            title: "Demo".into(),
            launcher: launcher.into(),
            launch_cmd: Vec::new(),
            cover_url: None,
            hero_url: None,
            logo_url: None,
            description: None,
            developer: None,
            publisher: None,
            genre: None,
            year: None,
            platform: None,
            exe: None,
            playtime_minutes: 0,
            installed: true,
            hidden: false,
            favorite: false,
            categories: Vec::new(),
        }
    }

    fn ready_state() -> LaunchOptionsState {
        LaunchOptionsState {
            open: true,
            game_id: "custom:demo".into(),
            game_title: "Demo".into(),
            status: LoadStatus::Ready,
            settings: Some(GameSettingsView::default()),
            ..LaunchOptionsState::default()
        }
    }

    #[test]
    fn compact_basic_only_without_extra_fields_or_open_advanced() {
        let steam = game("steam");
        let custom = game("custom");
        let settings = GameSettingsView::default();
        let mut state = ready_state();
        assert!(launch_basic_is_compact(&state, &settings, &steam));

        state.advanced_open = true;
        assert!(!launch_basic_is_compact(&state, &settings, &steam));
        state.advanced_open = false;

        // Auto em jogo não-Steam mostra argumentos: deixa de ser compacto.
        assert!(!launch_basic_is_compact(&state, &settings, &custom));

        let emulator = GameSettingsView {
            emulator_id: Some("retroarch".into()),
            ..GameSettingsView::default()
        };
        assert!(!launch_basic_is_compact(&state, &emulator, &steam));

        let exe_settings = GameSettingsView {
            exe_path: "/tmp/demo.exe".into(),
            ..GameSettingsView::default()
        };
        state.mode = Some("exe".into());
        assert!(!launch_basic_is_compact(&state, &exe_settings, &steam));
    }

    #[test]
    fn overlay_builds_in_steam_executable_emulator_loading_and_error_states() {
        let i18n = I18n::new(Lang::PtBr);

        let mut steam = ready_state();
        steam.mode = Some("steam".into());
        drop(launch_options_overlay(&i18n, &steam, &game("steam")));

        let mut executable = ready_state();
        executable.mode = Some("exe".into());
        executable.advanced_open = true;
        if let Some(settings) = executable.settings.as_mut() {
            settings.exe_path = "/tmp/demo.exe".into();
            settings.game_args = "--windowed".into();
        }
        drop(launch_options_overlay(&i18n, &executable, &game("custom")));

        let mut emulator = ready_state();
        if let Some(settings) = emulator.settings.as_mut() {
            settings.emulator_id = Some("retroarch".into());
            settings.rom_path = "/tmp/demo.sfc".into();
            settings.emulator_core_path = "/tmp/core.so".into();
        }
        emulator.emulators = vec![EmulatorInfo {
            id: "retroarch".into(),
            name: "RetroArch".into(),
            available: true,
            requires_core: true,
            ..EmulatorInfo::default()
        }];
        emulator.emulator_statuses = vec![EmulatorStatus {
            emulator_id: "retroarch".into(),
            ok: true,
            required: false,
            installed: true,
            ..EmulatorStatus::default()
        }];
        drop(launch_options_overlay(&i18n, &emulator, &game("custom")));

        let mut loading = ready_state();
        loading.status = LoadStatus::Loading;
        loading.settings = None;
        drop(launch_options_overlay(&i18n, &loading, &game("custom")));

        let mut error = ready_state();
        error.status = LoadStatus::Error("falha ao ler".into());
        error.settings = None;
        drop(launch_options_overlay(&i18n, &error, &game("custom")));
    }
}
