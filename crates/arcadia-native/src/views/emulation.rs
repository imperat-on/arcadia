//! Configurações > Emulação: catálogo/status, editor de perfil e varredura de
//! ROMs. A tela só lê o estado do `App`; todo IPC volta como `Message`.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Border, Color, Element, Fill};

use crate::app::{App, EmulationEditorState, LoadStatus, Message};
use crate::backend::{EmulatorInfo, EmulatorRomEntry};
use crate::i18n::I18n;
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.emulation;
    let mut content = column![header(app)].spacing(theme::SPACE_4).width(Fill);

    for warning in &state.warnings {
        content = content.push(views::warning_banner(warning));
    }

    if let LoadStatus::Error(error) = &state.status {
        content = content.push(views::error_state_labeled(
            app.i18n.t("native.emulation.load_error"),
            error,
            Message::EmulationDetect,
            app.i18n.t("downloads.tentar_novamente"),
        ));
    }

    if state.emulators.is_empty() {
        if matches!(state.status, LoadStatus::Idle | LoadStatus::Loading) {
            return content
                .push(views::loading_state(app.i18n.t("common.carregando")))
                .into();
        }
        if matches!(state.status, LoadStatus::Ready) {
            return content
                .push(views::empty_state(
                    app.i18n.t("native.emulation.empty_title"),
                    app.i18n.t("native.emulation.empty_desc"),
                ))
                .into();
        }
        return content.into();
    }

    if matches!(state.status, LoadStatus::Loading) {
        content = content.push(
            text(app.i18n.t("common.atualizando"))
                .size(11)
                .color(theme::ACCENT),
        );
    }

    if let Some(editor) = state.editor.as_ref() {
        if let Some(item) = state
            .emulators
            .iter()
            .find(|item| item.id == editor.emulator_id)
        {
            content = content.push(editor_view(app, item, editor));
            return content.into();
        }
    }

    let cards: Vec<Element<'_, Message>> =
        state.emulators.iter().map(|item| card(app, item)).collect();
    content = content.push(
        iced::widget::Grid::with_children(cards)
            .fluid(330.0)
            .spacing(theme::SPACE_3),
    );

    content.into()
}

fn header(app: &App) -> Element<'_, Message> {
    let i18n = &app.i18n;
    let loading = matches!(app.emulation.status, LoadStatus::Loading);
    let label = if loading {
        i18n.t("common.detectando")
    } else {
        i18n.t("emulacao.detectar_novamente")
    };
    let mut detect: iced::widget::Button<'_, Message> =
        button(text(label).size(12).font(theme::FONT_MEDIUM)).padding([8.0, 14.0]);
    if !loading {
        detect = detect.on_press(Message::EmulationDetect);
    }

    column![
        text(i18n.t("settings.emulacao.grupo").to_uppercase())
            .size(10)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_3),
        row![
            column![
                text(i18n.t("settings.emulacao"))
                    .size(24)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
                text(i18n.t("settings.emulacao.desc"))
                    .size(12)
                    .color(theme::TEXT_2),
            ]
            .spacing(3)
            .width(Fill),
            detect.style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Center),
    ]
    .spacing(4)
    .width(Fill)
    .into()
}

