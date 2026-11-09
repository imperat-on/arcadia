//! Política pura da fila de downloads.
//!
//! Port 1:1 de `app/electron/download-queue-policy.js` (`normalizePriority`,
//! `nextQueued`) mais a máquina de estados que o `downloadmanager.js` aplica
//! sobre a fila: prioridade, um slot ativo por padrão (serial), pause/resume,
//! cancelamento, retry, estados terminais e normalização da fila persistida.
//!
//! Nada aqui spawna processos, mexe em disco ou usa timers; as decisões apenas
//! descrevem o que o chamador deve fazer.

use serde_json::{Map, Value};

use crate::integrity::{
    cleanup_plan, integrity_mode, normalize_recovery_attempts, CleanupPlan, CleanupReason,
    RecoveryAction, RecoveryDecision, DEFAULT_MAX_RECOVERY_ATTEMPTS,
};
use crate::js::js_number;

pub const MIN_PRIORITY: i32 = -10;
pub const MAX_PRIORITY: i32 = 10;
/// O manager Electron mantém apenas um processo ativo por vez.
pub const DEFAULT_MAX_ACTIVE: usize = 1;

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_DOWNLOADING: &str = "downloading";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_DONE: &str = "done";
pub const STATUS_ERROR: &str = "error";
pub const STATUS_CANCELED: &str = "canceled";

pub const INTEGRITY_PENDING: &str = "pending";
pub const INTEGRITY_DOWNLOADING: &str = "downloading";
pub const INTEGRITY_VERIFYING: &str = "verifying";
pub const INTEGRITY_VERIFIED: &str = "verified";
pub const INTEGRITY_FAILED: &str = "failed";
pub const INTEGRITY_RETRYING: &str = "retrying";
pub const INTEGRITY_PAUSED: &str = "paused";

/// `normalizePriority(value)`: `Number()` -> `Math.trunc` -> clamp [-10, 10].
pub fn normalize_priority(value: &Value) -> i32 {
    match js_number(value) {
        Some(number) if number.is_finite() => number
            .trunc()
            .clamp(MIN_PRIORITY as f64, MAX_PRIORITY as f64)
            as i32,
        _ => 0,
    }
}

fn status(item: &Value) -> Option<&str> {
    item.get("status").and_then(Value::as_str)
}

fn appid(item: &Value) -> Option<&str> {
    item.get("appid")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
}

fn is_active_status(status: Option<&str>) -> bool {
    status == Some(STATUS_QUEUED)
        || status == Some(STATUS_DOWNLOADING)
        || status == Some(STATUS_PAUSED)
}

