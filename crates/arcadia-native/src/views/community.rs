//! Painel de Comunidade reutilizável pela ficha do jogo e pelo detalhe da loja.
//!
//! Somente leitura nesta onda: reviews e coleções públicas com paginação
//! explícita, estado offline e cache por appid na sessão. Mutações exigem
//! sessão e ficam para a fatia seguinte.

use std::collections::HashMap;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill};

use crate::app::{LoadStatus, Message};
use crate::backend::{
    CommunityCollection, CommunityCollectionsPage, CommunityReview, CommunityReviewsPage,
};
use crate::theme;
use crate::views;

pub const COMMUNITY_PAGE_SIZE: u64 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommunityTab {
    #[default]
    Reviews,
    Collections,
}

impl CommunityTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reviews => "Reviews",
            Self::Collections => "Coleções",
        }
    }
}

/// Estado de uma lista paginada; `offline` é independente de `status` para
/// continuar visível junto dos itens que vieram do cache.
#[derive(Debug, Clone)]
pub struct CommunityList<T> {
    pub items: Vec<T>,
    pub status: LoadStatus,
    pub error: Option<String>,
    pub offline: bool,
    pub has_more: bool,
    pub next_offset: u64,
    pub total: Option<u64>,
    pub loading_more: bool,
}

impl<T> Default for CommunityList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            status: LoadStatus::Idle,
            error: None,
            offline: false,
            has_more: false,
            next_offset: 0,
            total: None,
            loading_more: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommunityState {
    pub tab: CommunityTab,
    pub reviews: CommunityList<CommunityReview>,
    pub collections: CommunityList<CommunityCollection>,
}

impl CommunityState {
    pub fn apply_reviews_page(
        &mut self,
        page: CommunityReviewsPage,
        offset: u64,
        append: bool,
    ) {
        apply_page(
            &mut self.reviews,
            page.reviews,
            page.pagination.has_more,
            page.pagination.total,
            page.offline,
            offset,
            append,
        );
    }

    pub fn apply_collections_page(
        &mut self,
        page: CommunityCollectionsPage,
        offset: u64,
        append: bool,
    ) {
        apply_page(
            &mut self.collections,
            page.collections,
            page.pagination.has_more,
            page.pagination.total,
            page.offline,
            offset,
            append,
        );
    }
}

/// Cache da sessão por appid: abrir o mesmo jogo/loja não refaz o IPC.
#[derive(Debug, Clone, Default)]
pub struct CommunityCache {
    entries: HashMap<String, CommunityState>,
}

impl CommunityCache {
    /// Devolve o estado do appid e se ele acabou de ser criado (primeira
    /// carga). O chamador usa `true` para disparar as duas primeiras páginas.
    pub fn ensure(&mut self, appid: &str) -> (&mut CommunityState, bool) {
        let fresh = !self.entries.contains_key(appid);
        if fresh {
            self.entries
                .insert(appid.to_string(), CommunityState::default());
        }
        (
            self.entries
                .get_mut(appid)
                .expect("estado recém-inserido"),
            fresh,
        )
    }

    pub fn get(&self, appid: &str) -> Option<&CommunityState> {
        self.entries.get(appid)
    }

    pub fn get_mut(&mut self, appid: &str) -> Option<&mut CommunityState> {
        self.entries.get_mut(appid)
    }
}

/// Anexa uma página pelo offset de itens; páginas sobrepostas substituem o
/// trecho já exibido em vez de duplicar linhas.
pub(crate) fn merge_page<T>(items: &mut Vec<T>, incoming: Vec<T>, offset: u64) {
    let offset = usize::try_from(offset).unwrap_or(usize::MAX);
    if offset < items.len() {
        items.truncate(offset);
    }
    items.extend(incoming);
}

fn apply_page<T: CommunityItemKey>(
    list: &mut CommunityList<T>,
    incoming: Vec<T>,
    has_more: bool,
    total: Option<u64>,
    offline: bool,
    offset: u64,
    append: bool,
) {
    let incoming_len = incoming.len() as u64;
    let before = list.items.len();
    if append {
        merge_page(&mut list.items, incoming, offset);
        dedupe_by_key(&mut list.items);
    } else {
        list.items = incoming;
    }
    let added = list.items.len().saturating_sub(before) as u64;
    list.status = LoadStatus::Ready;
    list.error = None;
    list.offline = offline;
    list.has_more = has_more && incoming_len > 0 && (!append || added > 0);
    list.next_offset = offset.saturating_add(incoming_len);
    list.total = total;
    list.loading_more = false;
}

trait CommunityItemKey {
    fn key(&self) -> &str;
}

impl CommunityItemKey for CommunityReview {
    fn key(&self) -> &str {
        self.id.as_str()
    }
}