fn card<'a>(app: &'a App, item: &'a EmulatorInfo) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let status = app.emulation.status_for(&item.id);
    let index = app.emulation.rom_index_for(&item.id);
    let running = status.map(|status| status.running).unwrap_or(false);
    let bios_missing = status
        .map(|status| status.required && !status.installed)
        .unwrap_or(false);
    let core_missing = item.requires_core
        && item
            .profile
            .as_ref()
            .map(|profile| profile.core_path.trim().is_empty())
            .unwrap_or(true);

    let source = match item.source.as_str() {
        "detected" => i18n.t("native.emulation.source.detected"),
        "configured" => i18n.t("native.emulation.source.configured"),
        _ => i18n.t("native.emulation.source.builtin"),
    };
    let executable = if item.executable.trim().is_empty() {
        i18n.t("native.emulation.not_detected")
    } else {
        item.executable.as_str()
    };

    let mut badges = column![].spacing(4);
    let mut first = row![badge(
        if item.available {
            i18n.t("native.emulation.badge.available")
        } else {
            i18n.t("native.emulation.badge.unavailable")
        },
        if item.available {
            theme::SUCCESS
        } else {
            theme::WARNING
        }
    )]
    .spacing(4);
    if item.profile.is_some() {
        first = first.push(badge(i18n.t("emulacao.configurado"), theme::ACCENT));
    }
    badges = badges.push(first);
    if running || bios_missing || core_missing {
        let mut second = row![].spacing(4);
        if running {
            second = second.push(badge(i18n.t("emulacao.em_execucao"), theme::SUCCESS));
        }
        if bios_missing {
            second = second.push(badge(i18n.t("emulacao.bios_necessaria"), theme::WARNING));
        }
        if core_missing {
            second = second.push(badge(i18n.t("emulacao.core_necessario"), theme::WARNING));
        }
        badges = badges.push(second);
    }

    let summary = match index {
        Some(entry) => {
            let count = entry.roms.len().to_string();
            let folders = entry.folders.len().to_string();
            let mut summary = i18n.t_args(
                "native.emulation.roms_summary",
                &[("count", &count), ("folders", &folders)],
            );
            if entry.truncated {
                summary.push_str(" · ");
                summary.push_str(i18n.t("native.emulation.roms_truncated"));
            }
            summary
        }
        None => i18n.t("native.emulation.no_index").to_string(),
    };

    button(
        column![
            text(item.name.as_str())
                .size(16)
                .font(theme::FONT_SEMIBOLD)
                .color(theme::TEXT_1),
            row![
                text(item.id.as_str()).size(10).color(theme::TEXT_3),
                text("·").size(10).color(theme::TEXT_3),
                text(item.systems.join(" / ")).size(10).color(theme::TEXT_3),
            ]
            .spacing(5),
            text(item.description.as_str())
                .size(11)
                .color(theme::TEXT_2),
            text(format!("{}: {source}", i18n.t("native.emulation.source")))
                .size(11)
                .color(theme::TEXT_2),
            text(executable).size(10).color(theme::TEXT_3),
            badges,
            text(summary).size(11).color(theme::TEXT_2),
            text(format!("{} ›", i18n.t("emulacao.configurar")))
                .size(11)
                .font(theme::FONT_MEDIUM)
                .color(theme::ACCENT),
        ]
        .spacing(6),
    )
    .on_press(Message::EmulationOpenEditor(item.id.clone()))
    .padding(theme::SPACE_4)
    .width(Fill)
    .style(theme::card)
    .into()
}

fn editor_view<'a>(
    app: &'a App,
    item: &'a EmulatorInfo,
    editor: &'a EmulationEditorState,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let status = app.emulation.status_for(&item.id);
    let mut content = column![row![
        column![
            text(i18n.t("emulacao.configuracao"))
                .size(10)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_3),
            text(i18n.t_args("emulacao.configuracao_do", &[("system", &item.name)],))
                .size(20)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(format!("{} · {}", item.name, item.description))
                .size(11)
                .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        {
            let mut close: iced::widget::Button<'_, Message> = button(
                text(i18n.t("common.fechar"))
                    .size(11)
                    .font(theme::FONT_MEDIUM),
            )
            .padding([7.0, 12.0])
            .on_press(Message::EmulationCloseEditor);
            close = close.style(theme::ghost_button);
            close
        },
    ]
    .spacing(theme::SPACE_4)
    .align_y(iced::Center),]
    .spacing(theme::SPACE_3);

    if let Some(error) = editor.error.as_deref() {
        content = content.push(views::action_error_banner(error, None));
    }

    content = content.push(field(
        i18n.t("emulador.executavel_label"),
        text_input(
            i18n.t("native.emulation.executable_placeholder"),
            editor.executable.as_str(),
        )
        .on_input(Message::EmulationExecutableChanged)
        .padding([9.0, 12.0])
        .size(12)
        .style(theme::search_input)
        .into(),
        Some(i18n.t("native.emulation.executable_hint").to_string()),
    ));

    if editor.requires_core {
        content = content.push(field(
            i18n.t("emulacao.core_libretro"),
            text_input(
                i18n.t("emulacao.core_placeholder"),
                editor.core_path.as_str(),
            )
            .on_input(Message::EmulationCorePathChanged)
            .padding([9.0, 12.0])
            .size(12)
            .style(theme::search_input)
            .into(),
            Some(i18n.t("native.emulation.core_hint").to_string()),
        ));
    }

    if editor.bios_required {
        let bios_state = match status {
            Some(status) if status.installed => match status.detected_path.as_deref() {
                Some(path) => i18n.t_args("emulacao.bios_detectado_em", &[("path", path)]),
                None => i18n.t("emulacao.bios_detectado").to_string(),
            },
            Some(_) => i18n.t("emulacao.bios_nao_detectado").to_string(),
            None => i18n.t("native.emulation.bios_unknown").to_string(),
        };
        content = content.push(field(
            i18n.t("emulacao.pasta_bios"),
            text_input(
                i18n.t("emulacao.bios_placeholder"),
                editor.bios_path.as_str(),
            )
            .on_input(Message::EmulationBiosPathChanged)
            .padding([9.0, 12.0])
            .size(12)
            .style(theme::search_input)
            .into(),
            Some(bios_state),
        ));
    }

    content = content.push(rom_folders_editor(i18n, editor));
    content = content.push(field(
        i18n.t("emulacao.argumentos_adicionais"),
        text_input("--fullscreen", editor.args_text.as_str())
            .on_input(Message::EmulationArgsChanged)
            .padding([9.0, 12.0])
            .size(12)
            .style(theme::search_input)
            .into(),
        Some(i18n.t("native.emulation.args_hint").to_string()),
    ));
    content = content.push(scan_section(i18n, editor));
    content = content.push(footer(i18n, item, editor));

    container(content)
        .width(Fill)
        .padding(theme::SPACE_5)
        .style(theme::panel)
        .into()
}

