//! Comunidade (reviews e coleções) — port 1:1 de
//! `app/electron/supabase/community.js`.
//!
//! Todas as rotas vivem em `/community/v1` no mesmo host do backend:
//!
//! - `GET/POST /community/v1/reviews`
//! - `PATCH/DELETE /community/v1/reviews/{id}`
//! - `POST /community/v1/reviews/{id}/report`
//! - `GET/POST /community/v1/collections`
//! - `GET/PATCH/DELETE /community/v1/collections/{id}`
//! - `POST/PUT/DELETE /community/v1/collections/{id}/items[/{appid}]`
//! - `POST /community/v1/collections/{id}/report`
//!
//! Regras preservadas:
//!
//! - `limit` default 20, máximo 100, zero/inválido cai no default;
//!   `offset` default 0, negativo/inválido cai em 0 (ver [`PageOptions`]);
//! - `mine` só é enviado quando verdadeiro; `owner` cai para `user_id` e
//!   valores vazios não entram na query string;
//! - IDs de rota usam `encodeURIComponent` (espaço `%20`, barra `%2F`);
//! - a checagem de dono/RLS é do servidor: nenhuma decisão de ownership é
//!   duplicada no cliente, e 401/403 são propagados;
//! - cache por conta em `community_cache.json` (versão 1), usado somente como
//!   fallback offline de leituras; falha de rede vira `{ ok:true, offline:true }`
//!   quando há cache e erro tipado quando não há;
//! - uma resposta que atravessa troca de conta vira `conta_trocada` e nunca é
//!   gravada no cache da conta nova.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};

use arcadia_api::{ApiClient, HttpRequest, Method};
use arcadia_store::accounts::AccountScope;

use crate::error::{normalize_community_error, SocialError};
use crate::util;

pub const COMMUNITY_CACHE_FILE: &str = "community_cache.json";
pub const COMMUNITY_CACHE_VERSION: u64 = 1;
pub const DEFAULT_PAGE_SIZE: u64 = 20;
pub const MAX_PAGE_SIZE: u64 = 100;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Página já normalizada pelas regras do `community.js`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub limit: u64,
    pub offset: u64,
}

/// Opções de paginação/filtro aceitas pelo renderer.
///
/// Use [`PageOptions::from_value`] para aplicar a coerção do JavaScript
/// (`Number.isInteger`, `Math.min`, truthiness) sobre o JSON recebido do IPC.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageOptions {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub mine: bool,
    pub owner: Option<String>,
    pub user_id: Option<String>,
    pub visibility: Option<String>,
}

impl PageOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_mine(mut self, mine: bool) -> Self {
        self.mine = mine;
        self
    }

    /// `pageOptions(options)` + `asPositiveInt`.
    pub fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let limit = match object.and_then(|object| object.get("limit")) {
            Some(value) => as_positive_int(value, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE),
            None => DEFAULT_PAGE_SIZE,
        };
        // `asPositiveInt(...) || DEFAULT_PAGE_SIZE`: 0 vira 20.
        let limit = if limit == 0 { DEFAULT_PAGE_SIZE } else { limit };
        let offset = match object.and_then(|object| object.get("offset")) {
            Some(value) => as_positive_int(value, 0, MAX_SAFE_INTEGER),
            None => 0,
        };
        Self {
            limit: Some(limit as i64),
            offset: Some(offset as i64),
            mine: object
                .and_then(|object| object.get("mine"))
                .map(util::js_truthy)
                .unwrap_or(false),
            owner: object
                .and_then(|object| object.get("owner"))
                .filter(|value| util::js_truthy(value))
                .map(util::js_string),
            user_id: object
                .and_then(|object| object.get("user_id"))
                .filter(|value| util::js_truthy(value))
                .map(util::js_string),
            visibility: object
                .and_then(|object| object.get("visibility"))
                .filter(|value| util::js_truthy(value))
                .map(util::js_string),
        }
    }

    /// Rein aplica os limites no momento de montar a query (idempotente).
    pub fn page(&self) -> Page {
        let limit = match self.limit {
            Some(limit) if limit > 0 => (limit as u64).min(MAX_PAGE_SIZE),
            _ => DEFAULT_PAGE_SIZE,
        };
        let offset = match self.offset {
            Some(offset) if offset >= 0 => (offset as u64).min(MAX_SAFE_INTEGER),
            _ => 0,
        };
        Page { limit, offset }
    }

    /// `options.owner || options.user_id || ""`.
    pub fn owner_or_user_id(&self) -> &str {
        self.owner
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| self.user_id.as_deref().filter(|value| !value.is_empty()))
            .unwrap_or("")
    }

    fn visibility_or_empty(&self) -> &str {
        self.visibility
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("")
    }

    fn collection_cache_key(&self) -> String {
        let page = self.page();
        serde_json::to_string(&json!({
            "limit": page.limit,
            "offset": page.offset,
            "mine": self.mine,
            "owner": self.owner_or_user_id(),
            "visibility": self.visibility_or_empty(),
        }))
        .unwrap_or_else(|_| String::new())
    }
}

