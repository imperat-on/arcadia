//! Escolha de oferta e destino compartilhada pela loja desktop e Big Picture.
use iced::widget::{button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, Space, Stack};
use iced::{Color, Element, Fill, Length};

use crate::app::{App, LoadStatus, Message, StoreDetailState};
use crate::picker::PickPurpose;
use crate::theme;

pub fn available(detail: &StoreDetailState) -> bool {
    !detail.downloads.data.offers.is_empty()
}

pub fn summary<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let state = &detail.downloads;
    let mut body = column![super::section_label(i18n.t("sidebar.fontes"))].spacing(theme::SPACE_3);
    match &state.status {
        LoadStatus::Idle | LoadStatus::Loading => {
            body = body.push(text(i18n.t("native.store.sources.searching")).size(theme::TEXT_BODY).color(theme::TEXT_2));
        }
        LoadStatus::Error(_) => {
            body = body.push(text(i18n.t("native.store.sources.failed")).size(theme::TEXT_BODY).color(theme::WARNING));
        }
        LoadStatus::Ready => {
            let label = if available(detail) {
                i18n.t_args("native.store.sources.available", &[("count", &state.data.offers.len().to_string())])
            } else {
                i18n.t("store.nenhum_download").to_string()
            };
            body = body.push(text(label).size(theme::TEXT_BODY).color(theme::TEXT_2));
        }
    }
    if available(detail) {
        body = body.push(button(text(i18n.t("native.store.sources.choose")).size(theme::TEXT_BODY))
            .on_press_maybe((!detail.install_busy).then_some(Message::StoreDownloadRequested))
            .padding([8.0, 12.0]).style(theme::primary_button));
    }
    if !state.data.unavailable_sources.is_empty() {
        body = body.push(text(i18n.t_args("native.store.sources.unavailable", &[
            ("sources", &state.data.unavailable_sources.join(", ")),
        ])).size(theme::TEXT_CAPTION).color(theme::WARNING));
    }
    if !available(detail) && super::store::install_block_reason(i18n, detail).is_none() {
        body = body.push(button(text(i18n.t("native.store.sources.steam")).size(theme::TEXT_BODY))
            .on_press(Message::StoreDownloadRequested)
            .padding([8.0, 12.0]).style(theme::primary_button));
    }
    if matches!(state.status, LoadStatus::Error(_)) || !state.data.unavailable_sources.is_empty() {
        body = body.push(button(text(i18n.t("downloads.tentar_novamente")).size(theme::TEXT_CAPTION))
            .on_press_maybe((!detail.install_busy && !state.pick_busy).then_some(Message::StoreSourcesRetry))
            .padding([5.0, 8.0]).style(theme::ghost_button));
    }
    container(body).width(Fill).padding(theme::SPACE_5).style(theme::panel).into()
}

