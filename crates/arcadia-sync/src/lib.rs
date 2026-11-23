//! Sync com o backend: biblioteca, horas, conquistas e sources.
//!
//! Port 1:1 de `app/electron/supabase/biblioteca.js`, `sync.js` e `sources.js`
//! sobre [`arcadia_api`], [`arcadia_store`] e [`arcadia_contracts`].
//!
//! O [`SyncEngine`] nasce em modo **pull-only** para as rotinas de sincronização
//! em lote. Alterações explícitas da loja têm envio próprio. Para habilitar
//! sincronização em lote nos dois sentidos, use
//! [`SyncEngine::set_pull_only`]`(false)` — veja o relatório da tarefa para o
//! que ainda precisa de paridade antes disso em produção.
//!
//! Realtime (WebSocket `library-<user>` / `achievements-<user>`) está fora do
//! escopo desta camada: o servidor continua sendo a autoridade e o próximo
//! `reconcile`/`sync_now` converge.

pub mod achievements;
pub mod library;
pub mod playtime;
pub mod sources;
pub mod state;

mod util;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

use arcadia_api::{ApiClient, Error as ApiError};
use arcadia_store::accounts::AccountScope;

pub use achievements::{
    AchievementsPull, AchievementsPush, AchievementsSync, ACHIEVEMENTS_FILE, QUEUE_FILE,
};
pub use library::{
    LibraryPull, LibraryPush, CUSTOM_GAMES_FILE, OVERRIDES_FILE, PENDING_GAMES_FILE,
};
pub use playtime::SteamPush;
pub use sources::{SourcesPull, SourcesPush, SOURCES_FILE};
pub use state::{StateDocument, SyncState, SyncStateFile, SYNC_STATE_FILE};

/// Erro tipado do sync.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    #[error("nao_logado")]
    NotLoggedIn,
    #[error("conta_trocada")]
    AccountSwitched,
    #[error("token_invalido")]
    Unauthorized,
    /// Falha de rede/DNS/timeout. O detalhe fica disponível para diagnóstico.
    #[error("rede_indisponivel")]
    Network(String),
    #[error("modo_pull_only")]
    PullOnly,
    #[error("{message}")]
    Backend { message: String, retryable: bool },
    #[error("resposta_invalida: {0}")]
    InvalidResponse(String),
    #[error("armazenamento: {0}")]
    Storage(String),
}

impl SyncError {
    pub fn code(&self) -> &'static str {
        match self {
            SyncError::NotLoggedIn => "nao_logado",
            SyncError::AccountSwitched => "conta_trocada",
            SyncError::Unauthorized => "token_invalido",
            SyncError::Network(_) => "rede_indisponivel",
            SyncError::PullOnly => "modo_pull_only",
            SyncError::Backend { .. } => "backend",
            SyncError::InvalidResponse(_) => "resposta_invalida",
            SyncError::Storage(_) => "armazenamento",
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            SyncError::Network(_) => true,
            SyncError::Backend { retryable, .. } => *retryable,
            _ => false,
        }
    }

    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            SyncError::NotLoggedIn | SyncError::AccountSwitched | SyncError::Unauthorized
        )
    }

    /// Mensagem persistida em `sync_state.json.lastError`.
    pub fn state_message(&self) -> String {
        match self {
            SyncError::NotLoggedIn => "nao_logado".into(),
            SyncError::AccountSwitched => "conta_trocada".into(),
            SyncError::Unauthorized => "token_invalido".into(),
            SyncError::Network(_) => "rede_indisponivel".into(),
            SyncError::PullOnly => "modo_pull_only".into(),
            SyncError::Backend { message, .. } => message.clone(),
            SyncError::InvalidResponse(message) => format!("resposta_invalida: {message}"),
            SyncError::Storage(message) => format!("armazenamento: {message}"),
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            SyncError::Network(detail) => Some(detail),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(error: std::io::Error) -> Self {
        SyncError::Storage(error.to_string())
    }
}

pub(crate) fn map_api_error(error: ApiError) -> SyncError {
    match error {
        ApiError::Transport(error) => SyncError::Network(error.message),
        ApiError::Api(error) if error.status == 401 => SyncError::Unauthorized,
        ApiError::Api(error) if error.status == 409 && error.message == "sessao_trocada" => {
            SyncError::AccountSwitched
        }
        ApiError::Api(error) => {
            // `isRetryable` do JS: permission denied/42501 é bug de código;
            // todo o resto (incluindo 429) é transitório.
            let haystack = format!(
                "{} {}",
                error.message.to_ascii_lowercase(),
                error.code.as_deref().unwrap_or_default()
            );
            let retryable = !(haystack.contains("permission denied") || haystack.contains("42501"));
            SyncError::Backend {
                message: error.message,
                retryable,
            }
        }
        ApiError::Decode(error) => SyncError::InvalidResponse(error.message),
    }
}

pub(crate) async fn ensure_session(client: &ApiClient) -> Result<(), SyncError> {
    let Some(session) = client.auth().get_session().await else {
        return Err(SyncError::NotLoggedIn);
    };
    if session.user_id().is_none() {
        return Err(SyncError::NotLoggedIn);
    }
    Ok(())
}

/// Eventos emitidos pelo engine (mesmos canais do main process Electron).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    /// `library:changed` com `source: "biblioteca"`.
    LibraryChanged,
    /// `sync:state` com o estado tipado.
    SyncState(SyncState),
}