fn as_positive_int(value: &Value, fallback: u64, max: u64) -> u64 {
    match util::js_number(value) {
        Some(number) if number.is_finite() && number.fract() == 0.0 && number >= 0.0 => {
            number.min(max as f64) as u64
        }
        _ => fallback,
    }
}

/// Fachada de comunidade. `Clone` compartilha cliente e escopo de conta.
#[derive(Clone)]
pub struct CommunityService {
    client: ApiClient,
    scope: Arc<Mutex<AccountScope>>,
}

impl CommunityService {
    pub fn new(client: ApiClient, scope: AccountScope) -> Self {
        Self {
            client,
            scope: Arc::new(Mutex::new(scope)),
        }
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    /// Troca a conta ativa; requisições em voo detectam a troca e devolvem
    /// `conta_trocada` sem gravar no cache da conta nova.
    pub fn set_active(&self, username: Option<&str>) -> io::Result<()> {
        self.lock_scope().set_active(username)
    }

    pub fn account(&self) -> Option<String> {
        self.lock_scope().active().map(str::to_string)
    }

    pub fn cache_path(&self) -> PathBuf {
        self.lock_scope().account_file(COMMUNITY_CACHE_FILE)
    }

    fn lock_scope(&self) -> std::sync::MutexGuard<'_, AccountScope> {
        self.scope
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn scope_changed(&self, file: &Path) -> bool {
        self.cache_path() != file
    }

    // ── reviews ────────────────────────────────────────────────────────────

    /// `listReviews`: página de reviews do appid, com fallback offline.
    ///
    /// Envelope de sucesso: `{ ok:true, reviews, items, pagination }`.
    pub async fn list_reviews(
        &self,
        appid: &str,
        options: &PageOptions,
    ) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let page = options.page();
        let query = vec![
            ("appid".to_string(), appid.to_string()),
            ("limit".to_string(), page.limit.to_string()),
            ("offset".to_string(), page.offset.to_string()),
        ];
        let reply = self
            .request("/community/v1/reviews", Method::GET, query, None)
            .await;
        if self.scope_changed(&file) {
            return Err(SocialError::AccountSwitched);
        }
        match reply {
            Reply::Success(data) => {
                let normalized = review_list(&data);
                let mut cache = load_cache(&file);
                ensure_object(&mut cache, "reviews")
                    .insert(format!("reviews:{appid}"), normalized.clone());
                save_cache(&file, &cache)?;
                Ok(envelope_with("ok", &normalized))
            }
            Reply::Failure { status, body } => {
                if let Some(cached) = cached_reviews(&file, appid, options) {
                    Ok(cached)
                } else {
                    Err(http_error(status, &body))
                }
            }
            Reply::Offline { detail } => {
                if let Some(cached) = cached_reviews(&file, appid, options) {
                    Ok(cached)
                } else {
                    Err(SocialError::Network(detail))
                }
            }
        }
    }

