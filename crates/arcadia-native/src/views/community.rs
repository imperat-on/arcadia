//! Painel de Comunidade reutilizável pela ficha do jogo e pelo detalhe da loja.
//!
//! Reviews e coleções públicas com paginação explícita, estado offline e cache
//! por appid na sessão. Mutações usam os canais reais e só aparecem com sessão;
//! falhas de login/offline viram banner com a mensagem do backend.

use std::collections::HashMap;

use iced::widget::{button, column, container, row, text, text_input, Space};
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
    pub fn label<'a>(self, i18n: &'a crate::i18n::I18n) -> &'a str {
        match self {
            Self::Reviews => i18n.t("reviews.titulo"),
            Self::Collections => i18n.t("community.colecoes"),
        }
    }
}

/// Alvo de confirmação/denúncia; o id fica ao lado no estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunityTarget {
    Review,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommunityVisibility {
    #[default]
    Public,
    Unlisted,
    Private,
}

impl CommunityVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unlisted => "unlisted",
            Self::Private => "private",
        }
    }

    pub fn label<'a>(self, i18n: &'a crate::i18n::I18n) -> &'a str {
        match self {
            Self::Public => i18n.t("community.visibilidade.publica"),
            Self::Unlisted => i18n.t("community.visibilidade.nao_listada"),
            Self::Private => i18n.t("community.visibilidade.privada"),
        }
    }

    pub fn from_value(value: Option<&str>) -> Self {
        match value {
            Some("unlisted") => Self::Unlisted,
            Some("private") => Self::Private,
            _ => Self::Public,
        }
    }
}

/// Rascunho validado, pronto para virar payload de `community:review:*`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommunityReviewDraft {
    pub title: Option<String>,
    pub text: String,
    pub rating: u8,
    pub hours: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFormState {
    /// `Some(id)` edita a review existente; `None` cria uma nova.
    pub editing: Option<String>,
    pub title: String,
    pub text: String,
    pub rating: u8,
    pub hours: String,
}

impl Default for ReviewFormState {
    fn default() -> Self {
        Self {
            editing: None,
            title: String::new(),
            text: String::new(),
            rating: 5,
            hours: String::new(),
        }
    }
}

impl ReviewFormState {
    pub fn for_review(review: &CommunityReview) -> Self {
        let rating = review
            .rating
            .filter(|rating| rating.is_finite() && *rating > 0.0)
            .map(|rating| rating.round().clamp(1.0, 5.0) as u8)
            .unwrap_or(if review.positive == Some(false) { 1 } else { 5 });
        Self {
            editing: Some(review.id.clone()),
            title: review.title.clone().unwrap_or_default(),
            text: review.text.clone(),
            rating,
            hours: review
                .hours
                .filter(|hours| hours.is_finite() && *hours > 0.0)
                .map(|hours| {
                    if hours.fract().abs() < 0.05 {
                        format!("{hours:.0}")
                    } else {
                        format!("{hours:.1}")
                    }
                })
                .unwrap_or_default(),
        }
    }

    pub fn validate(&self) -> Result<CommunityReviewDraft, CommunityFormError> {
        let text = self.text.trim();
        if text.is_empty() {
            return Err(CommunityFormError::EmptyReviewText);
        }
        if !(1..=5).contains(&self.rating) {
            return Err(CommunityFormError::InvalidRating);
        }
        let title = self.title.trim();
        let hours = self.hours.trim();
        let hours = if hours.is_empty() {
            0.0
        } else {
            hours
                .parse::<f64>()
                .ok()
                .filter(|hours| hours.is_finite())
                .map(|hours| hours.max(0.0))
                .ok_or(CommunityFormError::InvalidHours)?
        };
        Ok(CommunityReviewDraft {
            title: (!title.is_empty()).then(|| title.to_string()),
            text: text.to_string(),
            rating: self.rating,
            hours,
        })
    }
}

/// Rascunho validado de coleção; `visibility` já é o valor do contrato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityCollectionDraft {
    pub title: String,
    pub description: Option<String>,
    pub visibility: CommunityVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionFormState {
    /// `Some(id)` edita a coleção existente; `None` cria uma nova.
    pub editing: Option<String>,
    pub title: String,
    pub description: String,
    pub visibility: CommunityVisibility,
}

impl Default for CollectionFormState {
    fn default() -> Self {
        Self {
            editing: None,
            title: String::new(),
            description: String::new(),
            visibility: CommunityVisibility::Public,
        }
    }
}

impl CollectionFormState {
    pub fn for_collection(collection: &CommunityCollection) -> Self {
        Self {
            editing: Some(collection.id.clone()),
            title: collection.title.clone(),
            description: collection.description.clone().unwrap_or_default(),
            visibility: CommunityVisibility::from_value(collection.visibility.as_deref()),
        }
    }