impl SyncEvent {
    pub fn channel(&self) -> &'static str {
        match self {
            SyncEvent::LibraryChanged => "library:changed",
            SyncEvent::SyncState(_) => "sync:state",
        }
    }

    pub fn source(&self) -> Option<&'static str> {
        match self {
            SyncEvent::LibraryChanged => Some("biblioteca"),
            SyncEvent::SyncState(_) => None,
        }
    }
}

/// Callback síncrono de eventos; nunca é chamado dentro de `await`.
pub type SyncEventListener = Arc<dyn Fn(&SyncEvent) + Send + Sync>;

/// Resultado de uma rodada completa (`reconcile`/`sync_now`/`sync_all`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub state: SyncState,
    pub events: Vec<SyncEvent>,
    pub library_changed: bool,
    pub library_pushed: usize,
    pub library_pulled: usize,
    pub playtime_pushed: usize,
    pub steam_rows: usize,
    pub achievements_pushed: usize,
    pub achievements_pulled: usize,
    pub sources_pushed: usize,
    pub sources_pulled: usize,
    pub full: bool,
}

/// Resultado de `sync_achievements` no engine.
pub type AchievementsSyncReport = AchievementsSync;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncMode {
    Now,
    Reconcile,
    All,
}

/// Fachada de sync; clone compartilha a trava e o modo pull-only.
#[derive(Clone)]
pub struct SyncEngine {
    client: ApiClient,
    scope: AccountScope,
    pull_only: Arc<AtomicBool>,
    listener: Option<SyncEventListener>,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl SyncEngine {
    /// Engine em modo pull-only (padrão de segurança).
    pub fn new(client: ApiClient, scope: AccountScope) -> Self {
        Self {
            client,
            scope,
            pull_only: Arc::new(AtomicBool::new(true)),
            listener: None,
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn with_pull_only(client: ApiClient, scope: AccountScope, pull_only: bool) -> Self {
        let engine = Self::new(client, scope);
        engine.set_pull_only(pull_only);
        engine
    }

    pub fn with_event_listener(
        mut self,
        listener: impl Fn(&SyncEvent) + Send + Sync + 'static,
    ) -> Self {
        self.listener = Some(Arc::new(listener));
        self
    }

    pub fn set_event_listener(&mut self, listener: impl Fn(&SyncEvent) + Send + Sync + 'static) {
        self.listener = Some(Arc::new(listener));
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    pub fn scope(&self) -> &AccountScope {
        &self.scope
    }

    pub fn pull_only(&self) -> bool {
        self.pull_only.load(Ordering::SeqCst)
    }

    pub fn set_pull_only(&self, pull_only: bool) {
        self.pull_only.store(pull_only, Ordering::SeqCst);
    }

    /// Estado tipado atual (`lastPullAt` é o cursor de conquistas).
    pub fn state(&self) -> SyncState {
        SyncStateFile::new(&self.scope).state(achievements::queue_length(&self.scope))
    }

    /// Login/boot: reenvia as ações da loja e sincroniza os outros domínios
    /// conforme o modo.
    pub async fn reconcile(&self) -> Result<SyncReport, SyncError> {
        self.run(SyncMode::Reconcile).await
    }

    /// Sync incremental de todos os domínios (conquistas: push da fila + pull
    /// delta; biblioteca/sources: pull + push quando habilitado).
    pub async fn sync_now(&self) -> Result<SyncReport, SyncError> {
        self.run(SyncMode::Now).await
    }

    /// Sync completo: conquistas fazem catch-up de TODAS as locais e pull sem
    /// cursor (`p_since = null`); biblioteca/sources reconciliam normalmente.
    pub async fn sync_all(&self) -> Result<SyncReport, SyncError> {
        self.run(SyncMode::All).await
    }

    // ── operações de domínio (cada uma pega a trava) ───────────────────────

    pub async fn pull_library(&self) -> Result<LibraryPull, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match library::pull(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(outcome.changed);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// Envia somente as inclusões e remoções explícitas da loja Steam.
    pub async fn send_library_changes(&self) -> Result<(), SyncError> {
        let _guard = self.lock.lock().await;
        match library::send_steam_changes(&self.client, &self.scope).await {
            Ok(pushed) => {
                if pushed > 0 {
                    self.emit_success(false);
                }
                Ok(())
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// Bloqueado em pull-only (`SyncError::PullOnly`, sem requisição).
    pub async fn push_library(&self) -> Result<LibraryPush, SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match library::push(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn pull_achievements(&self) -> Result<AchievementsPull, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match achievements::pull_delta(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn pull_achievements_full(&self) -> Result<AchievementsPull, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match achievements::pull_all(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// Sobe a fila de conquistas; bloqueado em pull-only.
    pub async fn push_achievements(&self) -> Result<AchievementsPush, SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match achievements::push_queue(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// `syncNow` de conquistas: push da fila + pull delta (em pull-only só o pull).
    pub async fn sync_achievements(&self) -> Result<AchievementsSyncReport, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        let mut pushed = 0_usize;
        if !self.pull_only() {
            match achievements::push_queue(&self.client, &self.scope).await {
                Ok(outcome) => pushed += outcome.pushed,
                Err(error) => return Err(self.fail(error)),
            }
        }
        match achievements::pull_delta(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(AchievementsSyncReport {
                    pushed,
                    pulled: outcome.pulled,
                    changed: outcome.changed,
                })
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// `syncAllLocal`: catch-up de todas as conquistas; bloqueado em pull-only.
    pub async fn push_achievements_all(&self) -> Result<AchievementsPush, SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match achievements::sync_all_local(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn delete_achievement(&self, appid: &str, apiname: &str) -> Result<(), SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match achievements::delete_achievement(&self.client, &self.scope, appid, apiname).await {
            Ok(()) => {
                self.emit_success(false);
                Ok(())
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn push_sources(&self) -> Result<SourcesPush, SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match sources::push(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn pull_sources(&self) -> Result<SourcesPull, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match sources::pull(&self.client, &self.scope).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    /// Envia totais absolutos de horas Steam; bloqueado em pull-only.
    pub async fn push_steam_playtime(&self, items: &[Value]) -> Result<SteamPush, SyncError> {
        if self.pull_only() {
            return Err(SyncError::PullOnly);
        }
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match playtime::push_steam(&self.client, items).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn reconcile_sources(&self) -> Result<(SourcesPush, SourcesPull), SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }
        match sources::reconcile(&self.client, &self.scope, self.pull_only()).await {
            Ok(outcome) => {
                self.emit_success(false);
                Ok(outcome)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    // ── interno ────────────────────────────────────────────────────────────

    async fn run(&self, mode: SyncMode) -> Result<SyncReport, SyncError> {
        let _guard = self.lock.lock().await;
        if let Err(error) = ensure_session(&self.client).await {
            return Err(self.fail(error));
        }

        let mut report = SyncReport {
            full: mode == SyncMode::All,
            ..SyncReport::default()
        };
        let mut first_error: Option<SyncError> = None;

        macro_rules! attempt {
            ($future:expr) => {
                match $future.await {
                    Ok(value) => Some(value),
                    Err(error) => {
                        if let Err(fatal) = record_error(&mut first_error, error) {
                            let fatal = self.fail(fatal);
                            return Err(fatal);
                        }
                        None
                    }
                }
            };
        }

        // O pull precede o push em lote. Ações explícitas têm prioridade sobre
        // o snapshot remoto e são reenviadas antes de baixá-lo.
        if let Some(pushed) = attempt!(library::send_steam_changes(&self.client, &self.scope)) {
            report.library_pushed += pushed;
        }
        if let Some(outcome) = attempt!(library::pull(&self.client, &self.scope)) {
            report.library_changed |= outcome.changed;
            report.library_pulled = outcome.pulled;
            report.steam_rows = outcome.steam_rows;
        }
        if !self.pull_only() {
            if let Some(outcome) = attempt!(library::push(&self.client, &self.scope)) {
                report.library_pushed += outcome.pushed;
                report.playtime_pushed += outcome.playtime;
            }
        }

        // Conquistas.
        match mode {
            SyncMode::Now => {
                if !self.pull_only() {
                    if let Some(outcome) =
                        attempt!(achievements::push_queue(&self.client, &self.scope))
                    {
                        report.achievements_pushed += outcome.pushed;
                    }
                }
                if let Some(outcome) = attempt!(achievements::pull_delta(&self.client, &self.scope))
                {
                    report.achievements_pulled += outcome.pulled;
                }
            }
            SyncMode::Reconcile | SyncMode::All => {
                let has_local = achievements::has_local_achievements(&self.scope);
                if self.pull_only() {
                    let pulled = if mode == SyncMode::All || !has_local {
                        attempt!(achievements::pull_all(&self.client, &self.scope))
                    } else {
                        attempt!(achievements::pull_delta(&self.client, &self.scope))
                    };
                    if let Some(outcome) = pulled {
                        report.achievements_pulled += outcome.pulled;
                    }
                } else {
                    if has_local {
                        if let Some(outcome) =
                            attempt!(achievements::sync_all_local(&self.client, &self.scope))
                        {
                            report.achievements_pushed += outcome.pushed;
                        }
                    }
                    if let Some(outcome) =
                        attempt!(achievements::push_queue(&self.client, &self.scope))
                    {
                        report.achievements_pushed += outcome.pushed;
                    }
                    let pulled = if mode == SyncMode::All || !has_local {
                        attempt!(achievements::pull_all(&self.client, &self.scope))
                    } else {
                        attempt!(achievements::pull_delta(&self.client, &self.scope))
                    };
                    if let Some(outcome) = pulled {
                        report.achievements_pulled += outcome.pulled;
                    }
                }
            }
        }

        // Sources: push e depois pull (ordem do `sources.js`).
        if !self.pull_only() {
            if let Some(outcome) = attempt!(sources::push(&self.client, &self.scope)) {
                report.sources_pushed += outcome.pushed;
            }
        }
        if let Some(outcome) = attempt!(sources::pull(&self.client, &self.scope)) {
            report.sources_pulled += outcome.pulled;
        }

        if let Some(error) = first_error {
            return Err(self.fail(error));
        }

        report.state = self.state();
        let mut events = Vec::new();
        if report.library_changed {
            events.push(SyncEvent::LibraryChanged);
        }
        events.push(SyncEvent::SyncState(report.state.clone()));
        report.events = self.emit_events(events);
        Ok(report)
    }

    fn emit_success(&self, library_changed: bool) -> Vec<SyncEvent> {
        let state = self.state();
        let mut events = Vec::new();
        if library_changed {
            events.push(SyncEvent::LibraryChanged);
        }
        events.push(SyncEvent::SyncState(state));
        self.emit_events(events)
    }

    fn emit_events(&self, events: Vec<SyncEvent>) -> Vec<SyncEvent> {
        if let Some(listener) = &self.listener {
            for event in &events {
                listener(event);
            }
        }
        events
    }

    fn fail(&self, error: SyncError) -> SyncError {
        if error != SyncError::PullOnly {
            let _ = SyncStateFile::new(&self.scope).update(|state| {
                state.set_last_error(Some(&error.state_message()));
            });
        }
        let state = self.state();
        self.emit_events(vec![SyncEvent::SyncState(state)]);
        error
    }
}

fn record_error(first_error: &mut Option<SyncError>, error: SyncError) -> Result<(), SyncError> {
    if error.is_fatal() {
        return Err(error);
    }
    if first_error.is_none() {
        *first_error = Some(error);
    }
    Ok(())
}

/// Serializa o estado no shape do `sync:state` do Electron.
pub fn sync_state_json(state: &SyncState) -> Value {
    serde_json::json!({
        "lastPullAt": state.last_pull_at,
        "lastSyncAt": state.last_sync_at,
        "lastError": state.last_error,
        "queueLen": state.queue_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_and_retryability_match_frozen_client() {
        assert_eq!(SyncError::Network("x".into()).code(), "rede_indisponivel");
        assert!(SyncError::Network("x".into()).is_retryable());
        assert!(!SyncError::Unauthorized.is_retryable());
        assert!(SyncError::Backend {
            message: "timeout".into(),
            retryable: true
        }
        .is_retryable());
        assert!(!SyncError::Backend {
            message: "permission denied".into(),
            retryable: false
        }
        .is_retryable());
    }

    #[test]
    fn state_json_uses_electron_keys() {
        let json = sync_state_json(&SyncState {
            last_pull_at: Some(10),
            last_sync_at: Some(20),
            last_error: Some("rede_indisponivel".into()),
            queue_len: 3,
        });
        assert_eq!(
            json,
            serde_json::json!({
                "lastPullAt": 10,
                "lastSyncAt": 20,
                "lastError": "rede_indisponivel",
                "queueLen": 3
            })
        );
    }
}