impl CommunityItemKey for CommunityCollection {
    fn key(&self) -> &str {
        self.id.as_str()
    }
}

/// Remove páginas repetidas devolvidas pelo cache offline sem descartar
/// itens sem id (que não têm chave confiável para comparar).
fn dedupe_by_key<T: CommunityItemKey>(items: &mut Vec<T>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| {
        let key = item.key();
        key.is_empty() || seen.insert(key.to_string())
    });
}

pub fn panel<'a>(title: &'a str, state: &'a CommunityState) -> Element<'a, Message> {
    let header = row![
        column![
            text("Comunidade")
                .size(16)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(format!("Reviews e coleções públicas de {title}."))
                .size(11)
                .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        row![
            tab_button(CommunityTab::Reviews.label(), CommunityTab::Reviews, state.tab),
            tab_button(CommunityTab::Collections.label(), CommunityTab::Collections, state.tab),
            button(
                text("Atualizar")
                    .size(10)
                    .font(theme::FONT_MEDIUM),
            )
            .on_press(Message::CommunityRefresh(state.tab))
            .padding([6.0, 10.0])
            .style(theme::ghost_button),
        ]
        .spacing(6),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    let body = match state.tab {
        CommunityTab::Reviews => reviews_body(&state.reviews),
        CommunityTab::Collections => collections_body(&state.collections),
    };

    container(column![header, body].spacing(theme::SPACE_3).width(Fill))
        .width(Fill)
        .padding(theme::SPACE_4)
        .style(theme::stub_panel)
        .into()
}

fn tab_button(label: &'static str, tab: CommunityTab, active: CommunityTab) -> Element<'static, Message> {
    let is_active = tab == active;
    let mut control = button(
        text(label)
            .size(11)
            .font(theme::FONT_MEDIUM)
            .color(if is_active { theme::TEXT_1 } else { theme::TEXT_2 }),
    )
    .padding([6.0, 12.0])
    .style(move |theme, status| theme::segment_button(theme, status, is_active));
    if !is_active {
        control = control.on_press(Message::CommunityTabChanged(tab));
    }
    control.into()
}

fn reviews_body(list: &CommunityList<CommunityReview>) -> Element<'_, Message> {
    let mut content = column![].spacing(theme::SPACE_2).width(Fill);
    content = content.push(status_banner(
        &list.status,
        list.offline,
        CommunityTab::Reviews,
    ));

    if list.items.is_empty() {
        if matches!(list.status, LoadStatus::Idle | LoadStatus::Loading) {
            content = content.push(hint("Carregando reviews...", theme::TEXT_3));
        } else if matches!(list.status, LoadStatus::Ready) {
            content = content.push(hint("Nenhuma review pública ainda.", theme::TEXT_3));
        }
    } else {
        for review in &list.items {
            content = content.push(review_card(review));
        }
        if let Some(error) = list.error.as_deref() {
            content = content.push(views::action_error_banner(
                error,
                Some(Message::CommunityRefresh(CommunityTab::Reviews)),
            ));
        }
        if list.loading_more {
            content = content.push(hint("Carregando mais reviews...", theme::TEXT_3));
        } else if list.has_more {
            content = content.push(load_more_button(CommunityTab::Reviews));
        }
        content = content.push(hint(
            &format!("{} review(s) nesta sessão.", list.items.len()),
            theme::TEXT_3,
        ));
    }

    content.into()
}

fn collections_body(list: &CommunityList<CommunityCollection>) -> Element<'_, Message> {
    let mut content = column![].spacing(theme::SPACE_2).width(Fill);
    content = content.push(status_banner(
        &list.status,
        list.offline,
        CommunityTab::Collections,
    ));

    if list.items.is_empty() {
        if matches!(list.status, LoadStatus::Idle | LoadStatus::Loading) {
            content = content.push(hint("Carregando coleções...", theme::TEXT_3));
        } else if matches!(list.status, LoadStatus::Ready) {
            content = content.push(hint("Nenhuma coleção pública encontrada.", theme::TEXT_3));
        }
    } else {
        for collection in &list.items {
            content = content.push(collection_card(collection));
        }
        if let Some(error) = list.error.as_deref() {
            content = content.push(views::action_error_banner(
                error,
                Some(Message::CommunityRefresh(CommunityTab::Collections)),
            ));
        }
        if list.loading_more {
            content = content.push(hint("Carregando mais coleções...", theme::TEXT_3));
        } else if list.has_more {
            content = content.push(load_more_button(CommunityTab::Collections));
        }
        content = content.push(hint(
            &format!("{} coleção(ões) nesta sessão.", list.items.len()),
            theme::TEXT_3,
        ));
    }

    content.into()
}

fn status_banner<'a>(
    status: &'a LoadStatus,
    offline: bool,
    tab: CommunityTab,
) -> Element<'a, Message> {
    match status {
        LoadStatus::Error(message) => {
            views::action_error_banner(message, Some(Message::CommunityRefresh(tab)))
        }
        _ if offline => views::warning_banner(
            "Sem conexão: exibindo dados salvos em cache quando disponíveis.",
        ),
        LoadStatus::Idle | LoadStatus::Loading | LoadStatus::Ready => {
            iced::widget::Space::new().height(0).into()
        }
    }
}

