//! Downloads: fila Epic/Steam (`dm:*`) + torrents (`torrent:*`) numa lista só.

use iced::widget::{button, column, container, progress_bar, row, scrollable, text, Space};
use iced::{Color, Element, Fill};

use crate::app::{App, LoadStatus, Message, Section};
use crate::backend::{DownloadAction, DownloadItem, DownloadKind, DownloadStatus};
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.downloads;
    let i18n = &app.i18n;
    let total = state.data.items.len();
    let header = views::page_header_labeled(
        i18n.t("downloads.titulo"),
        i18n.t("native.downloads.subtitle"),
        Some(i18n.t_args(
            "native.downloads.count",
            &[("count", &total.to_string())],
        )),
        Some(Message::Refresh(Section::Downloads)),
        i18n.t("native.refresh"),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            views::loading_state(i18n.t("native.downloads.loading"))
        }
        LoadStatus::Error(error) => container(views::error_state_labeled(
            i18n.t("native.downloads.load_error"),
            error,
            Message::Refresh(Section::Downloads),
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
    let state = &app.downloads;
    let active: Vec<&DownloadItem> = state
        .data
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                DownloadStatus::Active
                    | DownloadStatus::Caching
                    | DownloadStatus::Queued
                    | DownloadStatus::Paused
            )
        })
        .collect();
    let done: Vec<&DownloadItem> = state
        .data
        .items
        .iter()
        .filter(|item| item.status == DownloadStatus::Done)
        .collect();
    let failed: Vec<&DownloadItem> = state
        .data
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                DownloadStatus::Error | DownloadStatus::Canceled
            )
        })
        .collect();

    if state.data.items.is_empty() && state.data.warnings.is_empty() {
        return views::empty_state(
            app.i18n.t("downloads.vazio_titulo"),
            app.i18n.t("downloads.vazio_sub"),
        );
    }

    let mut content = column![].spacing(theme::SPACE_3);

    for warning in &state.data.warnings {
        content = content.push(views::warning_banner(warning));
    }
    if let Some(error) = state.action_error.as_deref() {
        content = content.push(views::action_error_banner(error, None));
    }

    content = content.push(download_group(
        app.i18n.t("native.downloads.group_active"),
        &active,
        app,
    ));
    if !done.is_empty() {
        content = content.push(download_group(
            app.i18n.t("downloads.secao.concluidos"),
            &done,
            app,
        ));
    }
    if !failed.is_empty() {
        content = content.push(download_group(
            app.i18n.t("downloads.secao.falhas"),
            &failed,
            app,
        ));
    }

    scrollable(
        container(content)
            .width(Fill)
            .padding([theme::SPACE_2, theme::PAGE_PADDING]),
    )
    .height(Fill)
    .style(theme::scrollable)
    .into()
}

fn download_group<'a>(
    title: &'a str,
    items: &[&'a DownloadItem],
    app: &'a App,
) -> Element<'a, Message> {
    let mut group = column![row![
        views::section_label(title),
        text(format!("{}", items.len()))
            .size(10)
            .color(theme::TEXT_3),
    ]
    .spacing(6)
    .align_y(iced::Center),]
    .spacing(theme::SPACE_2);

    if items.is_empty() {
        group = group.push(
            text(app.i18n.t("native.downloads.empty_group"))
                .size(12)
                .color(theme::TEXT_3),
        );
    } else {
        for item in items {
            group = group.push(card(app, item, app.downloads.busy_id.as_deref()));
        }
    }

    group.width(Fill).into()
}

fn card<'a>(app: &'a App, item: &'a DownloadItem, busy: Option<&str>) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let status_color = status_color(item.status);

    let title = row![
        container(
            text(kind_label(i18n, item.kind))
                .size(9)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_2)
        )
        .padding([3.0, 7.0])
        .style(theme::badge),
        text(item.title.as_str())
            .size(15)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
        Space::new().width(Fill),
        text(status_label(i18n, item.status))
            .size(11)
            .font(theme::FONT_MEDIUM)
            .color(status_color),
    ]
    .spacing(10)
    .align_y(iced::Center);

    let progress = progress_bar(0.0..=100.0, item.percent)
        .girth(8)
        .length(Fill);

    let mut detail = row![
        text(item.detail.as_str()).size(11).color(theme::TEXT_3),
        Space::new().width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center);

    if !item.live.is_empty() {
        detail = detail.push(text(item.live.as_str()).size(11).color(theme::TEXT_2));
    }

    let mut body = column![title, progress, detail].spacing(8).width(Fill);

    if item.status == DownloadStatus::Error && !item.error.is_empty() {
        body = body.push(text(item.error.as_str()).size(11).color(theme::DANGER));
    }

    let actions = if busy == Some(item.id.as_str()) {
        row![
            button(
                text(i18n.t("native.downloads.applying"))
                    .size(11)
                    .font(theme::FONT_MEDIUM),
            )
                .padding([7.0, 13.0])
                .style(theme::ghost_button),
        ]
        .into()
    } else {
        action_row(app, item)
    };

    container(
        row![body, actions]
            .spacing(theme::SPACE_4)
            .align_y(iced::Center),
    )
    .width(Fill)
    .padding(14)
    .style(theme::stub_panel)
    .into()
}