    /// `createReview`: `POST /community/v1/reviews` com o payload cru.
    ///
    /// Envelope de sucesso: `{ ok:true, review, data }`.
    pub async fn create_review(&self, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let body = payload_or_empty(payload);
        let reply = self
            .request(
                "/community/v1/reviews",
                Method::POST,
                Vec::new(),
                Some(&body),
            )
            .await;
        match reply {
            Reply::Success(data) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                let review = review_value(&data);
                cache_review(&file, &review)?;
                Ok(mutation_envelope("review", &review))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `updateReview`: `PATCH /community/v1/reviews/{encodeURIComponent(id)}`.
    pub async fn update_review(&self, id: &str, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let route = format!("/community/v1/reviews/{}", util::encode_uri_component(id));
        let body = payload_or_empty(payload);
        let reply = self
            .request(&route, Method::PATCH, Vec::new(), Some(&body))
            .await;
        match reply {
            Reply::Success(data) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                let review = review_value(&data);
                patch_cached_review(&file, &review)?;
                Ok(mutation_envelope("review", &review))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `removeReview`: `DELETE /community/v1/reviews/{id}`.
    pub async fn remove_review(&self, id: &str) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let route = format!("/community/v1/reviews/{}", util::encode_uri_component(id));
        let reply = self.request(&route, Method::DELETE, Vec::new(), None).await;
        match reply {
            Reply::Success(_) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                remove_review_from_cache(&file, id)?;
                Ok(json!({ "ok": true }))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `reportReview`: `POST /community/v1/reviews/{id}/report`.
    pub async fn report_review(&self, id: &str, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let route = format!(
            "/community/v1/reviews/{}/report",
            util::encode_uri_component(id)
        );
        let body = payload_or_empty(payload);
        let reply = self
            .request(&route, Method::POST, Vec::new(), Some(&body))
            .await;
        if self.scope_changed(&file) {
            return Err(SocialError::AccountSwitched);
        }
        match reply {
            Reply::Success(data) => Ok(report_envelope(&data)),
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    // ── coleções ───────────────────────────────────────────────────────────

    /// `listCollections`: página de coleções, com fallback offline.
    ///
    /// Envelope de sucesso: `{ ok:true, collections, lists, items, pagination }`.
    pub async fn list_collections(&self, options: &PageOptions) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let page = options.page();
        let mut query = vec![
            ("limit".to_string(), page.limit.to_string()),
            ("offset".to_string(), page.offset.to_string()),
        ];
        if options.mine {
            query.push(("mine".to_string(), "true".to_string()));
        }
        query.push(("owner".to_string(), options.owner_or_user_id().to_string()));
        query.push((
            "visibility".to_string(),
            options.visibility_or_empty().to_string(),
        ));

        let key = options.collection_cache_key();
        let reply = self
            .request("/community/v1/collections", Method::GET, query, None)
            .await;
        if self.scope_changed(&file) {
            return Err(SocialError::AccountSwitched);
        }
        match reply {
            Reply::Success(data) => {
                let normalized = collection_list(&data);
                let mut cache = load_cache(&file);
                ensure_object(&mut cache, "collections").insert(key, normalized.clone());
                save_cache(&file, &cache)?;
                Ok(envelope_with("ok", &normalized))
            }
            Reply::Failure { status, body } => {
                if let Some(cached) = cached_collections(&file, &key) {
                    Ok(cached)
                } else {
                    Err(http_error(status, &body))
                }
            }
            Reply::Offline { detail } => {
                if let Some(cached) = cached_collections(&file, &key) {
                    Ok(cached)
                } else {
                    Err(SocialError::Network(detail))
                }
            }
        }
    }

    /// `getCollection`: `GET /community/v1/collections/{id}`.
    pub async fn get_collection(&self, id: &str) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let key = id.to_string();
        let route = format!(
            "/community/v1/collections/{}",
            util::encode_uri_component(id)
        );
        let reply = self.request(&route, Method::GET, Vec::new(), None).await;
        if self.scope_changed(&file) {
            return Err(SocialError::AccountSwitched);
        }
        match reply {
            Reply::Success(data) => {
                let collection = collection_value(&data);
                let mut cache = load_cache(&file);
                ensure_object(&mut cache, "collectionItems").insert(key, collection.clone());
                save_cache(&file, &cache)?;
                Ok(json!({
                    "ok": true,
                    "collection": collection,
                    "data": collection,
                }))
            }
            Reply::Failure { status, body } => {
                if let Some(cached) = cached_collection(&file, &key) {
                    Ok(json!({
                        "ok": true,
                        "offline": true,
                        "collection": cached,
                        "data": cached,
                    }))
                } else {
                    Err(http_error(status, &body))
                }
            }
            Reply::Offline { detail } => {
                if let Some(cached) = cached_collection(&file, &key) {
                    Ok(json!({
                        "ok": true,
                        "offline": true,
                        "collection": cached,
                        "data": cached,
                    }))
                } else {
                    Err(SocialError::Network(detail))
                }
            }
        }
    }

    /// `createCollection`: `POST /community/v1/collections`.
    pub async fn create_collection(&self, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let body = payload_or_empty(payload);
        let reply = self
            .request(
                "/community/v1/collections",
                Method::POST,
                Vec::new(),
                Some(&body),
            )
            .await;
        match reply {
            Reply::Success(data) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                let collection = collection_value(&data);
                let id = collection
                    .as_object()
                    .and_then(|object| object.get("id"))
                    .map(util::js_string);
                invalidate_collections(&file, id.as_deref())?;
                Ok(mutation_envelope("collection", &collection))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `updateCollection`: `PATCH /community/v1/collections/{id}`.
    pub async fn update_collection(&self, id: &str, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let key = id.to_string();
        let route = format!(
            "/community/v1/collections/{}",
            util::encode_uri_component(id)
        );
        let body = payload_or_empty(payload);
        let reply = self
            .request(&route, Method::PATCH, Vec::new(), Some(&body))
            .await;
        match reply {
            Reply::Success(data) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                let collection = collection_value(&data);
                invalidate_collections(&file, Some(&key))?;
                Ok(mutation_envelope("collection", &collection))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `removeCollection`: `DELETE /community/v1/collections/{id}`.
    pub async fn remove_collection(&self, id: &str) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let key = id.to_string();
        let route = format!(
            "/community/v1/collections/{}",
            util::encode_uri_component(id)
        );
        let reply = self.request(&route, Method::DELETE, Vec::new(), None).await;
        match reply {
            Reply::Success(_) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                invalidate_collections(&file, Some(&key))?;
                Ok(json!({ "ok": true }))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    /// `addCollectionItem`: `POST .../items` com `{ appid }`.
    pub async fn add_collection_item(&self, id: &str, appid: &str) -> Result<Value, SocialError> {
        let body = json!({ "appid": appid });
        self.collection_items(id, Method::POST, Some(&body), None)
            .await
    }

    /// `replaceCollectionItems`: `PUT .../items` com `{ items }`.
    pub async fn replace_collection_items(
        &self,
        id: &str,
        items: &[Value],
    ) -> Result<Value, SocialError> {
        let body = json!({ "items": items });
        self.collection_items(id, Method::PUT, Some(&body), None)
            .await
    }

    /// `removeCollectionItem`: `DELETE .../items/{encodeURIComponent(appid)}`.
    pub async fn remove_collection_item(
        &self,
        id: &str,
        appid: &str,
    ) -> Result<Value, SocialError> {
        self.collection_items(id, Method::DELETE, None, Some(appid))
            .await
    }

    /// `reportCollection`: `POST /community/v1/collections/{id}/report`.
    pub async fn report_collection(&self, id: &str, payload: &Value) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let route = format!(
            "/community/v1/collections/{}/report",
            util::encode_uri_component(id)
        );
        let body = payload_or_empty(payload);
        let reply = self
            .request(&route, Method::POST, Vec::new(), Some(&body))
            .await;
        if self.scope_changed(&file) {
            return Err(SocialError::AccountSwitched);
        }
        match reply {
            Reply::Success(data) => Ok(report_envelope(&data)),
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    async fn collection_items(
        &self,
        id: &str,
        method: Method,
        body: Option<&Value>,
        appid: Option<&str>,
    ) -> Result<Value, SocialError> {
        let file = self.cache_path();
        let key = id.to_string();
        let route = format!(
            "/community/v1/collections/{}/items",
            util::encode_uri_component(id)
        );
        let route = if method == Method::DELETE {
            match appid {
                Some(appid) => format!("{route}/{}", util::encode_uri_component(appid)),
                None => route,
            }
        } else {
            route
        };
        let reply = self.request(&route, method, Vec::new(), body).await;
        match reply {
            Reply::Success(data) => {
                if self.scope_changed(&file) {
                    return Err(SocialError::AccountSwitched);
                }
                invalidate_collections(&file, Some(&key))?;
                Ok(collection_items_envelope(&data))
            }
            Reply::Failure { status, body } => Err(http_error(status, &body)),
            Reply::Offline { detail } => Err(SocialError::Network(detail)),
        }
    }

    // ── HTTP ───────────────────────────────────────────────────────────────

    async fn request(
        &self,
        route: &str,
        method: Method,
        query: Vec<(String, String)>,
        body: Option<&Value>,
    ) -> Reply {
        let base = self.client.config().url.trim_end_matches('/');
        let pairs: Vec<(String, String)> = query
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .collect();
        let suffix = if pairs.is_empty() {
            String::new()
        } else {
            format!(
                "?{}",
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(&pairs)
                    .finish()
            )
        };
        let url = format!("{base}{route}{suffix}");

        let mut request = HttpRequest::new(method, url)
            .header("accept", "application/json")
            .header("apikey", &self.client.config().anon_key);
        if let Some(token) = self.client.auth().access_token().await {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(body) = body {
            let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
            request = request
                .header("content-type", "application/json")
                .body(bytes);
        }

        let response = match self.client.http().send(&request).await {
            Ok(response) => response,
            Err(error) => {
                return Reply::Offline {
                    detail: error.message,
                }
            }
        };
        let status = response.status;
        let text = response.text();
        if response.is_success() {
            let data = if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or_else(|_| json!({ "error": text }))
            };
            Reply::Success(data)
        } else {
            Reply::Failure { status, body: text }
        }
    }
}

enum Reply {
    Success(Value),
    Failure { status: u16, body: String },
    Offline { detail: String },
}

fn http_error(status: u16, body: &str) -> SocialError {
    let normalized = normalize_community_error(Some(body), status);
    let message = normalized
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let code = normalized
        .get("code")
        .and_then(Value::as_str)
        .map(str::to_string);
    let request_id = normalized
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    if status == 401 {
        SocialError::Unauthorized {
            message,
            body: body.to_string(),
        }
    } else {
        SocialError::Backend {
            status,
            message,
            code,
            body: body.to_string(),
            request_id,
        }
    }
}

fn payload_or_empty(payload: &Value) -> Value {
    if util::js_truthy(payload) {
        payload.clone()
    } else {
        json!({})
    }
}

fn envelope_with(first: &str, value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(first.to_string(), Value::Bool(true));
    util::spread_into(&mut out, value);
    Value::Object(out)
}

fn first_truthy(data: &Value, key: &str) -> Option<Value> {
    data.as_object()
        .and_then(|object| object.get(key))
        .filter(|value| util::js_truthy(value))
        .cloned()
}

fn review_value(data: &Value) -> Value {
    first_truthy(data, "review")
        .or_else(|| first_truthy(data, "data"))
        .unwrap_or_else(|| data.clone())
}

fn collection_value(data: &Value) -> Value {
    first_truthy(data, "collection")
        .or_else(|| first_truthy(data, "data"))
        .unwrap_or_else(|| data.clone())
}

fn report_value(data: &Value) -> Value {
    first_truthy(data, "report").unwrap_or_else(|| data.clone())
}

fn review_list(data: &Value) -> Value {
    let reviews = data
        .get("reviews")
        .filter(|value| value.is_array())
        .or_else(|| data.get("items").filter(|value| value.is_array()))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pagination = data
        .get("pagination")
        .filter(|value| util::js_truthy(value))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "limit": reviews.len(),
                "offset": 0,
                "has_more": false,
            })
        });
    json!({
        "reviews": reviews.clone(),
        "items": reviews,
        "pagination": pagination,
    })
}

fn collection_list(data: &Value) -> Value {
    let collections = data
        .get("collections")
        .filter(|value| value.is_array())
        .or_else(|| data.get("lists").filter(|value| value.is_array()))
        .or_else(|| data.get("items").filter(|value| value.is_array()))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pagination = data
        .get("pagination")
        .filter(|value| util::js_truthy(value))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "limit": collections.len(),
                "offset": 0,
                "has_more": false,
            })
        });
    json!({
        "collections": collections.clone(),
        "lists": collections.clone(),
        "items": collections,
        "pagination": pagination,
    })
}

fn mutation_envelope(key: &str, data: &Value) -> Value {
    let value = first_truthy(data, key)
        .or_else(|| first_truthy(data, "data"))
        .unwrap_or_else(|| data.clone());
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    out.insert(key.to_string(), value.clone());
    out.insert("data".into(), value);
    Value::Object(out)
}

fn report_envelope(data: &Value) -> Value {
    json!({ "ok": true, "report": report_value(data) })
}

fn collection_items_envelope(data: &Value) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    if data.is_object() || data.is_array() {
        util::spread_into(&mut out, data);
    }
    let collection = first_truthy(data, "collection").or_else(|| first_truthy(data, "data"));
    match collection {
        Some(collection) => {
            out.insert("collection".into(), collection);
        }
        None => {
            out.remove("collection");
        }
    }
    out.insert("data".into(), data.clone());
    Value::Object(out)
}

// ── cache ──────────────────────────────────────────────────────────────────

fn load_cache(path: &Path) -> Map<String, Value> {
    let Some(Value::Object(cache)) = util::read_json(path) else {
        return Map::new();
    };
    if cache.get("version").and_then(Value::as_u64) != Some(COMMUNITY_CACHE_VERSION) {
        return Map::new();
    }
    cache
}

fn save_cache(path: &Path, cache: &Map<String, Value>) -> Result<(), SocialError> {
    let mut cache = cache.clone();
    cache.insert("version".into(), Value::from(COMMUNITY_CACHE_VERSION));
    util::write_json_atomic(path, &Value::Object(cache))
        .map_err(|error| SocialError::Storage(error.to_string()))
}

fn ensure_object<'a>(cache: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    if !cache.get(key).map(Value::is_object).unwrap_or(false) {
        cache.insert(key.to_string(), json!({}));
    }
    cache
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("objeto recém-criado")
}

