//! Camada HTTP: cliente reqwest + fallback de endereço.
//!
//! Regra central (a mesma de `app/electron/httpfetch.js`): **somente** falha de
//! rede troca o endereço. Uma resposta HTTP 4xx/5xx é resposta legítima do
//! servidor e jamais é repetida no endereço de reserva.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::ApiConfig;
use crate::error::{ApiError, DecodeError, TransportError, TransportKind};

/// Timeout total de cada tentativa (o JS usa 30 s no caminho normal).
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout de conexão/TLS.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Requisição de baixo nível aceita pelos métodos internos e por testes.
///
/// Cabeçalhos passam pelo saneador antes de sair: code points fora de
/// Latin-1 e caracteres de controle são descartados em vez de estourar.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(Method::GET, url)
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self::new(Method::POST, url)
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// Resposta HTTP já lida por inteiro (o cliente JS também faz `text()`).
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    /// URL efetivamente usada (útil para saber qual endereço respondeu).
    pub url: String,
}

impl RawResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// Decodifica JSON preservando o corpo bruto quando a decodificação falha.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, DecodeError> {
        serde_json::from_slice(&self.body).map_err(|error| DecodeError {
            status: self.status,
            message: error.to_string(),
            body: self.text(),
        })
    }

    /// Igual ao `_request` do JS: tenta JSON e, se não for JSON, devolve o
    /// texto; corpo vazio vira `null`.
    pub fn json_or_text(&self) -> Value {
        if self.body.is_empty() {
            return Value::Null;
        }
        match serde_json::from_slice(&self.body) {
            Ok(value) => value,
            Err(_) => Value::String(self.text()),
        }
    }

    /// Converte uma resposta não-2xx no erro tipado correspondente.
    pub fn api_error(&self) -> ApiError {
        ApiError::from_response(self.status, &self.body)
    }
}

/// Cliente HTTP compartilhável.
#[derive(Debug, Clone)]
pub struct HttpClient {
    config: Arc<ApiConfig>,
    client: reqwest::Client,
    preferred: Arc<AtomicUsize>,
}

impl HttpClient {
    pub fn new(config: Arc<ApiConfig>) -> Result<Self, TransportError> {
        Self::with_timeout(config, DEFAULT_HTTP_TIMEOUT)
    }

    pub fn with_timeout(config: Arc<ApiConfig>, timeout: Duration) -> Result<Self, TransportError> {
        let connect_timeout = DEFAULT_CONNECT_TIMEOUT.min(timeout);
        let client = reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(timeout)
            .build()
            .map_err(|error| TransportError::from_reqwest(&error))?;
        Ok(Self {
            config,
            client,
            preferred: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn config(&self) -> &ApiConfig {
        &self.config
    }

    /// Índice do último endereço que respondeu.
    pub fn preferred_index(&self) -> usize {
        self.preferred.load(Ordering::Relaxed)
    }

    /// Executa a requisição, trocando de endereço apenas em falha de rede.
    pub async fn send(&self, request: &HttpRequest) -> Result<RawResponse, TransportError> {
        let Some((base_index, base)) = self.base_for(&request.url) else {
            // URL externa (loja/CDN): comportamento de sempre, sem fallback.
            return self.send_to(&request.url, request).await;
        };

        let order = self.attempt_order(base_index);
        let mut last_error = None;

        for index in order {
            let Some(target_base) = self.config.urls.get(index) else {
                continue;
            };
            let target = if target_base == base {
                request.url.clone()
            } else {
                format!("{target_base}{}", &request.url[base.len()..])
            };
            match self.send_to(&target, request).await {
                Ok(response) => {
                    self.preferred.store(index, Ordering::Relaxed);
                    return Ok(response);
                }
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TransportError::new(TransportKind::Other, "nenhum endereço configurado")
        }))
    }

    fn base_for<'a>(&'a self, url: &str) -> Option<(usize, &'a str)> {
        self.config
            .urls
            .iter()
            .enumerate()
            .find(|(_, base)| url.starts_with(base.as_str()))
            .map(|(index, base)| (index, base.as_str()))
    }

    fn attempt_order(&self, first: usize) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.config.urls.len());
        if first < self.config.urls.len() {
            order.push(first);
        }
        for index in 0..self.config.urls.len() {
            if !order.contains(&index) {
                order.push(index);
            }
        }
        order
    }

    async fn send_to(
        &self,
        url: &str,
        request: &HttpRequest,
    ) -> Result<RawResponse, TransportError> {
        let headers = sanitize_headers(
            request
                .headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );

        let mut builder = self
            .client
            .request(request.method.clone(), url)
            .headers(headers);
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }

        let response = builder
            .send()
            .await
            .map_err(|error| TransportError::from_reqwest(&error))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let final_url = response.url().to_string();
        let body = response
            .bytes()
            .await
            .map_err(|error| TransportError::from_reqwest(&error))?
            .to_vec();

        Ok(RawResponse {
            status,
            headers,
            body,
            url: final_url,
        })
    }
}

/// Sanea o nome de um cabeçalho: mantém apenas ASCII imprimível (0x20–0x7E).
/// Devolve `None` quando o resultado não é um nome HTTP válido.
pub fn sanitize_header_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|character| ('\u{20}'..='\u{7e}').contains(character))
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Sanea o valor de um cabeçalho: descarta code points fora de Latin-1 e
/// caracteres de controle (a regra do `net-guard.js`, endurecida para que
/// CR/LF/NUL também não provoquem erro).
pub fn sanitize_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            let code = *character as u32;
            code == 0x09 || (0x20..=0x7e).contains(&code) || (0xa0..=0xff).contains(&code)
        })
        .collect()
}

/// Reconstrói um [`HeaderMap`] saneado. Nomes inválidos são descartados.
pub fn sanitize_headers<'a, I>(headers: I) -> HeaderMap
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let Some(name) = sanitize_header_name(name)
            .and_then(|name| HeaderName::from_bytes(name.as_bytes()).ok())
        else {
            continue;
        };
        let value = sanitize_header_value(value);
        let bytes: Vec<u8> = value
            .chars()
            .map(|character| character as u32 as u8)
            .collect();
        // `HeaderValue::from_str` guardaria UTF-8; o fetch do JS usa bytes
        // Latin-1 (ByteString), então montamos o valor byte a byte.
        if let Ok(value) = HeaderValue::from_bytes(&bytes) {
            // O JS monta um objeto de headers: nomes repetidos, o último vence.
            map.insert(name, value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_values_without_losing_latin1() {
        assert_eq!(sanitize_header_value("olá ☃\r\nmundo"), "olá mundo");
        assert_eq!(sanitize_header_value("a\tb"), "a\tb");
        assert_eq!(sanitize_header_value("ação"), "ação");
    }

    #[test]
    fn sanitizes_names_and_drops_impossible_ones() {
        assert_eq!(sanitize_header_name("x☃test"), Some("xtest".into()));
        assert_eq!(sanitize_header_name("x\r\ny"), Some("xy".into()));
        assert_eq!(sanitize_header_name("☃"), None);
    }

    #[test]
    fn headers_with_control_characters_do_not_panic() {
        let headers = sanitize_headers([
            ("x-test", "abc☃\r\ndef"),
            ("x-name\u{0}", "value"),
            ("x-empty", ""),
        ]);
        assert_eq!(headers.get("x-test").unwrap(), "abcdef");
        assert!(headers.get("x-empty").is_some());
    }
}
