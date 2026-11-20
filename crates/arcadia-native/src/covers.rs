//! Cache em memória das capas/heroes decodificados.
//!
//! A URL é a chave: a mesma imagem serve ao card da biblioteca e ao backdrop da
//! página do jogo. O cache guarda o `Handle` já em RGBA (decodificado pelo
//! crate `image`), então o renderer não baixa nem decodifica de novo.

use std::collections::HashMap;

use iced::widget::image::Handle;

#[derive(Debug, Clone)]
pub enum CoverState {
    Loading,
    Ready(Handle),
    Failed,
}

#[derive(Debug, Default)]
pub struct CoverCache {
    entries: HashMap<String, CoverState>,
}

impl CoverCache {
    /// Marca a URL como carregando e responde `true` quando uma busca deve ser
    /// disparada. URLs já prontas ou em voo não geram trabalho duplicado;
    /// falhas anteriores são tentadas de novo num reload.
    pub fn request(&mut self, url: &str) -> bool {
        match self.entries.get(url) {
            Some(CoverState::Loading | CoverState::Ready(_)) => false,
            Some(CoverState::Failed) | None => {
                self.entries.insert(url.to_string(), CoverState::Loading);
                true
            }
        }
    }

    pub fn insert(&mut self, url: impl Into<String>, handle: Handle) {
        self.entries.insert(url.into(), CoverState::Ready(handle));
    }

    pub fn fail(&mut self, url: &str) {
        self.entries.insert(url.to_string(), CoverState::Failed);
    }

    /// `Some(handle)` apenas quando a imagem já está pronta.
    pub fn ready(&self, url: Option<&str>) -> Option<&Handle> {
        match self.entries.get(url?) {
            Some(CoverState::Ready(handle)) => Some(handle),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel() -> Handle {
        Handle::from_rgba(1, 1, vec![0u8, 0, 0, 255])
    }

    #[test]
    fn request_is_idempotent_and_retries_failures() {
        let mut cache = CoverCache::default();

        assert!(cache.request("https://example.test/a.jpg"));
        assert!(!cache.request("https://example.test/a.jpg"));

        cache.fail("https://example.test/a.jpg");
        assert!(cache.ready(Some("https://example.test/a.jpg")).is_none());
        assert!(cache.request("https://example.test/a.jpg"));

        cache.insert("https://example.test/a.jpg", pixel());
        assert!(cache.ready(Some("https://example.test/a.jpg")).is_some());
        assert!(!cache.request("https://example.test/a.jpg"));
    }

    #[test]
    fn missing_or_empty_url_is_not_ready() {
        let cache = CoverCache::default();
        assert!(cache.ready(None).is_none());
        assert!(cache.ready(Some("")).is_none());
    }
}