fn cached_reviews(file: &Path, appid: &str, options: &PageOptions) -> Option<Value> {
    let cache = load_cache(file);
    let value = cache
        .get("reviews")
        .and_then(Value::as_object)
        .and_then(|reviews| reviews.get(&format!("reviews:{appid}")))
        .filter(|value| util::js_truthy(value))?;
    let page = options.page();
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    out.insert("offline".into(), Value::Bool(true));
    util::spread_into(&mut out, value);
    let pagination = value
        .get("pagination")
        .filter(|value| util::js_truthy(value))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "limit": page.limit,
                "offset": page.offset,
                "has_more": false,
            })
        });
    out.insert("pagination".into(), pagination);
    Some(Value::Object(out))
}

fn cached_collections(file: &Path, key: &str) -> Option<Value> {
    let cache = load_cache(file);
    let value = cache
        .get("collections")
        .and_then(Value::as_object)
        .and_then(|collections| collections.get(key))
        .filter(|value| util::js_truthy(value))?;
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(true));
    out.insert("offline".into(), Value::Bool(true));
    util::spread_into(&mut out, value);
    Some(Value::Object(out))
}

fn cached_collection(file: &Path, key: &str) -> Option<Value> {
    let cache = load_cache(file);
    cache
        .get("collectionItems")
        .and_then(Value::as_object)
        .and_then(|items| items.get(key))
        .filter(|value| util::js_truthy(value))
        .cloned()
}