    pub fn validate(&self) -> Result<CommunityCollectionDraft, CommunityFormError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(CommunityFormError::EmptyCollectionTitle);
        }
        let description = self.description.trim();
        Ok(CommunityCollectionDraft {
            title: title.to_string(),
            description: (!description.is_empty()).then(|| description.to_string()),
            visibility: self.visibility,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunityFormError {
    EmptyReviewText,
    InvalidRating,
    InvalidHours,
    EmptyCollectionTitle,
    EmptyReportReason,
}

impl CommunityFormError {
    pub fn message<'a>(self, i18n: &'a crate::i18n::I18n) -> &'a str {
        match self {
            Self::EmptyReviewText => i18n.t("community.erro_texto_avaliacao"),
            Self::InvalidRating => i18n.t("native.community.invalid_rating"),
            Self::InvalidHours => i18n.t("native.community.invalid_hours"),
            Self::EmptyCollectionTitle => i18n.t("community.erro_titulo_colecao"),
            Self::EmptyReportReason => i18n.t("native.community.report_reason_required"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityReportState {
    pub target: CommunityTarget,
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommunityConfirm {
    Review(String),
    Collection(String),
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
    /// Offset da última página aplicada; recarregar após mutar usa este valor.
    pub page_offset: u64,
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
            page_offset: 0,
            total: None,
            loading_more: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommunityState {
    pub last_open_load: Option<std::time::Instant>,
    pub tab: CommunityTab,
    pub reviews: CommunityList<CommunityReview>,
    pub collections: CommunityList<CommunityCollection>,
    /// Appid normalizado (sem `steam:`) desta ficha.
    pub appid: String,
    /// Sessão que pode mutar; `None` mantém os botões desabilitados.
    pub viewer_id: Option<String>,
    /// Lista restrita às coleções da sessão via `mine=true`.
    pub mine_only: bool,
    pub review_form: Option<ReviewFormState>,
    pub collection_form: Option<CollectionFormState>,
    pub confirm: Option<CommunityConfirm>,
    pub report: Option<CommunityReportState>,
    pub mutation_busy: bool,
    pub mutation_error: Option<String>,
    pub mutation_notice: Option<String>,
}

impl CommunityState {
    pub fn is_signed_in(&self) -> bool {
        self.viewer_id.is_some()
    }

    pub fn is_own_review(&self, review: &CommunityReview) -> bool {
        review
            .user_id
            .as_deref()
            .is_some_and(|id| self.viewer_id.as_deref() == Some(id))
    }

    pub fn is_own_collection(&self, collection: &CommunityCollection) -> bool {
        self.mine_only
            || collection
                .user_id
                .as_deref()
                .is_some_and(|id| self.viewer_id.as_deref() == Some(id))
    }

    pub fn has_current_game(&self, collection: &CommunityCollection) -> bool {
        !self.appid.is_empty() && collection.items.iter().any(|item| item.appid == self.appid)
    }

    pub fn apply_reviews_page(&mut self, page: CommunityReviewsPage, offset: u64, append: bool) {
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
    viewer_id: Option<String>,
}

impl CommunityCache {
    /// Atualiza a sessão visível nos painéis em cache. Quando a identidade
    /// muda (login, logout ou troca de conta), o cache é descartado: dados e
    /// formulários da sessão anterior não podem reaparecer para outra conta.
    pub fn set_viewer(&mut self, viewer_id: Option<String>) -> bool {
        if self.viewer_id == viewer_id {
            return false;
        }
        self.viewer_id = viewer_id;
        self.entries.clear();
        true
    }

    /// Devolve o estado do appid e se ele acabou de ser criado (primeira
    /// carga). O chamador usa `true` para disparar as duas primeiras páginas.
    pub fn ensure(&mut self, appid: &str) -> (&mut CommunityState, bool) {
        let fresh = !self.entries.contains_key(appid);
        if fresh {
            self.entries
                .insert(appid.to_string(), CommunityState::default());
        }
        let viewer_id = self.viewer_id.clone();
        let state = self.entries.get_mut(appid).expect("estado recém-inserido");
        state.appid = appid.to_string();
        state.viewer_id = viewer_id;
        (state, fresh)
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
    list.page_offset = offset;
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

pub fn panel<'a>(
    i18n: &'a crate::i18n::I18n,
    title: &'a str,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let header = row![
        column![
            text(i18n.t("community.titulo"))
                .size(17)
                .font(theme::FONT_BOLD)
                .color(theme::TEXT_1),
            text(i18n.t_args("community.subtitulo", &[("title", title)]))
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_2),
        ]
        .spacing(3)
        .width(Fill),
        row![
            tab_button(
                CommunityTab::Reviews.label(i18n),
                CommunityTab::Reviews,
                state.tab
            ),
            tab_button(
                CommunityTab::Collections.label(i18n),
                CommunityTab::Collections,
                state.tab
            ),

        ]
        .spacing(6),
    ]
    .spacing(theme::SPACE_3)
    .align_y(iced::Center);

    let body = match state.tab {
        CommunityTab::Reviews => reviews_body(i18n, state),
        CommunityTab::Collections => collections_body(i18n, title, state),
    };

    let mut content = column![header].spacing(theme::SPACE_3).width(Fill);
    if let Some(error) = state.mutation_error.as_deref() {
        content = content.push(views::action_error_banner(error, None));
    }
    if let Some(notice) = state.mutation_notice.as_deref() {
        content = content.push(success_banner(notice));
    }
    content = content.push(body);

    container(content)
        .width(Fill)
        .padding(theme::SPACE_4)
        .style(theme::stub_panel)
        .into()
}

fn tab_button<'a>(label: &'a str, tab: CommunityTab, active: CommunityTab) -> Element<'a, Message> {
    let is_active = tab == active;
    let mut control = button(
        text(label)
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM)
            .color(if is_active {
                theme::TEXT_1
            } else {
                theme::TEXT_2
            }),
    )
    .padding([6.0, 12.0])
    .style(move |theme, status| theme::segment_button(theme, status, is_active));
    if !is_active {
        control = control.on_press(Message::CommunityTabChanged(tab));
    }
    control.into()
}

fn reviews_body<'a>(
    i18n: &'a crate::i18n::I18n,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let list = &state.reviews;
    let mut content = column![].spacing(theme::SPACE_2).width(Fill);
    content = content.push(reviews_controls(i18n, state));
    if let Some(form) = state.review_form.as_ref() {
        content = content.push(review_form(i18n, form, state.mutation_busy));
    }
    content = content.push(status_banner(
        i18n,
        &list.status,
        list.offline,
        CommunityTab::Reviews,
    ));

    if list.items.is_empty() {
        if matches!(list.status, LoadStatus::Idle | LoadStatus::Loading) {
            content = content.push(hint(
                i18n.t("native.community.loading_reviews"),
                theme::TEXT_3,
            ));
        } else if matches!(list.status, LoadStatus::Ready) {
            content = content.push(hint(i18n.t("community.vazio_avaliacoes"), theme::TEXT_3));
        }
    } else {
        for review in &list.items {
            content = content.push(review_card(i18n, review, state));
        }
        if let Some(error) = list.error.as_deref() {
            content = content.push(views::action_error_banner_labeled(
                error,
                Some(Message::CommunityRefresh(CommunityTab::Reviews)),
                i18n.t("downloads.tentar_novamente"),
            ));
        }
        if list.loading_more {
            content = content.push(hint(
                i18n.t("native.community.loading_more_reviews"),
                theme::TEXT_3,
            ));
        } else if list.has_more {
            content = content.push(load_more_button(i18n, CommunityTab::Reviews));
        }
        content = content.push(hint(
            i18n.t_args(
                "native.community.reviews_count",
                &[("count", &list.items.len().to_string())],
            ),
            theme::TEXT_3,
        ));
    }

    content.into()
}

fn collections_body<'a>(
    i18n: &'a crate::i18n::I18n,
    title: &'a str,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let list = &state.collections;
    let mut content = column![].spacing(theme::SPACE_2).width(Fill);
    content = content.push(collections_controls(i18n, state));
    if let Some(form) = state.collection_form.as_ref() {
        content = content.push(collection_form(i18n, title, state, form));
    }
    content = content.push(status_banner(
        i18n,
        &list.status,
        list.offline,
        CommunityTab::Collections,
    ));

    if list.items.is_empty() {
        if matches!(list.status, LoadStatus::Idle | LoadStatus::Loading) {
            content = content.push(hint(
                i18n.t("native.community.loading_collections"),
                theme::TEXT_3,
            ));
        } else if matches!(list.status, LoadStatus::Ready) {
            content = content.push(hint(i18n.t("community.vazio_colecoes"), theme::TEXT_3));
        }
    } else {
        for collection in &list.items {
            content = content.push(collection_card(i18n, collection, state));
        }
        if let Some(error) = list.error.as_deref() {
            content = content.push(views::action_error_banner_labeled(
                error,
                Some(Message::CommunityRefresh(CommunityTab::Collections)),
                i18n.t("downloads.tentar_novamente"),
            ));
        }
        if list.loading_more {
            content = content.push(hint(
                i18n.t("native.community.loading_more_collections"),
                theme::TEXT_3,
            ));
        } else if list.has_more {
            content = content.push(load_more_button(i18n, CommunityTab::Collections));
        }
        content = content.push(hint(
            i18n.t_args(
                "native.community.collections_count",
                &[("count", &list.items.len().to_string())],
            ),
            theme::TEXT_3,
        ));
    }

    content.into()
}

fn reviews_controls<'a>(
    i18n: &'a crate::i18n::I18n,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let open = state.review_form.is_some();
    let label = if !state.is_signed_in() {
        i18n.t("native.community.login_cta")
    } else if open {
        i18n.t("common.cancelar")
    } else {
        i18n.t("community.escrever_avaliacao")
    };
    let message = if !state.is_signed_in() {
        None
    } else if open {
        Some(Message::CommunityReviewFormClosed)
    } else {
        Some(Message::CommunityReviewFormOpened)
    };
    row![
        Space::new().width(Fill),
        button(
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM)
        )
        .on_press_maybe(message)
        .padding([7.0, 12.0])
        .style(theme::primary_button),
    ]
    .width(Fill)
    .into()
}