fn set_string(item: &mut Value, key: &str, value: &str) {
    if let Some(object) = item.as_object_mut() {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn set_number(item: &mut Value, key: &str, value: f64) {
    if let Some(object) = item.as_object_mut() {
        if let Some(number) = serde_json::Number::from_f64(value) {
            object.insert(key.to_string(), Value::Number(number));
        }
    }
}

fn set_value(item: &mut Value, key: &str, value: Value) {
    if let Some(object) = item.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn set_integrity_state(item: &mut Value, state: &str) {
    let method = integrity_mode(item).as_str().to_string();
    let existing = item.get("integrity").cloned();
    let mut integrity = match existing {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    integrity.insert("state".to_string(), Value::String(state.to_string()));
    integrity.insert("method".to_string(), Value::String(method));
    set_value(item, "integrity", Value::Object(integrity));
}

fn set_integrity_retrying(item: &mut Value, last_error: &str) {
    let method = integrity_mode(item).as_str().to_string();
    let existing = item.get("integrity").cloned();
    let mut integrity = match existing {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    integrity.insert(
        "state".to_string(),
        Value::String(INTEGRITY_RETRYING.to_string()),
    );
    integrity.insert("method".to_string(), Value::String(method));
    integrity.insert(
        "lastError".to_string(),
        Value::String(last_error.to_string()),
    );
    set_value(item, "integrity", Value::Object(integrity));
}

/// Índice do próximo item `queued`: maior prioridade normalizada; empate
/// preserva FIFO pela posição original.
pub fn next_queued_index(queue: &[Value]) -> Option<usize> {
    queue
        .iter()
        .enumerate()
        .filter(|(_, item)| status(item) == Some(STATUS_QUEUED))
        .min_by(|(left_index, left), (right_index, right)| {
            normalize_priority(right.get("priority").unwrap_or(&Value::Null))
                .cmp(&normalize_priority(
                    left.get("priority").unwrap_or(&Value::Null),
                ))
                .then_with(|| left_index.cmp(right_index))
        })
        .map(|(index, _)| index)
}

/// `nextQueued(queue)` — retorna o item vencedor sem removê-lo.
pub fn next_queued(queue: &[Value]) -> Option<&Value> {
    next_queued_index(queue).map(|index| &queue[index])
}

/// Normaliza a fila lida do disco, como `load()` no manager:
/// prioridade/tentativas, integridade default, `downloading` -> `paused`
/// (o processo morreu junto com o app) e itens `done` saem da lista.
pub fn normalize_loaded_queue(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(normalize_loaded_item)
        .filter(|item| status(item) != Some(STATUS_DONE))
        .collect()
}

fn normalize_loaded_item(item: Value) -> Option<Value> {
    item.as_object()?;
    let mode = integrity_mode(&item).as_str().to_string();
    let priority = normalize_priority(item.get("priority").unwrap_or(&Value::Null));
    let recovery_attempts =
        normalize_recovery_attempts(item.get("recoveryAttempts").unwrap_or(&Value::Null));
    let was_downloading = status(&item) == Some(STATUS_DOWNLOADING);
    let old_integrity = item.get("integrity").cloned();

    let mut item = item;
    let object = item.as_object_mut()?;
    object.insert("priority".to_string(), Value::from(priority));
    object.insert(
        "recoveryAttempts".to_string(),
        Value::from(recovery_attempts),
    );

    let mut integrity = match old_integrity {
        Some(Value::Object(map)) => map,
        _ => {
            let mut map = Map::new();
            map.insert(
                "state".to_string(),
                Value::String(INTEGRITY_PENDING.to_string()),
            );
            map
        }
    };
    let method_ok = integrity
        .get("method")
        .and_then(Value::as_str)
        .is_some_and(|method| !method.is_empty());
    if !method_ok {
        integrity.insert("method".to_string(), Value::String(mode));
    }
    if was_downloading {
        object.insert(
            "status".to_string(),
            Value::String(STATUS_PAUSED.to_string()),
        );
        if integrity.get("state").and_then(Value::as_str) == Some(INTEGRITY_VERIFYING) {
            integrity.insert(
                "state".to_string(),
                Value::String(INTEGRITY_PAUSED.to_string()),
            );
        }
    }
    object.insert("integrity".to_string(), Value::Object(integrity));
    Some(item)
}

fn normalize_enqueued_item(item: &mut Value) {
    let priority = normalize_priority(item.get("priority").unwrap_or(&Value::Null));
    set_string(item, "status", STATUS_QUEUED);
    set_number(item, "percent", 0.0);
    set_number(item, "done", 0.0);
    set_number(item, "total", 0.0);
    set_string(item, "eta", "");
    set_number(item, "speed", 0.0);
    set_string(item, "error", "");
    set_value(item, "priority", Value::from(priority));
    set_value(item, "recoveryAttempts", Value::from(0));
    set_integrity_state(item, INTEGRITY_PENDING);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// Item entrou na fila; `replaced` conta itens terminais do mesmo appid que
    /// foram removidos (o manager evita dois cards com a mesma chave).
    Added { replaced: usize },
    /// Já existe item queued/downloading/paused para este appid.
    AlreadyQueued,
    /// Objeto sem `appid` string não vazia.
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseOutcome {
    PausedDownloading,
    PausedQueued,
    AlreadyPaused,
    Terminal,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOutcome {
    /// Havia processo vivo (SIGCONT): volta a `downloading`.
    ResumedProcess,
    /// Sem processo vivo: volta para `queued` e o chamador chama `start_next`.
    Requeued,
    /// Não há slot livre para o processo retomado; ficou `queued`.
    RequeuedNoSlot,
    NotPaused,
    NotFound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cancellation {
    pub item: Value,
    pub cleanup: CleanupPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueTransition {
    Stopped,
    Verify,
    Done,
    Retry { delay_ms: i64 },
    Error { error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishStatus {
    Done,
    Error,
}

/// Fila em memória com as mesmas transições observáveis do manager Electron.
///
/// Estado ativo é derivado de `status == "downloading"`; não existe token de
/// slot separado, portanto pausar/cancelar/concluir sempre libera o slot.
#[derive(Debug, Clone)]
pub struct DownloadQueue {
    items: Vec<Value>,
    max_active: usize,
}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ACTIVE)
    }
}

impl DownloadQueue {
    pub fn new(max_active: usize) -> Self {
        Self {
            items: Vec::new(),
            max_active,
        }
    }

    pub fn from_items(items: Vec<Value>, max_active: usize) -> Self {
        Self {
            items: normalize_loaded_queue(items),
            max_active,
        }
    }

    pub fn items(&self) -> &[Value] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Value> {
        self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn max_active(&self) -> usize {
        self.max_active
    }

    pub fn set_max_active(&mut self, max_active: usize) {
        self.max_active = max_active;
    }

    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| status(item) == Some(STATUS_DOWNLOADING))
            .count()
    }

    pub fn has_free_slot(&self) -> bool {
        self.active_count() < self.max_active
    }

    pub fn active_appid(&self) -> Option<&str> {
        self.items
            .iter()
            .find(|item| status(item) == Some(STATUS_DOWNLOADING))
            .and_then(appid)
    }

    pub fn get(&self, appid: &str) -> Option<&Value> {
        self.items
            .iter()
            .find(|item| item.get("appid").and_then(Value::as_str) == Some(appid))
    }

    fn get_mut(&mut self, appid: &str) -> Option<&mut Value> {
        self.items
            .iter_mut()
            .find(|item| item.get("appid").and_then(Value::as_str) == Some(appid))
    }

    /// Instala/recoloca um item. Duplicata ativa é ignorada; item terminal do
    /// mesmo appid é removido antes (mesma regra de `install`/`installSteam`).
    pub fn enqueue(&mut self, item: Value) -> EnqueueOutcome {
        let Some(id) = appid(&item).map(str::to_string) else {
            return EnqueueOutcome::Invalid;
        };
        if self.items.iter().any(|known| {
            known.get("appid").and_then(Value::as_str) == Some(id.as_str())
                && is_active_status(status(known))
        }) {
            return EnqueueOutcome::AlreadyQueued;
        }
        let replaced = self
            .items
            .iter()
            .filter(|known| known.get("appid").and_then(Value::as_str) == Some(id.as_str()))
            .count();
        self.items
            .retain(|known| known.get("appid").and_then(Value::as_str) != Some(id.as_str()));
        let mut item = item;
        normalize_enqueued_item(&mut item);
        self.items.push(item);
        EnqueueOutcome::Added { replaced }
    }

    /// Marca o próximo `queued` como `downloading` e o devolve para o chamador
    /// spawnar o processo. Sem slot livre, devolve `None` (sem furar o cap).
    pub fn start_next(&mut self) -> Option<Value> {
        if !self.has_free_slot() {
            return None;
        }
        let index = next_queued_index(&self.items)?;
        let item = &mut self.items[index];
        set_string(item, "status", STATUS_DOWNLOADING);
        set_string(item, "error", "");
        set_integrity_state(item, INTEGRITY_DOWNLOADING);
        Some(item.clone())
    }

    /// `setPriority`: recusa itens terminais, como no manager.
    pub fn set_priority(&mut self, appid: &str, priority: &Value) -> bool {
        let Some(item) = self.get_mut(appid) else {
            return false;
        };
        if matches!(
            status(item),
            Some(STATUS_DONE | STATUS_ERROR | STATUS_CANCELED)
        ) {
            return false;
        }
        let priority = normalize_priority(priority);
        set_value(item, "priority", Value::from(priority));
        true
    }

    pub fn pause(&mut self, appid: &str) -> PauseOutcome {
        let Some(item) = self.get_mut(appid) else {
            return PauseOutcome::NotFound;
        };
        match status(item) {
            Some(STATUS_DOWNLOADING) => {
                set_string(item, "status", STATUS_PAUSED);
                set_number(item, "speed", 0.0);
                PauseOutcome::PausedDownloading
            }
            Some(STATUS_QUEUED) => {
                set_string(item, "status", STATUS_PAUSED);
                PauseOutcome::PausedQueued
            }
            Some(STATUS_PAUSED) => PauseOutcome::AlreadyPaused,
            Some(STATUS_DONE | STATUS_ERROR | STATUS_CANCELED) => PauseOutcome::Terminal,
            _ => PauseOutcome::NotFound,
        }
    }

    /// `resume`: com processo vivo volta a baixar; sem processo vivo (app
    /// reiniciado) volta para a fila e depende de `start_next`.
    pub fn resume(&mut self, appid: &str, live_process: bool) -> ResumeOutcome {
        let free_slot = self.has_free_slot();
        let Some(item) = self.get_mut(appid) else {
            return ResumeOutcome::NotFound;
        };
        if status(item) != Some(STATUS_PAUSED) {
            return ResumeOutcome::NotPaused;
        }
        if live_process && free_slot {
            set_string(item, "status", STATUS_DOWNLOADING);
            set_integrity_state(item, INTEGRITY_DOWNLOADING);
            return ResumeOutcome::ResumedProcess;
        }
        set_string(item, "status", STATUS_QUEUED);
        if live_process {
            ResumeOutcome::RequeuedNoSlot
        } else {
            ResumeOutcome::Requeued
        }
    }

    /// Remove o item (inclusive ativo) e informa a limpeza. O manager cancela =
    /// começar do zero, apagando parciais/instalação.
    pub fn cancel(&mut self, appid: &str) -> Option<Cancellation> {
        let index = self
            .items
            .iter()
            .position(|item| item.get("appid").and_then(Value::as_str) == Some(appid))?;
        let mut item = self.items[index].clone();
        set_string(&mut item, "status", STATUS_CANCELED);
        set_number(&mut item, "speed", 0.0);
        self.items
            .retain(|known| known.get("appid").and_then(Value::as_str) != Some(appid));
        Some(Cancellation {
            item,
            cleanup: cleanup_plan(CleanupReason::Canceled),
        })
    }

    /// `retry`: só de `error`, zera erro/progresso/tentativas e descarta a
    /// fila de comandos de depot (comandos externos nunca são reutilizados).
    pub fn retry(&mut self, appid: &str) -> bool {
        let Some(item) = self.get_mut(appid) else {
            return false;
        };
        if status(item) != Some(STATUS_ERROR) {
            return false;
        }
        set_value(item, "fila", Value::Null);
        set_value(item, "filaIdx", Value::from(0));
        set_string(item, "depotAtualId", "");
        set_value(item, "depotsOk", Value::from(0));
        set_value(item, "depotsFalhos", Value::Array(Vec::new()));
        set_value(item, "recoveryAttempts", Value::from(0));
        set_string(item, "status", STATUS_QUEUED);
        set_string(item, "error", "");
        set_number(item, "percent", 0.0);
        set_number(item, "done", 0.0);
        set_number(item, "speed", 0.0);
        set_string(item, "eta", "");
        set_integrity_state(item, INTEGRITY_PENDING);
        true
    }

    /// `descartar`: só remove item terminal; não mexe em disco.
    pub fn dismiss(&mut self, appid: &str) -> bool {
        let Some(item) = self.get(appid) else {
            return false;
        };
        if is_active_status(status(item)) {
            return false;
        }
        self.items
            .retain(|known| known.get("appid").and_then(Value::as_str) != Some(appid));
        true
    }

    /// `finish(it, "done" | "error")`: estado terminal e liberação do slot.
    pub fn finish(&mut self, appid: &str, status_value: FinishStatus, error: &str) -> bool {
        let Some(item) = self.get_mut(appid) else {
            return false;
        };
        match status_value {
            FinishStatus::Done => {
                set_string(item, "status", STATUS_DONE);
                set_string(item, "error", "");
                set_number(item, "speed", 0.0);
                set_integrity_state(item, INTEGRITY_VERIFIED);
            }
            FinishStatus::Error => {
                set_string(item, "status", STATUS_ERROR);
                set_string(item, "error", error);
                set_number(item, "speed", 0.0);
                set_integrity_state(item, INTEGRITY_FAILED);
            }
        }
        true
    }

    /// Aplica a decisão de `recoveryDecision` ao item e devolve a transição.
    /// Timers de retry e spawn de verify ficam para a borda (Electron/tokio).
    pub fn apply_recovery(
        &mut self,
        appid: &str,
        decision: &RecoveryDecision,
    ) -> Option<QueueTransition> {
        let index = self
            .items
            .iter()
            .position(|item| item.get("appid").and_then(Value::as_str) == Some(appid))?;
        match decision.action {
            RecoveryAction::Stopped => Some(QueueTransition::Stopped),
            RecoveryAction::Verify => {
                let item = &mut self.items[index];
                set_integrity_state(item, INTEGRITY_VERIFYING);
                Some(QueueTransition::Verify)
            }
            RecoveryAction::Done => {
                let item = &mut self.items[index];
                set_string(item, "status", STATUS_DONE);
                set_string(item, "error", "");
                set_number(item, "speed", 0.0);
                set_integrity_state(item, INTEGRITY_VERIFIED);
                Some(QueueTransition::Done)
            }
            RecoveryAction::Retry => {
                let error = decision.error.clone().unwrap_or_default();
                let item = &mut self.items[index];
                set_string(item, "status", STATUS_QUEUED);
                set_value(item, "recoveryAttempts", Value::from(decision.attempts));
                set_string(
                    item,
                    "error",
                    &format!(
                        "falha: {error}; nova tentativa {}/{}",
                        decision.attempts, DEFAULT_MAX_RECOVERY_ATTEMPTS
                    ),
                );
                set_integrity_retrying(item, &error);
                Some(QueueTransition::Retry {
                    delay_ms: decision.delay_ms.unwrap_or(0),
                })
            }
            RecoveryAction::Error => {
                let error = decision.error.clone().unwrap_or_default();
                let item = &mut self.items[index];
                set_string(item, "status", STATUS_ERROR);
                set_string(item, "error", &error);
                set_number(item, "speed", 0.0);
                set_integrity_state(item, INTEGRITY_FAILED);
                Some(QueueTransition::Error { error })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::recovery_decision;
    use serde_json::json;

    fn item(appid: &str, status: &str, priority: i32) -> Value {
        json!({ "appid": appid, "status": status, "priority": priority })
    }

    #[test]
    fn prioridade_da_fila_e_limitada_e_normalizada() {
        assert_eq!(normalize_priority(&json!("4.9")), 4);
        assert_eq!(normalize_priority(&json!(99)), 10);
        assert_eq!(normalize_priority(&json!(-99)), -10);
        assert_eq!(normalize_priority(&json!("nan")), 0);
        assert_eq!(normalize_priority(&json!(null)), 0);
        assert_eq!(normalize_priority(&json!(-0.5)), 0);
    }

    #[test]
    fn fila_escolhe_prioridade_maior_e_preserva_fifo_em_empate() {
        let queue = vec![
            item("a", "queued", 0),
            item("b", "paused", 10),
            item("c", "queued", 5),
            item("d", "queued", 5),
        ];
        assert_eq!(
            next_queued(&queue)
                .and_then(|item| item.get("appid"))
                .and_then(Value::as_str),
            Some("c")
        );
        assert!(next_queued(&[item("x", "done", 0)]).is_none());
    }

    #[test]
    fn fila_ignora_pausado_no_inicio_e_duplicatas_mantem_fifo() {
        let queue = vec![
            item("pausado", "paused", 10),
            item("dup", "queued", 1),
            item("dup", "queued", 1),
            item("baixo", "queued", 0),
        ];
        assert_eq!(next_queued_index(&queue), Some(1));
        assert_eq!(
            next_queued(&queue)
                .and_then(|item| item.get("appid"))
                .and_then(Value::as_str),
            Some("dup")
        );
        assert!(next_queued(&[json!(null), json!(42), json!({ "status": "queued" })]).is_some());
    }

    #[test]
    fn fila_persistida_volta_pausada_e_descarta_done() {
        let loaded = normalize_loaded_queue(vec![
            json!({
                "appid": "a",
                "status": "downloading",
                "priority": 99,
                "recoveryAttempts": -3,
                "integrity": { "state": "verifying" },
            }),
            json!({ "appid": "b", "status": "done" }),
            json!({ "appid": "c", "status": "queued" }),
        ]);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["status"], "paused");
        assert_eq!(loaded[0]["priority"], 10);
        assert_eq!(loaded[0]["recoveryAttempts"], 0);
        assert_eq!(loaded[0]["integrity"]["state"], "paused");
        assert_eq!(loaded[0]["integrity"]["method"], "legendary-verify");
        assert_eq!(loaded[1]["integrity"]["state"], "pending");
    }

    #[test]
    fn cap_de_concorrencia_nao_vaza_slot() {
        let mut queue = DownloadQueue::new(2);
        queue.enqueue(item("a", "queued", 0));
        queue.enqueue(item("b", "queued", 5));
        queue.enqueue(item("c", "queued", 10));

        assert_eq!(queue.start_next().unwrap()["appid"], "c");
        assert_eq!(queue.start_next().unwrap()["appid"], "b");
        assert!(queue.start_next().is_none());
        assert_eq!(queue.active_count(), 2);

        assert_eq!(queue.pause("b"), PauseOutcome::PausedDownloading);
        assert_eq!(queue.active_count(), 1);
        assert_eq!(queue.start_next().unwrap()["appid"], "a");
        assert!(queue.start_next().is_none());

        queue.cancel("c").unwrap();
        assert_eq!(queue.active_count(), 1);
        queue.finish("a", FinishStatus::Done, "");
        assert_eq!(queue.active_count(), 0);
        assert!(queue.has_free_slot());
    }

    #[test]
    fn duplicate_appid_ativo_e_ignorado_e_terminal_e_substituido() {
        let mut queue = DownloadQueue::new(1);
        assert_eq!(
            queue.enqueue(item("a", "queued", 0)),
            EnqueueOutcome::Added { replaced: 0 }
        );
        assert_eq!(
            queue.enqueue(item("a", "queued", 10)),
            EnqueueOutcome::AlreadyQueued
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.start_next().unwrap()["priority"], 0);

        queue.finish("a", FinishStatus::Error, "boom");
        assert_eq!(
            queue.enqueue(item("a", "queued", 3)),
            EnqueueOutcome::Added { replaced: 1 }
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.get("a").unwrap()["status"], "queued");
        assert_eq!(queue.get("a").unwrap()["priority"], 3);
        assert_eq!(queue.get("a").unwrap()["recoveryAttempts"], 0);
    }

    #[test]
    fn enqueue_invalido_e_sem_appid_e_recusado() {
        let mut queue = DownloadQueue::new(1);
        assert_eq!(queue.enqueue(json!(null)), EnqueueOutcome::Invalid);
        assert_eq!(
            queue.enqueue(json!({ "appid": "  ", "status": "queued" })),
            EnqueueOutcome::Invalid
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn pause_resume_cancel_retry_e_terminais_seguem_o_manager() {
        let mut queue = DownloadQueue::new(1);
        queue.enqueue(item("a", "queued", 0));
        assert_eq!(queue.pause("a"), PauseOutcome::PausedQueued);
        assert_eq!(queue.resume("a", false), ResumeOutcome::Requeued);
        assert_eq!(queue.start_next().unwrap()["status"], "downloading");
        assert_eq!(queue.pause("a"), PauseOutcome::PausedDownloading);
        assert_eq!(queue.pause("a"), PauseOutcome::AlreadyPaused);
        assert_eq!(queue.resume("a", true), ResumeOutcome::ResumedProcess);
        assert_eq!(queue.resume("a", true), ResumeOutcome::NotPaused);

        queue.finish("a", FinishStatus::Error, "boom");
        assert!(!queue.set_priority("a", &json!(5)));
        assert_eq!(queue.pause("a"), PauseOutcome::Terminal);
        assert!(queue.retry("a"));
        assert_eq!(queue.get("a").unwrap()["status"], "queued");
        assert_eq!(queue.get("a").unwrap()["error"], "");
        assert_eq!(queue.get("a").unwrap()["integrity"]["state"], "pending");
        assert!(!queue.retry("a"));

        let cancellation = queue.cancel("a").unwrap();
        assert_eq!(cancellation.item["status"], "canceled");
        assert!(cancellation.cleanup.remove_install_dir);
        assert!(queue.get("a").is_none());
        assert_eq!(queue.cancel("a"), None);
    }

    #[test]
    fn dismiss_so_remove_terminal_nao_ativo() {
        let mut queue = DownloadQueue::new(1);
        queue.enqueue(item("a", "queued", 0));
        assert!(!queue.dismiss("a"));
        queue.start_next();
        assert!(!queue.dismiss("a"));
        queue.finish("a", FinishStatus::Error, "x");
        assert!(queue.dismiss("a"));
        assert!(!queue.dismiss("a"));
    }

    #[test]
    fn recovery_decision_move_estado_e_libera_slot() {
        let mut queue = DownloadQueue::new(1);
        queue.enqueue(item("a", "queued", 0));
        queue.start_next();

        let verify = recovery_decision(&json!({
            "phase": "download", "engine": "epic", "code": 0, "attempts": 0
        }));
        assert_eq!(
            queue.apply_recovery("a", &verify),
            Some(QueueTransition::Verify)
        );
        assert_eq!(queue.get("a").unwrap()["integrity"]["state"], "verifying");
        assert_eq!(queue.active_count(), 1);

        let done = recovery_decision(&json!({
            "phase": "verify", "engine": "epic", "code": 0, "attempts": 0
        }));
        assert_eq!(
            queue.apply_recovery("a", &done),
            Some(QueueTransition::Done)
        );
        assert_eq!(queue.get("a").unwrap()["status"], "done");
        assert_eq!(queue.active_count(), 0);

        queue.enqueue(item("b", "queued", 0));
        queue.start_next();
        let retry = recovery_decision(&json!({
            "phase": "download", "code": 1, "error": "network reset", "attempts": 0
        }));
        assert_eq!(
            queue.apply_recovery("b", &retry),
            Some(QueueTransition::Retry { delay_ms: 1500 })
        );
        assert_eq!(queue.get("b").unwrap()["status"], "queued");
        assert_eq!(queue.get("b").unwrap()["recoveryAttempts"], 1);
        assert_eq!(queue.get("b").unwrap()["integrity"]["state"], "retrying");
        assert_eq!(queue.active_count(), 0);
    }
}