fn cache_review(file: &Path, review: &Value) -> Result<(), SocialError> {
    let Some(appid) = review.get("appid").filter(|value| util::js_truthy(value)) else {
        return Ok(());
    };
    let key = format!("reviews:{}", util::js_string(appid));
    let mut cache = load_cache(file);
    let old_value = cache
        .get("reviews")
        .and_then(Value::as_object)
        .and_then(|reviews| reviews.get(&key))
        .cloned();
    let review_id = util::js_string_opt(review.get("id"));
    let mut reviews = vec![review.clone()];
    if let Some(old) = old_value
        .as_ref()
        .and_then(|value| value.get("reviews"))
        .and_then(Value::as_array)
    {
        for item in old {
            if util::js_string_opt(item.get("id")) != review_id {
                reviews.push(item.clone());
            }
        }
    }
    let mut entry = Map::new();
    entry.insert("reviews".into(), Value::Array(reviews.clone()));
    entry.insert("items".into(), Value::Array(reviews));
    if let Some(pagination) = old_value.as_ref().and_then(|value| value.get("pagination")) {
        entry.insert("pagination".into(), pagination.clone());
    }
    ensure_object(&mut cache, "reviews").insert(key, Value::Object(entry));
    save_cache(file, &cache)
}

fn patch_cached_review(file: &Path, review: &Value) -> Result<(), SocialError> {
    let Some(appid) = review.get("appid").filter(|value| util::js_truthy(value)) else {
        return Ok(());
    };
    let key = format!("reviews:{}", util::js_string(appid));
    let mut cache = load_cache(file);
    let Some(old_value) = cache
        .get("reviews")
        .and_then(Value::as_object)
        .and_then(|reviews| reviews.get(&key))
        .filter(|value| util::js_truthy(value))
        .cloned()
    else {
        return Ok(());
    };
    let review_id = util::js_string_opt(review.get("id"));
    let old_reviews = old_value
        .get("reviews")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let reviews: Vec<Value> = old_reviews
        .into_iter()
        .map(|item| {
            if util::js_string_opt(item.get("id")) == review_id {
                review.clone()
            } else {
                item
            }
        })
        .collect();
    let mut entry = old_value.as_object().cloned().unwrap_or_default();
    entry.insert("reviews".into(), Value::Array(reviews.clone()));
    entry.insert("items".into(), Value::Array(reviews));
    ensure_object(&mut cache, "reviews").insert(key, Value::Object(entry));
    save_cache(file, &cache)
}