pub fn overlay<'a>(app: &'a App, detail: &'a StoreDetailState) -> Element<'a, Message> {
    let i18n = &app.i18n;
    let state = &detail.downloads;
    let selected = state.data.offers.get(state.selected_offer);
    let busy = detail.install_busy || state.pick_busy;
    let mut body = column![
        text(i18n.t_args("store.metodo.titulo", &[("title", &detail.game.title)]))
            .size(theme::TEXT_HEADING).font(theme::FONT_BOLD).color(theme::TEXT_1),
        text(i18n.t("native.store.sources.choose")).size(theme::TEXT_BODY).color(theme::TEXT_2),
    ].spacing(theme::SPACE_3);
    if selected.is_some() {
        let picker = pick_list(state.data.offers.as_slice(), selected, |offer| Message::StoreSourceSelected(offer.reference))
            .width(Fill).padding(10).text_size(theme::TEXT_BODY);
        body = body.push(picker);
    } else {
        body = body.push(text(if matches!(state.status, LoadStatus::Loading) {
            i18n.t("native.store.sources.searching")
        } else {
            i18n.t("store.nenhum_download")
        }).size(theme::TEXT_BODY).color(theme::TEXT_2));
    }
    if let Some(offer) = selected {
        if !offer.file_size.is_empty() {
            body = body.push(text(&offer.file_size).size(theme::TEXT_CAPTION).color(theme::TEXT_2));
        }
        for (index, uri) in offer.uris.iter().enumerate() {
            let label = if uri.to_ascii_lowercase().starts_with("magnet:") {
                i18n.t("native.store.sources.torrent")
            } else {
                uri.split_once("://").map(|(_, rest)| rest.split('/').next().unwrap_or(rest)).unwrap_or(uri)
            };
            let chosen = index == state.selected_uri;
            body = body.push(button(text(format!("{} {label}", if chosen { "●" } else { "○" })).size(theme::TEXT_BODY))
                .on_press_maybe((!busy).then_some(Message::StoreSourceUriSelected(index)))
                .padding([8.0, 12.0]).width(Fill)
                .style(move |theme, status| theme::game_item(theme, status, chosen)));
        }
    }
    body = body.push(text(i18n.t("native.store.sources.destination")).size(theme::TEXT_CAPTION).color(theme::TEXT_2));
    body = body.push(row![
        text_input(i18n.t("native.store.sources.destination"), &state.path)
            .on_input(Message::StoreDownloadPathChanged)
            .on_submit(Message::StoreSourceDownloadConfirmed)
            .padding(10).size(theme::TEXT_BODY).width(Fill).style(theme::search_input),
        button(text("…").size(theme::TEXT_BODY)).padding(10)
            .on_press_maybe((!busy).then_some(Message::PickRequested(PickPurpose::StoreDownloadFolder {
                appid: detail.game.appid.clone(), generation: state.generation,
            }))).style(theme::ghost_button),
    ].spacing(theme::SPACE_2));
    let error = state.error.as_deref().or_else(|| match &state.status {
        LoadStatus::Error(error) => Some(error.as_str()),
        _ => None,
    });
    if let Some(error) = error {
        body = body.push(text(error).size(theme::TEXT_BODY).color(theme::DANGER));
    }
    if !state.data.unavailable_sources.is_empty() {
        body = body.push(text(i18n.t_args("native.store.sources.unavailable", &[
            ("sources", &state.data.unavailable_sources.join(", ")),
        ])).size(theme::TEXT_CAPTION).color(theme::WARNING));
    }
    let confirm = !busy && !state.path.trim().is_empty()
        && selected.is_some_and(|offer| state.selected_uri < offer.uris.len());
    let mut actions = row![
        button(text(i18n.t("common.cancelar")).size(theme::TEXT_BODY))
            .on_press_maybe((!busy).then_some(Message::StoreDownloadClosed))
            .padding([9.0, 14.0]).style(theme::ghost_button),
        Space::new().width(Fill),
    ].spacing(theme::SPACE_2).align_y(iced::Center);
    if super::store::install_block_reason(i18n, detail).is_none() && !state.pick_busy {
        actions = actions.push(button(text(i18n.t("native.store.sources.steam")).size(theme::TEXT_BODY))
            .on_press(Message::StoreInstallRequested).padding([9.0, 14.0]).style(theme::ghost_button));
    }
    actions = actions.push(button(text(if detail.install_busy {
        i18n.t("native.store.enqueueing")
    } else {
        i18n.t("store.torrent.confirmar")
    }).size(theme::TEXT_BODY))
        .on_press_maybe(confirm.then_some(Message::StoreSourceDownloadConfirmed))
        .padding([9.0, 14.0]).style(theme::primary_button));
    body = body.push(actions);

    let panel = mouse_area(container(scrollable(body).height(Length::Shrink))
        .width(Length::Fixed(640.0)).max_height((app.window_size.height - 80.0).max(260.0))
        .padding(theme::SPACE_5).style(theme::panel)).on_press(Message::UpdateNoop);
    Stack::new()
        .push(mouse_area(container(Space::new()).width(Fill).height(Fill).style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.85).into()),
            ..Default::default()
        })).on_press(Message::StoreDownloadClosed))
        .push(container(panel).width(Fill).height(Fill).center_x(Fill).center_y(Fill).padding(24))
        .width(Fill).height(Fill).into()
}
