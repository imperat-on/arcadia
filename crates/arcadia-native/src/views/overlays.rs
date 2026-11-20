//! Overlays globais do shell: splash de boot, diálogo de atualização e toasts
//! de conquista.
//!
//! Ficam acima da tela atual (inclusive do Big Picture) e não pertencem a
//! nenhuma view de seção. O splash usa um `MediaPlayer` próprio para nunca
//! disputar o pipeline do trailer.

use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, Space, Stack,
};
use iced::{Border, Color, ContentFit, Element, Fill, Length};

use crate::app::{App, Message, UpdateDialogKind, UpdateDialogState, UpdateStage};
use crate::backend::{PackagedPhase, UpdateCheck};
use crate::theme;

/// Empilha os overlays sobre a tela base. A ordem importa: diálogo acima dos
/// toasts e splash acima de tudo no boot.
pub fn stack<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let mut layers = Stack::new().push(base);

    if !app.toasts.is_empty() {
        layers = layers.push(toast_overlay(app));
    }
    if let Some(dialog) = app.update.dialog.as_ref() {
        layers = layers.push(update_dialog(app, dialog));
    }
    if app.boot.active {
        layers = layers.push(boot_overlay(app));
    }

    layers.width(Fill).height(Fill).into()
}

// ── Boot ────────────────────────────────────────────────────────────────────

fn boot_overlay(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match app.boot.media.texture() {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => {
            let label = column![
                text("ARCADIA")
                    .size(34)
                    .font(theme::FONT_BOLD)
                    .color(theme::TEXT_1),
                text(app.i18n.t("native.boot.loading"))
                    .size(12)
                    .color(theme::TEXT_3),
            ]
            .spacing(theme::SPACE_2)
            .align_x(iced::Center);

            container(label)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .into()
        }
    };

    mouse_area(
        container(content)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Color::BLACK.into()),
                ..container::Style::default()
            }),
    )
    .on_press(Message::DismissSplash)
    .into()
}

// ── Toasts de conquista ─────────────────────────────────────────────────────

fn toast_overlay(app: &App) -> Element<'_, Message> {
    let mut items = column![]
        .spacing(theme::SPACE_2)
        .align_x(iced::Alignment::End);

    for toast in app.toasts.iter() {
        if toast.alpha() <= 0.01 {
            continue;
        }
        if toast.offset_y() > 0.0 {
            items = items.push(Space::new().height(toast.offset_y()));
        }
        items = items.push(toast_item(app, toast));
    }

    container(items)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Alignment::End)
        .align_y(iced::Alignment::End)
        .padding(24)
        .into()
}

fn toast_item<'a>(app: &'a App, toast: &'a crate::toast::Toast) -> Element<'a, Message> {
    let alpha = toast.alpha().clamp(0.0, 1.0);
    let heading = with_alpha(theme::WARNING, alpha);
    let title_color = with_alpha(theme::TEXT_1, alpha);
    let desc_color = with_alpha(theme::TEXT_2, alpha);

    let icon: Element<'_, Message> = match app.covers.ready(toast.achievement.icon.as_deref()) {
        Some(handle) => image(handle.clone())
            .width(48)
            .height(48)
            .content_fit(ContentFit::Cover)
            .opacity(alpha)
            .into(),
        None => container(text("🏆").size(22).color(with_alpha(theme::TEXT_2, alpha)))
            .width(48)
            .height(48)
            .center_x(Fill)
            .center_y(Fill)
            .style(move |_| container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08 * alpha).into()),
                border: Border::default().rounded(3),
                ..container::Style::default()
            })
            .into(),
    };

    let body = column![
        text(app.i18n.t("conquistas.desbloquear_ok"))
            .size(10)
            .font(theme::FONT_SEMIBOLD)
            .color(heading),
        text(&toast.achievement.title)
            .size(13)
            .font(theme::FONT_SEMIBOLD)
            .color(title_color),
        text(toast.achievement.desc.as_deref().unwrap_or(" "))
            .size(11)
            .color(desc_color),
    ]
    .spacing(2)
    .width(Length::Fixed(220.0));

    let close = button(text("×").size(16).color(with_alpha(theme::TEXT_2, alpha)))
        .on_press(Message::DismissToast(toast.id))
        .padding([0.0, 4.0])
        .style(theme::ghost_button);

    let card = row![icon, body, close]
        .spacing(theme::SPACE_3)
        .align_y(iced::Center);

    container(card)
        .width(Length::Fixed(330.0))
        .padding([10.0, 12.0])
        .style(move |_| container::Style {
            background: Some(
                Color::from_rgba(
                    theme::SURFACE_3.r,
                    theme::SURFACE_3.g,
                    theme::SURFACE_3.b,
                    0.96 * alpha,
                )
                .into(),
            ),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.10 * alpha),
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Some(title_color),
            ..container::Style::default()
        })
        .into()
}