fn collections_controls<'a>(
    i18n: &'a crate::i18n::I18n,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let mut controls = row![].spacing(6);
    if state.is_signed_in() {
        controls = controls.push(segment_action(
            i18n.t("community.minhas_colecoes"),
            Message::CommunityMineToggled(!state.mine_only),
            state.mine_only,
            !state.mutation_busy,
        ));
    }
    let open = state.collection_form.is_some();
    let label = if !state.is_signed_in() {
        i18n.t("native.community.login_cta")
    } else if open {
        i18n.t("common.cancelar")
    } else {
        i18n.t("community.criar_colecao")
    };
    let message = if !state.is_signed_in() {
        None
    } else if open {
        Some(Message::CommunityCollectionFormClosed)
    } else {
        Some(Message::CommunityCollectionFormOpened)
    };
    controls = controls.push(Space::new().width(Fill));
    controls = controls.push(
        button(
            text(label)
                .size(theme::TEXT_CAPTION)
                .font(theme::FONT_MEDIUM),
        )
        .on_press_maybe(message)
        .padding([7.0, 12.0])
        .style(theme::primary_button),
    );
    controls.width(Fill).into()
}

fn segment_action<'a>(
    label: &'a str,
    message: Message,
    active: bool,
    enabled: bool,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM)
            .color(if active { theme::TEXT_1 } else { theme::TEXT_2 }),
    )
    .on_press_maybe(enabled.then_some(message))
    .padding([7.0, 12.0])
    .style(move |style_theme, status| theme::segment_button(style_theme, status, active))
    .into()
}

