//! Cache em memória das capas/heroes decodificados.
//!
//! A URL é a chave: a mesma imagem serve ao card da biblioteca e ao backdrop da
//! página do jogo. O cache guarda o `Handle` já em RGBA (decodificado pelo
//! crate `image`), então o renderer não baixa nem decodifica de novo.
//!
//! Além do `Handle`, o cache guarda as dimensões em pixels: o `Handle` do iced
//! não expõe tamanho, e a descrição precisa da proporção real de cada figura
//! para reservar a altura certa (um banner 1170×450 e uma peça vertical
//! 616×670 não podem ocupar a mesma caixa).
//!
//! O cache tem orçamento de bytes e despeja a entrada usada há mais tempo.
//! Capas despejadas que continuam na tela entram numa fila de recarga; o shell
//! drena essa fila no tick de eventos, então nenhuma imagem fica em branco para
//! sempre e o total não cresce com todas as telas visitadas na sessão.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::widget::image::Handle;

/// Teto de memória das capas decodificadas (RGBA). Cada capa custa no máximo
/// ~2 MiB; 384 MiB cobrem com folga a tela atual e o histórico recente.
pub const DEFAULT_BUDGET_BYTES: usize = 384 * 1024 * 1024;

/// Evita enfileirar a mesma capa várias vezes no mesmo tick.
const MAX_MISSES: usize = 64;

#[derive(Debug, Clone)]
pub enum CoverState {
    /// Handle pronto e as dimensões em pixels da imagem decodificada.
    Ready {
        handle: Handle,
        size: (u32, u32),
        loaded_at: Instant,
    },
    Loading,
    Failed,
}

#[derive(Debug)]
struct Entry {
    state: CoverState,
    last_used: Cell<u64>,
    bytes: usize,
}

#[derive(Debug)]
pub struct CoverCache {
    entries: HashMap<String, Entry>,
    /// URLs ausentes que a tela atual pediu; o shell recarrega no próximo tick.
    misses: RefCell<Vec<String>>,
    tick: Cell<u64>,
    budget: usize,
    used: usize,
}

impl Default for CoverCache {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET_BYTES)
    }
}