// ── Diálogo de atualização ──────────────────────────────────────────────────

fn update_dialog<'a>(app: &'a App, dialog: &'a UpdateDialogState) -> Element<'a, Message> {
    let veil: Element<'a, Message> = if dialog.is_busy() {
        mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(modal_veil),
        )
        .on_press(Message::UpdateNoop)
        .into()
    } else {
        mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(modal_veil),
        )
        .on_press(Message::UpdateDialogDismiss)
        .into()
    };

    let panel = container(dialog_body(app, dialog))
        .width(Length::Fixed(480.0))
        .padding(theme::SPACE_5)
        .style(modal_panel);

    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(veil)
        .push(
            container(panel)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .padding(24),
        )
        .into()
}

fn dialog_body<'a>(app: &'a App, dialog: &'a UpdateDialogState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut content = column![].spacing(theme::SPACE_3);

    let title = if dialog.kind == UpdateDialogKind::Git {
        i18n.t("update.titulo")
    } else {
        i18n.t("update.packaged.titulo")
    };
    content = content.push(
        text(title)
            .size(18)
            .font(theme::FONT_SEMIBOLD)
            .color(theme::TEXT_1),
    );

    if dialog.kind == UpdateDialogKind::Git {
        content = content.push(git_update_content(app, dialog));
    } else {
        content = content.push(packaged_update_content(app, dialog));
    }

    if let Some(error) = dialog.error.as_deref() {
        content = content.push(
            container(text(error).size(12).color(theme::DANGER).width(Fill))
                .width(Fill)
                .padding([8.0, 10.0])
                .style(theme::error_panel),
        );
    }

    content = content.push(action_footer(app, dialog));
    content.into()
}

fn git_update_content<'a>(app: &'a App, dialog: &'a UpdateDialogState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    if let Some(stage) = dialog.stage {
        let label = match stage {
            UpdateStage::Restarting => i18n.t("update.etapa.pronto"),
            _ => i18n.t("update.etapa.pull"),
        };
        return text(label).size(13).color(theme::TEXT_1).into();
    }

    let Some(check) = dialog.git.as_ref() else {
        if dialog.error.is_some() {
            return Space::new().height(0).into();
        }
        return text(i18n.t("update.packaged.em_dia"))
            .size(12)
            .color(theme::TEXT_2)
            .into();
    };
    let mut content = column![].spacing(theme::SPACE_2);

    if let Some(version) = app
        .update
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.git.as_ref())
        .map(|git| git.versao.as_str())
        .filter(|version| !version.is_empty())
    {
        content = content.push(
            text(i18n.t_args("native.update.current_version", &[("versao", version)]))
                .size(11)
                .color(theme::TEXT_3),
        );
    }

    let available = check.disponivel;
    let atrasado = check.atrasado.unwrap_or(check.commits.len() as i64);
    if available {
        content = content.push(
            text(i18n.t_args("update.subtitulo", &[("n", &atrasado.to_string())]))
                .size(12)
                .color(theme::TEXT_2),
        );
    } else if let Some(erro) = check.erro.as_deref() {
        content = content.push(text(erro).size(12).color(theme::DANGER));
    } else if let Some(motivo) = check.motivo.as_deref() {
        content = content.push(
            text(update_reason_label(i18n, motivo, &dialog.channel))
                .size(12)
                .color(theme::WARNING),
        );
    } else {
        content = content.push(text(i18n.t("update.em_dia")).size(12).color(theme::TEXT_2));
    }

    if !app.hide_changelog_on_start && !check.commits.is_empty() {
        content = content.push(commit_list(check));
    }
    if check.deps_mudaram && available {
        content = content.push(
            text(i18n.t("update.deps_aviso"))
                .size(11)
                .color(theme::TEXT_3),
        );
    }
    content.into()
}