fn remove_review_from_cache(file: &Path, id: &str) -> Result<(), SocialError> {
    let mut cache = load_cache(file);
    if let Some(Value::Object(mut reviews)) = cache.remove("reviews") {
        for value in reviews.values_mut() {
            let Some(entry) = value.as_object_mut() else {
                continue;
            };
            let Some(Value::Array(items)) = entry.get_mut("reviews") else {
                continue;
            };
            items.retain(|item| util::js_string_opt(item.get("id")) != id);
            let filtered = items.clone();
            entry.insert("items".into(), Value::Array(filtered));
        }
        cache.insert("reviews".into(), Value::Object(reviews));
    }
    save_cache(file, &cache)
}

fn invalidate_collections(file: &Path, id: Option<&str>) -> Result<(), SocialError> {
    let mut cache = load_cache(file);
    let items = ensure_object(&mut cache, "collectionItems");
    if let Some(id) = id {
        items.remove(id);
    }
    cache.remove("collections");
    save_cache(file, &cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_json(path: &Path, value: &Value) {
        std::fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
    }

    #[test]
    fn pagination_rules_match_as_positive_int() {
        assert_eq!(
            PageOptions::from_value(&json!({})).page(),
            Page {
                limit: 20,
                offset: 0
            }
        );
        assert_eq!(
            PageOptions::from_value(&json!({ "limit": 0 })).page(),
            Page {
                limit: 20,
                offset: 0
            }
        );
        assert_eq!(
            PageOptions::from_value(&json!({ "limit": 500 })).page(),
            Page {
                limit: 100,
                offset: 0
            }
        );
        assert_eq!(
            PageOptions::from_value(&json!({ "limit": "25", "offset": "10" })).page(),
            Page {
                limit: 25,
                offset: 10
            }
        );
        assert_eq!(
            PageOptions::from_value(&json!({ "limit": -5, "offset": -3 })).page(),
            Page {
                limit: 20,
                offset: 0
            }
        );
        assert_eq!(
            PageOptions::from_value(&json!({ "limit": 12.5 })).page(),
            Page {
                limit: 20,
                offset: 0
            }
        );
    }

    #[test]
    fn collection_cache_key_matches_js_json_stringify_shape() {
        let options = PageOptions::from_value(&json!({
            "limit": 10,
            "offset": 20,
            "mine": true,
            "user_id": "u1",
        }));
        assert_eq!(
            options.collection_cache_key(),
            r#"{"limit":10,"offset":20,"mine":true,"owner":"u1","visibility":""}"#
        );
    }

    #[test]
    fn review_list_normalizes_and_duplicates_reviews_as_items() {
        let normalized = review_list(&json!({
            "reviews": [{ "id": 1 }],
            "pagination": { "limit": 1, "offset": 0, "has_more": true }
        }));
        assert_eq!(normalized["reviews"], json!([{ "id": 1 }]));
        assert_eq!(normalized["items"], json!([{ "id": 1 }]));
        assert_eq!(normalized["pagination"]["has_more"], true);

        let fallback = review_list(&json!({ "items": [{ "id": 2 }] }));
        assert_eq!(fallback["reviews"], json!([{ "id": 2 }]));
        assert_eq!(
            fallback["pagination"],
            json!({ "limit": 1, "offset": 0, "has_more": false })
        );
    }

    #[test]
    fn collection_list_uses_lists_and_items_fallbacks() {
        let from_lists = collection_list(&json!({ "lists": [{ "id": "a" }] }));
        assert_eq!(from_lists["collections"], json!([{ "id": "a" }]));
        assert_eq!(from_lists["lists"], json!([{ "id": "a" }]));
        assert_eq!(from_lists["items"], json!([{ "id": "a" }]));

        let from_items = collection_list(&json!({ "items": [{ "id": "b" }] }));
        assert_eq!(from_items["collections"], json!([{ "id": "b" }]));
    }

    #[test]
    fn cached_reviews_falls_back_to_the_requested_page_without_pagination() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-social-cache-unit-{}-{}",
            std::process::id(),
            util::now_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("community_cache.json");
        write_json(
            &path,
            &json!({
                "version": 1,
                "reviews": {
                    "reviews:x": { "reviews": [], "items": [] }
                }
            }),
        );
        let cached = cached_reviews(
            &path,
            "x",
            &PageOptions::from_value(&json!({ "limit": 5, "offset": 10 })),
        )
        .unwrap();
        assert_eq!(
            cached["pagination"],
            json!({ "limit": 5, "offset": 10, "has_more": false })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
