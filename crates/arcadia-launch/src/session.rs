//! Launch/focus state machines.
//!
//! Port of `app/electron/focus-session.js`, `game-session.js` and
//! `window-focus.js`, plus the running-game watcher policy from `main.js`.
//! Clock and process presence are injected; the event names and payload
//! shapes match the Electron IPC channels exactly.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::clock::Clock;

pub const MAX_START_MS: i64 = 60_000;
pub const EMPTY_CONFIRMATIONS: u32 = 2;

pub fn is_steam_install_uri(value: &str) -> bool {
    value
        .get(..16)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("steam://install/"))
}

/// Installation URIs are store actions, not game sessions.
pub fn should_track_game_session(command: &[String]) -> bool {
    !command
        .iter()
        .any(|argument| is_steam_install_uri(argument))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusChange {
    pub focused: bool,
    pub forced: bool,
}

pub type FocusCallback = Box<dyn FnMut(bool, bool)>;
pub type LaunchCallback = Box<dyn FnMut(&LaunchTransition)>;

/// Gate that keeps launcher input disabled while a child game owns the
/// foreground.
#[derive(Default)]
pub struct FocusSession {
    active: bool,
    restored: bool,
    changes: Vec<FocusChange>,
    callback: Option<FocusCallback>,
}

impl FocusSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_callback(callback: FocusCallback) -> Self {
        Self {
            callback: Some(callback),
            ..Self::default()
        }
    }

    pub fn begin(&mut self) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.restored = false;
        self.emit(false, true);
        true
    }

    pub fn native_focus(&mut self, focused: bool) -> bool {
        // A transient compositor focus event must not reopen launcher input
        // while a launch session is active.
        if self.active && focused {
            return false;
        }
        self.emit(focused, false);
        true
    }

    pub fn finish(&mut self) -> bool {
        if !self.active || self.restored {
            return false;
        }
        self.restored = true;
        self.active = false;
        self.emit(true, true);
        true
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_restored(&self) -> bool {
        self.restored
    }

    pub fn take_changes(&mut self) -> Vec<FocusChange> {
        std::mem::take(&mut self.changes)
    }

    fn emit(&mut self, focused: bool, forced: bool) {
        let change = FocusChange { focused, forced };
        self.changes.push(change);
        if let Some(callback) = &mut self.callback {
            callback(change.focused, change.forced);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LaunchToken {
    pub id: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchMeta {
    pub game_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchState {
    Idle,
    Starting,
    Running,
    Stopping,
}

impl LaunchState {
    pub fn as_str(self) -> &'static str {
        match self {
            LaunchState::Idle => "idle",
            LaunchState::Starting => "starting",
            LaunchState::Running => "running",
            LaunchState::Stopping => "stopping",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTransition {
    pub state: LaunchState,
    pub token: LaunchToken,
    pub meta: LaunchMeta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSnapshot {
    pub state: LaunchState,
    pub token: Option<LaunchToken>,
    pub meta: LaunchMeta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopResult {
    pub ok: bool,
    pub state: LaunchState,
    pub already: bool,
    pub token: Option<LaunchToken>,
}

#[derive(Clone, Debug)]
struct CurrentLaunch {
    token: LaunchToken,
    state: LaunchState,
    meta: LaunchMeta,
    stop_requested: bool,
}

/// One launch generation: `starting → running → stopping → idle`.
/// A token stays owned until `finish`, so stale callbacks cannot touch a later
/// launch.
#[derive(Default)]
pub struct LaunchSession {
    current: Option<CurrentLaunch>,
    next_id: u64,
    transitions: Vec<LaunchTransition>,
    callback: Option<LaunchCallback>,
}

impl LaunchSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_callback(callback: LaunchCallback) -> Self {
        Self {
            callback: Some(callback),
            ..Self::default()
        }
    }

    pub fn begin(&mut self, meta: LaunchMeta) -> Option<LaunchToken> {
        if self.current.is_some() {
            return None;
        }
        self.next_id = self.next_id.saturating_add(1);
        let token = LaunchToken { id: self.next_id };
        self.current = Some(CurrentLaunch {
            token,
            state: LaunchState::Starting,
            meta: meta.clone(),
            stop_requested: false,
        });
        self.notify(LaunchTransition {
            state: LaunchState::Starting,
            token,
            meta,
        });
        Some(token)
    }

    pub fn owns(&self, token: LaunchToken) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.token == token)
    }

    pub fn mark_running(&mut self, token: LaunchToken) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        if current.token != token || current.state != LaunchState::Starting {
            return false;
        }
        current.state = LaunchState::Running;
        let transition = LaunchTransition {
            state: LaunchState::Running,
            token,
            meta: current.meta.clone(),
        };
        self.notify(transition);
        true
    }

    pub fn request_stop(&mut self, token: Option<LaunchToken>) -> StopResult {
        let Some(current) = self.current.as_mut() else {
            return StopResult {
                ok: false,
                state: LaunchState::Idle,
                already: false,
                token: None,
            };
        };
        if token.is_some_and(|value| value != current.token) {
            return StopResult {
                ok: false,
                state: LaunchState::Idle,
                already: false,
                token: None,
            };
        }
        if current.state == LaunchState::Stopping {
            return StopResult {
                ok: true,
                state: LaunchState::Stopping,
                already: true,
                token: Some(current.token),
            };
        }
        current.state = LaunchState::Stopping;
        current.stop_requested = true;
        let current_token = current.token;
        let meta = current.meta.clone();
        self.notify(LaunchTransition {
            state: LaunchState::Stopping,
            token: current_token,
            meta,
        });
        StopResult {
            ok: true,
            state: LaunchState::Stopping,
            already: false,
            token: Some(current_token),
        }
    }

    pub fn finish(&mut self, token: LaunchToken) -> bool {
        let owns = self
            .current
            .as_ref()
            .is_some_and(|current| current.token == token);
        if !owns {
            return false;
        }
        self.current = None;
        // The Node implementation notifies idle after clearing `current`, so
        // the transition meta is empty (game id resolution falls back to the
        // live records).
        self.notify(LaunchTransition {
            state: LaunchState::Idle,
            token,
            meta: LaunchMeta::default(),
        });
        true
    }

    pub fn get_state(&self, token: Option<LaunchToken>) -> LaunchState {
        if token.is_some_and(|value| !self.owns(value)) {
            return LaunchState::Idle;
        }
        self.current
            .as_ref()
            .map(|current| current.state)
            .unwrap_or(LaunchState::Idle)
    }

    pub fn get_snapshot(&self) -> LaunchSnapshot {
        match &self.current {
            Some(current) => LaunchSnapshot {
                state: current.state,
                token: Some(current.token),
                meta: current.meta.clone(),
            },
            None => LaunchSnapshot {
                state: LaunchState::Idle,
                token: None,
                meta: LaunchMeta::default(),
            },
        }
    }

    pub fn is_busy(&self) -> bool {
        self.current.is_some()
    }

    pub fn is_starting(&self, token: Option<LaunchToken>) -> bool {
        self.get_state(token) == LaunchState::Starting
    }

    pub fn is_running(&self, token: Option<LaunchToken>) -> bool {
        self.get_state(token) == LaunchState::Running
    }

    pub fn is_stopping(&self, token: Option<LaunchToken>) -> bool {
        self.get_state(token) == LaunchState::Stopping
    }

    pub fn stop_requested(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.stop_requested)
    }

    pub fn take_transitions(&mut self) -> Vec<LaunchTransition> {
        std::mem::take(&mut self.transitions)
    }

    fn notify(&mut self, transition: LaunchTransition) {
        if let Some(callback) = &mut self.callback {
            callback(&transition);
        }
        self.transitions.push(transition);
    }
}

/// Presence snapshot supplied by a process probe on each poll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    pub owned_group_alive: bool,
    pub handoff_waiting: bool,
    pub process_session_ready: bool,
    pub steam_client_running: bool,
    pub steam_game_running: bool,
    /// `pgrep -f` activity hint used for Steam games on Linux/macOS.
    pub pattern_running: bool,
}

pub trait GamePresenceProbe {
    fn presence(&self, game: &TrackedGame) -> Presence;
}

/// Deterministic probe for tests.
#[derive(Debug, Default)]
pub struct StaticGameProbe {
    presence: RefCell<Presence>,
}

impl StaticGameProbe {
    pub fn new(presence: Presence) -> Self {
        Self {
            presence: RefCell::new(presence),
        }
    }

    pub fn set(&self, presence: Presence) {
        *self.presence.borrow_mut() = presence;
    }
}

impl GamePresenceProbe for StaticGameProbe {
    fn presence(&self, _game: &TrackedGame) -> Presence {
        *self.presence.borrow()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedGame {
    pub pid: u32,
    pub game_id: String,
    pub steam_wrapper: bool,
    pub has_process_session: bool,
    pub launcher: String,
    pub started_at_ms: i64,
    pub post_script: Option<String>,
}

impl TrackedGame {
    pub fn new(pid: u32, game_id: impl Into<String>, started_at_ms: i64) -> Self {
        Self {
            pid,
            game_id: game_id.into(),
            steam_wrapper: false,
            has_process_session: false,
            launcher: String::new(),
            started_at_ms,
            post_script: None,
        }
    }

    pub fn steam(pid: u32, game_id: impl Into<String>, started_at_ms: i64) -> Self {
        Self {
            steam_wrapper: true,
            ..Self::new(pid, game_id, started_at_ms)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEnded {
    pub game_id: String,
    /// Whole minutes actually played (0 when under a minute or unconfirmed).
    pub minutes: u32,
    pub post_script: Option<String>,
    pub session_confirmed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TickOutcome {
    Idle,
    Waiting,
    Running,
    Finalized(SessionEnded),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    GameRunning(bool),
    GameActive {
        rodando: bool,
        game_id: String,
    },
    LaunchState {
        state: LaunchState,
        game_id: String,
        token: Option<u64>,
    },
    LaunchError {
        game_id: String,
        error: String,
    },
    LaunchWarning {
        game_id: String,
        warnings: Vec<String>,
    },
    Focus(bool),
}

impl SessionEvent {
    pub fn channel(&self) -> &'static str {
        match self {
            SessionEvent::GameRunning(_) => "game:running",
            SessionEvent::GameActive { .. } => "game:active",
            SessionEvent::LaunchState { .. } => "game:launchState",
            SessionEvent::LaunchError { .. } => "game:launchError",
            SessionEvent::LaunchWarning { .. } => "game:launchWarning",
            SessionEvent::Focus(_) => "app:focus",
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            SessionEvent::GameRunning(running) => json!(running),
            SessionEvent::GameActive { rodando, game_id } => {
                json!({ "rodando": rodando, "gameId": game_id })
            }
            SessionEvent::LaunchState {
                state,
                game_id,
                token,
            } => json!({
                "state": state.as_str(),
                "gameId": game_id,
                "token": token,
            }),
            SessionEvent::LaunchError { game_id, error } => {
                json!({ "gameId": game_id, "error": error })
            }
            SessionEvent::LaunchWarning { game_id, warnings } => {
                json!({ "gameId": game_id, "warnings": warnings })
            }
            SessionEvent::Focus(focused) => json!(focused),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({ "channel": self.channel(), "payload": self.payload() })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopState {
    Idle,
    Stopping,
}

impl StopState {
    pub fn as_str(self) -> &'static str {
        match self {
            StopState::Idle => "idle",
            StopState::Stopping => "stopping",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopOutcome {
    pub ok: bool,
    pub state: StopState,
    pub already: bool,
    pub starting: bool,
}

/// Launch lifecycle + running-game watcher + focus gate.
pub struct GameSession<'a> {
    clock: &'a dyn Clock,
    launch: LaunchSession,
    focus: FocusSession,
    running: bool,
    focused: bool,
    active: Option<TrackedGame>,
    ending: Option<TrackedGame>,
    last_active: Option<TrackedGame>,
    process_seen: bool,
    armed_at_ms: i64,
    signal: u32,
    generation: u64,
    stop_requested: bool,
    focus_applied: bool,
    pre_script_child: bool,
    steam_starter_child: bool,
    events: Vec<SessionEvent>,
    start_time_ms: i64,
}

impl<'a> GameSession<'a> {
    pub fn new(clock: &'a dyn Clock) -> Self {
        Self {
            clock,
            launch: LaunchSession::new(),
            focus: FocusSession::new(),
            running: false,
            focused: true,
            active: None,
            ending: None,
            last_active: None,
            process_seen: false,
            armed_at_ms: 0,
            signal: 0,
            generation: 0,
            stop_requested: false,
            focus_applied: false,
            pre_script_child: false,
            steam_starter_child: false,
            events: Vec::new(),
            start_time_ms: MAX_START_MS,
        }
    }

    pub fn with_start_timeout(mut self, start_time_ms: i64) -> Self {
        self.start_time_ms = start_time_ms;
        self
    }

    pub fn begin_launch(&mut self, meta: LaunchMeta) -> Option<LaunchToken> {
        if self.launch.is_busy() || self.running || self.active.is_some() || self.ending.is_some() {
            return None;
        }
        let token = self.launch.begin(meta)?;
        self.stop_requested = false;
        self.process_seen = false;
        self.signal = 0;
        self.armed_at_ms = 0;
        self.focus_applied = false;
        self.drain_launch();
        Some(token)
    }

    /// Called at the launch boundary for a tracked session.
    pub fn apply_launch_focus(&mut self) -> bool {
        let began = self.focus.begin();
        if began {
            self.focus_applied = true;
        }
        self.drain_focus();
        began
    }

    pub fn attach_spawn(&mut self, token: LaunchToken, game: TrackedGame) -> bool {
        if !self.launch.owns(token) || !self.launch.is_starting(Some(token)) {
            return false;
        }
        self.running = false;
        self.active = Some(game.clone());
        self.ending = None;
        self.last_active = Some(game);
        self.process_seen = false;
        self.armed_at_ms = self.clock.now_ms();
        self.signal = 0;
        self.generation = self.generation.saturating_add(1);
        if !self.launch.mark_running(token) {
            return false;
        }
        self.drain_launch();
        true
    }

    pub fn set_pre_script_child(&mut self, pending: bool) {
        self.pre_script_child = pending;
    }

    pub fn set_steam_starter_child(&mut self, pending: bool) {
        self.steam_starter_child = pending;
    }

    /// Poll one tick of the running-game watcher.
    pub fn tick(&mut self, probe: &dyn GamePresenceProbe) -> TickOutcome {
        let Some(game) = self.active.clone().or_else(|| self.ending.clone()) else {
            return TickOutcome::Idle;
        };
        let presence = probe.presence(&game);
        let now = self.clock.now_ms();

        if game.steam_wrapper {
            let visible = presence.steam_game_running || presence.pattern_running;
            if visible && !self.stop_requested {
                self.process_seen = true;
                self.mark_running_state(true, false);
                self.signal = 0;
                return TickOutcome::Running;
            }
            if !visible
                && !self.process_seen
                && !self.stop_requested
                && now - self.armed_at_ms < self.start_time_ms
            {
                self.signal = 0;
                return TickOutcome::Waiting;
            }
            return self.empty_tick(&game, probe, presence);
        }

        if presence.owned_group_alive {
            if game.has_process_session && !presence.process_session_ready {
                self.signal = 0;
                return TickOutcome::Waiting;
            }
            if !self.stop_requested {
                self.process_seen = true;
                self.mark_running_state(true, false);
            }
            self.signal = 0;
            return if self.stop_requested {
                TickOutcome::Waiting
            } else {
                TickOutcome::Running
            };
        }
        if presence.handoff_waiting {
            self.signal = 0;
            return TickOutcome::Waiting;
        }
        self.empty_tick(&game, probe, presence)
    }

    fn empty_tick(
        &mut self,
        game: &TrackedGame,
        probe: &dyn GamePresenceProbe,
        _presence: Presence,
    ) -> TickOutcome {
        // Never publish a transient false while the asynchronous launch is
        // still starting.
        if self.launch.is_starting(None) {
            self.signal = 0;
            return TickOutcome::Waiting;
        }
        self.signal += 1;
        if self.signal < EMPTY_CONFIRMATIONS {
            return TickOutcome::Waiting;
        }
        // Recheck the owned group immediately before declaring termination.
        let recheck = probe.presence(game);
        if recheck.owned_group_alive {
            self.signal = 0;
            return TickOutcome::Waiting;
        }
        self.finalize(game)
    }

    fn finalize(&mut self, game: &TrackedGame) -> TickOutcome {
        let confirmed = self.process_seen;
        self.mark_running_state(false, true);
        let elapsed = self.clock.now_ms().saturating_sub(game.started_at_ms);
        let minutes = if confirmed && elapsed > 0 {
            ((elapsed + 30_000) / 60_000) as u32
        } else {
            0
        };
        let post_script = if confirmed {
            game.post_script.clone()
        } else {
            None
        };
        let game_id = game.game_id.clone();
        self.active = None;
        self.ending = None;
        self.process_seen = false;
        self.signal = 0;
        self.armed_at_ms = 0;
        self.stop_requested = false;
        self.last_active = Some(game.clone());
        if let Some(token) = self.launch.get_snapshot().token {
            self.launch.finish(token);
        }
        if self.focus_applied {
            self.focus.finish();
            self.focus_applied = false;
        }
        self.drain_launch();
        self.drain_focus();
        TickOutcome::Finalized(SessionEnded {
            game_id,
            minutes,
            post_script,
            session_confirmed: confirmed,
        })
    }

    /// User Stop.  Releases the token only after process termination is
    /// confirmed (or immediately for a launch that never produced a child).
    pub fn stop(&mut self) -> StopOutcome {
        let had_game = self.active.is_some() || self.ending.is_some();
        let already = self.stop_requested;
        self.stop_requested = true;

        if let Some(active) = self.active.take() {
            self.ending = Some(active);
        }
        if had_game {
            self.mark_running_state(false, true);
            self.launch.request_stop(None);
            self.drain_launch();
            return StopOutcome {
                ok: true,
                state: StopState::Stopping,
                already,
                starting: false,
            };
        }
        if self.launch.is_busy() {
            self.launch.request_stop(None);
            self.drain_launch();
            return StopOutcome {
                ok: true,
                state: StopState::Stopping,
                already,
                starting: true,
            };
        }
        StopOutcome {
            ok: true,
            state: StopState::Idle,
            already,
            starting: false,
        }
    }

    /// `finishCancelledLaunch`: release the generation only when every child
    /// that could still lead to a game has exited.
    pub fn finish_cancelled(&mut self) -> bool {
        if !self.stop_requested
            || self.active.is_some()
            || self.ending.is_some()
            || self.pre_script_child
            || self.steam_starter_child
        {
            return false;
        }
        let Some(token) = self.launch.get_snapshot().token else {
            return false;
        };
        self.launch.finish(token);
        self.stop_requested = false;
        if self.focus_applied {
            self.focus.finish();
            self.focus_applied = false;
        }
        self.drain_launch();
        self.drain_focus();
        true
    }

    /// Synchronous spawn failure: no child exists, so focus can return now.
    pub fn spawn_failed_sync(&mut self, token: LaunchToken, error: impl Into<String>) -> bool {
        if !self.launch.owns(token) {
            return false;
        }
        let game_id = self.launch.get_snapshot().meta.game_id;
        let error = error.into();
        self.launch.finish(token);
        self.stop_requested = false;
        if self.focus_applied {
            self.focus.finish();
            self.focus_applied = false;
        }
        self.drain_launch();
        self.drain_focus();
        self.events
            .push(SessionEvent::LaunchError { game_id, error });
        true
    }

    /// Asynchronous spawn error: keep the token until the owned group is
    /// confirmed absent; the watcher then finalizes normally.
    pub fn spawn_failed_async(&mut self, token: LaunchToken, error: impl Into<String>) -> bool {
        if !self.launch.owns(token) {
            return false;
        }
        let game_id = self.launch.get_snapshot().meta.game_id;
        self.launch.request_stop(Some(token));
        self.stop_requested = true;
        if let Some(active) = self.active.take() {
            self.ending = Some(active);
        }
        self.drain_launch();
        self.events.push(SessionEvent::LaunchError {
            game_id,
            error: error.into(),
        });
        true
    }

    pub fn push_warning(&mut self, game_id: impl Into<String>, warnings: Vec<String>) {
        if !warnings.is_empty() {
            self.events.push(SessionEvent::LaunchWarning {
                game_id: game_id.into(),
                warnings,
            });
        }
    }

    pub fn native_focus(&mut self, focused: bool) -> bool {
        let result = self.focus.native_focus(focused);
        self.drain_focus();
        result
    }

    /// Gamescope hides the real window focus from Electron.  While a game is
    /// on screen the launcher must publish a synthetic blur once.
    pub fn focus_for_game_scene(&mut self, game_in_scene: bool) -> bool {
        if game_in_scene && self.focused {
            self.native_focus(false)
        } else {
            false
        }
    }

    pub fn launch_state(&self) -> LaunchState {
        self.launch.get_state(None)
    }

    pub fn launch_snapshot(&self) -> LaunchSnapshot {
        self.launch.get_snapshot()
    }

    pub fn is_launch_starting(&self) -> bool {
        self.launch.is_starting(None)
    }

    pub fn is_launch_busy(&self) -> bool {
        self.launch.is_busy()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn active_game(&self) -> Option<&TrackedGame> {
        self.active.as_ref().or(self.ending.as_ref())
    }

    pub fn game_id(&self) -> String {
        if self.launch.get_state(None) == LaunchState::Idle {
            return String::new();
        }
        let meta = self.launch.get_snapshot().meta.game_id;
        if !meta.is_empty() {
            return meta;
        }
        self.active
            .as_ref()
            .or(self.ending.as_ref())
            .or(self.last_active.as_ref())
            .map(|game| game.game_id.clone())
            .unwrap_or_default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn take_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    fn mark_running_state(&mut self, running: bool, force: bool) {
        if running == self.running && !force {
            return;
        }
        self.running = running;
        let game_id = if running {
            self.active
                .as_ref()
                .or(self.last_active.as_ref())
                .map(|game| game.game_id.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.events.push(SessionEvent::GameRunning(running));
        self.events.push(SessionEvent::GameActive {
            rodando: running,
            game_id,
        });
    }

    fn drain_launch(&mut self) {
        for transition in self.launch.take_transitions() {
            let game_id = if transition.state == LaunchState::Idle {
                String::new()
            } else {
                let meta = transition.meta.game_id.clone();
                if !meta.is_empty() {
                    meta
                } else {
                    self.active
                        .as_ref()
                        .or(self.ending.as_ref())
                        .or(self.last_active.as_ref())
                        .map(|game| game.game_id.clone())
                        .unwrap_or_default()
                }
            };
            self.events.push(SessionEvent::LaunchState {
                state: transition.state,
                game_id,
                token: Some(transition.token.id),
            });
        }
    }

    fn drain_focus(&mut self) {
        for change in self.focus.take_changes() {
            if change.focused != self.focused || change.forced {
                self.focused = change.focused;
                self.events.push(SessionEvent::Focus(change.focused));
            }
        }
    }
}

/// Minimal window surface needed by `restoreWindowFocus`.
pub trait WindowFocusTarget {
    fn is_destroyed(&self) -> bool;
    fn is_minimized(&self) -> bool;
    fn is_visible(&self) -> bool;
    fn restore(&mut self);
    fn show(&mut self);
    fn focus_steal(&mut self) -> bool;
    fn focus_plain(&mut self) -> bool;
    fn focus_web_contents(&mut self) -> bool;
}

/// Restore/show/focus the launcher after a child game ends.
pub fn restore_window_focus(
    target: Option<&mut dyn WindowFocusTarget>,
    can_restore: &dyn Fn() -> bool,
    on_focused: &mut dyn FnMut(),
) -> bool {
    if !can_restore() {
        return false;
    }
    let Some(target) = target else {
        return false;
    };
    if target.is_destroyed() {
        return false;
    }
    if target.is_minimized() {
        target.restore();
    }
    if !target.is_visible() {
        target.show();
    }
    if !target.focus_steal() {
        let _ = target.focus_plain();
    }
    let _ = target.focus_web_contents();
    if !can_restore() {
        return false;
    }
    on_focused();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[test]
    fn focus_session_publishes_false_then_one_true() {
        let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = changes.clone();
        let mut gate = FocusSession::with_callback(Box::new(move |focused, forced| {
            sink.borrow_mut().push((focused, forced));
        }));
        assert!(gate.begin());
        assert_eq!(*changes.borrow(), vec![(false, true)]);
        assert!(!gate.native_focus(true));
        assert_eq!(*changes.borrow(), vec![(false, true)]);
        assert!(gate.finish());
        assert!(!gate.finish());
        assert_eq!(*changes.borrow(), vec![(false, true), (true, true)]);
        assert!(!gate.is_active());
    }

    #[test]
    fn native_focus_works_outside_a_session() {
        let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = changes.clone();
        let mut gate = FocusSession::with_callback(Box::new(move |focused, _| {
            sink.borrow_mut().push(focused);
        }));
        assert!(gate.native_focus(false));
        assert!(gate.native_focus(true));
        assert_eq!(*changes.borrow(), vec![false, true]);
    }

    #[test]
    fn second_focus_session_can_begin_after_finish() {
        let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = changes.clone();
        let mut gate = FocusSession::with_callback(Box::new(move |focused, _| {
            sink.borrow_mut().push(focused);
        }));
        assert!(gate.begin());
        assert!(gate.finish());
        assert!(gate.begin());
        assert_eq!(*changes.borrow(), vec![false, true, false]);
    }

    #[test]
    fn repeated_clicks_do_not_reopen_focus() {
        let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = changes.clone();
        let mut gate = FocusSession::with_callback(Box::new(move |focused, forced| {
            sink.borrow_mut().push((focused, forced));
        }));
        assert!(gate.begin());
        for _ in 0..3 {
            assert!(!gate.native_focus(true));
        }
        assert!(gate.is_active());
        assert_eq!(*changes.borrow(), vec![(false, true)]);
        assert!(gate.finish());
        assert_eq!(*changes.borrow(), vec![(false, true), (true, true)]);
    }

    #[test]
    fn launch_session_keeps_token_until_confirmed_stop() {
        let transitions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = transitions.clone();
        let mut lifecycle = LaunchSession::with_callback(Box::new(move |transition| {
            sink.borrow_mut().push((
                transition.state.as_str().to_string(),
                transition.token.id,
                transition.meta.game_id.clone(),
            ));
        }));
        let first = lifecycle
            .begin(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        assert!(lifecycle
            .begin(LaunchMeta {
                game_id: "custom:two".into()
            })
            .is_none());
        assert!(lifecycle.mark_running(first));
        let stopped = lifecycle.request_stop(Some(first));
        assert!(stopped.ok);
        assert!(lifecycle.is_busy());
        assert!(!lifecycle.mark_running(first));
        assert!(lifecycle.finish(first));
        let second = lifecycle
            .begin(LaunchMeta {
                game_id: "custom:two".into(),
            })
            .unwrap();
        assert_ne!(second.id, first.id);
        assert!(!lifecycle.finish(first));
        assert_eq!(
            transitions
                .borrow()
                .iter()
                .map(|(state, _, _)| state.as_str())
                .collect::<Vec<_>>(),
            vec!["starting", "running", "stopping", "idle", "starting"]
        );
        assert_eq!(transitions.borrow()[0].2, "custom:one");
        assert_eq!(transitions.borrow()[3].2, "");
    }

    #[test]
    fn launch_session_ignores_stale_generations() {
        let mut lifecycle = LaunchSession::new();
        let first = lifecycle.begin(LaunchMeta::default()).unwrap();
        assert!(lifecycle.finish(first));
        let second = lifecycle.begin(LaunchMeta::default()).unwrap();
        assert!(!lifecycle.mark_running(first));
        assert!(!lifecycle.request_stop(Some(first)).ok);
        assert!(!lifecycle.is_running(Some(second)));
        assert!(lifecycle.finish(second));
    }

    #[test]
    fn install_uri_is_not_a_tracked_session() {
        assert!(is_steam_install_uri("steam://install/123"));
        assert!(is_steam_install_uri("STEAM://INSTALL/123"));
        assert!(!should_track_game_session(&[
            "steam".into(),
            "steam://install/123".into()
        ]));
        assert!(should_track_game_session(&[
            "steam".into(),
            "steam://rungameid/123".into()
        ]));
        assert!(should_track_game_session(&[
            "/games/game".into(),
            "--fullscreen".into()
        ]));
    }

    #[test]
    fn game_session_publishes_exact_event_shapes() {
        let clock = ManualClock::new(1_000_000);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "steam:440".into(),
            })
            .unwrap();
        assert!(session.apply_launch_focus());
        let game = TrackedGame::steam(4242, "steam:440", 1_000_000);
        assert!(session.attach_spawn(token, game));
        let probe = StaticGameProbe::new(Presence {
            pattern_running: true,
            steam_client_running: true,
            ..Presence::default()
        });
        assert_eq!(session.tick(&probe), TickOutcome::Running);

        let events = session.take_events();
        let channels: Vec<&str> = events.iter().map(SessionEvent::channel).collect();
        assert_eq!(
            channels,
            vec![
                "game:launchState",
                "app:focus",
                "game:launchState",
                "game:running",
                "game:active"
            ]
        );
        assert_eq!(
            events[0].payload(),
            json!({"state": "starting", "gameId": "steam:440", "token": 1})
        );
        assert_eq!(events[1].payload(), json!(false));
        assert_eq!(
            events[2].payload(),
            json!({"state": "running", "gameId": "steam:440", "token": 1})
        );
        assert_eq!(events[3].payload(), json!(true));
        assert_eq!(
            events[4].payload(),
            json!({"rodando": true, "gameId": "steam:440"})
        );

        probe.set(Presence::default());
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(3_000);
        let outcome = session.tick(&probe);
        assert!(matches!(outcome, TickOutcome::Finalized(_)));
        assert!(!session.is_running());
        let events = session.take_events();
        assert_eq!(events[0].channel(), "game:running");
        assert_eq!(events[0].payload(), json!(false));
        assert_eq!(events[1].channel(), "game:active");
        assert_eq!(events[1].payload(), json!({"rodando": false, "gameId": ""}));
        assert_eq!(events[2].channel(), "game:launchState");
        assert_eq!(
            events[2].payload(),
            json!({"state": "idle", "gameId": "", "token": 1})
        );
        assert_eq!(events[3].channel(), "app:focus");
        assert_eq!(events[3].payload(), json!(true));
    }

    #[test]
    fn steam_startup_grace_avoids_a_transient_false() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock).with_start_timeout(60_000);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "steam:440".into(),
            })
            .unwrap();
        session.attach_spawn(token, TrackedGame::steam(1, "steam:440", 0));
        let probe = StaticGameProbe::new(Presence {
            steam_client_running: true,
            ..Presence::default()
        });
        clock.advance(10_000);
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(61_000);
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(3_000);
        assert!(matches!(session.tick(&probe), TickOutcome::Finalized(_)));
    }

    #[test]
    fn stop_keeps_the_token_until_the_group_is_gone() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        session.attach_spawn(token, TrackedGame::new(123, "custom:one", 0));
        let probe = StaticGameProbe::new(Presence {
            owned_group_alive: true,
            ..Presence::default()
        });
        assert_eq!(session.tick(&probe), TickOutcome::Running);
        let stopped = session.stop();
        assert_eq!(stopped.state, StopState::Stopping);
        assert!(!stopped.already);
        assert!(session.is_launch_busy());
        // Still alive: the session waits instead of releasing focus.
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        assert!(session.is_launch_busy());
        probe.set(Presence::default());
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(3_000);
        assert!(matches!(session.tick(&probe), TickOutcome::Finalized(_)));
        assert!(!session.is_launch_busy());
    }

    #[test]
    fn cancel_before_spawn_releases_immediately() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        session.apply_launch_focus();
        let stopped = session.stop();
        assert!(stopped.starting);
        assert_eq!(stopped.state, StopState::Stopping);
        assert!(session.is_launch_busy());
        assert!(session.finish_cancelled());
        assert!(!session.is_launch_busy());
        assert_eq!(session.launch_state(), LaunchState::Idle);
        let events = session.take_events();
        assert!(events
            .iter()
            .any(|event| event.channel() == "app:focus" && event.payload() == json!(true)));
    }

    #[test]
    fn pre_script_child_blocks_cancel_release() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        session.set_pre_script_child(true);
        session.stop();
        assert!(!session.finish_cancelled());
        session.set_pre_script_child(false);
        assert!(session.finish_cancelled());
    }

    #[test]
    fn sync_spawn_failure_releases_and_restores_focus() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        session.apply_launch_focus();
        assert!(session.spawn_failed_sync(token, "spawn falhou: ENOENT"));
        assert!(!session.is_launch_busy());
        let events = session.take_events();
        assert_eq!(
            events
                .iter()
                .find(|event| event.channel() == "game:launchError")
                .unwrap()
                .payload(),
            json!({"gameId": "custom:one", "error": "spawn falhou: ENOENT"})
        );
        assert!(events
            .iter()
            .any(|event| event.channel() == "app:focus" && event.payload() == json!(true)));
    }

    #[test]
    fn async_spawn_failure_waits_for_absence() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        session.attach_spawn(token, TrackedGame::new(55, "custom:one", 0));
        assert!(session.spawn_failed_async(token, "spawn falhou"));
        assert!(session.is_launch_busy());
        assert_eq!(session.launch_state(), LaunchState::Stopping);
        let probe = StaticGameProbe::new(Presence::default());
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(3_000);
        assert!(matches!(session.tick(&probe), TickOutcome::Finalized(_)));
        assert!(!session.is_launch_busy());
    }

    #[test]
    fn playtime_is_rounded_and_post_script_only_after_confirmation() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        let mut game = TrackedGame::new(77, "custom:one", 0);
        game.post_script = Some("/scripts/post.sh".into());
        session.attach_spawn(token, game);
        let alive = StaticGameProbe::new(Presence {
            owned_group_alive: true,
            ..Presence::default()
        });
        assert_eq!(session.tick(&alive), TickOutcome::Running);
        clock.advance(45_000);
        let gone = StaticGameProbe::new(Presence::default());
        assert_eq!(session.tick(&gone), TickOutcome::Waiting);
        clock.advance(3_000);
        match session.tick(&gone) {
            TickOutcome::Finalized(ended) => {
                assert_eq!(ended.minutes, 1);
                assert_eq!(ended.post_script.as_deref(), Some("/scripts/post.sh"));
                assert!(ended.session_confirmed);
            }
            other => panic!("expected finalize, got {other:?}"),
        }
    }

    #[test]
    fn unconfirmed_launch_does_not_run_post_script() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "custom:one".into(),
            })
            .unwrap();
        let mut game = TrackedGame::new(77, "custom:one", 0);
        game.post_script = Some("/scripts/post.sh".into());
        session.attach_spawn(token, game);
        let probe = StaticGameProbe::new(Presence::default());
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        clock.advance(3_000);
        match session.tick(&probe) {
            TickOutcome::Finalized(ended) => {
                assert_eq!(ended.minutes, 0);
                assert_eq!(ended.post_script, None);
                assert!(!ended.session_confirmed);
            }
            other => panic!("expected finalize, got {other:?}"),
        }
    }

    #[test]
    fn process_session_startup_barrier_does_not_mark_the_game() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        let token = session
            .begin_launch(LaunchMeta {
                game_id: "steam:440".into(),
            })
            .unwrap();
        let mut game = TrackedGame::new(90, "steam:440", 0);
        game.has_process_session = true;
        session.attach_spawn(token, game);
        let probe = StaticGameProbe::new(Presence {
            owned_group_alive: true,
            process_session_ready: false,
            ..Presence::default()
        });
        assert_eq!(session.tick(&probe), TickOutcome::Waiting);
        assert!(!session.is_running());
        probe.set(Presence {
            owned_group_alive: true,
            process_session_ready: true,
            ..Presence::default()
        });
        assert_eq!(session.tick(&probe), TickOutcome::Running);
        assert!(session.is_running());
    }

    struct FakeWindow {
        calls: Vec<&'static str>,
        destroyed: bool,
        minimized: bool,
        visible: bool,
        focus_steal_result: bool,
    }

    impl WindowFocusTarget for FakeWindow {
        fn is_destroyed(&self) -> bool {
            self.destroyed
        }
        fn is_minimized(&self) -> bool {
            self.minimized
        }
        fn is_visible(&self) -> bool {
            self.visible
        }
        fn restore(&mut self) {
            self.calls.push("restore");
            self.minimized = false;
        }
        fn show(&mut self) {
            self.calls.push("show");
            self.visible = true;
        }
        fn focus_steal(&mut self) -> bool {
            self.calls.push("focus-steal");
            self.focus_steal_result
        }
        fn focus_plain(&mut self) -> bool {
            self.calls.push("focus-plain");
            true
        }
        fn focus_web_contents(&mut self) -> bool {
            self.calls.push("webContents.focus");
            true
        }
    }

    #[test]
    fn window_focus_restores_hidden_and_minimized_window() {
        let mut target = FakeWindow {
            calls: Vec::new(),
            destroyed: false,
            minimized: true,
            visible: false,
            focus_steal_result: true,
        };
        let mut notified = 0;
        assert!(restore_window_focus(
            Some(&mut target),
            &|| true,
            &mut || notified += 1
        ));
        assert_eq!(
            target.calls,
            vec!["restore", "show", "focus-steal", "webContents.focus"]
        );
        assert_eq!(notified, 1);
    }

    #[test]
    fn window_focus_respects_destroyed_and_guard() {
        let mut target = FakeWindow {
            calls: Vec::new(),
            destroyed: true,
            minimized: false,
            visible: true,
            focus_steal_result: true,
        };
        assert!(!restore_window_focus(
            Some(&mut target),
            &|| true,
            &mut || panic!("must not notify")
        ));
        assert!(target.calls.is_empty());

        let mut target = FakeWindow {
            calls: Vec::new(),
            destroyed: false,
            minimized: false,
            visible: true,
            focus_steal_result: false,
        };
        assert!(!restore_window_focus(
            Some(&mut target),
            &|| false,
            &mut || panic!("must not notify")
        ));
        assert!(target.calls.is_empty());
    }

    #[test]
    fn gamescope_scene_publishes_synthetic_blur_once() {
        let clock = ManualClock::new(0);
        let mut session = GameSession::new(&clock);
        assert!(session.is_focused());
        assert!(session.focus_for_game_scene(true));
        assert!(!session.is_focused());
        assert!(!session.focus_for_game_scene(true));
        let events = session.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].channel(), "app:focus");
        assert_eq!(events[0].payload(), json!(false));
    }

    #[test]
    fn window_focus_falls_back_when_steal_fails() {
        let mut target = FakeWindow {
            calls: Vec::new(),
            destroyed: false,
            minimized: false,
            visible: true,
            focus_steal_result: false,
        };
        assert!(restore_window_focus(
            Some(&mut target),
            &|| true,
            &mut || {}
        ));
        assert_eq!(
            target.calls,
            vec!["focus-steal", "focus-plain", "webContents.focus"]
        );
    }
}
