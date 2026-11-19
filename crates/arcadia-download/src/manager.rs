//! Manager que liga a fila/integridade puras ao estado persistido em
//! `downloads.json`.
//!
//! Port da parte de estado do `app/electron/downloadmanager.js`: carregar e
//! normalizar a fila, enfileirar, iniciar respeitando slots, pausar/retomar,
//! cancelar, retry, descartar, progresso, decisões de recuperação e limpeza
//! terminal. Spawn/sinais/limpeza de disco são um trait injetado; persistência
//! é outro trait (a implementação de arquivo usa `arcadia_store::atomic`).
//!
//! Campos desconhecidos são preservados no round-trip. Só chaves transitórias
//! conhecidas (pid/comandos externos da fila de depots) são omitidas na
//! gravação — comandos nunca vão para o JSON, como no oráculo.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::integrity::{
    integrity_mode, recovery_decision, CleanupPlan, RecoveryAction, DEFAULT_MAX_RECOVERY_ATTEMPTS,
};
use crate::js::{js_string, js_truthy};
use crate::queue::{
    next_queued, normalize_loaded_queue, Cancellation, DownloadQueue, EnqueueOutcome, FinishStatus,
    PauseOutcome, ResumeOutcome, STATUS_DONE, STATUS_DOWNLOADING, STATUS_PAUSED, STATUS_QUEUED,
};

/// Chaves que nunca vão para `downloads.json`: PID e a fila de comandos
/// externos preparada pelo `steamstore` (nunca reutilizar JSON adulterado).
const TRANSIENT_KEYS: &[&str] = &["pid", "fila", "filaIdx", "depotsBaixando", "depotAtual"];

// ---------------------------------------------------------------------------
// Persistência
// ---------------------------------------------------------------------------

pub trait DownloadsPersistence {
    /// `Ok(None)` = arquivo ainda não existe; `Err` = falha de leitura.
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, contents: &str) -> Result<(), String>;
}

/// Persistência em memória (testes e embedders sem disco).
#[derive(Debug, Default)]
pub struct MemoryPersistence {
    contents: Mutex<Option<String>>,
}

impl MemoryPersistence {
    pub fn new(contents: Option<String>) -> Self {
        Self {
            contents: Mutex::new(contents),
        }
    }

    pub fn contents(&self) -> Option<String> {
        self.contents.lock().ok().and_then(|text| text.clone())
    }

    pub fn set(&self, contents: Option<String>) {
        if let Ok(mut text) = self.contents.lock() {
            *text = contents;
        }
    }
}

impl DownloadsPersistence for MemoryPersistence {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.contents())
    }

    fn save(&self, contents: &str) -> Result<(), String> {
        self.set(Some(contents.to_string()));
        Ok(())
    }
}

/// `downloads.json` com escrita atômica (tmp + rename) do `arcadia-store`.
#[derive(Debug, Clone)]
pub struct FilePersistence {
    path: PathBuf,
}

impl FilePersistence {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DownloadsPersistence for FilePersistence {
    fn load(&self) -> Result<Option<String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, contents: &str) -> Result<(), String> {
        arcadia_store::atomic::write_atomic_text(&self.path, contents)
            .map_err(|error| error.to_string())
    }
}

// ---------------------------------------------------------------------------
// Executor injetado
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
    Download,
    Verify,
    Depot,
}