fn review_form<'a>(
    i18n: &'a crate::i18n::I18n,
    form: &'a ReviewFormState,
    busy: bool,
) -> Element<'a, Message> {
    let mut rating = row![].spacing(4);
    for value in 1..=5u8 {
        rating = rating.push(
            button(
                text(value.to_string())
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM)
                    .color(if form.rating == value {
                        theme::TEXT_1
                    } else {
                        theme::TEXT_2
                    }),
            )
            .on_press_maybe((!busy).then_some(Message::CommunityReviewRatingChanged(value)))
            .padding([6.0, 10.0])
            .style(move |style_theme, status| {
                theme::segment_button(style_theme, status, form.rating == value)
            }),
        );
    }

    let submit = if form.editing.is_some() {
        i18n.t("community.salvar_alteracoes")
    } else {
        i18n.t("community.publicar")
    };

    container(
        column![
            row![
                column![
                    text(i18n.t("community.titulo_opcional"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_3),
                    text_input(i18n.t("community.titulo_opcional"), &form.title)
                        .on_input(Message::CommunityReviewTitleChanged)
                        .padding([7.0, 10.0])
                        .size(theme::TEXT_BODY)
                        .width(Fill)
                        .style(theme::search_input),
                ]
                .spacing(3)
                .width(Fill),
                column![
                    text(i18n.t("community.nota"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_3),
                    rating,
                ]
                .spacing(3),
                column![
                    text(i18n.t("community.horas"))
                        .size(theme::TEXT_MICRO)
                        .color(theme::TEXT_3),
                    text_input("0.0", &form.hours)
                        .on_input(Message::CommunityReviewHoursChanged)
                        .padding([7.0, 10.0])
                        .size(theme::TEXT_BODY)
                        .width(90)
                        .style(theme::search_input),
                ]
                .spacing(3),
            ]
            .spacing(theme::SPACE_2)
            .align_y(iced::Bottom),
            text_input(i18n.t("community.avaliacao"), &form.text)
                .on_input(Message::CommunityReviewTextChanged)
                .padding([7.0, 10.0])
                .size(theme::TEXT_BODY)
                .width(Fill)
                .style(theme::search_input),
            row![
                Space::new().width(Fill),
                button(
                    text(i18n.t("common.cancelar"))
                        .size(theme::TEXT_CAPTION)
                        .font(theme::FONT_MEDIUM)
                )
                .on_press(Message::CommunityReviewFormClosed)
                .padding([7.0, 12.0])
                .style(theme::ghost_button),
                button(
                    text(submit)
                        .size(theme::TEXT_CAPTION)
                        .font(theme::FONT_MEDIUM)
                )
                .on_press_maybe((!busy).then_some(Message::CommunityReviewSubmit))
                .padding([7.0, 12.0])
                .style(theme::primary_button),
            ]
            .spacing(theme::SPACE_2),
        ]
        .spacing(theme::SPACE_2),
    )
    .width(Fill)
    .padding(theme::SPACE_3)
    .style(theme::empty_panel)
    .into()
}

fn collection_form<'a>(
    i18n: &'a crate::i18n::I18n,
    game_title: &'a str,
    state: &'a CommunityState,
    form: &'a CollectionFormState,
) -> Element<'a, Message> {
    let mut visibility = row![].spacing(4);
    for option in [
        CommunityVisibility::Public,
        CommunityVisibility::Unlisted,
        CommunityVisibility::Private,
    ] {
        visibility = visibility.push(
            button(
                text(option.label(i18n))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_MEDIUM)
                    .color(if form.visibility == option {
                        theme::TEXT_1
                    } else {
                        theme::TEXT_2
                    }),
            )
            .on_press_maybe(
                (!state.mutation_busy)
                    .then_some(Message::CommunityCollectionVisibilityChanged(option)),
            )
            .padding([6.0, 9.0])
            .style(move |style_theme, status| {
                theme::segment_button(style_theme, status, form.visibility == option)
            }),
        );
    }

    let mut content = column![
        row![
            column![
                text(i18n.t("community.titulo_colecao"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
                text_input(i18n.t("community.titulo_colecao"), &form.title)
                    .on_input(Message::CommunityCollectionTitleChanged)
                    .padding([7.0, 10.0])
                    .size(theme::TEXT_BODY)
                    .width(Fill)
                    .style(theme::search_input),
            ]
            .spacing(3)
            .width(Fill),
            column![
                text(i18n.t("community.visibilidade"))
                    .size(theme::TEXT_MICRO)
                    .color(theme::TEXT_3),
                visibility,
            ]
            .spacing(3),
        ]
        .spacing(theme::SPACE_2)
        .align_y(iced::Bottom),
        text_input(i18n.t("community.descricao_opcional"), &form.description)
            .on_input(Message::CommunityCollectionDescriptionChanged)
            .padding([7.0, 10.0])
            .size(theme::TEXT_BODY)
            .width(Fill)
            .style(theme::search_input),
    ]
    .spacing(theme::SPACE_2);
    if form.editing.is_none() {
        let game = if game_title.trim().is_empty() {
            i18n.t_args("community.para_jogo", &[("appid", &state.appid)])
        } else {
            game_title.to_string()
        };
        content = content.push(hint(
            i18n.t_args("community.colecao_comeca", &[("game", &game)]),
            theme::TEXT_3,
        ));
    }
    let submit = if form.editing.is_some() {
        i18n.t("community.salvar_alteracoes")
    } else {
        i18n.t("community.criar")
    };
    content = content.push(
        row![
            Space::new().width(Fill),
            button(
                text(i18n.t("common.cancelar"))
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press(Message::CommunityCollectionFormClosed)
            .padding([7.0, 12.0])
            .style(theme::ghost_button),
            button(
                text(submit)
                    .size(theme::TEXT_CAPTION)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press_maybe((!state.mutation_busy).then_some(Message::CommunityCollectionSubmit),)
            .padding([7.0, 12.0])
            .style(theme::primary_button),
        ]
        .spacing(theme::SPACE_2),
    );

    container(content)
        .width(Fill)
        .padding(theme::SPACE_3)
        .style(theme::empty_panel)
        .into()
}

fn confirm_row<'a>(
    i18n: &'a crate::i18n::I18n,
    target: CommunityTarget,
    busy: bool,
) -> Element<'a, Message> {
    let question = match target {
        CommunityTarget::Review => i18n.t("community.confirmar_remover_avaliacao"),
        CommunityTarget::Collection => i18n.t("community.confirmar_remover_colecao"),
    };
    row![
        text(question).size(theme::TEXT_MICRO).color(theme::TEXT_2),
        button(
            text(i18n.t("community.remover"))
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_MEDIUM)
        )
        .on_press_maybe((!busy).then_some(Message::CommunityConfirmAccepted))
        .padding([5.0, 9.0])
        .style(theme::ghost_button),
        button(
            text(i18n.t("common.cancelar"))
                .size(theme::TEXT_MICRO)
                .font(theme::FONT_MEDIUM)
        )
        .on_press(Message::CommunityConfirmCancelled)
        .padding([5.0, 9.0])
        .style(theme::ghost_button),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn report_form<'a>(
    i18n: &'a crate::i18n::I18n,
    report: &'a CommunityReportState,
    busy: bool,
) -> Element<'a, Message> {
    let prompt = match report.target {
        CommunityTarget::Review => i18n.t("community.prompt_denunciar"),
        CommunityTarget::Collection => i18n.t("community.prompt_denunciar_colecao"),
    };
    column![
        text(prompt).size(theme::TEXT_MICRO).color(theme::TEXT_2),
        row![
            text_input(i18n.t("community.conteudo_inadequado"), &report.reason)
                .on_input(Message::CommunityReportReasonChanged)
                .padding([6.0, 9.0])
                .size(theme::TEXT_CAPTION)
                .width(Fill)
                .style(theme::search_input),
            button(
                text(i18n.t("community.denunciar"))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press_maybe((!busy).then_some(Message::CommunityReportSubmit))
            .padding([5.0, 9.0])
            .style(theme::primary_button),
            button(
                text(i18n.t("common.cancelar"))
                    .size(theme::TEXT_MICRO)
                    .font(theme::FONT_MEDIUM)
            )
            .on_press(Message::CommunityReportCancelled)
            .padding([5.0, 9.0])
            .style(theme::ghost_button),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(3)
    .width(Fill)
    .into()
}

fn success_banner(message: &str) -> Element<'_, Message> {
    container(
        text(message)
            .size(theme::TEXT_BODY)
            .color(theme::SUCCESS)
            .width(Fill),
    )
    .width(Fill)
    .padding([10.0, 14.0])
    .style(theme::stub_panel)
    .into()
}

fn status_banner<'a>(
    i18n: &'a crate::i18n::I18n,
    status: &'a LoadStatus,
    offline: bool,
    tab: CommunityTab,
) -> Element<'a, Message> {
    match status {
        LoadStatus::Error(message) => views::action_error_banner_labeled(
            message,
            Some(Message::CommunityRefresh(tab)),
            i18n.t("downloads.tentar_novamente"),
        ),
        _ if offline => views::warning_banner(i18n.t("native.community.offline_cache")),
        LoadStatus::Idle | LoadStatus::Loading | LoadStatus::Ready => {
            iced::widget::Space::new().height(0).into()
        }
    }
}

