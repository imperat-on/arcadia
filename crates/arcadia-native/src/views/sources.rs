//! Fontes: catálogos JSON listados por `sources:list` (e CRUD confirmado no
//! dispatcher: add/remove/sync).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::{App, LoadStatus, Message, Section};
use crate::backend::SourceItem;
use crate::theme;
use crate::views;

pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.sources;
    let i18n = &app.i18n;
    let header = views::page_header_labeled(
        i18n.t("sidebar.fontes"),
        i18n.t("native.sources.subtitle"),
        Some(i18n.t_args(
            "native.sources.count",
            &[("count", &state.items.len().to_string())],
        )),
        Some(Message::Refresh(Section::Sources)),
        i18n.t("native.refresh"),
    );

    let body: Element<'_, Message> = match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            views::loading_state(i18n.t("native.sources.loading"))
        }
        LoadStatus::Error(error) => container(views::error_state_labeled(
            i18n.t("native.sources.load_error"),
            error,
            Message::Refresh(Section::Sources),
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
    let state = &app.sources;
    let i18n = &app.i18n;

    let add = row![
        text_input(i18n.t("native.sources.url_placeholder"), &state.url_draft)
            .on_input(Message::SourceUrlChanged)
            .padding([10.0, 14.0])
            .size(12)
            .width(Fill)
            .style(theme::search_input),
        {
            let mut control: iced::widget::Button<'_, Message> =
                button(
                    text(i18n.t("fontes.adicionar"))
                        .size(12)
                        .font(theme::FONT_MEDIUM),
                )
                .padding([10.0, 18.0]);
            if !state.busy && !state.url_draft.trim().is_empty() {
                control = control.on_press(Message::SourceAdd);
            }
            let rendered: Element<'_, Message> = control.style(theme::primary_button).into();
            rendered
        },
    ]
    .spacing(8)
    .align_y(iced::Center);

    let mut content = column![
        add,
        text(i18n.t("native.sources.note"))
            .size(11)
            .color(theme::TEXT_3),
    ]
    .spacing(theme::SPACE_2);

    if let Some(error) = state.action_error.as_deref() {
        content = content.push(views::action_error_banner(error, None));
    }

    if state.items.is_empty() {
        content = content.push(inline_empty(
            i18n.t("native.sources.empty_title"),
            i18n.t("native.sources.empty_desc"),
        ));
    } else {
        let mut actions = row![].spacing(8).align_y(iced::Center);
        if !state.busy {
            actions = actions.push(
                button(
                    text(i18n.t("fontes.sincronizar"))
                        .size(11)
                        .font(theme::FONT_MEDIUM),
                )
                    .on_press(Message::SourceSync)
                    .padding([7.0, 13.0])
                    .style(theme::ghost_button),
            );
        }
        actions = actions.push(
            text(i18n.t_args(
                "native.sources.registered",
                &[("count", &state.items.len().to_string())],
            ))
            .size(11)
            .color(theme::TEXT_3),
        );

        content = content.push(
            container(actions)
                .width(Fill)
                .padding([theme::SPACE_3, 0.0]),
        );

        let mut list = column![].spacing(theme::SPACE_2);
        for item in &state.items {
            list = list.push(source_card(app, item, state.busy));
        }
        content = content.push(list);
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

fn source_card<'a>(app: &'a App, item: &'a SourceItem, busy: bool) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let mut remove: iced::widget::Button<'_, Message> =
        button(text(i18n.t("common.remover")).size(11).font(theme::FONT_MEDIUM))
            .padding([6.0, 12.0]);
    if !busy {
        remove = remove.on_press(Message::SourceRemove(item.id.clone()));
    }

    let meta = row![
        text(source_status_label(i18n, item))
            .size(11)
            .font(theme::FONT_MEDIUM)
            .color(theme::ACCENT),
        text("·").size(11).color(theme::TEXT_3),
        text(if item.url.is_empty() {
            i18n.t("native.sources.url_missing")
        } else {
            item.url.as_str()
        })
        .size(11)
        .color(theme::TEXT_3),
    ]
    .spacing(6);

    container(
        row![
            column![
                text(item.name.as_str())
                    .size(14)
                    .font(theme::FONT_MEDIUM)
                    .color(theme::TEXT_1),
                meta,
            ]
            .spacing(3)
            .width(Fill),
            remove.style(theme::ghost_button),
        ]
        .spacing(theme::SPACE_4)
        .align_y(iced::Center),
    )
    .width(Fill)
    .padding(14)
    .style(theme::stub_panel)
    .into()
}

fn source_status_label<'a>(i18n: &'a crate::i18n::I18n, item: &SourceItem) -> String {
    match item.count {
        Some(count) => i18n.t_args("native.sources.games", &[("count", &count.to_string())]),
        None => i18n.t("native.sources.no_index").to_string(),
    }
}

fn inline_empty<'a>(title: &'a str, message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(16)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_STRONG),
            text(message).size(12).color(theme::TEXT_2),
            Space::new().height(4),
        ]
        .spacing(theme::SPACE_2),
    )
    .width(Fill)
    .padding(theme::SPACE_5)
    .style(theme::stub_panel)
    .into()
}
