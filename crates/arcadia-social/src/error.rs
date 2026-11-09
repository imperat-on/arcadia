//! Erros tipados da camada social.
//!
//! Códigos estáveis (contrato para o IPC):
//!
//! - `nao_logado` — não há sessão com `user.id`.
//! - `conta_trocada` — a conta ativa mudou durante a requisição.
//! - `token_invalido` — o backend respondeu 401.
//! - `parametros_invalidos` — destino/identificador inválido.
//! - `rede_indisponivel` — DNS/conexão/timeout (nunca resposta HTTP).
//! - `armazenamento` — falha ao gravar o cache local.
//!
//! Respostas HTTP não-2xx viram [`SocialError::Backend`] (ou
//! [`SocialError::Unauthorized`] no 401) preservando status, código e corpo.

use serde_json::{json, Map, Value};

use arcadia_api::Error as ApiFailure;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SocialError {
    #[error("nao_logado")]
    NotLoggedIn,
    #[error("conta_trocada")]
    AccountSwitched,
    #[error("{0}")]
    InvalidParams(String),
    #[error("rede_indisponivel")]
    Network(String),
    #[error("{message}")]
    Unauthorized { message: String, body: String },
    #[error("{message}")]
    Backend {
        status: u16,
        message: String,
        code: Option<String>,
        body: String,
        request_id: Option<String>,
    },
    #[error("resposta_invalida: {0}")]
    InvalidResponse(String),
    #[error("armazenamento: {0}")]
    Storage(String),
}

impl SocialError {
    /// Código estável para o renderer/IPC.
    pub fn code(&self) -> &str {
        match self {
            SocialError::NotLoggedIn => "nao_logado",
            SocialError::AccountSwitched => "conta_trocada",
            SocialError::InvalidParams(_) => "parametros_invalidos",
            SocialError::Network(_) => "rede_indisponivel",
            SocialError::Unauthorized { .. } => "token_invalido",
            SocialError::Backend { status: 401, .. } => "token_invalido",
            SocialError::Backend {
                code: Some(code), ..
            } => code,
            SocialError::Backend { .. } => "backend",
            SocialError::InvalidResponse(_) => "resposta_invalida",
            SocialError::Storage(_) => "armazenamento",
        }
    }

    /// Texto visível usado nos envelopes de amigos (`{ ok:false, error }`).
    pub fn friends_message(&self) -> String {
        match self {
            SocialError::NotLoggedIn => "nao_logado".to_string(),
            SocialError::AccountSwitched => "conta_trocada".to_string(),
            SocialError::InvalidParams(message) => message.clone(),
            SocialError::Network(_) => "rede_indisponivel".to_string(),
            SocialError::Unauthorized { message, .. } => {
                if message.is_empty() {
                    "token_invalido".to_string()
                } else {
                    message.clone()
                }
            }
            SocialError::Backend { message, .. } => message.clone(),
            SocialError::InvalidResponse(message) => format!("resposta_invalida: {message}"),
            SocialError::Storage(message) => format!("armazenamento: {message}"),
        }
    }

    /// Envelope exato do `friends.js` para falhas.
    pub fn to_friends_envelope(&self) -> Value {
        json!({ "ok": false, "error": self.friends_message() })
    }

    /// Envelope exato do `community.js` (`normalizeError`).
    pub fn to_community_envelope(&self) -> Value {
        match self {
            SocialError::AccountSwitched => json!({
                "ok": false,
                "error": "conta_trocada",
                "code": "conta_trocada",
            }),
            SocialError::Network(detail) => json!({
                "ok": false,
                "offline": true,
                "error": "offline",
                "code": "offline",
                "message": if detail.is_empty() { "rede indisponivel" } else { detail.as_str() },
                "status": 0,
            }),
            SocialError::Unauthorized { body, .. } => {
                // Mesmo `normalizeError(data, 401)` do community.js; o código
                // estável `token_invalido` fica em `SocialError::code()`.
                let mut out = normalize_community_error(Some(body), 401);
                out.insert("ok".into(), Value::Bool(false));
                Value::Object(out)
            }
            SocialError::Backend {
                status,
                message,
                code,
                body,
                request_id,
            } => {
                let mut out = normalize_community_error(Some(body), *status);
                if !out.contains_key("error") {
                    out.insert(
                        "error".into(),
                        Value::String(code.clone().unwrap_or_else(|| message.clone())),
                    );
                }
                if !out.contains_key("message") {
                    out.insert("message".into(), Value::String(message.clone()));
                }
                if let Some(request_id) = request_id {
                    out.entry("request_id".to_string())
                        .or_insert_with(|| Value::String(request_id.clone()));
                }
                out.insert("ok".into(), Value::Bool(false));
                Value::Object(out)
            }
            SocialError::NotLoggedIn => json!({
                "ok": false,
                "error": "nao_logado",
                "code": "nao_logado",
                "message": "nao_logado",
            }),
            SocialError::InvalidParams(message) => json!({
                "ok": false,
                "error": message,
                "code": "parametros_invalidos",
                "message": message,
            }),
            SocialError::InvalidResponse(message) => json!({
                "ok": false,
                "error": message,
                "code": "resposta_invalida",
                "message": message,
            }),
            SocialError::Storage(message) => json!({
                "ok": false,
                "error": "armazenamento",
                "code": "armazenamento",
                "message": message,
            }),
        }
    }
}