fn action_row<'a>(app: &'a App, item: &'a DownloadItem) -> Element<'a, Message> {
    let mut actions = row![].spacing(6);
    for action in actions_for(item.status) {
        let primary = matches!(action, DownloadAction::Resume | DownloadAction::Retry);
        let button: iced::widget::Button<'_, Message> =
            button(text(action_label(&app.i18n, *action)).size(11).font(theme::FONT_MEDIUM))
                .on_press(Message::DownloadActionRequested {
                    item: item.clone(),
                    action: *action,
                })
                .padding([7.0, 13.0]);

        let rendered: Element<'_, Message> = if primary {
            button.style(theme::primary_button).into()
        } else {
            button.style(theme::ghost_button).into()
        };
        actions = actions.push(rendered);
    }
    actions.into()
}

/// Mesma matriz de `actionsFor` da UI web.
fn actions_for(status: DownloadStatus) -> &'static [DownloadAction] {
    match status {
        DownloadStatus::Active => &[DownloadAction::Pause, DownloadAction::Cancel],
        DownloadStatus::Queued => &[DownloadAction::Cancel],
        DownloadStatus::Paused => &[DownloadAction::Resume, DownloadAction::Cancel],
        DownloadStatus::Caching => &[DownloadAction::Cancel],
        DownloadStatus::Error => &[DownloadAction::Retry, DownloadAction::Dismiss],
        DownloadStatus::Canceled => &[DownloadAction::Dismiss],
        DownloadStatus::Done => &[DownloadAction::Dismiss],
    }
}

fn kind_label<'a>(i18n: &'a crate::i18n::I18n, kind: DownloadKind) -> &'a str {
    match kind {
        DownloadKind::Epic => i18n.t("downloads.origem.epic"),
        DownloadKind::Steam => i18n.t("downloads.origem.steam"),
        DownloadKind::Torrent => i18n.t("downloads.origem.torrent"),
        DownloadKind::Http => i18n.t("downloads.origem.http"),
        DownloadKind::Debrid => i18n.t("downloads.origem.debrid"),
    }
}

fn status_label<'a>(i18n: &'a crate::i18n::I18n, status: DownloadStatus) -> &'a str {
    match status {
        DownloadStatus::Active => i18n.t("downloads.status.baixando"),
        DownloadStatus::Queued => i18n.t("downloads.status.na_fila"),
        DownloadStatus::Paused => i18n.t("downloads.status.paused"),
        DownloadStatus::Caching => i18n.t("native.downloads.status.caching"),
        DownloadStatus::Done => i18n.t("downloads.status.done"),
        DownloadStatus::Error => i18n.t("downloads.status.error"),
        DownloadStatus::Canceled => i18n.t("downloads.status.canceled"),
    }
}

fn action_label<'a>(i18n: &'a crate::i18n::I18n, action: DownloadAction) -> &'a str {
    match action {
        DownloadAction::Pause => i18n.t("downloads.pausar"),
        DownloadAction::Resume => i18n.t("downloads.retomar"),
        DownloadAction::Cancel => i18n.t("common.cancelar"),
        DownloadAction::Retry => i18n.t("downloads.tentar_novamente"),
        DownloadAction::Dismiss => i18n.t("common.remover"),
    }
}

fn status_color(status: DownloadStatus) -> Color {
    match status {
        DownloadStatus::Active | DownloadStatus::Caching => theme::ACCENT,
        DownloadStatus::Done => theme::SUCCESS,
        DownloadStatus::Error => theme::DANGER,
        DownloadStatus::Paused => theme::WARNING,
        DownloadStatus::Queued | DownloadStatus::Canceled => theme::TEXT_2,
    }
}