fn load_more_button<'a>(i18n: &'a crate::i18n::I18n, tab: CommunityTab) -> Element<'a, Message> {
    button(
        text(i18n.t("native.community.load_more"))
            .size(theme::TEXT_CAPTION)
            .font(theme::FONT_MEDIUM),
    )
    .on_press(Message::CommunityLoadMore(tab))
    .padding([7.0, 14.0])
    .style(theme::ghost_button)
    .into()
}

fn review_card<'a>(
    i18n: &'a crate::i18n::I18n,
    review: &'a CommunityReview,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let stars = rating_stars(review);
    let date = review
        .created_at
        .as_deref()
        .map(format_date)
        .filter(|date| !date.is_empty());

    let mut header = row![text(review.author.as_str())
        .size(theme::TEXT_BODY)
        .font(theme::FONT_MEDIUM)
        .color(theme::TEXT_1),]
    .spacing(8)
    .align_y(iced::Center);
    if !stars.is_empty() {
        header = header.push(text(stars).size(theme::TEXT_CAPTION).color(theme::WARNING));
    }
    if let Some(positive) = review.positive {
        header = header.push(badge(
            if positive {
                i18n.t("native.community.positive")
            } else {
                i18n.t("native.community.negative")
            },
            if positive {
                theme::SUCCESS
            } else {
                theme::DANGER
            },
        ));
    }
    if let Some(date) = date {
        header = header.push(iced::widget::Space::new().width(Fill));
        header = header.push(text(date).size(theme::TEXT_MICRO).color(theme::TEXT_3));
    }

    let mut content = column![header].spacing(6).width(Fill);
    if let Some(title) = review.title.as_deref().filter(|title| !title.is_empty()) {
        content = content.push(
            text(title)
                .size(theme::TEXT_READ)
                .font(theme::FONT_MEDIUM)
                .color(theme::TEXT_STRONG),
        );
    }
    content = content.push(
        text(if review.text.is_empty() {
            i18n.t("community.sem_texto")
        } else {
            review.text.as_str()
        })
        .size(theme::TEXT_BODY)
        .color(theme::TEXT_2)
        .width(Fill),
    );

    let hours = review
        .hours
        .map(|hours| format_hours(i18n, hours))
        .unwrap_or_default();
    if !hours.is_empty() {
        content = content.push(text(hours).size(theme::TEXT_MICRO).color(theme::TEXT_3));
    }
    if state.is_signed_in() {
        content = content.push(review_actions(i18n, review, state));
    }

    container(content)
        .width(Fill)
        .padding(12)
        .style(theme::empty_panel)
        .into()
}

