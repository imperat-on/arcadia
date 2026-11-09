//! Camada HTTP trocável.
//!
//! O cliente congelado usa `fetchRede(url, opts)` e sempre lê a resposta
//! inteira. Aqui o mesmo contrato é exposto por [`Fetcher`]: os provedores
//! recebem uma implementação injetada, então os testes nunca tocam a rede.
//!
//! [`HttpClientFetcher`] é a implementação de produção sobre o
//! [`arcadia_api::HttpClient`]. O timeout por requisição do JS (15 s na IGDB,
//! 20 s nos demais) é aplicado por `tokio::time::timeout` em volta da chamada;
//! o cliente do `arcadia-api` continua com o timeout global de 30 s.

use std::sync::Arc;
use std::time::Duration;

use arcadia_api::{HttpClient, HttpRequest, RawResponse};
use async_trait::async_trait;
use serde_json::Value;
use url::form_urlencoded;

use crate::error::{FetchError, MediaError, Result};

/// Timeout usado por `getJSON` no JS (`AbortSignal.timeout(20000)`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout da IGDB no JS.
pub const IGDB_TIMEOUT: Duration = Duration::from_secs(15);
/// User-Agent genérico das chamadas de loja no JS.
pub const MOZILLA_UA: &str = "Mozilla/5.0";

/// Método HTTP aceito pelos provedores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Head,
}