fn load_more_button(tab: CommunityTab) -> Element<'static, Message> {
    button(
        text("Carregar mais")
            .size(11)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::CommunityLoadMore(tab))
    .padding([7.0, 14.0])
    .style(theme::ghost_button)
    .into()
}

fn review_card(review: &CommunityReview) -> Element<'_, Message> {
    let stars = rating_stars(review);
    let date = review
        .created_at
        .as_deref()
        .map(format_date)
        .filter(|date| !date.is_empty());

    let mut header = row![
        text(review.author.as_str())
            .size(12)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if !stars.is_empty() {
        header = header.push(text(stars).size(11).color(theme::WARNING));
    }
    if let Some(positive) = review.positive {
        header = header.push(badge(
            if positive { "Positiva" } else { "Negativa" },
            if positive { theme::SUCCESS } else { theme::DANGER },
        ));
    }
    if let Some(date) = date {
        header = header.push(iced::widget::Space::new().width(Fill));
        header = header.push(text(date).size(10).color(theme::TEXT_3));
    }

    let mut content = column![header].spacing(6).width(Fill);
    if let Some(title) = review.title.as_deref().filter(|title| !title.is_empty()) {
        content = content.push(
            text(title)
                .size(13)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
        );
    }
    content = content.push(
        text(if review.text.is_empty() {
            "Sem texto."
        } else {
            review.text.as_str()
        })
        .size(12)
        .color(theme::TEXT_2)
        .width(Fill),
    );

    let hours = review.hours.map(format_hours).unwrap_or_default();
    if !hours.is_empty() {
        content = content.push(text(hours).size(10).color(theme::TEXT_3));
    }

    container(content)
        .width(Fill)
        .padding(12)
        .style(theme::empty_panel)
        .into()
}

fn collection_card(collection: &CommunityCollection) -> Element<'_, Message> {
    let visibility = collection
        .visibility
        .as_deref()
        .map(visibility_label)
        .unwrap_or("Pública");
    let mut header = row![
        text(collection.title.as_str())
            .size(13)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1)
            .width(Fill),
        badge(visibility, theme::TEXT_2),
    ]
    .spacing(8)
    .align_y(iced::Center);

    header = header.push(badge(
        format!("{} jogo(s)", collection.item_count),
        theme::ACCENT,
    ));

    let mut content = column![
        header,
        text(format!("por {}", collection.owner))
            .size(10)
            .color(theme::TEXT_3),
    ]
    .spacing(5)
    .width(Fill);

    if let Some(description) = collection
        .description
        .as_deref()
        .filter(|description| !description.is_empty())
    {
        content = content.push(
            text(description)
                .size(12)
                .color(theme::TEXT_2)
                .width(Fill),
        );
    }

    if !collection.items.is_empty() {
        let preview = collection
            .items
            .iter()
            .take(5)
            .map(|item| item.title.as_deref().unwrap_or(item.appid.as_str()))
            .collect::<Vec<_>>()
            .join(" · ");
        content = content.push(
            text(format!("Itens: {preview}"))
                .size(10)
                .color(theme::TEXT_3)
                .width(Fill),
        );
    }

    container(content)
        .width(Fill)
        .padding(12)
        .style(theme::empty_panel)
        .into()
}

fn badge<'a>(label: impl Into<String>, color: iced::Color) -> Element<'a, Message> {
    container(text(label.into()).size(10).font(theme::FONT_MEDIUM).color(color))
        .padding([3.0, 8.0])
        .style(theme::badge)
        .into()
}

fn hint<'a>(message: impl Into<String>, color: iced::Color) -> Element<'a, Message> {
    text(message.into()).size(11).color(color).into()
}

fn rating_stars(review: &CommunityReview) -> String {
    let rating = review.rating.unwrap_or_else(|| {
        if review.positive == Some(false) {
            1.0
        } else if review.positive == Some(true) {
            5.0
        } else {
            0.0
        }
    });
    if !rating.is_finite() || rating <= 0.0 {
        return String::new();
    }
    "★".repeat(rating.round().clamp(1.0, 5.0) as usize)
}

fn format_date(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.format("%d/%m/%Y").to_string())
        .unwrap_or_else(|_| value.chars().take(10).collect())
}

fn format_hours(hours: f64) -> String {
    if !hours.is_finite() || hours <= 0.0 {
        return String::new();
    }
    if hours.fract().abs() < 0.05 {
        format!("{hours:.0} h jogadas")
    } else {
        format!("{hours:.1} h jogadas")
    }
}