fn review_actions<'a>(
    i18n: &'a crate::i18n::I18n,
    review: &'a CommunityReview,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    if matches!(
        state.confirm.as_ref(),
        Some(CommunityConfirm::Review(id)) if id == &review.id
    ) {
        return confirm_row(i18n, CommunityTarget::Review, state.mutation_busy);
    }
    if state
        .report
        .as_ref()
        .is_some_and(|report| report.target == CommunityTarget::Review && report.id == review.id)
    {
        if let Some(report) = state.report.as_ref() {
            return report_form(i18n, report, state.mutation_busy);
        }
    }

    let enabled = !state.mutation_busy;
    let mut actions = row![].spacing(8);
    if state.is_own_review(review) {
        actions = actions.push(text_action(
            i18n.t("community.editar"),
            Message::CommunityReviewEditRequested(review.id.clone()),
            enabled,
        ));
        actions = actions.push(text_action(
            i18n.t("community.remover"),
            Message::CommunityConfirmRequested(CommunityTarget::Review, review.id.clone()),
            enabled,
        ));
    }
    actions = actions.push(text_action(
        i18n.t("community.denunciar"),
        Message::CommunityReportRequested(CommunityTarget::Review, review.id.clone()),
        enabled,
    ));
    actions.into()
}

fn text_action<'a>(label: &'a str, message: Message, enabled: bool) -> Element<'a, Message> {
    button(text(label).size(theme::TEXT_MICRO).font(theme::FONT_MEDIUM))
        .on_press_maybe(enabled.then_some(message))
        .padding([4.0, 6.0])
        .style(theme::ghost_button)
        .into()
}

