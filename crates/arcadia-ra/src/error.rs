//! Erros estáveis do cliente RetroAchievements.
//!
//! O cliente Electron devolve `{ ok: false, error: "..." }` com códigos fixos
//! para falhas locais e com a mensagem crua do servidor quando a RA responde
//! `{"Error": "..."}` / `{"message": "..."}`. [`RaError::code`] preserva essa
//! string e [`RaError::to_value`] monta o envelope exato para o IPC.

use serde_json::{json, Value};

/// Códigos de erro locais (idênticos aos do cliente JS).
pub const CODE_CREDENCIAIS_VAZIAS: &str = "credenciais_vazias";
pub const CODE_PARAMETROS_INVALIDOS: &str = "parametros_invalidos";
pub const CODE_RESPOSTA_INVALIDA: &str = "resposta_invalida";
pub const CODE_TEMPO_ESGOTADO: &str = "tempo_esgotado";
pub const CODE_FALHA_REDE: &str = "falha_rede";
pub const CODE_CREDENCIAIS_INVALIDAS: &str = "credenciais_invalidas";
pub const CODE_CHAVE_INVALIDA: &str = "chave_invalida";
pub const CODE_FALHA_BUSCA: &str = "falha_busca";
pub const CODE_NAO_CONECTADO: &str = "nao_conectado";
pub const CODE_SEM_WEB_API_KEY: &str = "sem_web_api_key";
pub const CODE_SISTEMA_NAO_SUPORTADO: &str = "sistema_nao_suportado";

/// Falha de qualquer operação RetroAchievements.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RaError {
    #[error("credenciais_vazias")]
    EmptyCredentials,
    #[error("parametros_invalidos")]
    InvalidParameters,
    #[error("resposta_invalida")]
    InvalidResponse,
    #[error("tempo_esgotado")]
    Timeout,
    #[error("falha_rede")]
    Network,
    #[error("credenciais_invalidas")]
    InvalidCredentials,
    #[error("chave_invalida")]
    InvalidApiKey,
    #[error("falha_busca")]
    SearchFailed,
    #[error("nao_conectado")]
    NotConnected,
    #[error("sem_web_api_key")]
    MissingApiKey,
    #[error("sistema_nao_suportado")]
    UnsupportedSystem,
    /// Mensagem real devolvida pelo servidor (`Error`/`message`).
    #[error("{0}")]
    Server(String),
    /// Falha ao persistir o config injetado (nunca contém a senha).
    #[error("{0}")]
    Storage(String),
}

impl RaError {
    /// String que o renderer recebe em `error`, igual ao cliente congelado.
    pub fn code(&self) -> &str {
        match self {
            RaError::EmptyCredentials => CODE_CREDENCIAIS_VAZIAS,
            RaError::InvalidParameters => CODE_PARAMETROS_INVALIDOS,
            RaError::InvalidResponse => CODE_RESPOSTA_INVALIDA,
            RaError::Timeout => CODE_TEMPO_ESGOTADO,
            RaError::Network => CODE_FALHA_REDE,
            RaError::InvalidCredentials => CODE_CREDENCIAIS_INVALIDAS,
            RaError::InvalidApiKey => CODE_CHAVE_INVALIDA,
            RaError::SearchFailed => CODE_FALHA_BUSCA,
            RaError::NotConnected => CODE_NAO_CONECTADO,
            RaError::MissingApiKey => CODE_SEM_WEB_API_KEY,
            RaError::UnsupportedSystem => CODE_SISTEMA_NAO_SUPORTADO,
            RaError::Server(message) | RaError::Storage(message) => message,
        }
    }

    /// `{ ok: false, error }` no formato dos handlers `retroachievements:*`.
    pub fn to_value(&self) -> Value {
        json!({ "ok": false, "error": self.code() })
    }
}

/// Resultado das operações RetroAchievements.
pub type RaResult<T> = std::result::Result<T, RaError>;
