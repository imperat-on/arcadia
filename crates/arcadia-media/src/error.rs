//! Erros da camada de mídia.
//!
//! As mensagens visíveis reproduzem as do cliente Electron onde a UI as
//! apresenta (`HTTP 500 em …`, `SteamGridDB recusou a busca…`, `download
//! falhou…`). Erros que o JS engole silenciosamente continuam disponíveis nas
//! funções de baixo nível; as funções de alto nível devolvem `Option`, como o
//! cliente congelado.

use std::fmt;

/// Resultado padrão dos provedores.
pub type Result<T> = std::result::Result<T, MediaError>;

/// Falha de transporte devolvida por [`crate::http::Fetcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchError {
    message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

/// Erro tipado dos provedores.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// Falha de rede/DNS/TLS/timeout.
    #[error("{0}")]
    Transport(String),
    /// Resposta HTTP não-2xx no `getJSON`/`getText`.
    #[error("HTTP {status} em {url}")]
    Http { status: u16, url: String },
    /// Corpo que deveria ser JSON não decodificou.
    #[error("resposta inválida: {0}")]
    Decode(String),
    /// Falha de download de arte.
    #[error("download falhou: HTTP {status}")]
    Download { status: u16 },
    /// Erro do HowLongToBeat com a mensagem do cliente (`hltb http N`).
    #[error("hltb http {status}")]
    HltbHttp { status: u16 },
    /// Erro de provedor que no JS é só um `Error(mensagem)`.
    #[error("{0}")]
    Message(String),
}

impl MediaError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn decode(message: impl Into<String>) -> Self {
        Self::Decode(message.into())
    }
}

impl From<FetchError> for MediaError {
    fn from(error: FetchError) -> Self {
        Self::Transport(error.message)
    }
}