fn collection_card<'a>(
    i18n: &'a crate::i18n::I18n,
    collection: &'a CommunityCollection,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    let visibility = collection
        .visibility
        .as_deref()
        .map(|visibility| visibility_label(i18n, visibility))
        .unwrap_or_else(|| i18n.t("community.visibilidade.publica"));
    let mut header = row![
        text(collection.title.as_str())
            .size(theme::TEXT_READ)
            .font(theme::FONT_MEDIUM)
            .color(theme::TEXT_1)
            .width(Fill),
        badge(visibility, theme::TEXT_2),
    ]
    .spacing(8)
    .align_y(iced::Center);

    header = header.push(badge(
        i18n.t_args(
            "native.community.items_count",
            &[("count", &collection.item_count.to_string())],
        ),
        theme::ACCENT,
    ));

    let mut content = column![
        header,
        text(i18n.t_args("native.community.by_owner", &[("owner", &collection.owner)]))
            .size(theme::TEXT_MICRO)
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
                .size(theme::TEXT_BODY)
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
            text(i18n.t_args("native.community.items_preview", &[("preview", &preview)]))
                .size(theme::TEXT_MICRO)
                .color(theme::TEXT_3)
                .width(Fill),
        );
    }

    if state.is_signed_in() {
        content = content.push(collection_actions(i18n, collection, state));
    }

    container(content)
        .width(Fill)
        .padding(12)
        .style(theme::empty_panel)
        .into()
}

fn collection_actions<'a>(
    i18n: &'a crate::i18n::I18n,
    collection: &'a CommunityCollection,
    state: &'a CommunityState,
) -> Element<'a, Message> {
    if matches!(
        state.confirm.as_ref(),
        Some(CommunityConfirm::Collection(id)) if id == &collection.id
    ) {
        return confirm_row(i18n, CommunityTarget::Collection, state.mutation_busy);
    }
    if state.report.as_ref().is_some_and(|report| {
        report.target == CommunityTarget::Collection && report.id == collection.id
    }) {
        if let Some(report) = state.report.as_ref() {
            return report_form(i18n, report, state.mutation_busy);
        }
    }

    let enabled = !state.mutation_busy;
    let own = state.is_own_collection(collection);
    let mut actions = row![].spacing(8);
    if own {
        actions = actions.push(text_action(
            i18n.t("community.editar"),
            Message::CommunityCollectionEditRequested(collection.id.clone()),
            enabled,
        ));
        actions = actions.push(text_action(
            i18n.t("community.remover"),
            Message::CommunityConfirmRequested(CommunityTarget::Collection, collection.id.clone()),
            enabled,
        ));
        if !state.appid.is_empty() {
            if state.has_current_game(collection) {
                actions = actions.push(text_action(
                    i18n.t("native.community.remove_current_game"),
                    Message::CommunityCollectionItemRemove(collection.id.clone()),
                    enabled,
                ));
            } else {
                actions = actions.push(text_action(
                    i18n.t("native.community.add_current_game"),
                    Message::CommunityCollectionItemAdd(collection.id.clone()),
                    enabled,
                ));
            }
        }
    }
    actions = actions.push(text_action(
        i18n.t("community.denunciar"),
        Message::CommunityReportRequested(CommunityTarget::Collection, collection.id.clone()),
        enabled,
    ));
    actions.into()
}

fn badge<'a>(label: impl Into<String>, color: iced::Color) -> Element<'a, Message> {
    container(
        text(label.into())
            .size(theme::TEXT_MICRO)
            .font(theme::FONT_MEDIUM)
            .color(color),
    )
    .padding([3.0, 8.0])
    .style(theme::badge)
    .into()
}

