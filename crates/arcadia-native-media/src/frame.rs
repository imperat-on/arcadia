//! Um frame de vídeo já decodificado e convertido para RGBA.

use std::time::Duration;

/// Frame RGBA empacotado, pronto para virar textura na UI.
///
/// `bytes` tem exatamente `stride * height` bytes e as linhas já estão
/// compactadas (`stride == width * 4`), então o buffer pode ser entregue
/// direto para `iced::widget::image::Handle::from_rgba`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Largura em pixels.
    pub width: u32,
    /// Altura em pixels.
    pub height: u32,
    /// Bytes por linha. Sempre `width as usize * 4` nesta implementação.
    pub stride: usize,
    /// Pixels RGBA (R, G, B, A), linha a linha.
    pub bytes: Vec<u8>,
    /// PTS do buffer, quando o pipeline informa.
    pub pts: Option<Duration>,
}

impl Frame {
    /// `true` se não há pixels.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.bytes.is_empty()
    }
}
