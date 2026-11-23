//! Implementação de `arcadia_ipc::Shell` para o app nativo.
//!
//! Nesta fatia os métodos de janela são no-op: o controle de janela ainda é do
//! SO (decorações nativas). `emit` registra no stderr e, para os eventos de
//! lançamento, conquista e download, alimenta uma fila thread-safe que o `App` drena
//! numa `Subscription` de baixa frequência. O mesmo sink é usado pelo
//! `LaunchRuntime` do IPC, que emite `game:running`/`game:active`/
//! `game:launchState`/`game:launchWarning` em threads de background.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use arcadia_ipc::Shell;
use serde_json::Value;

/// Eventos do lançamento que a UI nativa consome.
///
/// A lista é explícita para não encaminhar eventos que a UI não consome.
pub const LAUNCH_EVENTS: &[&str] = &[
    "game:running",
    "game:active",
    "game:launchState",
    "game:launchWarning",
    "game:launchError",
];

/// Eventos de conquista que viram toast na UI nativa.
pub const ACHIEVEMENT_EVENTS: &[&str] = &["achievement:unlocked"];
pub const DOWNLOAD_EVENTS: &[&str] = &["dm:progress", "torrent:progress"];

/// Teto da fila: um consumidor parado (por exemplo, durante um frame lento)
/// não pode acumular eventos sem limite. Eventos antigos são descartados.
const MAX_PENDING_EVENTS: usize = 128;

pub fn is_launch_event(event: &str) -> bool {
    LAUNCH_EVENTS.contains(&event)
}

pub fn is_achievement_event(event: &str) -> bool {
    ACHIEVEMENT_EVENTS.contains(&event)
}

pub fn is_download_event(event: &str) -> bool {
    DOWNLOAD_EVENTS.contains(&event)
}

/// Eventos que a fila da UI aceita.
pub fn is_ui_event(event: &str) -> bool {
    is_launch_event(event) || is_achievement_event(event) || is_download_event(event) || event == "library:changed"
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

    /// Progresso contém a lista completa: basta o retrato mais recente de cada
    /// fila, preservando espaço para conquistas e eventos de lançamento.
    pub fn push(&self, event: &str, payload: Value) {
        if !is_ui_event(event) {
            return;
        }

        let mut pending = self.lock();
        if is_download_event(event) || event == "library:changed" {
            if let Some(index) = pending.iter().position(|queued| queued.event == event) {
                pending.remove(index);
            }
        }
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
    data_dir: Option<PathBuf>,
}

impl NativeShell {
    #[allow(dead_code)] // usado pelos testes e por integrações sem data dir.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shell com o data dir do app, usado na busca do `boot.mp4`.
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            events: EventQueue::default(),
            data_dir: Some(data_dir.into()),
        }
    }

    /// Handle clonável para o app drenar a fila na Subscription.
    pub fn events(&self) -> EventQueue {
        self.events.clone()
    }

    /// Procura o vídeo no pacote, no checkout e na pasta de dados.
    pub fn boot_video_candidates(&self) -> Vec<PathBuf> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut candidates = Vec::new();

        if let Ok(executable) = std::env::current_exe() {
            if let Some(dir) = executable.parent() {
                candidates.push(dir.join("boot.mp4"));
                candidates.push(dir.join("resources").join("boot.mp4"));
            }
        }
        candidates.push(manifest.join("../../boot.mp4"));
        if let Some(data_dir) = self.data_dir.clone() {
            candidates.push(data_dir.join("boot.mp4"));
        }
        candidates
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
        self.boot_video_candidates()
            .into_iter()
            .find_map(|path| std::fs::read(path).ok())
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
    fn download_progress_keeps_the_latest_snapshot_without_evicting_launch_events() {
        let queue = EventQueue::default();
        queue.push("game:running", json!(true));
        for percent in 0..1000 {
            queue.push("dm:progress", json!([{ "appid": "steam:10", "percent": percent }]));
            queue.push("torrent:progress", json!([{ "gameId": "tor:10", "progress": percent }]));
        }
        let events = queue.drain();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event, "game:running");
        assert_eq!(events[1].event, "dm:progress");
        assert_eq!(events[1].payload[0]["percent"], 999);
        assert_eq!(events[2].event, "torrent:progress");
        assert_eq!(events[2].payload[0]["progress"], 999);
    }

    #[test]
    fn launch_and_library_events_enter_the_ui_queue() {
        let shell = NativeShell::new();
        let queue = shell.events();

        shell.emit("game:running", json!(true));
        shell.emit("library:changed", json!({ "source": "teste" }));
        shell.emit("sync:state", json!({ "queueLen": 0 }));

        let events = queue.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event, "library:changed");
        assert_eq!(events[0].event, "game:running");
        assert_eq!(events[0].payload, json!(true));
        assert!(queue.is_empty());
    }

    #[test]
    fn achievement_events_enter_the_queue_and_unrelated_ones_stay_out() {
        let shell = NativeShell::new();
        let queue = shell.events();

        shell.emit(
            "achievement:unlocked",
            json!({ "appid": "10", "key": "ACH", "title": "Conquista" }),
        );
        shell.emit("library:changed", json!({ "source": "teste" }));
        shell.emit("update:packaged:changed", json!({ "fase": "disponivel" }));

        let events = queue.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event, "library:changed");
        assert_eq!(events[0].event, "achievement:unlocked");
        assert_eq!(events[0].payload["title"], "Conquista");
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