fn visibility_label(visibility: &str) -> &str {
    match visibility {
        "public" => "Pública",
        "unlisted" => "Não listada",
        "private" => "Privada",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(id: &str) -> CommunityReview {
        CommunityReview {
            id: id.to_string(),
            author: "Autor".to_string(),
            title: None,
            text: "Texto".to_string(),
            rating: Some(5.0),
            positive: Some(true),
            hours: Some(2.0),
            created_at: None,
        }
    }

    fn collection(id: &str) -> CommunityCollection {
        CommunityCollection {
            id: id.to_string(),
            title: format!("Coleção {id}"),
            description: None,
            visibility: Some("public".to_string()),
            owner: "Dono".to_string(),
            item_count: 1,
            items: Vec::new(),
        }
    }

    #[test]
    fn merge_page_respects_offset_and_replaces_overlap() {
        let mut items = vec!["a", "b", "c"];
        merge_page(&mut items, vec!["d", "e"], 3);
        assert_eq!(items, vec!["a", "b", "c", "d", "e"]);

        let mut items = vec!["a", "b", "c"];
        merge_page(&mut items, vec!["B", "C"], 1);
        assert_eq!(items, vec!["a", "B", "C"]);
    }

    #[test]
    fn cache_reuses_the_same_state_per_appid() {
        let mut cache = CommunityCache::default();
        let (state, fresh) = cache.ensure("10");
        assert!(fresh);
        state.tab = CommunityTab::Collections;

        let (state, fresh) = cache.ensure("10");
        assert!(!fresh);
        assert_eq!(state.tab, CommunityTab::Collections);

        let (_, fresh) = cache.ensure("20");
        assert!(fresh);
        assert!(cache.get("10").is_some());
        assert!(cache.get("20").is_some());
    }

    #[test]
    fn applying_pages_merges_offsets_and_marks_offline() {
        let mut state = CommunityState::default();
        state.apply_reviews_page(
            CommunityReviewsPage {
                reviews: vec![review("1"), review("2")],
                pagination: crate::backend::CommunityPagination {
                    limit: 2,
                    offset: 0,
                    has_more: true,
                    total: Some(3),
                },
                offline: false,
            },
            0,
            false,
        );
        assert_eq!(state.reviews.items.len(), 2);
        assert_eq!(state.reviews.next_offset, 2);
        assert!(state.reviews.has_more);

        state.apply_reviews_page(
            CommunityReviewsPage {
                reviews: vec![review("3")],
                pagination: crate::backend::CommunityPagination {
                    limit: 2,
                    offset: 2,
                    has_more: false,
                    total: Some(3),
                },
                offline: true,
            },
            2,
            true,
        );
        let ids: Vec<&str> = state
            .reviews
            .items
            .iter()
            .map(|review| review.id.as_str())
            .collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
        assert!(state.reviews.offline);
        assert!(!state.reviews.has_more);
        assert_eq!(state.reviews.next_offset, 3);
    }

    #[test]
    fn applying_offline_pages_dedupes_repeated_ids() {
        let mut state = CommunityState::default();
        state.apply_reviews_page(
            CommunityReviewsPage {
                reviews: vec![review("1"), review("2")],
                pagination: crate::backend::CommunityPagination {
                    limit: 2,
                    offset: 0,
                    has_more: true,
                    total: None,
                },
                offline: false,
            },
            0,
            false,
        );
        state.apply_reviews_page(
            CommunityReviewsPage {
                reviews: vec![review("1"), review("2")],
                pagination: crate::backend::CommunityPagination {
                    limit: 2,
                    offset: 0,
                    has_more: true,
                    total: None,
                },
                offline: true,
            },
            2,
            true,
        );
        assert_eq!(state.reviews.items.len(), 2);
        assert!(state.reviews.offline);
        assert!(!state.reviews.has_more);
    }

    #[test]
    fn applying_collections_keeps_existing_items_on_append() {
        let mut state = CommunityState::default();
        state.apply_collections_page(
            CommunityCollectionsPage {
                collections: vec![collection("a")],
                pagination: crate::backend::CommunityPagination {
                    limit: 1,
                    offset: 0,
                    has_more: true,
                    total: None,
                },
                offline: false,
            },
            0,
            false,
        );
        state.apply_collections_page(
            CommunityCollectionsPage {
                collections: vec![collection("b")],
                pagination: crate::backend::CommunityPagination {
                    limit: 1,
                    offset: 1,
                    has_more: false,
                    total: None,
                },
                offline: true,
            },
            1,
            true,
        );
        assert_eq!(state.collections.items.len(), 2);
        assert_eq!(state.collections.items[1].title, "Coleção b");
        assert!(state.collections.offline);
    }
}
