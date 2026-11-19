//! Relógio, cache LRU com TTL e circuit breaker.
//!
//! Reproduz as três estruturas de `app/electron/metadata.js` que decidem
//! quando uma chamada à IGDB pode ser evitada:
//!
//! - [`LruCache`]: 100 entradas, TTL de 1 hora, `get` reordena;
//! - [`CircuitBreaker`]: abre após 5 falhas consecutivas por 60 s e fecha no
//!   primeiro sucesso em half-open;
//! - [`Clock`]: injetável para os testes não dependerem do relógio real.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Fonte de tempo em milissegundos desde a época.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Relógio do sistema.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}

/// Relógio manual para testes determinísticos.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    pub fn new(now_ms: u64) -> Self {
        Self {
            now: AtomicU64::new(now_ms),
        }
    }

    pub fn set(&self, now_ms: u64) {
        self.now.store(now_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, delta_ms: u64) {
        self.now.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

/// Cache LRU com expiração, equivalente à classe `LRUCache` do JS.
#[derive(Debug)]
pub struct LruCache<V> {
    max_size: usize,
    ttl_ms: u64,
    entries: HashMap<String, (V, u64)>,
    order: VecDeque<String>,
}

impl<V: Clone> LruCache<V> {
    pub fn new(max_size: usize, ttl_ms: u64) -> Self {
        Self {
            max_size,
            ttl_ms,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &str, now_ms: u64) -> Option<V> {
        let (value, expires) = self.entries.get(key)?.clone();
        if now_ms > expires {
            self.entries.remove(key);
            self.order.retain(|entry| entry != key);
            return None;
        }
        self.touch(key);
        Some(value)
    }

    pub fn set(&mut self, key: &str, value: V, now_ms: u64) {
        if self.entries.remove(key).is_some() {
            self.order.retain(|entry| entry != key);
        }
        if self.entries.len() >= self.max_size {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.entries
            .insert(key.to_string(), (value, now_ms.saturating_add(self.ttl_ms)));
        self.order.push_back(key.to_string());
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|entry| entry != key);
        self.order.push_back(key.to_string());
    }
}

/// Estados do circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker com a mesma contagem e janela do JS.
#[derive(Debug)]
pub struct CircuitBreaker {
    threshold: u32,
    timeout_ms: u64,
    failures: u32,
    state: BreakerState,
    next_attempt_ms: u64,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, timeout_ms: u64) -> Self {
        Self {
            threshold: threshold.max(1),
            timeout_ms,
            failures: 0,
            state: BreakerState::Closed,
            next_attempt_ms: 0,
        }
    }

    pub fn state(&self) -> BreakerState {
        self.state
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// `execute` antes de chamar `fn`: `false` significa rejeitar rápido.
    ///
    /// Quando o prazo venceu, o estado vira half-open e a chamada é permitida.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if self.state == BreakerState::Open {
            if now_ms < self.next_attempt_ms {
                return false;
            }
            self.state = BreakerState::HalfOpen;
        }
        true
    }

    /// Sucesso zera as falhas e fecha o breaker.
    pub fn record_success(&mut self) {
        if self.state == BreakerState::HalfOpen || self.state == BreakerState::Closed {
            self.state = BreakerState::Closed;
            self.failures = 0;
        }
    }

    /// Falha incrementa o contador e abre o breaker ao atingir o limite.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.failures += 1;
        if self.failures >= self.threshold {
            self.state = BreakerState::Open;
            self.next_attempt_ms = now_ms.saturating_add(self.timeout_ms);
        }
    }

    pub fn reset(&mut self) {
        self.failures = 0;
        self.state = BreakerState::Closed;
        self.next_attempt_ms = 0;
    }
}

/// Mutex interno usado por clientes com estado (IGDB/HLTB).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_get_refreshes_recency_and_set_evicts_the_oldest() {
        let mut cache = LruCache::new(2, 1_000);
        cache.set("a", 1, 0);
        cache.set("b", 2, 0);
        assert_eq!(cache.get("a", 10), Some(1));
        cache.set("c", 3, 10);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get("b", 10), None);
        assert_eq!(cache.get("a", 10), Some(1));
        assert_eq!(cache.get("c", 10), Some(3));
    }

    #[test]
    fn lru_expires_entries_and_treats_the_boundary_as_live() {
        let mut cache = LruCache::new(4, 100);
        cache.set("a", "x", 1_000);
        assert_eq!(cache.get("a", 1_100), Some("x"));
        assert_eq!(cache.get("a", 1_101), None);
        cache.set("b", "y", 2_000);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn breaker_opens_after_threshold_and_closes_after_success() {
        let mut breaker = CircuitBreaker::new(3, 1_000);
        assert!(breaker.allow(0));
        breaker.record_failure(0);
        breaker.record_failure(10);
        assert_eq!(breaker.state(), BreakerState::Closed);
        breaker.record_failure(20);
        assert_eq!(breaker.state(), BreakerState::Open);

        assert!(!breaker.allow(500));
        assert!(breaker.allow(1_020));
        assert_eq!(breaker.state(), BreakerState::HalfOpen);
        breaker.record_success();
        assert_eq!(breaker.state(), BreakerState::Closed);
        assert_eq!(breaker.failures(), 0);
    }

    #[test]
    fn breaker_reopens_when_half_open_fails() {
        let mut breaker = CircuitBreaker::new(2, 1_000);
        breaker.record_failure(0);
        breaker.record_failure(0);
        assert!(breaker.allow(1_000));
        breaker.record_failure(1_000);
        assert_eq!(breaker.state(), BreakerState::Open);
        assert!(!breaker.allow(1_500));
    }

    #[test]
    fn manual_clock_advances_deterministically() {
        let clock = ManualClock::new(100);
        assert_eq!(clock.now_ms(), 100);
        clock.advance(25);
        assert_eq!(clock.now_ms(), 125);
        clock.set(7);
        assert_eq!(clock.now_ms(), 7);
    }
}
