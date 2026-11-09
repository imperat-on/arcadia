//! Erros tipados do cliente.
//!
//! O cliente JavaScript devolve `{ data, error }` e trata respostas não-JSON
//! como texto. Aqui a mesma informação vira um erro tipado, com o corpo bruto
//! preservado — nenhuma resposta inválida derruba o processo.

use std::fmt;

use serde_json::Value;

/// Erro devolvido pelo servidor (resposta HTTP não-2xx).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    /// Campo `code` do JSON de erro, quando presente.
    pub code: Option<String>,
    pub message: String,
    /// Corpo bruto da resposta, exatamente como chegou.
    pub body: String,
}

impl ApiError {
    pub fn new(
        status: u16,
        code: Option<String>,
        message: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            body: body.into(),
        }
    }

    /// Constrói o erro a partir de uma resposta não-2xx.
    ///
    /// Precedência da mensagem: `error`, `msg`, `message` (as duas primeiras
    /// são as usadas pelo cliente JS), depois `HTTP {status}`. Um corpo que não
    /// é JSON nunca causa pânico: vira mensagem genérica e o corpo bruto fica
    /// preservado em [`ApiError::body`].
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        let raw = String::from_utf8_lossy(body).into_owned();
        let parsed: Option<Value> = serde_json::from_slice(body).ok();

        let message = parsed
            .as_ref()
            .and_then(api_message)
            .unwrap_or_else(|| format!("HTTP {status}"));
        let code = parsed.as_ref().and_then(api_code);

        Self {
            status,
            code,
            message,
            body: raw,
        }
    }

    /// Corpo da resposta como JSON, quando ele é válido.
    pub fn json_body(&self) -> Option<Value> {
        serde_json::from_str(&self.body).ok()
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Classificação de uma falha de transporte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// DNS, recusa de conexão, handshake TLS.
    Connect,
    Timeout,
    /// Falha ao montar/enviar a requisição ou ler a resposta.
    Request,
    Other,
}

/// Falha de rede — o único caso que troca de endereço (primário ↔ reserva).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    pub kind: TransportKind,
    pub message: String,
}

impl TransportError {
    pub fn new(kind: TransportKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn from_reqwest(error: &reqwest::Error) -> Self {
        let kind = if error.is_timeout() {
            TransportKind::Timeout
        } else if error.is_connect() {
            TransportKind::Connect
        } else if error.is_request() {
            TransportKind::Request
        } else {
            TransportKind::Other
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "falha de rede: {}", self.message)
    }
}

impl std::error::Error for TransportError {}

/// Resposta 2xx que não pôde ser decodificada como o JSON esperado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    pub status: u16,
    pub message: String,
    /// Corpo bruto preservado para diagnóstico.
    pub body: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "resposta JSON inválida (HTTP {}): {}",
            self.status, self.message
        )
    }
}

impl std::error::Error for DecodeError {}

/// Erro unificado do cliente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// O servidor respondeu com status não-2xx.
    Api(ApiError),
    /// A requisição não chegou a uma resposta HTTP (DNS, conexão, TLS, timeout).
    Transport(TransportError),
    /// A resposta chegou, mas o corpo não é o JSON esperado.
    Decode(DecodeError),
}

impl Error {
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api(error) => Some(error.status),
            Error::Decode(error) => Some(error.status),
            Error::Transport(_) => None,
        }
    }

    pub fn is_auth_error(&self) -> bool {
        self.status() == Some(401)
    }

    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Error::Api(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api(error) => write!(formatter, "{error}"),
            Error::Transport(error) => write!(formatter, "{error}"),
            Error::Decode(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Api(error) => Some(error),
            Error::Transport(error) => Some(error),
            Error::Decode(error) => Some(error),
        }
    }
}

impl From<ApiError> for Error {
    fn from(error: ApiError) -> Self {
        Error::Api(error)
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        Error::Transport(error)
    }
}

impl From<DecodeError> for Error {
    fn from(error: DecodeError) -> Self {
        Error::Decode(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn field_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

fn api_message(value: &Value) -> Option<String> {
    field_string(value, "error")
        .or_else(|| field_string(value, "msg"))
        .or_else(|| field_string(value, "message"))
}

fn api_code(value: &Value) -> Option<String> {
    field_string(value, "code")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_prefers_the_same_fields_as_the_js_client() {
        let error = ApiError::from_response(
            400,
            br#"{"error":"senha_incorreta","msg":"ignorado","code":"invalid_credentials"}"#,
        );
        assert_eq!(error.status, 400);
        assert_eq!(error.message, "senha_incorreta");
        assert_eq!(error.code.as_deref(), Some("invalid_credentials"));
        assert!(error.body.contains("senha_incorreta"));
    }

    #[test]
    fn api_error_falls_back_to_msg_then_status_without_panicking() {
        assert_eq!(
            ApiError::from_response(404, br#"{"msg":"rota_ausente"}"#).message,
            "rota_ausente"
        );
        let html = ApiError::from_response(404, b"<html>nao encontrado</html>");
        assert_eq!(html.message, "HTTP 404");
        assert_eq!(html.body, "<html>nao encontrado</html>");
        assert_eq!(html.json_body(), None);
    }
}