impl DownloadPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Verify => "verify",
            Self::Depot => "depot",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadSignal {
    Pause,
    Resume,
    Kill,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRequest {
    pub appid: String,
    pub phase: DownloadPhase,
    /// Item já atualizado (status/integridade e, em depot, `fila`/`filaIdx`).
    pub item: Value,
}

/// Spawn, sinais e limpeza de processos/staging. A borda (Tauri) implementa;
/// os testes usam um executor que só registra as chamadas.
pub trait DownloadExecutor {
    fn ensure_ready(&mut self, item: &Value) -> Result<(), String>;
    fn spawn(&mut self, request: &DownloadRequest) -> Result<(), String>;
    fn signal(&mut self, appid: &str, signal: DownloadSignal) -> Result<(), String>;
    fn cleanup(&mut self, item: &Value, plan: CleanupPlan);
}

/// Executor nulo: aceita tudo e não spawna nada.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopExecutor;

impl DownloadExecutor for NoopExecutor {
    fn ensure_ready(&mut self, _item: &Value) -> Result<(), String> {
        Ok(())
    }

    fn spawn(&mut self, _request: &DownloadRequest) -> Result<(), String> {
        Ok(())
    }

    fn signal(&mut self, _appid: &str, _signal: DownloadSignal) -> Result<(), String> {
        Ok(())
    }

    fn cleanup(&mut self, _item: &Value, _plan: CleanupPlan) {}
}

// ---------------------------------------------------------------------------
// Resultados
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    Empty,
    Loaded(usize),
    /// JSON inválido/não-array: a fila em memória fica vazia, como no load().
    Invalid(String),
    /// Falha de I/O: a fila atual é preservada.
    LoadFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Added { replaced: usize },
    AlreadyQueued,
    Invalid,
    EngineUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    NoQueued,
    NoSlot,
    Started {
        appid: String,
        phase: DownloadPhase,
    },
    SpawnFailed {
        appid: String,
        error: String,
        transition: ManagerTransition,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerTransition {
    Stopped,
    Verify,
    NextDepot {
        depot_id: String,
    },
    Done {
        appid: String,
    },
    Retry {
        delay_ms: i64,
    },
    Error {
        error: String,
    },
    SpawnFailed {
        phase: DownloadPhase,
        error: String,
        follow_up: Box<ManagerTransition>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExit {
    pub phase: DownloadPhase,
    /// `None` = spawn error/close sem código; vira `null` no recovery input
    /// (`Number(null) === 0`, mesma semântica do oráculo).
    pub code: Option<i32>,
    pub signal: Option<String>,
    pub error: Option<String>,
    /// `verificationOutputLooksFailed` já avaliado pela borda.
    pub output_failed: bool,
    pub verified_at: Option<String>,
}

impl Default for ProcessExit {
    fn default() -> Self {
        Self {
            phase: DownloadPhase::Download,
            code: None,
            signal: None,
            error: None,
            output_failed: false,
            verified_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct DownloadManager<P: DownloadsPersistence, E: DownloadExecutor> {
    queue: DownloadQueue,
    persistence: P,
    executor: E,
    last_persist_error: Option<String>,
}

impl<P: DownloadsPersistence, E: DownloadExecutor> DownloadManager<P, E> {
    pub fn new(persistence: P, executor: E, max_active: usize) -> Self {
        Self {
            queue: DownloadQueue::new(max_active),
            persistence,
            executor,
            last_persist_error: None,
        }
    }

    /// Cria o manager e carrega `downloads.json` uma vez.
    pub fn open(persistence: P, executor: E, max_active: usize) -> Self {
        let mut manager = Self::new(persistence, executor, max_active);
        manager.reload();
        manager
    }

    pub fn queue(&self) -> &DownloadQueue {
        &self.queue
    }

    pub fn items(&self) -> &[Value] {
        self.queue.items()
    }

    pub fn executor(&self) -> &E {
        &self.executor
    }

    pub fn persistence(&self) -> &P {
        &self.persistence
    }

    pub fn last_persist_error(&self) -> Option<&str> {
        self.last_persist_error.as_deref()
    }

    pub fn active_count(&self) -> usize {
        self.queue.active_count()
    }

    pub fn has_free_slot(&self) -> bool {
        self.queue.has_free_slot()
    }

    pub fn set_max_active(&mut self, max_active: usize) {
        self.queue.set_max_active(max_active);
    }

    /// `load()`: normaliza prioridade/tentativas/integridade, transforma
    /// `downloading` em `paused` (o processo morreu com o app) e descarta
    /// `done`. JSON inválido vira fila vazia; falha de I/O preserva a atual.
    pub fn reload(&mut self) -> LoadOutcome {
        match self.persistence.load() {
            Ok(None) => {
                self.queue = DownloadQueue::new(self.queue.max_active());
                LoadOutcome::Empty
            }
            Ok(Some(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Array(items)) => {
                    let items = normalize_loaded_queue(items);
                    let count = items.len();
                    self.queue = DownloadQueue::from_items(items, self.queue.max_active());
                    LoadOutcome::Loaded(count)
                }
                Ok(_) => {
                    self.queue = DownloadQueue::new(self.queue.max_active());
                    LoadOutcome::Invalid("downloads.json não é uma lista".to_string())
                }
                Err(error) => {
                    self.queue = DownloadQueue::new(self.queue.max_active());
                    LoadOutcome::Invalid(error.to_string())
                }
            },
            Err(error) => LoadOutcome::LoadFailed(error),
        }
    }

    /// Persiste agora (fora das mutações automáticas).
    pub fn persist_now(&mut self) {
        self.persist();
    }

    /// `install`: valida, garante o runtime e enfileira. Duplicata ativa é
    /// ignorada; item terminal do mesmo appid é substituído.
    pub fn add(&mut self, item: Value) -> InstallOutcome {
        let Some(appid) = item
            .get("appid")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|appid| !appid.is_empty())
            .map(str::to_string)
        else {
            return InstallOutcome::Invalid;
        };
        if self.queue.get(&appid).is_some_and(is_active) {
            return InstallOutcome::AlreadyQueued;
        }
        if let Err(error) = self.executor.ensure_ready(&item) {
            return InstallOutcome::EngineUnavailable(error);
        }
        match self.queue.enqueue(item) {
            EnqueueOutcome::Added { replaced } => {
                self.persist();
                InstallOutcome::Added { replaced }
            }
            EnqueueOutcome::AlreadyQueued => InstallOutcome::AlreadyQueued,
            EnqueueOutcome::Invalid => InstallOutcome::Invalid,
        }
    }

    /// `next()`: ocupa um slot, marca `downloading` e pede o spawn. O spawn
    /// acontece depois do persist, como no manager Electron.
    pub fn start_next(&mut self) -> StartOutcome {
        if !self.queue.has_free_slot() {
            return StartOutcome::NoSlot;
        }
        if next_queued(self.queue.items()).is_none() {
            return StartOutcome::NoQueued;
        }
        let Some(item) = self.queue.start_next() else {
            return StartOutcome::NoQueued;
        };
        let appid = item
            .get("appid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.persist();
        let request = DownloadRequest {
            appid: appid.clone(),
            phase: DownloadPhase::Download,
            item,
        };
        match self.executor.spawn(&request) {
            Ok(()) => StartOutcome::Started {
                appid,
                phase: DownloadPhase::Download,
            },
            Err(error) => {
                let transition = self
                    .process_exit(
                        &appid,
                        ProcessExit {
                            phase: DownloadPhase::Download,
                            error: Some(error.clone()),
                            ..ProcessExit::default()
                        },
                    )
                    .unwrap_or_else(|| ManagerTransition::Error {
                        error: error.clone(),
                    });
                StartOutcome::SpawnFailed {
                    appid,
                    error,
                    transition,
                }
            }
        }
    }

    pub fn pause(&mut self, appid: &str) -> PauseOutcome {
        let outcome = self.queue.pause(appid);
        if outcome == PauseOutcome::PausedDownloading {
            let _ = self.executor.signal(appid, DownloadSignal::Pause);
        }
        if outcome != PauseOutcome::NotFound {
            self.persist();
        }
        outcome
    }

    /// `resume`: com processo vivo sinaliza; sem processo vivo volta à fila e
    /// o chamador decide quando chamar `start_next`.
    pub fn resume(&mut self, appid: &str, live_process: bool) -> ResumeOutcome {
        let outcome = self.queue.resume(appid, live_process);
        if outcome == ResumeOutcome::ResumedProcess {
            let _ = self.executor.signal(appid, DownloadSignal::Resume);
        }
        if outcome != ResumeOutcome::NotFound {
            self.persist();
        }
        outcome
    }

    /// `cancel`: remove o item, mata o grupo e deixa a limpeza (parciais +
    /// instalação) para o executor, com o plano do módulo de integridade.
    pub fn cancel(&mut self, appid: &str) -> Option<Cancellation> {
        let cancellation = self.queue.cancel(appid)?;
        let _ = self.executor.signal(appid, DownloadSignal::Kill);
        self.executor
            .cleanup(&cancellation.item, cancellation.cleanup);
        self.persist();
        Some(cancellation)
    }

    pub fn retry(&mut self, appid: &str) -> bool {
        let changed = self.queue.retry(appid);
        if changed {
            self.persist();
        }
        changed
    }

    pub fn dismiss(&mut self, appid: &str) -> bool {
        let changed = self.queue.dismiss(appid);
        if changed {
            self.persist();
        }
        changed
    }

    pub fn set_priority(&mut self, appid: &str, priority: &Value) -> bool {
        let changed = self.queue.set_priority(appid, priority);
        if changed {
            self.persist();
        }
        changed
    }

    /// `update(appid, patch)` / `Object.assign`: progresso, installDir, etc.
    pub fn update(&mut self, appid: &str, patch: &Value) -> bool {
        let changed = self.queue.update(appid, patch);
        if changed {
            self.persist();
        }
        changed
    }

    pub fn update_progress(&mut self, appid: &str, patch: &Value) -> bool {
        self.update(appid, patch)
    }

    /// Fecha o ciclo de um processo e aplica `recoveryDecision`. Retorna a
    /// transição para a borda (agendar retry, spawnar verify/próximo depot,
    /// notificar done, etc.).
    pub fn process_exit(&mut self, appid: &str, exit: ProcessExit) -> Option<ManagerTransition> {
        let item = self.queue.get(appid)?.clone();
        if exit.phase == DownloadPhase::Depot {
            return self.process_depot_exit(appid, &item, &exit);
        }
        let decision = recovery_decision(&recovery_input(&item, &exit));
        match decision.action {
            RecoveryAction::Stopped => {
                self.persist();
                Some(ManagerTransition::Stopped)
            }
            RecoveryAction::Verify => {
                self.queue.apply_recovery(appid, &decision);
                self.persist();
                let request_item = self.queue.get(appid)?.clone();
                match self.executor.spawn(&DownloadRequest {
                    appid: appid.to_string(),
                    phase: DownloadPhase::Verify,
                    item: request_item,
                }) {
                    Ok(()) => Some(ManagerTransition::Verify),
                    Err(error) => Some(self.spawn_failed(appid, DownloadPhase::Verify, error)),
                }
            }
            RecoveryAction::Done => {
                self.mark_verified(appid, &exit.verified_at);
                self.queue.finish(appid, FinishStatus::Done, "");
                self.persist();
                Some(ManagerTransition::Done {
                    appid: appid.to_string(),
                })
            }
            RecoveryAction::Retry => {
                self.queue.apply_recovery(appid, &decision);
                self.persist();
                Some(ManagerTransition::Retry {
                    delay_ms: decision.delay_ms.unwrap_or(0),
                })
            }
            RecoveryAction::Error => {
                self.queue.apply_recovery(appid, &decision);
                self.persist();
                Some(ManagerTransition::Error {
                    error: decision.error.unwrap_or_default(),
                })
            }
        }
    }

    /// Um sucesso `done` sai da lista após a notificação (o Electron espera
    /// 6s; o timer fica na borda). Devolve os appids removidos.
    pub fn cleanup_done(&mut self) -> Vec<String> {
        let done: Vec<String> = self
            .queue
            .items()
            .iter()
            .filter(|item| item_status(item) == Some(STATUS_DONE))
            .filter_map(item_appid)
            .collect();
        for appid in &done {
            self.queue.remove(appid);
        }
        if !done.is_empty() {
            self.persist();
        }
        done
    }

    /// `killActive` do shutdown: marca o ativo como pausado ANTES de matar,
    /// para o close não agendar retry/ressurreição.
    pub fn shutdown_active(&mut self) -> Vec<String> {
        let active: Vec<String> = self
            .queue
            .items()
            .iter()
            .filter(|item| item_status(item) == Some(STATUS_DOWNLOADING))
            .filter_map(item_appid)
            .collect();
        for appid in &active {
            let method = self
                .queue
                .get(appid)
                .map(integrity_mode)
                .unwrap_or(crate::integrity::IntegrityMode::LegendaryVerify)
                .as_str()
                .to_string();
            let integrity = integrity_value(
                self.queue.get(appid).unwrap_or(&Value::Null),
                "paused",
                &method,
                None,
            );
            self.queue.update(
                appid,
                &json!({"status": STATUS_PAUSED, "integrity": integrity}),
            );
            let _ = self.executor.signal(appid, DownloadSignal::Kill);
        }
        if !active.is_empty() {
            self.persist();
        }
        active
    }

    // -- internos -----------------------------------------------------------

    fn process_depot_exit(
        &mut self,
        appid: &str,
        item: &Value,
        exit: &ProcessExit,
    ) -> Option<ManagerTransition> {
        let decision = recovery_decision(&recovery_input(item, exit));
        if decision.action != RecoveryAction::Done {
            let index = item
                .get("filaIdx")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0) as usize;
            let depot_id = item
                .get("fila")
                .and_then(Value::as_array)
                .and_then(|queue| queue.get(index))
                .and_then(|depot| depot.get("depotId"))
                .map(string_or_empty)
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| "?".to_string());
            let mut failures = item
                .get("depotsFalhos")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            failures.push(Value::String(depot_id));
            self.queue.update(
                appid,
                &json!({
                    "depotsFalhos": failures,
                    "fila": null,
                    "filaIdx": 0,
                    "depotAtualId": "",
                    "depotsOk": 0,
                }),
            );
            return match decision.action {
                RecoveryAction::Stopped => {
                    self.persist();
                    Some(ManagerTransition::Stopped)
                }
                RecoveryAction::Retry => {
                    self.queue.apply_recovery(appid, &decision);
                    self.persist();
                    Some(ManagerTransition::Retry {
                        delay_ms: decision.delay_ms.unwrap_or(0),
                    })
                }
                RecoveryAction::Error => {
                    self.queue.apply_recovery(appid, &decision);
                    self.persist();
                    Some(ManagerTransition::Error {
                        error: decision.error.unwrap_or_default(),
                    })
                }
                _ => {
                    self.persist();
                    Some(ManagerTransition::Error {
                        error: "transição inesperada de depot".to_string(),
                    })
                }
            };
        }

        let fila = item
            .get("fila")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let index = item
            .get("filaIdx")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let depots_ok = item
            .get("depotsOk")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .saturating_add(1);
        if index + 1 < fila.len() {
            let next = &fila[index + 1];
            let next_id = next.get("depotId").map(string_or_empty).unwrap_or_default();
            let method = integrity_mode(item).as_str().to_string();
            let mut integrity = integrity_value(item, "downloading", &method, None);
            if let Some(object) = integrity.as_object_mut() {
                object.insert("method".to_string(), Value::String(method));
            }
            self.queue.update(
                appid,
                &json!({
                    "depotsOk": depots_ok,
                    "filaIdx": (index + 1) as i64,
                    "depotAtualId": next_id,
                    "depotAtual": (index + 2) as i64,
                    "depotsTotal": fila.len() as i64,
                    "integrity": integrity,
                }),
            );
            self.persist();
            let request_item = self.queue.get(appid)?.clone();
            match self.executor.spawn(&DownloadRequest {
                appid: appid.to_string(),
                phase: DownloadPhase::Depot,
                item: request_item,
            }) {
                Ok(()) => Some(ManagerTransition::NextDepot { depot_id: next_id }),
                Err(error) => Some(self.spawn_failed(appid, DownloadPhase::Depot, error)),
            }
        } else {
            self.queue.update(appid, &json!({"depotsOk": depots_ok}));
            self.queue.finish(appid, FinishStatus::Done, "");
            let integrity = integrity_value(
                self.queue.get(appid).unwrap_or(&Value::Null),
                "verified",
                "depot-manifest",
                exit.verified_at.as_deref(),
            );
            self.queue.update(appid, &json!({"integrity": integrity}));
            self.persist();
            Some(ManagerTransition::Done {
                appid: appid.to_string(),
            })
        }
    }

    fn mark_verified(&mut self, appid: &str, verified_at: &Option<String>) {
        let method = self
            .queue
            .get(appid)
            .map(integrity_mode)
            .unwrap_or(crate::integrity::IntegrityMode::LegendaryVerify)
            .as_str()
            .to_string();
        let integrity = integrity_value(
            self.queue.get(appid).unwrap_or(&Value::Null),
            "verified",
            &method,
            verified_at.as_deref(),
        );
        self.queue.update(appid, &json!({"integrity": integrity}));
    }

    fn spawn_failed(
        &mut self,
        appid: &str,
        phase: DownloadPhase,
        error: String,
    ) -> ManagerTransition {
        let follow_up = self
            .process_exit(
                appid,
                ProcessExit {
                    phase,
                    error: Some(error.clone()),
                    ..ProcessExit::default()
                },
            )
            .unwrap_or_else(|| ManagerTransition::Error {
                error: error.clone(),
            });
        ManagerTransition::SpawnFailed {
            phase,
            error,
            follow_up: Box::new(follow_up),
        }
    }

    fn persist(&mut self) {
        let snapshot = persistable_items(self.queue.items());
        match serde_json::to_string_pretty(&snapshot) {
            Ok(text) => match self.persistence.save(&text) {
                Ok(()) => self.last_persist_error = None,
                Err(error) => self.last_persist_error = Some(error),
            },
            Err(error) => self.last_persist_error = Some(error.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers puros (também usados nos testes)
// ---------------------------------------------------------------------------

fn item_status(item: &Value) -> Option<&str> {
    item.get("status").and_then(Value::as_str)
}

fn item_appid(item: &Value) -> Option<String> {
    item.get("appid")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn is_active(item: &Value) -> bool {
    matches!(
        item_status(item),
        Some(STATUS_QUEUED | STATUS_DOWNLOADING | STATUS_PAUSED)
    )
}

fn string_or_empty(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        text.to_string()
    } else if js_truthy(value) {
        js_string(value)
    } else {
        String::new()
    }
}

fn integrity_value(item: &Value, state: &str, method: &str, verified_at: Option<&str>) -> Value {
    let mut map = item
        .get("integrity")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    map.insert("state".to_string(), Value::String(state.to_string()));
    map.insert("method".to_string(), Value::String(method.to_string()));
    if let Some(verified_at) = verified_at {
        map.insert(
            "verifiedAt".to_string(),
            Value::String(verified_at.to_string()),
        );
    }
    Value::Object(map)
}

fn recovery_input(item: &Value, exit: &ProcessExit) -> Value {
    let mut input = Map::new();
    input.insert(
        "phase".to_string(),
        Value::String(exit.phase.as_str().to_string()),
    );
    input.insert(
        "engine".to_string(),
        Value::String(
            item.get("engine")
                .and_then(Value::as_str)
                .unwrap_or("epic")
                .to_string(),
        ),
    );
    input.insert(
        "code".to_string(),
        match exit.code {
            Some(code) => Value::from(code),
            None => Value::Null,
        },
    );
    if exit.output_failed {
        input.insert("code".to_string(), Value::from(1));
    }
    if let Some(signal) = &exit.signal {
        input.insert("signal".to_string(), Value::String(signal.clone()));
    }
    input.insert(
        "status".to_string(),
        Value::String(item_status(item).unwrap_or("downloading").to_string()),
    );
    input.insert(
        "attempts".to_string(),
        Value::from(
            item.get("recoveryAttempts")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        ),
    );
    input.insert(
        "maxAttempts".to_string(),
        Value::from(DEFAULT_MAX_RECOVERY_ATTEMPTS),
    );
    if let Some(error) = &exit.error {
        input.insert("error".to_string(), Value::String(error.clone()));
    }
    Value::Object(input)
}

fn persistable_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if let Some(object) = item.as_object_mut() {
                for key in TRANSIENT_KEYS {
                    object.remove(*key);
                }
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::CleanupReason;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug, Default)]
    struct FakeExecutor {
        ready: bool,
        fail_spawn: bool,
        spawns: Vec<DownloadRequest>,
        signals: Vec<(String, DownloadSignal)>,
        cleanups: Vec<(String, CleanupPlan)>,
    }

    impl FakeExecutor {
        fn ready() -> Self {
            Self {
                ready: true,
                ..Self::default()
            }
        }
    }

    impl DownloadExecutor for FakeExecutor {
        fn ensure_ready(&mut self, _item: &Value) -> Result<(), String> {
            if self.ready {
                Ok(())
            } else {
                Err("engine offline".to_string())
            }
        }

        fn spawn(&mut self, request: &DownloadRequest) -> Result<(), String> {
            if self.fail_spawn {
                return Err("spawn boom".to_string());
            }
            self.spawns.push(request.clone());
            Ok(())
        }

        fn signal(&mut self, appid: &str, signal: DownloadSignal) -> Result<(), String> {
            self.signals.push((appid.to_string(), signal));
            Ok(())
        }

        fn cleanup(&mut self, item: &Value, plan: CleanupPlan) {
            self.cleanups
                .push((item_appid(item).unwrap_or_default(), plan));
        }
    }

    #[derive(Debug, Default)]
    struct FlakyStore {
        inner: Mutex<FlakyInner>,
    }

    #[derive(Debug, Default)]
    struct FlakyInner {
        contents: Option<String>,
        fail_load: bool,
        fail_save: bool,
    }

    impl FlakyStore {
        fn new(contents: Option<String>) -> Self {
            Self {
                inner: Mutex::new(FlakyInner {
                    contents,
                    fail_load: false,
                    fail_save: false,
                }),
            }
        }

        fn set_fail_load(&self, value: bool) {
            self.inner.lock().unwrap().fail_load = value;
        }

        fn set_fail_save(&self, value: bool) {
            self.inner.lock().unwrap().fail_save = value;
        }

        fn contents(&self) -> Option<String> {
            self.inner.lock().unwrap().contents.clone()
        }
    }

    impl DownloadsPersistence for FlakyStore {
        fn load(&self) -> Result<Option<String>, String> {
            let inner = self.inner.lock().unwrap();
            if inner.fail_load {
                Err("load boom".to_string())
            } else {
                Ok(inner.contents.clone())
            }
        }

        fn save(&self, contents: &str) -> Result<(), String> {
            let mut inner = self.inner.lock().unwrap();
            if inner.fail_save {
                Err("save boom".to_string())
            } else {
                inner.contents = Some(contents.to_string());
                Ok(())
            }
        }
    }

    fn item(appid: &str, status: &str) -> Value {
        json!({ "appid": appid, "appName": appid, "status": status, "priority": 0 })
    }

    fn manager(items: Vec<Value>) -> DownloadManager<MemoryPersistence, FakeExecutor> {
        DownloadManager::open(
            MemoryPersistence::new(Some(Value::Array(items).to_string())),
            FakeExecutor::ready(),
            1,
        )
    }

    fn persisted(manager: &DownloadManager<MemoryPersistence, FakeExecutor>) -> Vec<Value> {
        let text = manager.persistence().contents().unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn load_normaliza_sem_perder_campos_desconhecidos() {
        let manager = manager(vec![
            json!({
                "appid": "a",
                "status": "downloading",
                "priority": 99,
                "custom": {"x": 1},
                "integrity": {"state": "verifying"},
            }),
            json!({"appid": "b", "status": "done"}),
            json!({"appid": "c", "status": "queued"}),
        ]);
        assert_eq!(manager.items().len(), 2);
        let first = &manager.items()[0];
        assert_eq!(first["status"], "paused");
        assert_eq!(first["priority"], 10);
        assert_eq!(first["custom"]["x"], 1);
        assert_eq!(first["integrity"]["state"], "paused");
        assert_eq!(first["integrity"]["method"], "legendary-verify");
        assert_eq!(manager.items()[1]["integrity"]["state"], "pending");
    }

    #[test]
    fn load_json_invalido_zera_mas_falha_de_io_preserva() {
        let mut invalid = DownloadManager::open(
            MemoryPersistence::new(Some("{ não é json".to_string())),
            FakeExecutor::ready(),
            1,
        );
        assert!(invalid.items().is_empty());
        assert!(matches!(invalid.reload(), LoadOutcome::Invalid(_)));

        let store = FlakyStore::new(Some(json!([item("a", "queued")]).to_string()));
        let mut manager = DownloadManager::open(store, FakeExecutor::ready(), 1);
        assert_eq!(manager.items().len(), 1);
        manager.persistence().set_fail_load(true);
        assert_eq!(
            manager.reload(),
            LoadOutcome::LoadFailed("load boom".to_string())
        );
        assert_eq!(manager.items().len(), 1);
    }

    #[test]
    fn add_valida_duplicata_substitui_terminal_e_exige_runtime() {
        let mut manager = manager(vec![]);
        assert_eq!(manager.add(json!(null)), InstallOutcome::Invalid);
        assert_eq!(manager.add(json!({"appid": "  "})), InstallOutcome::Invalid);
        assert_eq!(
            manager.add(item("a", "queued")),
            InstallOutcome::Added { replaced: 0 }
        );
        assert_eq!(
            manager.add(item("a", "queued")),
            InstallOutcome::AlreadyQueued
        );

        manager.process_exit(
            "a",
            ProcessExit {
                phase: DownloadPhase::Download,
                code: Some(1),
                error: Some("boom".to_string()),
                ..ProcessExit::default()
            },
        );
        // Ainda error após 2 retries? Aqui houve 1 retry; força terminal.
        manager.process_exit(
            "a",
            ProcessExit {
                phase: DownloadPhase::Download,
                code: Some(1),
                error: Some("boom".to_string()),
                ..ProcessExit::default()
            },
        );
        manager.process_exit(
            "a",
            ProcessExit {
                phase: DownloadPhase::Download,
                code: Some(1),
                error: Some("boom".to_string()),
                ..ProcessExit::default()
            },
        );
        assert_eq!(manager.items()[0]["status"], "error");
        assert_eq!(
            manager.add(item("a", "queued")),
            InstallOutcome::Added { replaced: 1 }
        );

        let mut offline =
            DownloadManager::open(MemoryPersistence::default(), FakeExecutor::default(), 1);
        assert_eq!(
            offline.add(item("b", "queued")),
            InstallOutcome::EngineUnavailable("engine offline".to_string())
        );
        assert!(offline.items().is_empty());
    }

    #[test]
    fn start_respeita_slots_e_spawna_com_item_atualizado() {
        let mut manager = manager(vec![item("a", "queued"), item("b", "queued")]);
        assert_eq!(
            manager.start_next(),
            StartOutcome::Started {
                appid: "a".to_string(),
                phase: DownloadPhase::Download,
            }
        );
        assert_eq!(manager.start_next(), StartOutcome::NoSlot);
        assert_eq!(manager.active_count(), 1);
        assert_eq!(manager.executor().spawns.len(), 1);
        assert_eq!(manager.executor().spawns[0].item["status"], "downloading");
        assert_eq!(
            manager.executor().spawns[0].item["integrity"]["state"],
            "downloading"
        );

        manager.pause("a");
        assert_eq!(manager.active_count(), 0);
        assert_eq!(
            manager.start_next(),
            StartOutcome::Started {
                appid: "b".to_string(),
                phase: DownloadPhase::Download,
            }
        );
        manager.cancel("b");
        assert_eq!(manager.start_next(), StartOutcome::NoQueued);
    }

    #[test]
    fn prioridade_define_o_proximo_e_fifo_desempata() {
        let mut manager = manager(vec![
            json!({"appid": "a", "status": "queued", "priority": 0}),
            json!({"appid": "b", "status": "queued", "priority": 0}),
        ]);
        assert!(manager.set_priority("b", &json!(5)));
        assert_eq!(
            manager.start_next(),
            StartOutcome::Started {
                appid: "b".to_string(),
                phase: DownloadPhase::Download,
            }
        );
        assert!(!manager.set_priority("missing", &json!(1)));
    }

    #[test]
    fn pause_resume_e_cancel_sinalizam_o_executor() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.start_next();
        assert_eq!(manager.pause("a"), PauseOutcome::PausedDownloading);
        assert_eq!(
            manager.executor().signals.last(),
            Some(&("a".to_string(), DownloadSignal::Pause))
        );
        assert_eq!(manager.active_count(), 0);

        assert_eq!(manager.resume("a", true), ResumeOutcome::ResumedProcess);
        assert_eq!(
            manager.executor().signals.last(),
            Some(&("a".to_string(), DownloadSignal::Resume))
        );
        assert_eq!(manager.items()[0]["status"], "downloading");

        assert_eq!(manager.pause("a"), PauseOutcome::PausedDownloading);
        assert_eq!(manager.resume("a", false), ResumeOutcome::Requeued);
        assert_eq!(manager.items()[0]["status"], "queued");

        manager.start_next();
        let cancellation = manager.cancel("a").unwrap();
        assert_eq!(cancellation.item["status"], "canceled");
        assert!(cancellation.cleanup.remove_install_dir);
        assert_eq!(
            manager.executor().signals.last(),
            Some(&("a".to_string(), DownloadSignal::Kill))
        );
        assert_eq!(
            manager.executor().cleanups.last(),
            Some(&(
                "a".to_string(),
                crate::integrity::cleanup_plan(CleanupReason::Canceled)
            ))
        );
        assert!(manager.items().is_empty());
    }

    #[test]
    fn auto_verify_e_done_do_epic_seguem_a_integridade() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.start_next();
        assert_eq!(
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Download,
                    code: Some(0),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Verify)
        );
        assert_eq!(manager.items()[0]["status"], "downloading");
        assert_eq!(manager.items()[0]["integrity"]["state"], "verifying");
        assert_eq!(manager.executor().spawns.len(), 2);
        assert_eq!(manager.executor().spawns[1].phase, DownloadPhase::Verify);

        assert_eq!(
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Verify,
                    code: Some(0),
                    verified_at: Some("2026-11-09T12:00:00.000Z".to_string()),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Done {
                appid: "a".to_string()
            })
        );
        let done = &manager.items()[0];
        assert_eq!(done["status"], "done");
        assert_eq!(done["integrity"]["state"], "verified");
        assert_eq!(done["integrity"]["method"], "legendary-verify");
        assert_eq!(done["integrity"]["verifiedAt"], "2026-11-09T12:00:00.000Z");

        assert_eq!(manager.cleanup_done(), vec!["a".to_string()]);
        assert!(manager.items().is_empty());
        assert!(persisted(&manager).is_empty());
    }

    #[test]
    fn falha_recupera_com_delay_e_depois_vira_erro_preservando_item() {
        let mut manager = manager(vec![json!({
            "appid": "a",
            "appName": "a",
            "status": "queued",
            "custom": {"keep": true}
        })]);
        manager.start_next();
        assert_eq!(
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Download,
                    code: Some(1),
                    error: Some("network reset".to_string()),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Retry { delay_ms: 1500 })
        );
        assert_eq!(manager.items()[0]["status"], "queued");
        assert_eq!(manager.items()[0]["recoveryAttempts"], 1);
        assert_eq!(manager.items()[0]["custom"]["keep"], true);
        assert!(manager.items()[0]["error"]
            .as_str()
            .unwrap()
            .contains("nova tentativa 1/2"));

        manager.start_next();
        manager.process_exit(
            "a",
            ProcessExit {
                phase: DownloadPhase::Download,
                code: Some(1),
                error: Some("network reset".to_string()),
                ..ProcessExit::default()
            },
        );
        manager.start_next();
        assert_eq!(
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Download,
                    code: Some(1),
                    error: Some("network reset".to_string()),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Error {
                error: "network reset".to_string()
            })
        );
        assert_eq!(manager.items()[0]["status"], "error");
        assert_eq!(manager.items()[0]["error"], "network reset");
        assert_eq!(manager.items()[0]["integrity"]["state"], "failed");
        assert_eq!(manager.active_count(), 0);
        assert!(manager.has_free_slot());
    }

    #[test]
    fn pausa_cancelamento_nunca_viram_retry() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.start_next();
        manager.pause("a");
        assert_eq!(
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Download,
                    code: Some(137),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Stopped)
        );
        assert_eq!(manager.items()[0]["status"], "paused");
        assert_eq!(manager.items()[0]["recoveryAttempts"], 0);
    }

    #[test]
    fn retry_e_dismiss_seguem_os_estados_terminais() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.start_next();
        for _ in 0..3 {
            manager.process_exit(
                "a",
                ProcessExit {
                    phase: DownloadPhase::Download,
                    code: Some(1),
                    error: Some("boom".to_string()),
                    ..ProcessExit::default()
                },
            );
            manager.start_next();
        }
        assert_eq!(manager.items()[0]["status"], "error");
        assert!(!manager.dismiss("missing"));
        assert!(!manager.retry("missing"));
        assert!(manager.retry("a"));
        assert_eq!(manager.items()[0]["status"], "queued");
        assert_eq!(manager.items()[0]["error"], "");
        assert_eq!(manager.items()[0]["integrity"]["state"], "pending");

        manager.process_exit(
            "a",
            ProcessExit {
                phase: DownloadPhase::Download,
                code: Some(1),
                ..ProcessExit::default()
            },
        );
        assert_eq!(manager.items()[0]["status"], "queued");
        assert!(!manager.dismiss("a"));
    }

    #[test]
    fn progresso_merge_preserva_desconhecidos_e_omite_comandos_no_json() {
        let mut manager = manager(vec![json!({
            "appid": "a",
            "status": "queued",
            "custom": {"nested": [1, 2]},
            "fila": [{"depotId": "1", "cmd": "dotnet", "args": []}],
            "filaIdx": 0,
            "pid": 123,
        })]);
        assert!(manager.update_progress(
            "a",
            &json!({"percent": 42.5, "done": 10, "speed": 2.0, "eta": "00:10"})
        ));
        assert_eq!(manager.items()[0]["percent"], 42.5);
        assert_eq!(manager.items()[0]["custom"]["nested"], json!([1, 2]));
        assert!(!manager.update("a", &json!(null)));
        assert!(!manager.update("missing", &json!({"percent": 1})));

        let saved = persisted(&manager);
        assert_eq!(saved[0]["custom"]["nested"], json!([1, 2]));
        assert_eq!(saved[0]["percent"], 42.5);
        assert!(saved[0].get("fila").is_none());
        assert!(saved[0].get("filaIdx").is_none());
        assert!(saved[0].get("pid").is_none());
    }

    #[test]
    fn estado_persistido_faz_round_trip_com_campos_desconhecidos() {
        let store = MemoryPersistence::default();
        let mut manager = DownloadManager::open(store, FakeExecutor::ready(), 1);
        manager.add(json!({
            "appid": "a",
            "appName": "Jogo",
            "status": "queued",
            "custom": {"user": "zes"},
            "fila": [{"depotId": "1"}],
        }));
        manager.start_next();
        manager.pause("a");
        let text = manager.persistence().contents().unwrap();
        assert!(text.contains("\"user\": \"zes\""));

        let mut reloaded =
            DownloadManager::open(MemoryPersistence::new(Some(text)), FakeExecutor::ready(), 1);
        assert_eq!(reloaded.items().len(), 1);
        assert_eq!(reloaded.items()[0]["status"], "paused");
        assert_eq!(reloaded.items()[0]["custom"]["user"], "zes");
        assert!(reloaded.items()[0].get("fila").is_none());
        assert!(!reloaded.retry("a"));
        assert!(!reloaded.dismiss("a"));
        let cancellation = reloaded.cancel("a").unwrap();
        assert!(cancellation.cleanup.remove_install_dir);
    }

    #[test]
    fn falha_de_persistencia_nao_perde_estado_e_e_reportada() {
        let store = Arc::new(FlakyStore::new(None));
        let mut manager =
            DownloadManager::open(SharedStore(Arc::clone(&store)), FakeExecutor::ready(), 1);
        store.set_fail_save(true);
        assert_eq!(
            manager.add(item("a", "queued")),
            InstallOutcome::Added { replaced: 0 }
        );
        assert_eq!(manager.last_persist_error(), Some("save boom"));
        assert_eq!(manager.items().len(), 1);

        store.set_fail_save(false);
        assert!(manager.update("a", &json!({"percent": 1})));
        assert_eq!(manager.last_persist_error(), None);
        assert!(store.contents().unwrap().contains("\"percent\": 1"));
    }

    #[test]
    fn depot_avanca_em_sequencia_e_so_conclui_no_ultimo() {
        let mut manager = manager(vec![json!({
            "appid": "steam:440",
            "appName": "440",
            "engine": "steam",
            "status": "queued",
            "fila": [
                {"depotId": "111", "cmd": "dotnet", "args": ["-app", "440"]},
                {"depotId": "222", "cmd": "dotnet", "args": ["-app", "440"]}
            ],
            "filaIdx": 0,
        })]);
        manager.start_next();
        assert_eq!(
            manager.process_exit(
                "steam:440",
                ProcessExit {
                    phase: DownloadPhase::Depot,
                    code: Some(0),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::NextDepot {
                depot_id: "222".to_string()
            })
        );
        let item = &manager.items()[0];
        assert_eq!(item["depotsOk"], 1);
        assert_eq!(item["filaIdx"], 1);
        assert_eq!(item["depotAtualId"], "222");
        assert_eq!(item["depotAtual"], 2);
        assert_eq!(item["depotsTotal"], 2);
        assert_eq!(item["integrity"]["state"], "downloading");
        assert_eq!(manager.executor().spawns[1].phase, DownloadPhase::Depot);

        assert_eq!(
            manager.process_exit(
                "steam:440",
                ProcessExit {
                    phase: DownloadPhase::Depot,
                    code: Some(0),
                    verified_at: Some("2026-11-09T00:00:00Z".to_string()),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Done {
                appid: "steam:440".to_string()
            })
        );
        let item = &manager.items()[0];
        assert_eq!(item["status"], "done");
        assert_eq!(item["depotsOk"], 2);
        assert_eq!(item["integrity"]["state"], "verified");
        assert_eq!(item["integrity"]["method"], "depot-manifest");
        assert_eq!(item["integrity"]["verifiedAt"], "2026-11-09T00:00:00Z");
    }

    #[test]
    fn falha_de_depot_marca_falho_e_descarta_comandos() {
        let mut manager = manager(vec![json!({
            "appid": "steam:440",
            "appName": "440",
            "engine": "steam",
            "status": "queued",
            "fila": [
                {"depotId": "111"},
                {"depotId": "222"}
            ],
            "filaIdx": 0,
            "depotsFalhos": []
        })]);
        manager.start_next();
        assert_eq!(
            manager.process_exit(
                "steam:440",
                ProcessExit {
                    phase: DownloadPhase::Depot,
                    code: Some(1),
                    error: Some("401".to_string()),
                    ..ProcessExit::default()
                }
            ),
            Some(ManagerTransition::Retry { delay_ms: 1500 })
        );
        let item = &manager.items()[0];
        assert_eq!(item["status"], "queued");
        assert_eq!(item["depotsFalhos"], json!(["111"]));
        assert_eq!(item["fila"], Value::Null);
        assert_eq!(item["filaIdx"], 0);
        assert_eq!(item["depotAtualId"], "");
        assert_eq!(item["depotsOk"], 0);
        assert!(persisted(&manager)[0].get("fila").is_none());
    }

    #[test]
    fn shutdown_marca_pausado_antes_de_matar() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.start_next();
        assert_eq!(manager.shutdown_active(), vec!["a".to_string()]);
        assert_eq!(manager.items()[0]["status"], "paused");
        assert_eq!(manager.items()[0]["integrity"]["state"], "paused");
        assert_eq!(
            manager.executor().signals.last(),
            Some(&("a".to_string(), DownloadSignal::Kill))
        );
        assert!(manager.shutdown_active().is_empty());
    }

    #[test]
    fn spawn_com_falha_entra_no_ciclo_de_recuperacao() {
        let mut manager = manager(vec![item("a", "queued")]);
        manager.executor.fail_spawn = true;
        assert!(matches!(
            manager.start_next(),
            StartOutcome::SpawnFailed { .. }
        ));
        // code nulo + falha de spawn: o oráculo trata Number(null)==0 como
        // sucesso e o Epic vai para verify; o verify também falha ao spawnar
        // e, pela mesma regra, termina "done" após a fase de verificação.
        let status = manager.items()[0]["status"].clone();
        assert!(
            status == "done" || status == "queued",
            "estado inesperado: {status}"
        );
    }

    // Adaptador para compartilhar o store flaky sem mudar o tipo do manager.
    #[derive(Clone)]
    struct SharedStore(Arc<FlakyStore>);

    impl DownloadsPersistence for SharedStore {
        fn load(&self) -> Result<Option<String>, String> {
            self.0.load()
        }

        fn save(&self, contents: &str) -> Result<(), String> {
            self.0.save(contents)
        }
    }

    #[test]
    fn file_persistence_grava_atomicamente() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-download-manager-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("downloads.json");
        let store = FilePersistence::new(&path);
        let mut manager = DownloadManager::open(store, FakeExecutor::ready(), 1);
        manager.add(item("a", "queued"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"appid\": \"a\""));
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "sobrou temporário: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
