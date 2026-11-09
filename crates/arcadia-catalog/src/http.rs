//! Camada HTTP trocável.
//!
//! O cliente Electron congelado usa `fetchRede(url, opts)` e lê a resposta
//! inteira. Aqui o mesmo contrato é exposto por [`Fetcher`], injetado em todos
//! os clientes: nenhum teste toca a rede real.
//!
//! [`HttpClientFetcher`] é a implementação de produção sobre
//! [`arcadia_api::HttpClient`]. O timeout por requisição (8 s, 15 s, 30 s ou
//! 60 s, conforme o caminho do JS) é aplicado em volta da chamada; o cliente
//! do `arcadia-api` mantém o timeout global de 30 s.

use std::sync::Arc;
use std::time::Duration;

use arcadia_api::{HttpClient, HttpRequest, RawResponse};
use async_trait::async_trait;
use serde_json::Value;

/// Timeout padrão de uma requisição deste crate (o `fetchRede` sem override).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Falha de transporte ou de decodificação de uma resposta.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct FetchError {
    pub message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Método HTTP aceito pelos clientes do crate.
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

    pub fn json_value(&self) -> Result<Value, FetchError> {
        serde_json::from_slice(&self.body).map_err(|error| {
            FetchError::new(format!(
                "resposta JSON inválida: {error} ({})",
                self.text_body()
            ))
        })
    }
}

/// Fonte de respostas HTTP. Produção usa [`HttpClientFetcher`]; testes usam um
/// duplo que devolve fixtures.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, request: Request) -> Result<Response, FetchError>;
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
    pub fn from_base_url(base: &str) -> Result<Self, FetchError> {
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
    async fn fetch(&self, request: Request) -> Result<Response, FetchError> {
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
    async fn fetch(&self, request: Request) -> Result<Response, FetchError> {
        (**self).fetch(request).await
    }
}

/// `Boolean(value)` do JS.
pub(crate) fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(boolean)) => *boolean,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `encodeURIComponent`: espaço vira `%20`; `!'()*-._~` ficam literais.
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

/// `URLSearchParams.toString()` (espaço vira `+`).
pub fn form_urlencode(params: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in params {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_uri_component_matches_javascript() {
        assert_eq!(encode_uri_component("Portal 2"), "Portal%202");
        assert_eq!(
            encode_uri_component("https://example.com/f?x=1&y=2"),
            "https%3A%2F%2Fexample.com%2Ff%3Fx%3D1%26y%3D2"
        );
        assert_eq!(encode_uri_component("*-._~!'()"), "*-._~!'()");
    }

    #[test]
    fn form_urlencode_uses_plus_and_encodes_brackets() {
        assert_eq!(
            form_urlencode(&[("agent", "arcadia"), ("magnets[]", "magnet:x y")]),
            "agent=arcadia&magnets%5B%5D=magnet%3Ax+y"
        );
    }
}
