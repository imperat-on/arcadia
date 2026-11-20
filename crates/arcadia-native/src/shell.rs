//! Implementação de `arcadia_ipc::Shell` para o app nativo.
//!
//! Nesta fatia os métodos de janela são no-op: o controle de janela ainda é do
//! SO (decorações nativas). `emit` registra no stderr e, para os eventos de
//! lançamento, alimenta uma fila thread-safe que o `App` drena numa
//! `Subscription` de baixa frequência. O mesmo sink é usado pelo
//! `LaunchRuntime` do IPC, que emite `game:running`/`game:active`/
//! `game:launchState`/`game:launchWarning` em threads de background.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use arcadia_ipc::Shell;
use serde_json::Value;

/// Eventos do lançamento que a UI nativa consome.
///
/// A lista é explícita de propósito: o sink do `AppState` também recebe
/// progresso de downloads e eventos de tempo real, que não devem encher a
/// fila da UI nem acordar a tela do jogo.
pub const LAUNCH_EVENTS: &[&str] = &[
    "game:running",
    "game:active",
    "game:launchState",
    "game:launchWarning",
    "game:launchError",
];

/// Teto da fila: um consumidor parado (por exemplo, durante um frame lento)
/// não pode acumular eventos sem limite. Eventos antigos são descartados.
const MAX_PENDING_EVENTS: usize = 128;

pub fn is_launch_event(event: &str) -> bool {
    LAUNCH_EVENTS.contains(&event)
}

/// Evento cru vindo do shell ou do sink de background do IPC.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellEvent {
    pub event: String,
    pub payload: Value,
}

/// Fila compartilhada entre o shell, o runtime de launch e a thread do iced.
#[derive(Debug, Clone, Default)]
pub struct EventQueue {
    pending: Arc<Mutex<VecDeque<ShellEvent>>>,
}

impl EventQueue {
    fn lock(&self) -> MutexGuard<'_, VecDeque<ShellEvent>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Enfileira somente eventos de lançamento; o resto é ignorado.
    pub fn push(&self, event: &str, payload: Value) {
        if !is_launch_event(event) {
            return;
        }

        let mut pending = self.lock();
        if pending.len() >= MAX_PENDING_EVENTS {
            pending.pop_front();
        }
        pending.push_back(ShellEvent {
            event: event.to_string(),
            payload,
        });
    }

    /// Retira todos os eventos pendentes de uma vez (ordem de chegada).
    pub fn drain(&self) -> Vec<ShellEvent> {
        let drained = std::mem::take(&mut *self.lock());
        drained.into_iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct NativeShell {
    events: EventQueue,
}

impl NativeShell {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle clonável para o app drenar a fila na Subscription.
    pub fn events(&self) -> EventQueue {
        self.events.clone()
    }
}

impl Shell for NativeShell {
    fn minimize(&self) {}

    fn toggle_maximize(&self) {}

    fn close(&self) {}

    fn quit(&self) {
        eprintln!("[native-shell] quit solicitado pelo IPC");
    }

    fn set_fullscreen(&self, _on: bool) -> Result<(), String> {
        Ok(())
    }

    fn toggle_fullscreen(&self) -> Result<(), String> {
        Ok(())
    }

    fn is_focused(&self) -> bool {
        true
    }

    fn boot_video(&self) -> Option<Vec<u8>> {
        // O vídeo de boot (GStreamer/mpv) fica para a onda de mídia.
        None
    }

    fn emit(&self, event: &str, payload: Value) {
        let text = payload.to_string();
        let preview: String = text.chars().take(240).collect();
        eprintln!("[native-shell] evento {event}: {preview}");
        self.events.push(event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_launch_events_enter_the_ui_queue() {
        let shell = NativeShell::new();
        let queue = shell.events();

        shell.emit("game:running", json!(true));
        shell.emit("library:changed", json!({ "source": "teste" }));
        shell.emit("sync:state", json!({ "queueLen": 0 }));

        let events = queue.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "game:running");
        assert_eq!(events[0].payload, json!(true));
        assert!(queue.is_empty());
    }

    #[test]
    fn launch_state_sink_feeds_the_same_queue_in_order() {
        let shell = NativeShell::new();
        let queue = shell.events();
        let sink: arcadia_ipc::EventSink = Arc::new({
            let queue = queue.clone();
            move |event, payload| queue.push(event, payload)
        });

        sink(
            "game:launchState",
            json!({ "state": "starting", "gameId": "steam:10" }),
        );
        sink("game:running", json!(true));

        let events = queue.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "game:launchState");
        assert_eq!(events[1].event, "game:running");
    }

    #[test]
    fn queue_drops_oldest_events_when_full() {
        let queue = EventQueue::default();
        for index in 0..(MAX_PENDING_EVENTS + 5) {
            queue.push("game:launchState", json!({ "state": "idle", "n": index }));
        }

        let events = queue.drain();
        assert_eq!(events.len(), MAX_PENDING_EVENTS);
        assert_eq!(events[0].payload["n"], 5);
        assert_eq!(
            events[MAX_PENDING_EVENTS - 1].payload["n"],
            MAX_PENDING_EVENTS + 4
        );
    }
}