fn hint<'a>(message: impl Into<String>, color: iced::Color) -> Element<'a, Message> {
    text(message.into())
        .size(theme::TEXT_CAPTION)
        .color(color)
        .into()
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

fn format_hours(i18n: &crate::i18n::I18n, hours: f64) -> String {
    if !hours.is_finite() || hours <= 0.0 {
        return String::new();
    }
    let value = if hours.fract().abs() < 0.05 {
        format!("{hours:.0}")
    } else {
        format!("{hours:.1}")
    };
    i18n.t_args("community.horas_jogadas", &[("h", &value)])
}

fn visibility_label<'a>(i18n: &'a crate::i18n::I18n, visibility: &'a str) -> &'a str {
    match visibility {
        "public" => i18n.t("community.visibilidade.publica"),
        "unlisted" => i18n.t("community.visibilidade.nao_listada"),
        "private" => i18n.t("community.visibilidade.privada"),
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
            user_id: None,
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
            user_id: None,
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
        assert_eq!(state.reviews.page_offset, 2);
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

    #[test]
    fn review_form_rejects_empty_text_and_invalid_rating() {
        let form = ReviewFormState::default();
        assert_eq!(form.validate(), Err(CommunityFormError::EmptyReviewText));

        let form = ReviewFormState {
            text: "Bom jogo".to_string(),
            rating: 0,
            ..ReviewFormState::default()
        };
        assert_eq!(form.validate(), Err(CommunityFormError::InvalidRating));

        let form = ReviewFormState {
            text: "Bom jogo".to_string(),
            rating: 6,
            ..ReviewFormState::default()
        };
        assert_eq!(form.validate(), Err(CommunityFormError::InvalidRating));
    }

    #[test]
    fn review_form_normalizes_title_hours_and_rating() {
        let form = ReviewFormState {
            title: "  Vale a pena  ".to_string(),
            text: "  Texto  ".to_string(),
            rating: 4,
            hours: " 12.5 ".to_string(),
            ..ReviewFormState::default()
        };
        let draft = form.validate().expect("formulário válido");
        assert_eq!(draft.title.as_deref(), Some("Vale a pena"));
        assert_eq!(draft.text, "Texto");
        assert_eq!(draft.rating, 4);
        assert_eq!(draft.hours, 12.5);

        let no_hours = ReviewFormState {
            text: "Texto".to_string(),
            ..ReviewFormState::default()
        };
        assert_eq!(no_hours.validate().expect("horas vazias").hours, 0.0);

        let invalid_hours = ReviewFormState {
            text: "Texto".to_string(),
            hours: "muitas".to_string(),
            ..ReviewFormState::default()
        };
        assert_eq!(
            invalid_hours.validate(),
            Err(CommunityFormError::InvalidHours)
        );
    }

    #[test]
    fn review_form_from_review_prefills_editable_fields() {
        let mut source = review("r1");
        source.title = Some("Título".to_string());
        source.text = "Texto".to_string();
        source.rating = Some(3.0);
        source.hours = Some(7.5);
        let form = ReviewFormState::for_review(&source);
        assert_eq!(form.editing.as_deref(), Some("r1"));
        assert_eq!(form.title, "Título");
        assert_eq!(form.rating, 3);
        assert_eq!(form.hours, "7.5");
    }

    #[test]
    fn collection_form_requires_title_and_keeps_visibility() {
        let form = CollectionFormState::default();
        assert_eq!(
            form.validate(),
            Err(CommunityFormError::EmptyCollectionTitle)
        );

        let form = CollectionFormState {
            title: " Minha lista ".to_string(),
            description: "  ".to_string(),
            visibility: CommunityVisibility::Private,
            ..CollectionFormState::default()
        };
        let draft = form.validate().expect("coleção válida");
        assert_eq!(draft.title, "Minha lista");
        assert_eq!(draft.description, None);
        assert_eq!(draft.visibility.as_str(), "private");
    }

    #[test]
    fn ownership_checks_use_viewer_and_mine_listing() {
        let mut target = review("r1");
        target.user_id = Some("u1".to_string());
        let collection = collection("c1");
        let mut state = CommunityState {
            viewer_id: Some("u1".to_string()),
            mine_only: true,
            appid: "10".to_string(),
            ..CommunityState::default()
        };
        assert!(state.is_own_review(&target));
        assert!(state.is_own_collection(&collection));
        assert!(!state.has_current_game(&collection));

        state.viewer_id = Some("u2".to_string());
        state.mine_only = false;
        assert!(!state.is_own_review(&target));
        assert!(!state.is_own_collection(&collection));
    }

    #[test]
    fn leaving_the_session_clears_private_form_state() {
        let mut cache = CommunityCache::default();
        assert!(cache.set_viewer(Some("u1".to_string())));
        {
            let (state, _) = cache.ensure("10");
            state.mine_only = true;
            state.review_form = Some(ReviewFormState::default());
            state.collection_form = Some(CollectionFormState::default());
        }
        assert!(cache.set_viewer(None));
        assert!(cache.get("10").is_none());
        let (state, _) = cache.ensure("10");
        assert!(!state.mine_only);
        assert!(state.review_form.is_none());
        assert!(state.collection_form.is_none());
        assert!(!state.is_signed_in());
    }
}