impl From<std::io::Error> for SocialError {
    fn from(error: std::io::Error) -> Self {
        SocialError::Storage(error.to_string())
    }
}

/// Converte um erro do [`arcadia_api`] no erro tipado da camada social.
pub(crate) fn map_api_error(error: ApiFailure) -> SocialError {
    match error {
        ApiFailure::Transport(error) => SocialError::Network(error.message),
        ApiFailure::Decode(error) => SocialError::InvalidResponse(error.message),
        ApiFailure::Api(error) if error.status == 401 => SocialError::Unauthorized {
            message: error.message,
            body: error.body,
        },
        ApiFailure::Api(error) => {
            let request_id = serde_json::from_str::<Value>(&error.body)
                .ok()
                .and_then(|value| {
                    value
                        .get("request_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            SocialError::Backend {
                status: error.status,
                message: error.message,
                code: error.code,
                body: error.body,
                request_id,
            }
        }
    }
}

/// `normalizeError(data, status)` de `community.js`.
pub(crate) fn normalize_community_error(body: Option<&str>, status: u16) -> Map<String, Value> {
    let parsed = body.and_then(|body| {
        if body.is_empty() {
            None
        } else {
            Some(serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!({ "error": body })))
        }
    });
    let object = parsed.as_ref().and_then(Value::as_object);
    let error = object
        .and_then(|object| object.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("http_{status}"));
    let code = object
        .and_then(|object| object.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.clone());
    let message = object
        .and_then(|object| object.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.clone());
    let mut out = Map::new();
    out.insert("error".into(), Value::String(error));
    out.insert("code".into(), Value::String(code));
    out.insert("message".into(), Value::String(message));
    if let Some(request_id) = object
        .and_then(|object| object.get("request_id"))
        .and_then(Value::as_str)
    {
        out.insert("request_id".into(), Value::String(request_id.to_string()));
    }
    out.insert("status".into(), Value::from(status));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_codes_match_the_ipc_contract() {
        assert_eq!(SocialError::NotLoggedIn.code(), "nao_logado");
        assert_eq!(SocialError::AccountSwitched.code(), "conta_trocada");
        assert_eq!(
            SocialError::InvalidParams("destino_invalido".into()).code(),
            "parametros_invalidos"
        );
        assert_eq!(
            SocialError::Network("dns".into()).code(),
            "rede_indisponivel"
        );
        assert_eq!(
            SocialError::Unauthorized {
                message: "x".into(),
                body: String::new()
            }
            .code(),
            "token_invalido"
        );
    }

    #[test]
    fn community_error_envelope_matches_normalize_error() {
        let error = SocialError::Backend {
            status: 403,
            message: "sem_permissao".into(),
            code: Some("42501".into()),
            body: r#"{"error":"sem_permissao","code":"42501","message":"detalhe","request_id":"req-1"}"#
                .into(),
            request_id: Some("req-1".into()),
        };
        assert_eq!(
            error.to_community_envelope(),
            json!({
                "ok": false,
                "error": "sem_permissao",
                "code": "42501",
                "message": "detalhe",
                "request_id": "req-1",
                "status": 403,
            })
        );
    }

    #[test]
    fn community_network_error_is_marked_offline() {
        assert_eq!(
            SocialError::Network("connect refused".into()).to_community_envelope(),
            json!({
                "ok": false,
                "offline": true,
                "error": "offline",
                "code": "offline",
                "message": "connect refused",
                "status": 0,
            })
        );
    }

    #[test]
    fn friends_envelope_is_minimal() {
        assert_eq!(
            SocialError::NotLoggedIn.to_friends_envelope(),
            json!({ "ok": false, "error": "nao_logado" })
        );
    }
}
