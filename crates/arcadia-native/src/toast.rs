//! Fila dos toasts de conquista.
//!
//! O backend entrega o payload cru em `achievement:unlocked`; o `App` converte
//! em [`AchievementToast`] e enfileira aqui. A fila é limitada a três itens
//! (o mais novo na frente) e cada item tem um relógio em milissegundos que a
//! `Subscription` avança — assim entrada, saída e expiração são determinísticas
//! nos testes e não dependem do relógio do sistema.

use std::collections::VecDeque;

use crate::backend::AchievementToast;

/// Mesmo limite da UI web (`slice(0, 3)`).
pub const MAX_TOASTS: usize = 3;
/// Tempo de entrada da animação (fade + subida curta).
pub const ENTER_MS: u64 = 300;
/// Tempo visível antes de começar a sair.
pub const VISIBLE_MS: u64 = 5_000;
/// Tempo da animação de saída.
pub const EXIT_MS: u64 = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub achievement: AchievementToast,
    /// Idade em ms; avança no tick de ~50 ms enquanto o toast existe.
    pub age_ms: u64,
}

impl Toast {
    /// Opacidade da entrada/saída (0.0–1.0).
    pub fn alpha(&self) -> f32 {
        if self.age_ms < ENTER_MS {
            self.age_ms as f32 / ENTER_MS as f32
        } else if self.age_ms >= VISIBLE_MS {
            let exit = (self.age_ms - VISIBLE_MS).min(EXIT_MS) as f32 / EXIT_MS as f32;
            (1.0 - exit).max(0.0)
        } else {
            1.0
        }
    }

    /// Deslocamento vertical em px: sobe ao entrar e continua subindo ao sair.
    pub fn offset_y(&self) -> f32 {
        if self.age_ms < ENTER_MS {
            12.0 * (1.0 - self.age_ms as f32 / ENTER_MS as f32)
        } else if self.age_ms >= VISIBLE_MS {
            -8.0 * (self.age_ms - VISIBLE_MS).min(EXIT_MS) as f32 / EXIT_MS as f32
        } else {
            0.0
        }
    }

    #[cfg(test)]
    pub fn is_leaving(&self) -> bool {
        self.age_ms >= VISIBLE_MS
    }

    pub fn is_expired(&self) -> bool {
        self.age_ms >= VISIBLE_MS + EXIT_MS
    }
}

/// Fila com no máximo [`MAX_TOASTS`] itens, do mais novo para o mais antigo.
#[derive(Debug, Default)]
pub struct ToastQueue {
    items: VecDeque<Toast>,
    next_id: u64,
}

impl ToastQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enfileira no topo; com a fila cheia, o mais antigo é descartado.
    pub fn push(&mut self, achievement: AchievementToast) -> u64 {
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        self.items.push_front(Toast {
            id,
            achievement,
            age_ms: 0,
        });
        while self.items.len() > MAX_TOASTS {
            self.items.pop_back();
        }
        id
    }

    /// Avança o relógio de todos os itens e remove os que terminarem a saída.
    pub fn advance(&mut self, delta_ms: u64) {
        for toast in &mut self.items {
            toast.age_ms = toast.age_ms.saturating_add(delta_ms);
        }
        self.items.retain(|toast| !toast.is_expired());
    }

    /// Fecha um toast pelo id (botão "×").
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|toast| toast.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(title: &str) -> AchievementToast {
        AchievementToast {
            appid: "10".to_string(),
            key: title.to_string(),
            title: title.to_string(),
            desc: None,
            icon: None,
            percent: None,
            unlock: None,
        }
    }

    #[test]
    fn queue_keeps_at_most_three_newest_first() {
        let mut queue = ToastQueue::new();
        queue.push(toast("A"));
        queue.push(toast("B"));
        queue.push(toast("C"));
        queue.push(toast("D"));

        let titles: Vec<_> = queue
            .iter()
            .map(|toast| toast.achievement.title.as_str())
            .collect();
        assert_eq!(titles, ["D", "C", "B"]);
        assert_eq!(queue.len(), MAX_TOASTS);
    }

    #[test]
    fn queue_preserves_arrival_order_per_item() {
        let mut queue = ToastQueue::new();
        let first = queue.push(toast("A"));
        let second = queue.push(toast("B"));
        assert!(second > first);

        let ids: Vec<_> = queue.iter().map(|toast| toast.id).collect();
        assert_eq!(ids, [second, first]);
    }

    #[test]
    fn toast_expires_after_visible_time_plus_exit_animation() {
        let mut queue = ToastQueue::new();
        queue.push(toast("A"));

        queue.advance(VISIBLE_MS - 1);
        assert_eq!(queue.len(), 1);
        assert!(!queue.iter().next().unwrap().is_leaving());
        assert_eq!(queue.iter().next().unwrap().alpha(), 1.0);

        queue.advance(1);
        let toast = queue.iter().next().unwrap();
        assert!(toast.is_leaving());
        assert_eq!(toast.alpha(), 1.0);

        queue.advance(EXIT_MS / 2);
        assert!(queue.iter().next().unwrap().alpha() < 1.0);
        assert_eq!(queue.len(), 1);

        queue.advance(EXIT_MS);
        assert!(queue.is_empty());
    }

    #[test]
    fn enter_animation_starts_transparent_and_ends_opaque() {
        let mut queue = ToastQueue::new();
        queue.push(toast("A"));
        assert_eq!(queue.iter().next().unwrap().alpha(), 0.0);

        queue.advance(ENTER_MS / 2);
        let alpha = queue.iter().next().unwrap().alpha();
        assert!(alpha > 0.4 && alpha < 0.6, "alpha: {alpha}");

        queue.advance(ENTER_MS / 2);
        assert_eq!(queue.iter().next().unwrap().alpha(), 1.0);
        assert_eq!(queue.iter().next().unwrap().offset_y(), 0.0);
    }

    #[test]
    fn dismiss_removes_only_the_requested_toast() {
        let mut queue = ToastQueue::new();
        let first = queue.push(toast("A"));
        let second = queue.push(toast("B"));

        assert!(queue.dismiss(first));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().next().unwrap().id, second);
        assert!(!queue.dismiss(first));
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut queue = ToastQueue::new();
        queue.push(toast("A"));
        queue.clear();
        assert!(queue.is_empty());
    }
}