/// Requisição equivalente ao objeto aceito por `fetch` no JS.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self::new(Method::Post, url)
    }

    pub fn head(url: impl Into<String>) -> Self {
        Self::new(Method::Head, url)
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn headers<I, K, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.headers.extend(
            headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Resposta já lida por inteiro (equivalente ao `Response` do `fetch`).
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn json(status: u16, value: &Value) -> Self {
        let mut response = Self::new(status, serde_json::to_vec(value).unwrap_or_default());
        response
            .headers
            .push(("content-type".into(), "application/json".into()));
        response
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, body.into().into_bytes())
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text_body(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Busca de cabeçalho sem diferenciar caixa (o `Headers.get` do JS).
    pub fn find_header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn json_value(&self) -> Result<Value> {
        serde_json::from_slice(&self.body)
            .map_err(|error| MediaError::decode(format!("{} ({})", error, self.text_body())))
    }
}

/// Fonte de respostas HTTP. Produção usa [`HttpClientFetcher`]; testes usam
/// um duplo que devolve fixtures.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, request: Request) -> std::result::Result<Response, FetchError>;
}

/// Adaptador para o cliente HTTP compartilhado do `arcadia-api`.
#[derive(Debug, Clone)]
pub struct HttpClientFetcher {
    client: HttpClient,
}

impl HttpClientFetcher {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    /// Constrói o cliente a partir de uma URL base (útil para testes locais).
    pub fn from_base_url(base: &str) -> std::result::Result<Self, FetchError> {
        let config = arcadia_api::ApiConfig::resolve(Some(base), None, None, None);
        let client = HttpClient::new(Arc::new(config))
            .map_err(|error| FetchError::new(error.to_string()))?;
        Ok(Self::new(client))
    }

    pub fn client(&self) -> &HttpClient {
        &self.client
    }
}

#[async_trait]
impl Fetcher for HttpClientFetcher {
    async fn fetch(&self, request: Request) -> std::result::Result<Response, FetchError> {
        let method = match request.method {
            Method::Get => arcadia_api::Method::GET,
            Method::Post => arcadia_api::Method::POST,
            Method::Head => arcadia_api::Method::HEAD,
        };
        let url = request.url.clone();
        let timeout = request.timeout;
        let mut outbound = HttpRequest::new(method, request.url);
        for (name, value) in request.headers {
            outbound = outbound.header(name, value);
        }
        if let Some(body) = request.body {
            outbound = outbound.body(body);
        }
        let response = tokio::time::timeout(timeout, self.client.send(&outbound))
            .await
            .map_err(|_| {
                FetchError::new(format!("timeout de {}s em {url}", timeout.as_secs_f64()))
            })?
            .map_err(|error| FetchError::new(error.to_string()))?;
        let RawResponse {
            status,
            headers,
            body,
            ..
        } = response;
        let headers = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

#[async_trait]
impl<T: Fetcher + ?Sized> Fetcher for Arc<T> {
    async fn fetch(&self, request: Request) -> std::result::Result<Response, FetchError> {
        (**self).fetch(request).await
    }
}

/// `getJSON(url, headers)` do JS: falha HTTP vira erro, corpo vira JSON.
pub async fn get_json(
    fetcher: &dyn Fetcher,
    url: impl Into<String>,
    headers: &[(&str, &str)],
) -> Result<Value> {
    let url = url.into();
    let request = Request::get(url.clone()).headers(
        headers
            .iter()
            .map(|(name, value)| (*name, *value))
            .collect::<Vec<_>>(),
    );
    let response = fetcher.fetch(request).await?;
    if !response.is_success() {
        return Err(MediaError::Http {
            status: response.status,
            url,
        });
    }
    response.json_value()
}

/// `getText(url)` do PSN: mesma checagem de status, sem JSON.
pub async fn get_text(
    fetcher: &dyn Fetcher,
    url: impl Into<String>,
    headers: &[(&str, &str)],
) -> Result<String> {
    let url = url.into();
    let request = Request::get(url.clone()).headers(
        headers
            .iter()
            .map(|(name, value)| (*name, *value))
            .collect::<Vec<_>>(),
    );
    let response = fetcher.fetch(request).await?;
    if !response.is_success() {
        return Err(MediaError::Http {
            status: response.status,
            url,
        });
    }
    Ok(response.text_body())
}

/// Serializa parâmetros como `URLSearchParams` (espaço vira `+`).
///
/// A ordem dos pares é preservada, como no objeto do JS.
pub fn query_string(params: &[(&str, String)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (name, value) in params {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

/// `encodeURIComponent`: espaço vira `%20`; `!'()*~-._` ficam literais.
///
/// É diferente de [`query_string`] e é usado nos segmentos de caminho
/// (`/search/autocomplete/{titulo}`, `/search/{titulo}`).
pub fn encode_uri_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let character = *byte as char;
        let keep = character.is_ascii_alphanumeric()
            || matches!(
                character,
                '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')'
            );
        if keep {
            out.push(character);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

/// `String(value || "")` limitado a strings — o formato dos campos de texto
/// dos provedores.
pub(crate) fn value_str(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(boolean)) => boolean.to_string(),
        _ => String::new(),
    }
}

/// `Number(value) || 0`, aceitando números e strings numéricas.
pub(crate) fn value_number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(true)) => 1.0,
        _ => 0.0,
    }
}

/// `Boolean(value)` do JS para os campos que o oráculo testa com `!value`.
pub(crate) fn value_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(boolean)) => *boolean,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `Math.round` do JS para os valores não negativos usados em metadados.
pub(crate) fn js_round(value: f64) -> f64 {
    if value.is_finite() {
        (value + 0.5).floor()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_uri_component_matches_javascript() {
        assert_eq!(encode_uri_component("Portal 2"), "Portal%202");
        assert_eq!(
            encode_uri_component("Tom Clancy's: Rainbox Six?!&=/"),
            "Tom%20Clancy's%3A%20Rainbox%20Six%3F!%26%3D%2F"
        );
        assert_eq!(encode_uri_component("ação"), "a%C3%A7%C3%A3o");
        assert_eq!(encode_uri_component("*-._~!'()"), "*-._~!'()");
    }

    #[test]
    fn query_string_uses_plus_for_spaces_and_preserves_order() {
        let encoded = query_string(&[
            ("query", "Elden Ring".to_string()),
            ("market", "BR".to_string()),
            ("deviceFamily", "Windows.Desktop".to_string()),
        ]);
        assert_eq!(
            encoded,
            "query=Elden+Ring&market=BR&deviceFamily=Windows.Desktop"
        );
    }

    #[test]
    fn query_string_encodes_special_characters_like_urlsearchparams() {
        assert_eq!(
            query_string(&[("q", "Tom's ~ (game)!".to_string())]),
            "q=Tom%27s+%7E+%28game%29%21"
        );
    }

    #[test]
    fn response_finds_headers_case_insensitively() {
        let response = Response::new(200, "").header("Content-Type", "image/png; charset=x");
        assert_eq!(
            response.find_header("content-type"),
            Some("image/png; charset=x")
        );
        assert!(response.is_success());
        assert!(!Response::new(404, "").is_success());
    }

    #[test]
    fn response_json_value_reports_the_raw_body() {
        let response = Response::text(200, "<html>não é json</html>");
        let error = response.json_value().unwrap_err();
        assert!(error.to_string().contains("<html>não é json</html>"));
    }

    #[test]
    fn number_helpers_mirror_javascript_coercions() {
        assert_eq!(value_number(Some(&Value::from(2.4))), 2.4);
        assert_eq!(value_number(Some(&Value::from(" 42.5 "))), 42.5);
        assert_eq!(value_number(Some(&Value::from("x"))), 0.0);
        assert_eq!(js_round(1.5), 2.0);
        assert_eq!(js_round(1.4), 1.0);
        assert_eq!(js_round(f64::NAN), 0.0);
    }

    #[test]
    fn error_messages_match_the_frozen_client() {
        assert_eq!(
            MediaError::Http {
                status: 500,
                url: "https://x/y".into()
            }
            .to_string(),
            "HTTP 500 em https://x/y"
        );
        assert_eq!(
            MediaError::Download { status: 404 }.to_string(),
            "download falhou: HTTP 404"
        );
        assert_eq!(
            MediaError::HltbHttp { status: 503 }.to_string(),
            "hltb http 503"
        );
    }
}
