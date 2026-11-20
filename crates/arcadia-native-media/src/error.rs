//! Erros públicos do player.
//!
//! Os erros de pipeline que chegam pelo bus do GStreamer não interrompem a
//! API: eles viram [`PlayerState::Error`](crate::PlayerState::Error), porque
//! acontecem em outra thread. As funções que falham de forma síncrona (URI
//! inválida, plugin ausente, seek recusado) devolvem [`enum@Error`].

use std::time::Duration;

use thiserror::Error;

/// Resultado das operações do player.
pub type Result<T> = std::result::Result<T, Error>;

/// Falha síncrona ao montar ou controlar o player.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// O GStreamer não pôde ser inicializado neste processo.
    #[error("GStreamer não inicializou: {0}")]
    Init(String),

    /// URI vazia, com esquema não suportado ou apontando para arquivo ausente.
    #[error("mídia inválida: {0}")]
    InvalidUri(String),

    /// Nenhuma mídia foi aberta (ou a sessão foi fechada).
    #[error("nenhuma mídia aberta")]
    NoMedia,

    /// Falta um plugin GStreamer necessário (por exemplo `hlsdemux`).
    #[error(
        "plugin GStreamer ausente: {0} (instale os plugins base/good/bad e, \
         para H.264, gst-libav ou openh264)"
    )]
    MissingPlugin(&'static str),

    /// O elemento existe na fábrica mas não pôde ser criado.
    #[error("elemento GStreamer indisponível: {0}")]
    MissingElement(&'static str),

    /// O pipeline recusou uma operação ou está em estado de erro.
    #[error("falha no pipeline: {0}")]
    Pipeline(String),

    /// O pipeline recusou o seek para a posição pedida.
    #[error("falha ao posicionar em {0:?}")]
    Seek(Duration),
}