impl CoverCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            misses: RefCell::new(Vec::new()),
            tick: Cell::new(0),
            budget,
            used: 0,
        }
    }

    fn next_tick(&self) -> u64 {
        let tick = self.tick.get().wrapping_add(1);
        self.tick.set(tick);
        tick
    }

    /// Marca a URL como carregando e responde `true` quando uma busca deve ser
    /// disparada. URLs já prontas ou em voo não geram trabalho duplicado;
    /// falhas anteriores são tentadas de novo num reload.
    pub fn request(&mut self, url: &str) -> bool {
        if matches!(
            self.entries.get(url),
            Some(Entry {
                state: CoverState::Loading | CoverState::Ready { .. },
                ..
            })
        ) {
            return false;
        }
        let tick = self.next_tick();
        self.entries.insert(
            url.to_string(),
            Entry {
                state: CoverState::Loading,
                last_used: Cell::new(tick),
                bytes: 0,
            },
        );
        true
    }

    pub fn insert(&mut self, url: impl Into<String>, handle: Handle, size: (u32, u32)) {
        let url = url.into();
        let bytes = (size.0 as usize)
            .saturating_mul(size.1 as usize)
            .saturating_mul(4);
        let tick = self.next_tick();
        if let Some(previous) = self.entries.remove(&url) {
            self.used = self.used.saturating_sub(previous.bytes);
        }
        self.entries.insert(
            url,
            Entry {
                state: CoverState::Ready {
                    handle,
                    size,
                    loaded_at: Instant::now(),
                },
                last_used: Cell::new(tick),
                bytes,
            },
        );
        self.used = self.used.saturating_add(bytes);
        self.evict();
    }

    pub fn fail(&mut self, url: &str) {
        let tick = self.next_tick();
        if let Some(previous) = self.entries.remove(url) {
            self.used = self.used.saturating_sub(previous.bytes);
        }
        self.entries.insert(
            url.to_string(),
            Entry {
                state: CoverState::Failed,
                last_used: Cell::new(tick),
                bytes: 0,
            },
        );
    }

    /// `Some(handle)` apenas quando a imagem já está pronta.
    pub fn ready(&self, url: Option<&str>) -> Option<&Handle> {
        Self::handle(self.ready_entry(url?)?)
    }

    /// Consulta de memória para uma tela que agenda as próprias cargas.
    pub fn ready_cached(&self, url: &str) -> Option<&Handle> {
        Self::handle(self.entries.get(url)?)
    }

    pub fn touch(&self, url: &str) {
        if let Some(entry) = self.entries.get(url) {
            self.use_ready(entry);
        }
    }

    fn handle(entry: &Entry) -> Option<&Handle> {
        match &entry.state {
            CoverState::Ready { handle, .. } => Some(handle),
            _ => None,
        }
    }

    pub fn ready_with_age(&self, url: Option<&str>) -> Option<(&Handle, Duration)> {
        let entry = self.ready_entry(url?)?;
        match &entry.state {
            CoverState::Ready {
                handle, loaded_at, ..
            } => Some((handle, loaded_at.elapsed())),
            _ => None,
        }
    }

    /// Dimensões em pixels da imagem pronta, quando disponíveis.
    ///
    /// A descrição usa isso para calcular a altura da figura pela proporção
    /// real em vez de fixar um valor que achata banners ou esmaga verticais.
    pub fn size(&self, url: Option<&str>) -> Option<(u32, u32)> {
        let entry = self.ready_entry(url?)?;
        match &entry.state {
            CoverState::Ready { size, .. } => Some(*size),
            _ => None,
        }
    }

    /// URLs que a tela pediu mas já não estão no cache (por despejo). O shell
    /// chama isto no tick e dispara `cover_tasks` para elas.
    pub fn take_misses(&self) -> Vec<String> {
        std::mem::take(&mut *self.misses.borrow_mut())
    }

    #[cfg(test)]
    fn used_bytes(&self) -> usize {
        self.used
    }

    fn ready_entry(&self, url: &str) -> Option<&Entry> {
        let Some(entry) = self.entries.get(url) else {
            self.record_miss(url);
            return None;
        };
        self.use_ready(entry)
    }

    fn use_ready<'a>(&self, entry: &'a Entry) -> Option<&'a Entry> {
        if matches!(entry.state, CoverState::Ready { .. }) {
            entry.last_used.set(self.next_tick());
            Some(entry)
        } else {
            None
        }
    }

    fn record_miss(&self, url: &str) {
        if url.is_empty() {
            return;
        }
        let mut misses = self.misses.borrow_mut();
        if misses.len() < MAX_MISSES && !misses.iter().any(|miss| miss == url) {
            misses.push(url.to_string());
        }
    }

    fn evict(&mut self) {
        while self.used > self.budget {
            let victim = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.bytes > 0)
                .min_by_key(|(_, entry)| entry.last_used.get())
                .map(|(url, _)| url.clone());
            let Some(url) = victim else { break };
            if let Some(entry) = self.entries.remove(&url) {
                self.used = self.used.saturating_sub(entry.bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel() -> Handle {
        Handle::from_rgba(1, 1, vec![0, 0, 0, 255])
    }

    #[test]
    fn request_is_idempotent_and_retries_failures() {
        let mut cache = CoverCache::default();

        assert!(cache.request("https://example.test/a.jpg"));
        assert!(!cache.request("https://example.test/a.jpg"));

        cache.fail("https://example.test/a.jpg");
        assert!(cache.ready(Some("https://example.test/a.jpg")).is_none());
        assert!(cache.request("https://example.test/a.jpg"));

        cache.insert("https://example.test/a.jpg", pixel(), (1, 1));
        assert!(cache.ready(Some("https://example.test/a.jpg")).is_some());
        assert!(!cache.request("https://example.test/a.jpg"));
    }

    #[test]
    fn missing_or_empty_url_is_not_ready() {
        let cache = CoverCache::default();
        assert!(cache.ready(None).is_none());
        assert!(cache.ready(Some("")).is_none());
        assert!(cache.size(None).is_none());
    }

    #[test]
    fn keeps_the_decoded_dimensions_for_layout() {
        let mut cache = CoverCache::default();
        // Banner de card de seção do `about`: 1170×450.
        cache.insert("https://shared.test/banner.avif", pixel(), (1170, 450));

        assert_eq!(
            cache.size(Some("https://shared.test/banner.avif")),
            Some((1170, 450))
        );
        // A proporção é o que a figura usa; confere que 2.6:1 chega intacta.
        let (width, height) = cache.size(Some("https://shared.test/banner.avif")).unwrap();
        assert!((width as f32 / height as f32 - 2.6).abs() < 0.01);
    }

    #[test]
    fn budget_evicts_the_least_recently_used_and_counts_bytes() {
        // Cada pixel 1×1 gasta 4 bytes; o teto é 16 bytes.
        let mut cache = CoverCache::with_budget(16);
        for url in ["a", "b", "c", "d"] {
            cache.insert(url, pixel(), (1, 1));
        }
        assert_eq!(cache.used_bytes(), 16);

        // "a" passa a ser o mais recente; o próximo insert deve despejar "b".
        assert!(cache.ready(Some("a")).is_some());
        cache.insert("e", pixel(), (1, 1));

        assert_eq!(cache.used_bytes(), 16);
        assert!(cache.ready(Some("a")).is_some());
        assert!(cache.ready(Some("c")).is_some());
        assert!(cache.ready(Some("b")).is_none());
        assert_eq!(cache.take_misses(), vec!["b".to_string()]);
        assert!(cache.take_misses().is_empty());
    }

    #[test]
    fn eviction_never_keeps_loading_or_failed_entries_in_the_budget() {
        let mut cache = CoverCache::with_budget(4);
        cache.request("loading");
        cache.fail("failed");
        assert_eq!(cache.used_bytes(), 0);
        cache.insert("ready", pixel(), (1, 1));
        assert_eq!(cache.used_bytes(), 4);
        assert!(cache.ready(Some("loading")).is_none());
        assert!(cache.ready(Some("failed")).is_none());
        assert!(cache.take_misses().is_empty());
    }

    #[test]
    fn absent_cover_is_queued_once_for_reload() {
        let cache = CoverCache::default();
        let url = "https://example.test/evicted.jpg";
        assert!(cache.ready(Some(url)).is_none());
        assert!(cache.ready(Some(url)).is_none());
        assert_eq!(cache.take_misses(), vec![url.to_string()]);
        assert!(cache.take_misses().is_empty());
    }
}