fn field<'a>(
    label: &'a str,
    control: Element<'a, Message>,
    hint: Option<String>,
) -> Element<'a, Message> {
    let mut content = column![
        text(label)
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_STRONG),
        control,
    ]
    .spacing(4);
    if let Some(hint) = hint {
        content = content.push(text(hint).size(10).color(theme::TEXT_3));
    }
    content.width(Fill).into()
}

fn rom_folders_editor<'a>(
    i18n: &'a I18n,
    editor: &'a EmulationEditorState,
) -> Element<'a, Message> {
    let mut list = column![].spacing(4);
    if editor.rom_folders.is_empty() {
        list = list.push(
            container(
                text(i18n.t("emulacao.nenhuma_pasta"))
                    .size(11)
                    .color(theme::TEXT_3),
            )
            .width(Fill)
            .padding([8.0, 10.0])
            .style(theme::empty_panel),
        );
    } else {
        for (index, folder) in editor.rom_folders.iter().enumerate() {
            let recursive = if folder.recursive {
                i18n.t("native.emulation.recursive_on")
            } else {
                i18n.t("native.emulation.recursive_off")
            };
            list = list.push(
                container(
                    row![
                        text(folder.path.as_str())
                            .size(11)
                            .color(theme::TEXT_2)
                            .width(Fill),
                        button(text(recursive).size(10).font(theme::FONT_MEDIUM))
                            .on_press(Message::EmulationFolderRecursiveToggled(index))
                            .padding([4.0, 8.0])
                            .style(theme::ghost_button),
                        button(text(i18n.t("common.remover")).size(10))
                            .on_press(Message::EmulationFolderRemove(index))
                            .padding([4.0, 8.0])
                            .style(theme::ghost_button),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .width(Fill)
                .padding([7.0, 9.0])
                .style(theme::stub_panel),
            );
        }
    }

    let add = row![
        text_input(
            i18n.t("native.emulation.folder_placeholder"),
            editor.folder_draft.as_str(),
        )
        .on_input(Message::EmulationFolderDraftChanged)
        .padding([8.0, 12.0])
        .size(11)
        .width(Fill)
        .style(theme::search_input),
        button(
            text(i18n.t("emulacao.adicionar_pasta"))
                .size(11)
                .font(theme::FONT_MEDIUM),
        )
        .on_press(Message::EmulationFolderAdd)
        .padding([8.0, 12.0])
        .style(theme::ghost_button),
    ]
    .spacing(6)
    .align_y(iced::Center);

    column![
        text(i18n.t("emulacao.pastas_rom_label"))
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_STRONG),
        text(i18n.t("emulacao.pastas_rom_hint"))
            .size(10)
            .color(theme::TEXT_3),
        list,
        add,
    ]
    .spacing(6)
    .width(Fill)
    .into()
}

