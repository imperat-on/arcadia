//! Stub do player de mídia.
//!
//! O player GStreamer (trailers HLS da Steam, vídeo de boot e wallpapers
//! animados) entra numa onda própria. A página do jogo já reserva a área e a
//! interface abaixo evita que a view precise conhecer a tecnologia final.

/// Texto exibido no card "Trailer" enquanto o player não existe.
pub const TRAILER_PLACEHOLDER: &str =
    "Player GStreamer ainda não embarcado. O trailer real entra na próxima onda de mídia.";

/// Contrato mínimo que a página do jogo espera de um player futuro.
#[derive(Debug, Default)]
pub struct MediaPlayer {
    _private: (),
}

impl MediaPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `false` enquanto o backend GStreamer não estiver implementado.
    pub fn is_available(&self) -> bool {
        false
    }

    pub fn status_label(&self) -> &'static str {
        TRAILER_PLACEHOLDER
    }
}
