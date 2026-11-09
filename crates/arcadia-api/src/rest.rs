//! Builder REST `/rest/v1/{table}` — port do `QueryBuilder` de
//! `app/electron/supabase/client.js`, acrescido de `order`, `upsert` e do
//! cabeçalho `Prefer: return=representation` (a superfície que o restante do
//! app consome é coberta 1:1).

use std::future::{Future, IntoFuture};
use std::pin::Pin;

use reqwest::Method;
use serde_json::Value;

use crate::error::{DecodeError, Error, Result};
use crate::ApiClient;

/// Direção de ordenação do PostgREST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

impl Order {
    fn as_str(self) -> &'static str {
        match self {
            Order::Asc => "asc",
            Order::Desc => "desc",
        }
    }
}

/// Query builder consumível: encadeie e aguarde (`IntoFuture`), como o builder
/// *thenable* do JS.
///
/// ```no_run
/// # async fn exemplo(client: &arcadia_api::ApiClient) -> Result<(), arcadia_api::Error> {
/// let perfil = client
///     .from("profiles")
///     .select("username, avatar_url")
///     .eq("id", "uuid")
///     .maybe_single()
///     .await?;
/// # let _ = perfil;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct QueryBuilder<'a> {
    client: &'a ApiClient,
    table: String,
    select: String,
    filters: Vec<(String, String)>,
    or: Option<String>,
    orders: Vec<String>,
    limit: Option<u64>,
    single: bool,
    method: Method,
    body: Option<Value>,
    upsert: bool,
    representation: bool,
}

impl<'a> QueryBuilder<'a> {
    pub(crate) fn new(client: &'a ApiClient, table: String) -> Self {
        Self {
            client,
            table,
            select: "*".to_string(),
            filters: Vec::new(),
            or: None,
            orders: Vec::new(),
            limit: None,
            single: false,
            method: Method::GET,
            body: None,
            upsert: false,
            representation: false,
        }
    }

    /// Colunas do `select` (sempre enviado, como no JS).
    pub fn select(mut self, columns: impl Into<String>) -> Self {
        self.select = columns.into();
        self
    }

    /// `col=eq.valor`.
    pub fn eq(mut self, column: impl Into<String>, value: impl ToString) -> Self {
        self.filters
            .push((column.into(), format!("eq.{}", value.to_string())));
        self
    }

    /// Filtro `or=(...)` cru (ex.: `user_a.eq.X,user_b.eq.Y`).
    pub fn or(mut self, condition: impl Into<String>) -> Self {
        self.or = Some(condition.into());
        self
    }

    /// `col=ilike.padrão`.
    pub fn ilike(mut self, column: impl Into<String>, pattern: impl ToString) -> Self {
        self.filters
            .push((column.into(), format!("ilike.{}", pattern.to_string())));
        self
    }

    /// Ordenação (`order=coluna.asc|desc`); chamadas múltiplas são combinadas.
    pub fn order(mut self, column: impl Into<String>, direction: Order) -> Self {
        self.orders
            .push(format!("{}.{}", column.into(), direction.as_str()));
        self
    }

    pub fn order_asc(self, column: impl Into<String>) -> Self {
        self.order(column, Order::Asc)
    }

    pub fn order_desc(self, column: impl Into<String>) -> Self {
        self.order(column, Order::Desc)
    }

    /// `limit`; zero é ignorado, como o `if (this._limit)` do JS.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// `single()`: devolve o primeiro item do array (ou `null`).
    pub fn single(mut self) -> Self {
        self.single = true;
        self
    }

    /// No cliente congelado `maybeSingle` tem a mesma semântica de `single`.
    pub fn maybe_single(mut self) -> Self {
        self.single = true;
        self
    }

    /// `INSERT` (POST).
    pub fn insert(mut self, value: impl Into<Value>) -> Self {
        self.method = Method::POST;
        self.body = Some(value.into());
        self.representation = true;
        self
    }

    /// `UPSERT` (POST + `Prefer: resolution=merge-duplicates`).
    pub fn upsert(mut self, value: impl Into<Value>) -> Self {
        self.method = Method::POST;
        self.body = Some(value.into());
        self.upsert = true;
        self.representation = true;
        self
    }

    /// `UPDATE` (PATCH).
    pub fn update(mut self, value: impl Into<Value>) -> Self {
        self.method = Method::PATCH;
        self.body = Some(value.into());
        self.representation = true;
        self
    }

    /// `DELETE`.
    pub fn delete(mut self) -> Self {
        self.method = Method::DELETE;
        self.body = None;
        self.representation = true;
        self
    }

    /// Pede `Prefer: return=representation` (padrão das mutações).
    pub fn return_representation(mut self) -> Self {
        self.representation = true;
        self
    }

    /// Remove `return=representation` (resposta mínima do PostgREST).
    pub fn return_minimal(mut self) -> Self {
        self.representation = false;
        self
    }

    /// Executa a query.
    pub async fn execute(self) -> Result<Value> {
        let pairs = self.query_pairs();
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&pairs)
            .finish();
        let url = if query.is_empty() {
            format!("{}/rest/v1/{}", self.client.config().url, self.table)
        } else {
            format!(
                "{}/rest/v1/{}?{query}",
                self.client.config().url,
                self.table
            )
        };

        let mut body = None;
        let mut content_type = None;
        if self.method != Method::GET {
            content_type = Some("application/json");
            if let Some(value) = &self.body {
                body = Some(serde_json::to_vec(value).map_err(|error| {
                    Error::Decode(DecodeError {
                        status: 0,
                        message: error.to_string(),
                        body: String::new(),
                    })
                })?);
            }
        }

        let prefer = self.prefer_header();
        let response = self
            .client
            .request_authenticated(self.method, url, body, content_type, prefer.as_deref())
            .await?;

        if !response.is_success() {
            return Err(response.api_error().into());
        }

        let data: Value = if response.body.is_empty() {
            Value::Array(Vec::new())
        } else {
            response.json()?
        };

        if self.single {
            return Ok(match data {
                Value::Array(mut items) => items.drain(..).next().unwrap_or(Value::Null),
                other => other,
            });
        }
        Ok(data)
    }

    fn query_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        pairs.push(("select".to_string(), self.select.clone()));

        // URLSearchParams.set: filtro repetido na mesma coluna sobrescreve o
        // valor, mantendo a posição original.
        let mut filters: Vec<(String, String)> = Vec::new();
        for (column, expression) in &self.filters {
            if let Some(existing) = filters.iter_mut().find(|(name, _)| name == column) {
                existing.1 = expression.clone();
            } else {
                filters.push((column.clone(), expression.clone()));
            }
        }
        pairs.extend(filters);

        if let Some(condition) = &self.or {
            if !condition.is_empty() {
                pairs.push(("or".to_string(), format!("({condition})")));
            }
        }
        if !self.orders.is_empty() {
            pairs.push(("order".to_string(), self.orders.join(",")));
        }
        if let Some(limit) = self.limit.filter(|limit| *limit > 0) {
            pairs.push(("limit".to_string(), limit.to_string()));
        }
        pairs
    }

    fn prefer_header(&self) -> Option<String> {
        if self.method == Method::GET {
            return None;
        }
        let mut parts = Vec::new();
        if self.upsert {
            parts.push("resolution=merge-duplicates");
        }
        if self.representation {
            parts.push("return=representation");
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

impl<'a> IntoFuture for QueryBuilder<'a> {
    type Output = Result<Value>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.execute())
    }
}