fn scan_section<'a>(i18n: &'a I18n, editor: &'a EmulationEditorState) -> Element<'a, Message> {
    let scan_label = if editor.scanning {
        i18n.t("common.pesquisando")
    } else {
        i18n.t("native.emulation.scan_configured")
    };
    let mut scan: iced::widget::Button<'_, Message> =
        button(text(scan_label).size(11).font(theme::FONT_MEDIUM)).padding([8.0, 14.0]);
    if !editor.scanning {
        scan = scan.on_press(Message::EmulationScan);
    }

    let mut content = column![
        row![
            text(i18n.t("emulacao.roms_encontradas"))
                .size(12)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
            Space::new().width(Fill),
            text(format!(
                "{}{}",
                editor.preview_roms.len(),
                if editor.preview_truncated { "+" } else { "" }
            ))
            .size(10)
            .color(theme::TEXT_3),
        ]
        .spacing(8)
        .align_y(iced::Center),
        scan.style(theme::ghost_button),
        text(i18n.t("native.emulation.scan_hint"))
            .size(10)
            .color(theme::TEXT_3),
    ]
    .spacing(6)
    .width(Fill);

    if let Some(error) = editor.scan_error.as_deref() {
        content = content.push(views::action_error_banner(error, None));
    } else if editor.preview_available && editor.preview_roms.is_empty() && !editor.scanning {
        content = content.push(
            text(i18n.t("native.emulation.scan_empty"))
                .size(11)
                .color(theme::TEXT_3),
        );
    }

    for rom in editor.preview_roms.iter().take(64) {
        content = content.push(rom_row(rom));
    }
    if editor.preview_roms.len() > 64 {
        let remaining = (editor.preview_roms.len() - 64).to_string();
        content = content.push(
            text(i18n.t_args("native.emulation.more_roms", &[("count", &remaining)]))
                .size(10)
                .color(theme::TEXT_3),
        );
    }
    if editor.preview_truncated {
        content = content.push(views::warning_banner(
            i18n.t("native.emulation.scan_truncated"),
        ));
    }

    content.into()
}

fn rom_row<'a>(rom: &'a EmulatorRomEntry) -> Element<'a, Message> {
    let extension = if rom.extension.trim().is_empty() {
        rom.kind.to_uppercase()
    } else {
        rom.extension.to_uppercase()
    };
    container(
        column![
            row![
                text(rom.name.as_str())
                    .size(11)
                    .color(theme::TEXT_STRONG)
                    .width(Fill),
                text(extension).size(10).color(theme::ACCENT),
                text(format_size(rom.size_bytes))
                    .size(10)
                    .color(theme::TEXT_3),
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(rom.relative_path.as_str())
                .size(10)
                .color(theme::TEXT_3),
        ]
        .spacing(2),
    )
    .width(Fill)
    .padding([6.0, 9.0])
    .style(theme::stub_panel)
    .into()
}

fn footer<'a>(
    i18n: &'a I18n,
    item: &'a EmulatorInfo,
    editor: &'a EmulationEditorState,
) -> Element<'a, Message> {
    let save_label = if editor.saving {
        i18n.t("common.salvando")
    } else {
        i18n.t("emulacao.salvar_configuracao")
    };
    let mut save: iced::widget::Button<'_, Message> =
        button(text(save_label).size(12).font(theme::FONT_MEDIUM)).padding([9.0, 18.0]);
    if !editor.saving {
        save = save.on_press(Message::EmulationProfileSave);
    }

    let mut content = column![save.style(theme::primary_button)]
        .spacing(8)
        .width(Fill);

    if editor.original.is_some() {
        if editor.confirm_remove {
            content = content.push(
                text(i18n.t_args("native.emulation.confirm_remove", &[("name", &item.name)]))
                    .size(11)
                    .color(theme::WARNING),
            );
            content = content.push(
                row![
                    button(
                        text(i18n.t("common.remover"))
                            .size(11)
                            .font(theme::FONT_MEDIUM)
                    )
                    .on_press(Message::EmulationRemoveConfirm)
                    .padding([7.0, 12.0])
                    .style(danger_button),
                    button(text(i18n.t("common.cancelar")).size(11))
                        .on_press(Message::EmulationRemoveCancel)
                        .padding([7.0, 12.0])
                        .style(theme::ghost_button),
                ]
                .spacing(6),
            );
        } else {
            content = content.push(
                button(
                    text(i18n.t("emulacao.remover_configuracao"))
                        .size(11)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::EmulationRemoveRequest)
                .padding([7.0, 12.0])
                .style(theme::ghost_button),
            );
        }
    }

    content.into()
}

fn badge<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
    container(text(label).size(10).color(color))
        .padding([3.0, 8.0])
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(Color::from_rgba(color.r, color.g, color.b, 0.12).into()),
            border: Border {
                color: Color::from_rgba(color.r, color.g, color.b, 0.35),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn danger_button(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb8(0xff, 0x8a, 0x9b),
        button::Status::Pressed => Color::from_rgb8(0xd9, 0x4f, 0x64),
        button::Status::Disabled => theme::SURFACE_3,
        button::Status::Active => theme::DANGER,
    };
    button::Style {
        background: Some(background.into()),
        text_color: theme::BACKGROUND,
        border: Border::default().rounded(3),
        ..button::Style::default()
    }
}

fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    if bytes == 0 {
        return "0 B".to_string();
    }
    let bytes = bytes as f64;
    if bytes >= KIB * KIB * KIB {
        format!("{:.1} GiB", bytes / (KIB * KIB * KIB))
    } else if bytes >= KIB * KIB {
        format!("{:.1} MiB", bytes / (KIB * KIB))
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}