fn packaged_update_content<'a>(
    app: &'a App,
    dialog: &'a UpdateDialogState,
) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let Some(state) = dialog.packaged.as_ref() else {
        if dialog.error.is_some() {
            return Space::new().height(0).into();
        }
        return text(i18n.t("update.packaged.em_dia"))
            .size(12)
            .color(theme::TEXT_2)
            .into();
    };

    let mut content = column![].spacing(theme::SPACE_2);
    let sem_suporte = state.is_unsupported();

    if let Some(stage) = dialog.stage {
        let label = match stage {
            UpdateStage::Downloading => i18n.t_args(
                "update.packaged.baixando",
                &[("pct", &state.progresso.to_string())],
            ),
            UpdateStage::Applying => i18n.t("update.etapa.pull").to_string(),
            UpdateStage::Installing => i18n.t("native.update.installing").to_string(),
            UpdateStage::Restarting => i18n.t("update.etapa.pronto").to_string(),
        };
        content = content.push(text(label).size(13).color(theme::TEXT_1));
        if matches!(
            stage,
            UpdateStage::Downloading | UpdateStage::Applying | UpdateStage::Installing
        ) {
            content = content.push(progress_bar(state.progresso));
        }
    } else if sem_suporte {
        content = content.push(
            text(i18n.t("update.packaged.sem_suporte"))
                .size(12)
                .color(theme::WARNING),
        );
        content = content.push(
            text(i18n.t_args("native.update.channel", &[("canal", &state.canal)]))
                .size(11)
                .color(theme::TEXT_3),
        );
    } else {
        match state.fase {
            PackagedPhase::Disponivel => {
                let version = state.versao_oferecida();
                let label = match state.tamanho {
                    Some(size) => i18n.t_args(
                        "update.packaged.disponivel_tamanho",
                        &[("versao", version), ("tamanho", &fmt_bytes(size))],
                    ),
                    None => i18n.t_args("update.packaged.disponivel", &[("versao", version)]),
                };
                content = content.push(text(label).size(13).color(theme::TEXT_1));
                content = content.push(
                    text(i18n.t_args(
                        "native.update.version_line",
                        &[("atual", &state.versao_atual), ("nova", version)],
                    ))
                    .size(11)
                    .color(theme::TEXT_3),
                );
            }
            PackagedPhase::Baixando => {
                content = content.push(
                    text(i18n.t_args(
                        "update.packaged.baixando",
                        &[("pct", &state.progresso.to_string())],
                    ))
                    .size(13)
                    .color(theme::TEXT_1),
                );
                content = content.push(progress_bar(state.progresso));
            }
            PackagedPhase::Pronto => {
                content = content.push(
                    text(i18n.t("update.packaged.pronto"))
                        .size(13)
                        .color(theme::SUCCESS),
                );
            }
            PackagedPhase::Erro => {
                content = content.push(
                    text(if state.erro_acao.as_deref() == Some("instalar") {
                        i18n.t("update.packaged.erro_instalar")
                    } else {
                        i18n.t("update.packaged.erro")
                    })
                    .size(12)
                    .color(theme::DANGER),
                );
                if let Some(erro) = state.erro.as_deref() {
                    content = content.push(text(erro).size(11).color(theme::TEXT_3));
                }
            }
            PackagedPhase::Ocioso | PackagedPhase::SemSuporte => {
                content = content.push(
                    text(i18n.t("update.packaged.em_dia"))
                        .size(12)
                        .color(theme::TEXT_2),
                );
            }
        }
    }

    if state.jogo_rodando {
        content = content.push(
            container(
                text(i18n.t("native.update.jogo_rodando"))
                    .size(11)
                    .color(theme::WARNING)
                    .width(Fill),
            )
            .width(Fill)
            .padding([8.0, 10.0])
            .style(theme::stub_panel),
        );
    }

    content.into()
}

fn commit_list<'a>(check: &'a UpdateCheck) -> Element<'a, Message> {
    let mut commits = column![].spacing(6);
    for commit in &check.commits {
        commits = commits.push(
            row![
                text(short_sha(&commit.sha))
                    .size(10)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_3)
                    .width(Length::Fixed(64.0)),
                text(&commit.titulo)
                    .size(12)
                    .color(theme::TEXT_STRONG)
                    .width(Fill),
            ]
            .spacing(8),
        );
    }

    container(
        scrollable(commits)
            .height(Length::Fixed(190.0))
            .style(theme::scrollable),
    )
    .width(Fill)
    .padding(10)
    .style(theme::empty_panel)
    .into()
}

fn action_footer<'a>(app: &'a App, dialog: &'a UpdateDialogState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    if dialog.stage.is_some() {
        return Space::new().height(0).into();
    }

    let mut buttons = row![].spacing(theme::SPACE_2).align_y(iced::Center);
    buttons = buttons.push(Space::new().width(Fill));
    buttons = buttons.push(
        button(text(i18n.t("update.packaged.depois")).size(12))
            .on_press(Message::UpdateDialogDismiss)
            .padding([8.0, 14.0])
            .style(theme::ghost_button),
    );

    if dialog.error.is_some() {
        buttons = buttons.push(
            button(
                text(i18n.t("update.packaged.tentar"))
                    .size(12)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::UpdateRetry)
            .padding([8.0, 14.0])
            .style(theme::primary_button),
        );
    } else if dialog.kind == UpdateDialogKind::Git {
        if dialog.git.as_ref().is_some_and(|check| check.disponivel) {
            buttons = buttons.push(
                button(
                    text(i18n.t("update.aplicar"))
                        .size(12)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::UpdateDownloadRequested)
                .padding([8.0, 14.0])
                .style(theme::primary_button),
            );
        }
    } else if let Some(state) = dialog.packaged.as_ref() {
        if state.is_unsupported() {
            // Sem suporte: só explica; nenhum botão instala.
        } else if state.fase == PackagedPhase::Pronto && !state.jogo_rodando {
            buttons = buttons.push(
                button(
                    text(i18n.t("update.packaged.reiniciar"))
                        .size(12)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::UpdateInstallRequested)
                .padding([8.0, 14.0])
                .style(theme::primary_button),
            );
        } else if state.fase == PackagedPhase::Disponivel {
            buttons = buttons.push(
                button(
                    text(i18n.t("update.packaged.baixar"))
                        .size(12)
                        .font(theme::FONT_MEDIUM),
                )
                .on_press(Message::UpdateDownloadRequested)
                .padding([8.0, 14.0])
                .style(theme::primary_button),
            );
        }
    }

    buttons.into()
}

fn update_reason_label(i18n: &crate::i18n::I18n, motivo: &str, canal: &str) -> String {
    match motivo {
        "canal_nao_suportado" => i18n.t_args("native.update.sem_suporte", &[("canal", canal)]),
        "jogo_rodando" => i18n.t("native.update.jogo_rodando").to_string(),
        other => other.to_string(),
    }
}

fn progress_bar(percent: u8) -> Element<'static, Message> {
    let total = 400.0_f32;
    let filled = total * percent.min(100) as f32 / 100.0;
    let fill = container(Space::new())
        .width(Length::Fixed(filled))
        .height(Length::Fixed(8.0))
        .style(|_| container::Style {
            background: Some(theme::ACCENT.into()),
            border: Border::default().rounded(4),
            ..container::Style::default()
        });

    container(
        row![
            fill,
            Space::new()
                .width(Length::Fixed((total - filled).max(0.0)))
                .height(Length::Fixed(8.0)),
        ]
        .height(Length::Fixed(8.0)),
    )
    .width(Length::Fixed(total))
    .height(Length::Fixed(8.0))
    .padding(0)
    .style(|_| container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()),
        border: Border::default().rounded(4),
        ..container::Style::default()
    })
    .into()
}

fn modal_veil(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.72).into()),
        ..container::Style::default()
    }
}

fn modal_panel(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(theme::SURFACE_1.into()),
        border: Border {
            color: theme::BORDER_STRONG,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(theme::TEXT_1),
        ..container::Style::default()
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha.clamp(0.0, 1.0),
        ..color
    }
}

fn short_sha(sha: &str) -> &str {
    if sha.len() > 7 {
        &sha[..7]
    } else {
        sha
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GB", value / GIB)
    } else {
        format!("{:.0} MB", (value / MIB).max(1.0))
    }
}
