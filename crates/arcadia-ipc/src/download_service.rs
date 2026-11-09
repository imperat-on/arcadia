//! Serviço de downloads do IPC: fila persistida (`dm:*`), runtime
//! Legendary/DepotDownloader e bridge torrent (`torrent:*`).
//!
//! O oráculo congelado é `app/electron/downloadmanager.js`, `torrent.js`,
//! `aria2.js`, `depotdownloader.js` e os handlers `dm:*`/`torrent:*` de
//! `main.js`. As regras puras (fila, integridade, depot, wire e progresso)
//! vivem em `arcadia-download`; este módulo é a borda que spawna processos,
//! persiste os dois estados e emite os eventos para a janela.
//!
//! Processos nunca passam por shell: `ProcessSpec` carrega programa e argv, e
//! `SystemRunner` usa `std::process::Command` diretamente. Testes usam
//! `FakeProcessRunner`, `MemoryPersistence` e um conector torrent roteirizado;
//! nenhum subprocesso real é iniciado.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};

use arcadia_download::aria2::{Aria2Client, HttpTransport};
use arcadia_download::depot::{
    depotdownloader_installed, dotnet_binary, prepare_download, sanitize_install_dir,
    DepotDownloadRequest, DepotFsProbe, DepotSpec, DLL_NAME, NO_MANIFESTS_ERROR,
};
use arcadia_download::integrity::{
    verification_command, verification_output_looks_failed, CleanupPlan,
};
use arcadia_download::manager::{
    DownloadExecutor, DownloadManager, DownloadPhase, DownloadRequest, DownloadSignal,
    DownloadsPersistence, FilePersistence, InstallOutcome, ManagerTransition, ProcessExit,
    StartOutcome,
};
use arcadia_download::queue::{
    next_queued, normalize_priority, PauseOutcome, ResumeOutcome, STATUS_ERROR, STATUS_QUEUED,
};
use arcadia_download::torrent::{
    merge_progress, normalize_download_uri, normalize_torrent_id, parse_state, serialize_state,
    worker_command, TorrentRpcClient, TorrentRpcError, TorrentTransport, TorrentTransportError,
};

/// Evento com a fila Epic/Steam inteira (payload `DmItem[]`).
pub const EVENT_DM_PROGRESS: &str = "dm:progress";
/// Evento com os downloads torrent (`payload` `TorrentItem[]`).
pub const EVENT_TORRENT_PROGRESS: &str = "torrent:progress";

/// Mesma mensagem de política do `torrent.js` (2026-09-12).
pub const ERRO_DEBRID: &str = "Sem debrid conectado — downloads por torrent exigem debrid.";
pub const ERRO_LIBTORRENT_WINDOWS: &str =
    "lista de arquivos indisponível no Windows (engine aria2)";
pub const ERRO_LIBTORRENT: &str = "libtorrent não instalado (sudo pacman -S libtorrent-rasterbar)";

pub type EventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;
pub type DoneSink = Arc<dyn Fn(Value) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(boolean)) => boolean.to_string(),
        _ => String::new(),
    }
}

/// Equivalente a `Boolean(value)` do JS para os argumentos de `torrent:start`.
fn truthy_of(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(boolean)) => *boolean,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

fn number_of(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(true)) => 1.0,
        Some(Value::Bool(false)) => 0.0,
        _ => 0.0,
    }
}

fn ok_envelope() -> Value {
    json!({ "ok": true })
}

fn queued_error(error: impl Into<String>) -> Value {
    json!({ "ok": false, "queued": true, "error": error.into() })
}

fn error_envelope(error: impl Into<String>) -> Value {
    json!({ "ok": false, "error": error.into() })
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn tail_chars(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max {
        return value.to_string();
    }
    value.chars().skip(count - max).collect()
}

// ---------------------------------------------------------------------------
// ProcessRunner
// ---------------------------------------------------------------------------

/// Programa + argv. Nunca existe string de shell aqui: cada argumento é um
/// item separado, como no `spawn(cmd, args)` do Electron.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

impl ProcessSpec {
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutcome {
    pub code: Option<i32>,
    /// Plataformas POSIX podem reportar sinal; no Windows o kill devolve
    /// `code=None`.
    pub signal: Option<String>,
    pub error: Option<String>,
}

impl ProcessOutcome {
    pub fn success() -> Self {
        Self {
            code: Some(0),
            ..Self::default()
        }
    }

    pub fn failure(code: i32) -> Self {
        Self {
            code: Some(code),
            ..Self::default()
        }
    }

    pub fn is_success(&self) -> bool {
        self.code == Some(0) && self.signal.is_none() && self.error.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Exited,
}

/// Controle interno de um processo. `SystemRunner` usa pipes/threads;
/// `FakeProcessRunner` usa uma fila roteirizada. Nenhum método interpola shell.
pub trait ProcessControl: Send {
    fn pid(&self) -> u32;
    fn write_stdin(&mut self, data: &[u8]) -> Result<(), String>;
    fn close_stdin(&mut self) -> Result<(), String>;
    fn try_read_stdout(&mut self) -> Result<Option<String>, String>;
    fn read_stdout_timeout(&mut self, timeout: Duration) -> Result<Option<String>, String>;
    fn try_read_stderr(&mut self) -> Result<Option<String>, String>;
    fn try_wait(&mut self) -> Result<Option<ProcessOutcome>, String>;
    fn wait_timeout(&mut self, timeout: Option<Duration>) -> Result<ProcessOutcome, String>;
    fn kill(&mut self) -> Result<(), String>;
    fn status(&mut self) -> ProcessState;
}

/// Handle clonável para um processo. As operações pedidas pelo contrato
/// (`wait`, `kill`, `status`) ficam disponíveis aqui e são reexportadas por
/// [`ProcessRunner`] para quem trata o runner como trait object.
#[derive(Clone)]
pub struct ProcessHandle {
    pid: u32,
    control: Arc<Mutex<Box<dyn ProcessControl>>>,
}

impl std::fmt::Debug for ProcessHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessHandle")
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

impl ProcessHandle {
    pub fn new(pid: u32, control: impl ProcessControl + 'static) -> Self {
        Self {
            pid,
            control: Arc::new(Mutex::new(Box::new(control))),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn write_stdin(&self, data: &[u8]) -> Result<(), String> {
        lock(&self.control).write_stdin(data)
    }

    pub fn close_stdin(&self) -> Result<(), String> {
        lock(&self.control).close_stdin()
    }

    pub fn try_read_line(&self) -> Result<Option<String>, String> {
        lock(&self.control).try_read_stdout()
    }

    pub fn read_line(&self, timeout: Duration) -> Result<Option<String>, String> {
        lock(&self.control).read_stdout_timeout(timeout)
    }

    pub fn try_read_stderr(&self) -> Result<Option<String>, String> {
        lock(&self.control).try_read_stderr()
    }

    pub fn drain_stdout(&self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(Some(line)) = self.try_read_line() {
            lines.push(line);
        }
        lines
    }

    pub fn drain_stderr(&self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(Some(line)) = self.try_read_stderr() {
            lines.push(line);
        }
        lines
    }

    pub fn try_wait(&self) -> Result<Option<ProcessOutcome>, String> {
        lock(&self.control).try_wait()
    }

    pub fn wait(&self, timeout: Option<Duration>) -> Result<ProcessOutcome, String> {
        lock(&self.control).wait_timeout(timeout)
    }

    pub fn kill(&self) -> Result<(), String> {
        lock(&self.control).kill()
    }

    pub fn status(&self) -> ProcessState {
        lock(&self.control).status()
    }
}

/// Spawn/wait/kill/status. Implementações reais só usam argv; a fake registra
/// as chamadas e devolve linhas roteirizadas.
pub trait ProcessRunner: Send + Sync {
    fn spawn(&self, spec: &ProcessSpec) -> Result<ProcessHandle, String>;

    fn wait(
        &self,
        handle: &ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessOutcome, String> {
        handle.wait(timeout)
    }

    fn kill(&self, handle: &ProcessHandle) -> Result<(), String> {
        handle.kill()
    }

    fn status(&self, handle: &ProcessHandle) -> ProcessState {
        handle.status()
    }
}

/// Runner de produção. Programa/argv vão direto ao SO; stdout/stderr são
/// lidos por threads e enfileirados para o serviço drenar sem bloquear.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

struct SystemProcess {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: Receiver<String>,
    stderr: Receiver<String>,
    outcome: Option<ProcessOutcome>,
    killed: bool,
}

fn spawn_line_reader(mut stream: impl Read + Send + 'static) -> Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(&mut stream);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    receiver
}

impl ProcessControl for SystemProcess {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn write_stdin(&mut self, data: &[u8]) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "stdin do processo fechado".to_string())?;
        stdin.write_all(data).map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())
    }

    fn close_stdin(&mut self) -> Result<(), String> {
        self.stdin.take();
        Ok(())
    }

    fn try_read_stdout(&mut self) -> Result<Option<String>, String> {
        match self.stdout.try_recv() {
            Ok(line) => Ok(Some(line)),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn read_stdout_timeout(&mut self, timeout: Duration) -> Result<Option<String>, String> {
        match self.stdout.recv_timeout(timeout) {
            Ok(line) => Ok(Some(line)),
            Err(RecvTimeoutError::Timeout) => Err("rpc_timeout".to_string()),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn try_read_stderr(&mut self) -> Result<Option<String>, String> {
        match self.stderr.try_recv() {
            Ok(line) => Ok(Some(line)),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn try_wait(&mut self) -> Result<Option<ProcessOutcome>, String> {
        if let Some(outcome) = &self.outcome {
            return Ok(Some(outcome.clone()));
        }
        match self.child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => {
                let outcome = ProcessOutcome {
                    code: status.code(),
                    signal: if status.code().is_none() && self.killed {
                        Some("SIGKILL".to_string())
                    } else {
                        None
                    },
                    error: None,
                };
                self.outcome = Some(outcome.clone());
                Ok(Some(outcome))
            }
            None => Ok(None),
        }
    }

    fn wait_timeout(&mut self, timeout: Option<Duration>) -> Result<ProcessOutcome, String> {
        let started = Instant::now();
        loop {
            if let Some(outcome) = self.try_wait()? {
                return Ok(outcome);
            }
            if let Some(timeout) = timeout {
                if started.elapsed() >= timeout {
                    return Err("process_timeout".to_string());
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn kill(&mut self) -> Result<(), String> {
        if self.outcome.is_some() {
            return Ok(());
        }
        self.killed = true;
        self.child.kill().map_err(|error| error.to_string())
    }

    fn status(&mut self) -> ProcessState {
        match self.try_wait() {
            Ok(Some(_)) => ProcessState::Exited,
            _ => ProcessState::Running,
        }
    }
}

impl ProcessRunner for SystemRunner {
    fn spawn(&self, spec: &ProcessSpec) -> Result<ProcessHandle, String> {
        if spec.program.trim().is_empty() {
            return Err("processo sem programa".to_string());
        }
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let pid = child.id();
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .map(spawn_line_reader)
            .ok_or_else(|| "stdout indisponível".to_string())?;
        let stderr = child
            .stderr
            .take()
            .map(spawn_line_reader)
            .ok_or_else(|| "stderr indisponível".to_string())?;
        Ok(ProcessHandle::new(
            pid,
            SystemProcess {
                child,
                stdin,
                stdout,
                stderr,
                outcome: None,
                killed: false,
            },
        ))
    }
}

/// Script roteirizado de um processo fake.
#[derive(Debug, Clone)]
pub struct FakeScript {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub outcome: ProcessOutcome,
    pub auto_exit: bool,
}

impl Default for FakeScript {
    fn default() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            outcome: ProcessOutcome::success(),
            auto_exit: true,
        }
    }
}

impl FakeScript {
    pub fn success() -> Self {
        Self::default()
    }

    pub fn failure(code: i32) -> Self {
        Self {
            outcome: ProcessOutcome::failure(code),
            ..Self::default()
        }
    }

    pub fn with_stdout<I, S>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.stdout = lines.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_stderr<I, S>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.stderr = lines.into_iter().map(Into::into).collect();
        self
    }

    pub fn keep_running(mut self) -> Self {
        self.auto_exit = false;
        self
    }
}

#[derive(Debug)]
struct FakeProcessShared {
    pid: u32,
    stdin: Vec<u8>,
    stdout: VecDeque<String>,
    stderr: VecDeque<String>,
    outcome: ProcessOutcome,
    auto_exit: bool,
    exited: bool,
    killed: bool,
}

struct FakeProcess {
    pid: u32,
    state: Arc<Mutex<FakeProcessShared>>,
}

impl ProcessControl for FakeProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn write_stdin(&mut self, data: &[u8]) -> Result<(), String> {
        lock(&self.state).stdin.extend_from_slice(data);
        Ok(())
    }

    fn close_stdin(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn try_read_stdout(&mut self) -> Result<Option<String>, String> {
        Ok(lock(&self.state).stdout.pop_front())
    }

    fn read_stdout_timeout(&mut self, timeout: Duration) -> Result<Option<String>, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(line) = lock(&self.state).stdout.pop_front() {
                return Ok(Some(line));
            }
            if lock(&self.state).exited || Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn try_read_stderr(&mut self) -> Result<Option<String>, String> {
        Ok(lock(&self.state).stderr.pop_front())
    }

    fn try_wait(&mut self) -> Result<Option<ProcessOutcome>, String> {
        let mut state = lock(&self.state);
        if state.killed {
            state.exited = true;
            return Ok(Some(ProcessOutcome {
                code: None,
                signal: Some("SIGKILL".to_string()),
                error: None,
            }));
        }
        if state.auto_exit || state.exited {
            state.exited = true;
            return Ok(Some(state.outcome.clone()));
        }
        Ok(None)
    }

    fn wait_timeout(&mut self, timeout: Option<Duration>) -> Result<ProcessOutcome, String> {
        let started = Instant::now();
        loop {
            if let Some(outcome) = self.try_wait()? {
                return Ok(outcome);
            }
            if let Some(timeout) = timeout {
                if started.elapsed() >= timeout {
                    return Err("process_timeout".to_string());
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn kill(&mut self) -> Result<(), String> {
        let mut state = lock(&self.state);
        state.killed = true;
        state.exited = true;
        Ok(())
    }

    fn status(&mut self) -> ProcessState {
        let mut state = lock(&self.state);
        if state.killed {
            state.exited = true;
            return ProcessState::Exited;
        }
        if state.auto_exit || state.exited {
            return ProcessState::Exited;
        }
        ProcessState::Running
    }
}

#[derive(Debug, Default)]
struct FakeRunnerState {
    specs: Vec<ProcessSpec>,
    pids: Vec<u32>,
    processes: HashMap<u32, Arc<Mutex<FakeProcessShared>>>,
    scripts: VecDeque<FakeScript>,
    next_pid: u32,
    fail_spawn: bool,
    auto_exit: bool,
    default_outcome: ProcessOutcome,
}

impl FakeRunnerState {
    fn register(&mut self, spec: &ProcessSpec) -> Arc<Mutex<FakeProcessShared>> {
        if self.next_pid == 0 {
            self.next_pid = 1000;
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        let (stdout, stderr, outcome, auto_exit) = match self.scripts.pop_front() {
            Some(script) => (
                script.stdout,
                script.stderr,
                script.outcome,
                script.auto_exit && self.auto_exit,
            ),
            None => (
                Vec::new(),
                Vec::new(),
                self.default_outcome.clone(),
                self.auto_exit,
            ),
        };
        let state = Arc::new(Mutex::new(FakeProcessShared {
            pid,
            stdin: Vec::new(),
            stdout: stdout.into(),
            stderr: stderr.into(),
            outcome,
            auto_exit,
            exited: false,
            killed: false,
        }));
        self.specs.push(spec.clone());
        self.pids.push(pid);
        self.processes.insert(pid, Arc::clone(&state));
        state
    }
}

/// Runner de teste: registra argv, enfileira stdout/stderr e nunca toca em
/// processos do SO.
#[derive(Debug, Clone)]
pub struct FakeProcessRunner {
    state: Arc<Mutex<FakeRunnerState>>,
}

impl Default for FakeProcessRunner {
    fn default() -> Self {
        let mut state = FakeRunnerState {
            next_pid: 1000,
            auto_exit: true,
            default_outcome: ProcessOutcome::success(),
            ..FakeRunnerState::default()
        };
        state.next_pid = 1000;
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl FakeProcessRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn_count(&self) -> usize {
        lock(&self.state).specs.len()
    }

    pub fn specs(&self) -> Vec<ProcessSpec> {
        lock(&self.state).specs.clone()
    }

    pub fn last_spec(&self) -> Option<ProcessSpec> {
        lock(&self.state).specs.last().cloned()
    }

    pub fn pids(&self) -> Vec<u32> {
        lock(&self.state).pids.clone()
    }

    pub fn last_pid(&self) -> Option<u32> {
        lock(&self.state).pids.last().copied()
    }

    pub fn set_auto_exit(&self, auto_exit: bool) {
        lock(&self.state).auto_exit = auto_exit;
    }

    pub fn set_fail_spawn(&self, fail: bool) {
        lock(&self.state).fail_spawn = fail;
    }

    pub fn set_default_outcome(&self, outcome: ProcessOutcome) {
        lock(&self.state).default_outcome = outcome;
    }

    pub fn queue_script(&self, script: FakeScript) {
        lock(&self.state).scripts.push_back(script);
    }

    pub fn push_stdout(&self, pid: u32, text: &str) {
        let state = lock(&self.state);
        let Some(process) = state.processes.get(&pid) else {
            return;
        };
        let mut process = lock(process);
        for line in text.split('\n') {
            if !line.is_empty() {
                process.stdout.push_back(line.to_string());
            }
        }
    }

    pub fn push_stderr(&self, pid: u32, text: &str) {
        let state = lock(&self.state);
        let Some(process) = state.processes.get(&pid) else {
            return;
        };
        let mut process = lock(process);
        for line in text.split('\n') {
            if !line.is_empty() {
                process.stderr.push_back(line.to_string());
            }
        }
    }

    pub fn finish(&self, pid: u32, outcome: ProcessOutcome) {
        let state = lock(&self.state);
        let Some(process) = state.processes.get(&pid) else {
            return;
        };
        let mut process = lock(process);
        process.outcome = outcome;
        process.auto_exit = true;
        process.exited = true;
    }

    pub fn stdin_bytes(&self, pid: u32) -> Vec<u8> {
        let state = lock(&self.state);
        state
            .processes
            .get(&pid)
            .map(|process| lock(process).stdin.clone())
            .unwrap_or_default()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn spawn(&self, spec: &ProcessSpec) -> Result<ProcessHandle, String> {
        let mut state = lock(&self.state);
        if state.fail_spawn {
            return Err("fake_spawn_failed".to_string());
        }
        let shared = state.register(spec);
        let pid = lock(&shared).pid;
        Ok(ProcessHandle::new(pid, FakeProcess { pid, state: shared }))
    }
}

// ---------------------------------------------------------------------------
// Configuração e caminhos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DownloadServiceConfig {
    pub data_dir: PathBuf,
    pub downloads_file: PathBuf,
    pub torrent_state_file: PathBuf,
    pub games_dir: PathBuf,
    pub legendary_bin: PathBuf,
    pub steam_dir: Option<PathBuf>,
    pub deps_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub dotnet_local: PathBuf,
    pub dotnet: String,
    pub python: String,
    pub torrent_worker: PathBuf,
    pub torrent_download_path: Option<PathBuf>,
    pub debrid_configured: bool,
    pub libtorrent_available: bool,
    pub slssteam_enabled: bool,
    pub download_cpu_cores: u32,
    pub max_active: usize,
    pub max_downloads: u32,
    pub steam_progress_poll: bool,
    pub immediate_retries: bool,
    pub event_throttle: Duration,
    pub done_grace: Duration,
    pub monitor_interval: Duration,
}

impl DownloadServiceConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let config = arcadia_store::config::read_config(&data_dir);
        let ext = if cfg!(windows) { ".exe" } else { "" };
        let steam_dir = config
            .get("steam_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                let home = arcadia_store::paths::home_dir();
                let roots = arcadia_steamstore::library::steam_base_candidates(&home);
                arcadia_steamstore::library::find_steam_base(
                    &roots,
                    &arcadia_steamstore::library::SystemFs,
                )
            });
        let torrent_download_path = config
            .get("torrent_download_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        Self {
            downloads_file: data_dir.join("downloads.json"),
            torrent_state_file: data_dir.join("torrent_state.json"),
            games_dir: data_dir.join("games"),
            legendary_bin: data_dir.join("runners").join(format!("legendary{ext}")),
            steam_dir,
            deps_dir: data_dir.join("bin").join("deps").join("depotdownloader"),
            tmp_dir: data_dir.join("bin").join("tmp"),
            dotnet_local: data_dir
                .join("bin")
                .join("dotnet")
                .join(format!("dotnet{ext}")),
            dotnet: format!("dotnet{ext}"),
            python: "python3".to_string(),
            torrent_worker: data_dir.join("torrent_rpc").join("main.py"),
            torrent_download_path,
            debrid_configured: arcadia_catalog::hosters::has_debrid_configured(&config),
            libtorrent_available: true,
            slssteam_enabled: true,
            download_cpu_cores: config
                .get("download_cpu_cores")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            max_active: 1,
            max_downloads: 20,
            steam_progress_poll: true,
            immediate_retries: false,
            event_throttle: Duration::from_millis(500),
            done_grace: Duration::from_secs(6),
            monitor_interval: Duration::from_millis(250),
            data_dir,
        }
    }

    pub fn with_steam_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.steam_dir = Some(path.into());
        self
    }

    pub fn with_torrent_worker(mut self, path: impl Into<PathBuf>) -> Self {
        self.torrent_worker = path.into();
        self
    }

    pub fn with_debrid(mut self, configured: bool) -> Self {
        self.debrid_configured = configured;
        self
    }

    pub fn with_event_throttle(mut self, throttle: Duration) -> Self {
        self.event_throttle = throttle;
        self
    }
}

/// Probe real de filesystem para o DepotDownloader (lstat + tamanho > 0).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDepotProbe;

impl DepotFsProbe for SystemDepotProbe {
    fn is_regular_non_empty_file(&self, path: &Path) -> bool {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                metadata.file_type().is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() > 0
            }
            Err(_) => false,
        }
    }
}

/// Adapta `&dyn DepotFsProbe` (trait object) às funções do crate que recebem
/// `impl DepotFsProbe`.
struct ProbeRef<'a>(&'a (dyn DepotFsProbe + Send + Sync));

impl DepotFsProbe for ProbeRef<'_> {
    fn is_regular_non_empty_file(&self, path: &Path) -> bool {
        self.0.is_regular_non_empty_file(path)
    }

    fn dotnet_runtimes(&self) -> Option<String> {
        self.0.dotnet_runtimes()
    }
}

// ---------------------------------------------------------------------------
// Executor Legendary/DepotDownloader (spawn apenas via ProcessRunner)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSpawn {
    pub appid: String,
    pub phase: DownloadPhase,
    pub program: String,
    pub args: Vec<String>,
}

struct ActiveProcess {
    handle: ProcessHandle,
    phase: DownloadPhase,
    last_error: String,
    verify_output: String,
    steam: bool,
    steam_total: f64,
    install_dir: String,
    percent_max: f64,
    last_disk_sample: Option<(Instant, u64)>,
}

struct ServiceExecutor {
    runner: Arc<dyn ProcessRunner>,
    config: Arc<DownloadServiceConfig>,
    active: Arc<Mutex<HashMap<String, ActiveProcess>>>,
    spawns: Arc<Mutex<Vec<RecordedSpawn>>>,
}

impl ServiceExecutor {
    fn legendary_install_spec(&self, item: &Value) -> Result<ProcessSpec, String> {
        let app_name = item
            .get("appName")
            .map(Some)
            .map(text_of)
            .unwrap_or_default();
        if app_name.trim().is_empty() {
            return Err("appName ausente".to_string());
        }
        let base = item
            .get("installPath")
            .map(Some)
            .map(text_of)
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| self.config.games_dir.to_string_lossy().into_owned());
        let mut args = vec![
            "install".to_string(),
            app_name,
            "--base-path".to_string(),
            base,
            "-y".to_string(),
        ];
        if self.config.download_cpu_cores > 0 {
            args.push("--max-workers".to_string());
            args.push(self.config.download_cpu_cores.to_string());
        }
        Ok(ProcessSpec::new(
            self.config.legendary_bin.to_string_lossy().into_owned(),
            args,
        ))
    }

    fn verification_spec(&self, item: &Value) -> Result<ProcessSpec, String> {
        let command = verification_command(
            item,
            &self.config.legendary_bin.to_string_lossy(),
            &self.config.games_dir.to_string_lossy(),
        )
        .ok_or_else(|| "verificação de integridade indisponível".to_string())?;
        Ok(ProcessSpec::new(command.cmd, command.args))
    }

    fn depot_spec(&self, item: &Value) -> Result<ProcessSpec, String> {
        let index = item
            .get("filaIdx")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let commands = item
            .get("fila")
            .and_then(Value::as_array)
            .ok_or_else(|| "fila de depots ausente".to_string())?;
        let command = commands
            .get(index)
            .ok_or_else(|| "depot atual fora da fila".to_string())?;
        let program = command
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !self.command_allowed(&program) {
            return Err(format!("comando de depot recusado: {program}"));
        }
        let args = command
            .get("args")
            .and_then(Value::as_array)
            .ok_or_else(|| "args de depot inválidos".to_string())?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "arg de depot não é string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ProcessSpec::new(program, args))
    }

    fn command_allowed(&self, program: &str) -> bool {
        let normalized = program.replace('\\', "/").to_ascii_lowercase();
        let configured = self.config.dotnet.replace('\\', "/").to_ascii_lowercase();
        let local = self
            .config
            .dotnet_local
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        normalized == configured
            || normalized == local
            || normalized.ends_with("/dotnet")
            || normalized.ends_with("/dotnet.exe")
            || normalized == "dotnet"
            || normalized == "dotnet.exe"
    }
}

impl DownloadExecutor for ServiceExecutor {
    fn ensure_ready(&mut self, item: &Value) -> Result<(), String> {
        if item.get("engine").and_then(Value::as_str) == Some("steam") {
            return Ok(());
        }
        if self
            .config
            .legendary_bin
            .to_string_lossy()
            .trim()
            .is_empty()
        {
            return Err("legendary indisponível".to_string());
        }
        Ok(())
    }

    fn spawn(&mut self, request: &DownloadRequest) -> Result<(), String> {
        let steam = request.item.get("engine").and_then(Value::as_str) == Some("steam");
        let phase = if request.phase == DownloadPhase::Download && steam {
            DownloadPhase::Depot
        } else {
            request.phase
        };
        let spec = match phase {
            DownloadPhase::Download => self.legendary_install_spec(&request.item)?,
            DownloadPhase::Verify => self.verification_spec(&request.item)?,
            DownloadPhase::Depot => self.depot_spec(&request.item)?,
        };
        let handle = self.runner.spawn(&spec)?;
        lock(&self.spawns).push(RecordedSpawn {
            appid: request.appid.clone(),
            phase,
            program: spec.program.clone(),
            args: spec.args.clone(),
        });
        let install_dir = request
            .item
            .get("installDir")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let steam_total = number_of(request.item.get("total"));
        lock(&self.active).insert(
            request.appid.clone(),
            ActiveProcess {
                handle,
                phase,
                last_error: String::new(),
                verify_output: String::new(),
                steam,
                steam_total,
                install_dir,
                percent_max: 0.0,
                last_disk_sample: None,
            },
        );
        Ok(())
    }

    fn signal(&mut self, appid: &str, signal: DownloadSignal) -> Result<(), String> {
        match signal {
            DownloadSignal::Resume => Ok(()),
            DownloadSignal::Pause | DownloadSignal::Kill => {
                if let Some(process) = lock(&self.active).remove(appid) {
                    let _ = self.runner.kill(&process.handle);
                }
                Ok(())
            }
        }
    }

    fn cleanup(&mut self, item: &Value, plan: CleanupPlan) {
        if plan.keep_partial {
            return;
        }
        cleanup_item_paths(item, &plan, &self.config);
    }
}

// ---------------------------------------------------------------------------
// Torrent bridge
// ---------------------------------------------------------------------------

/// Fornece um transporte JSON-lines para o worker torrent. Produção spawna o
/// Python via `ProcessRunner`; testes devolvem um transporte roteirizado.
pub trait TorrentConnector: Send + Sync {
    fn connect(&self) -> Result<Box<dyn TorrentTransport + Send>, String>;
}

struct ProcessTorrentTransport {
    handle: ProcessHandle,
}

impl TorrentTransport for ProcessTorrentTransport {
    fn send_line(&mut self, line: &str) -> Result<(), TorrentTransportError> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        self.handle
            .write_stdin(&bytes)
            .map_err(TorrentTransportError)
    }

    fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError> {
        self.handle
            .read_line(Duration::from_secs(120))
            .map_err(TorrentTransportError)
    }
}

impl Drop for ProcessTorrentTransport {
    fn drop(&mut self) {
        if self.handle.status() == ProcessState::Running {
            let _ = self.handle.kill();
        }
    }
}

struct BoxedTorrentTransport(Box<dyn TorrentTransport + Send>);

impl TorrentTransport for BoxedTorrentTransport {
    fn send_line(&mut self, line: &str) -> Result<(), TorrentTransportError> {
        self.0.send_line(line)
    }

    fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError> {
        self.0.recv_line()
    }
}

/// Conector de produção: `python3 <worker>` com stdio como transporte.
pub struct PythonTorrentConnector {
    runner: Arc<dyn ProcessRunner>,
    spec: ProcessSpec,
}

impl PythonTorrentConnector {
    pub fn new(runner: Arc<dyn ProcessRunner>, spec: ProcessSpec) -> Self {
        Self { runner, spec }
    }

    pub fn from_config(config: &DownloadServiceConfig, runner: Arc<dyn ProcessRunner>) -> Self {
        let command = worker_command(&config.python, &config.torrent_worker, None);
        Self::new(runner, ProcessSpec::new(command.cmd, command.args))
    }

    pub fn command(&self) -> &ProcessSpec {
        &self.spec
    }
}

impl TorrentConnector for PythonTorrentConnector {
    fn connect(&self) -> Result<Box<dyn TorrentTransport + Send>, String> {
        let handle = self.runner.spawn(&self.spec)?;
        Ok(Box::new(ProcessTorrentTransport { handle }))
    }
}

struct TorrentBridge {
    client: TorrentRpcClient<BoxedTorrentTransport>,
}

// ---------------------------------------------------------------------------
// DownloadService
// ---------------------------------------------------------------------------

struct ServiceCore<P: DownloadsPersistence> {
    manager: DownloadManager<P, ServiceExecutor>,
    torrent: Vec<Value>,
    last_dm_emit: String,
    last_dm_at: Instant,
    last_torrent_emit: String,
    last_torrent_at: Instant,
    retry_deadlines: HashMap<String, Instant>,
    done_deadlines: HashMap<String, Instant>,
}

/// Serviço único do app: fila `downloads.json`, estado `torrent_state.json`,
/// execução de Legendary/DepotDownloader e bridge torrent.
pub struct DownloadService<P: DownloadsPersistence + Send + 'static> {
    core: Arc<Mutex<ServiceCore<P>>>,
    config: Arc<DownloadServiceConfig>,
    runner: Arc<dyn ProcessRunner>,
    probe: Arc<dyn DepotFsProbe + Send + Sync>,
    connector: Arc<dyn TorrentConnector>,
    active: Arc<Mutex<HashMap<String, ActiveProcess>>>,
    spawns: Arc<Mutex<Vec<RecordedSpawn>>>,
    events: Arc<Mutex<Option<EventSink>>>,
    done_events: Arc<Mutex<Option<DoneSink>>>,
    bridge: Arc<Mutex<Option<TorrentBridge>>>,
    canceled_torrents: Arc<Mutex<HashSet<String>>>,
    pumping: Arc<AtomicBool>,
    monitor_running: Arc<AtomicBool>,
    monitor: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl<P: DownloadsPersistence + Send + 'static> Clone for DownloadService<P> {
    fn clone(&self) -> Self {
        Self {
            core: Arc::clone(&self.core),
            config: Arc::clone(&self.config),
            runner: Arc::clone(&self.runner),
            probe: Arc::clone(&self.probe),
            connector: Arc::clone(&self.connector),
            active: Arc::clone(&self.active),
            spawns: Arc::clone(&self.spawns),
            events: Arc::clone(&self.events),
            done_events: Arc::clone(&self.done_events),
            bridge: Arc::clone(&self.bridge),
            canceled_torrents: Arc::clone(&self.canceled_torrents),
            pumping: Arc::clone(&self.pumping),
            monitor_running: Arc::clone(&self.monitor_running),
            monitor: Arc::clone(&self.monitor),
        }
    }
}

impl DownloadService<FilePersistence> {
    /// Abre o serviço de produção em `<data_dir>/downloads.json`, usando
    /// `SystemRunner` e o conector Python.
    pub fn open(data_dir: impl Into<PathBuf>, config: DownloadServiceConfig) -> Self {
        let runner: Arc<dyn ProcessRunner> = Arc::new(SystemRunner);
        Self::open_with_runner(data_dir, config, runner)
    }

    pub fn open_with_runner(
        data_dir: impl Into<PathBuf>,
        config: DownloadServiceConfig,
        runner: Arc<dyn ProcessRunner>,
    ) -> Self {
        let data_dir = data_dir.into();
        let persistence = FilePersistence::new(data_dir.join("downloads.json"));
        let connector: Arc<dyn TorrentConnector> = Arc::new(PythonTorrentConnector::from_config(
            &config,
            Arc::clone(&runner),
        ));
        Self::with_persistence(
            persistence,
            config,
            runner,
            connector,
            Arc::new(SystemDepotProbe),
        )
    }

    /// Igual a [`open`](Self::open), mas também inicia o monitor de progresso.
    pub fn start(data_dir: impl Into<PathBuf>, config: DownloadServiceConfig) -> Self {
        let service = Self::open(data_dir, config);
        service.start_monitor();
        service
    }
}

impl<P: DownloadsPersistence + Send + 'static> DownloadService<P> {
    #[allow(clippy::too_many_arguments)]
    pub fn with_persistence(
        persistence: P,
        config: DownloadServiceConfig,
        runner: Arc<dyn ProcessRunner>,
        connector: Arc<dyn TorrentConnector>,
        probe: Arc<dyn DepotFsProbe + Send + Sync>,
    ) -> Self {
        let config = Arc::new(config);
        let active = Arc::new(Mutex::new(HashMap::new()));
        let spawns = Arc::new(Mutex::new(Vec::new()));
        let executor = ServiceExecutor {
            runner: Arc::clone(&runner),
            config: Arc::clone(&config),
            active: Arc::clone(&active),
            spawns: Arc::clone(&spawns),
        };
        let manager = DownloadManager::open(persistence, executor, config.max_active);
        let torrent = std::fs::read_to_string(&config.torrent_state_file)
            .map(|text| parse_state(&text))
            .unwrap_or_default();
        Self {
            core: Arc::new(Mutex::new(ServiceCore {
                manager,
                torrent,
                last_dm_emit: String::new(),
                last_dm_at: Instant::now(),
                last_torrent_emit: String::new(),
                last_torrent_at: Instant::now(),
                retry_deadlines: HashMap::new(),
                done_deadlines: HashMap::new(),
            })),
            config,
            runner,
            probe,
            connector,
            active,
            spawns,
            events: Arc::new(Mutex::new(None)),
            done_events: Arc::new(Mutex::new(None)),
            bridge: Arc::new(Mutex::new(None)),
            canceled_torrents: Arc::new(Mutex::new(HashSet::new())),
            pumping: Arc::new(AtomicBool::new(false)),
            monitor_running: Arc::new(AtomicBool::new(false)),
            monitor: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_event_sink(&self, sink: EventSink) {
        *lock(&self.events) = Some(sink);
    }

    pub fn clear_event_sink(&self) {
        *lock(&self.events) = None;
    }

    pub fn set_done_sink(&self, sink: DoneSink) {
        *lock(&self.done_events) = Some(sink);
    }

    pub fn config(&self) -> &DownloadServiceConfig {
        &self.config
    }

    pub fn spawns(&self) -> Vec<RecordedSpawn> {
        lock(&self.spawns).clone()
    }

    pub fn active_count(&self) -> usize {
        lock(&self.active).len()
    }

    /// Texto persistido da fila (usado nos testes de round-trip e no host).
    pub fn read_persisted(&self) -> Result<Option<String>, String> {
        let core = lock(&self.core);
        core.manager.persistence().load()
    }

    // -- fila / IPC dm:* ----------------------------------------------------

    /// `dm:queue` — `DmItem[]` com os campos obrigatórios normalizados.
    pub fn queue(&self) -> Vec<Value> {
        let core = lock(&self.core);
        core.manager.items().iter().map(dm_item).collect()
    }

    pub fn dm_queue(&self) -> Value {
        Value::Array(self.queue())
    }

    /// `dm:install` — só jogos Epic (`epic:<app_name>`).
    pub fn install(&self, payload: &Value) -> Value {
        let appid = text_of(payload.get("appid"));
        let Some(app_name) = appid.strip_prefix("epic:").map(str::trim) else {
            return error_envelope("não é um jogo Epic");
        };
        if app_name.is_empty() || app_name == appid {
            return error_envelope("não é um jogo Epic");
        }
        let install_path = {
            let raw = text_of(payload.get("installPath"));
            let path = if raw.trim().is_empty() {
                self.config.games_dir.clone()
            } else {
                PathBuf::from(raw)
            };
            if std::fs::create_dir_all(&path).is_err() {
                return error_envelope("falha ao criar a pasta de instalação");
            }
            path
        };
        let item = json!({
            "appid": appid,
            "appName": app_name,
            "title": text_of(payload.get("title")),
            "cover": text_of(payload.get("cover")),
            "status": STATUS_QUEUED,
            "percent": 0,
            "done": 0,
            "total": 0,
            "eta": "",
            "speed": 0,
            "error": "",
            "installPath": install_path.to_string_lossy(),
            "integrity": { "state": "pending", "method": "legendary-verify" },
            "recoveryAttempts": 0,
            "priority": normalize_priority(payload.get("priority").unwrap_or(&Value::Null)),
        });
        let outcome = {
            let mut core = lock(&self.core);
            core.manager.add(item)
        };
        match outcome {
            InstallOutcome::Added { .. } => {
                self.emit_dm(true);
                self.advance_queue();
                ok_envelope()
            }
            InstallOutcome::AlreadyQueued => {
                self.emit_dm(true);
                ok_envelope()
            }
            InstallOutcome::Invalid => error_envelope("jogo inválido"),
            InstallOutcome::EngineUnavailable(error) => error_envelope(error),
        }
    }

    pub fn dm_install(&self, payload: &Value) -> Value {
        self.install(payload)
    }

    /// `store:install` — Steam via DepotDownloader. O host deve bloquear antes
    /// quando o plugin SLSsteam estiver desligado; `slssteam_enabled` mantém o
    /// envelope do oráculo para chamadas diretas.
    pub fn install_steam(&self, payload: &Value) -> Value {
        if !self.config.slssteam_enabled {
            return json!({ "ok": false, "plugin": "slssteam" });
        }
        let raw_appid = text_of(payload.get("appid"));
        let appid = raw_appid
            .trim()
            .strip_prefix("steam:")
            .unwrap_or(raw_appid.trim())
            .to_string();
        if appid.is_empty() {
            return error_envelope("appid inválido");
        }
        let steam_dir = payload
            .get("steamDir")
            .map(Some)
            .map(text_of)
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| self.config.steam_dir.clone());
        let Some(steam_dir) = steam_dir else {
            return error_envelope("Steam não encontrada");
        };
        let installdir = sanitize_install_dir(&text_of(payload.get("installdir")));
        let common = steam_dir.join("steamapps").join("common");
        if std::fs::create_dir_all(&common).is_err() {
            return error_envelope("falha ao criar steamapps/common");
        }
        let id = format!("steam:{appid}");
        let item = json!({
            "appid": id,
            "appName": appid,
            "title": text_of(payload.get("title")),
            "cover": text_of(payload.get("cover")),
            "engine": "steam",
            "installdir": installdir,
            "depots": payload.get("depots").cloned().unwrap_or_else(|| json!([])),
            "token": text_of(payload.get("token")),
            "dlcs": payload.get("dlcs").cloned().unwrap_or_else(|| json!([])),
            "steamDir": steam_dir.to_string_lossy(),
            "status": STATUS_QUEUED,
            "percent": 0,
            "done": 0,
            "total": 0,
            "eta": "",
            "speed": 0,
            "error": "",
            "installPath": common.to_string_lossy(),
            "installDir": common.join(&installdir).to_string_lossy(),
            "integrity": { "state": "pending", "method": "depot-manifest" },
            "recoveryAttempts": 0,
            "priority": normalize_priority(payload.get("priority").unwrap_or(&Value::Null)),
        });
        let outcome = {
            let mut core = lock(&self.core);
            core.manager.add(item)
        };
        match outcome {
            InstallOutcome::Added { .. } => {
                self.emit_dm(true);
                self.advance_queue();
                ok_envelope()
            }
            InstallOutcome::AlreadyQueued => {
                self.emit_dm(true);
                ok_envelope()
            }
            InstallOutcome::Invalid => error_envelope("jogo inválido"),
            InstallOutcome::EngineUnavailable(error) => error_envelope(error),
        }
    }

    pub fn store_install(&self, payload: &Value) -> Value {
        self.install_steam(payload)
    }

    /// `dm:pause` — no Windows o oráculo mata o processo e mantém parciais.
    pub fn pause(&self, appid: &str) -> Value {
        let outcome = {
            let mut core = lock(&self.core);
            core.manager.pause(appid)
        };
        if outcome != PauseOutcome::NotFound {
            self.emit_dm(true);
        }
        Value::Null
    }

    pub fn dm_pause(&self, appid: &str) -> Value {
        self.pause(appid)
    }

    /// `dm:resume` — sem processo vivo volta à fila; o Legendary retoma os
    /// parciais do disco.
    pub fn resume(&self, appid: &str) -> Value {
        let live = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(appid).map(|process| process.handle.status()))
            .is_some_and(|status| status == ProcessState::Running);
        let outcome = {
            let mut core = lock(&self.core);
            core.manager.resume(appid, live)
        };
        match outcome {
            ResumeOutcome::ResumedProcess => self.emit_dm(true),
            ResumeOutcome::Requeued | ResumeOutcome::RequeuedNoSlot => {
                self.emit_dm(true);
                self.advance_queue();
            }
            ResumeOutcome::NotPaused | ResumeOutcome::NotFound => {}
        }
        Value::Null
    }

    pub fn dm_resume(&self, appid: &str) -> Value {
        self.resume(appid)
    }

    /// `dm:cancel` — mata o processo, remove o item e apaga parciais/pasta.
    pub fn cancel(&self, appid: &str) -> Value {
        let removed = {
            let mut core = lock(&self.core);
            core.done_deadlines.remove(appid);
            core.retry_deadlines.remove(appid);
            core.manager.cancel(appid)
        };
        if removed.is_some() {
            self.emit_dm(true);
            self.advance_queue();
        }
        Value::Null
    }

    pub fn dm_cancel(&self, appid: &str) -> Value {
        self.cancel(appid)
    }

    /// `dm:retry` — só itens em `error`; mantém destino/depots escolhidos.
    pub fn retry(&self, appid: &str) -> Value {
        let changed = {
            let mut core = lock(&self.core);
            core.retry_deadlines.remove(appid);
            core.manager.retry(appid)
        };
        if changed {
            self.emit_dm(true);
            self.advance_queue();
        }
        Value::Null
    }

    pub fn dm_retry(&self, appid: &str) -> Value {
        self.retry(appid)
    }

    /// `dm:dismiss` — remove item terminal (done/error/canceled), sem disco.
    pub fn dismiss(&self, appid: &str) -> Value {
        let changed = {
            let mut core = lock(&self.core);
            core.done_deadlines.remove(appid);
            core.retry_deadlines.remove(appid);
            core.manager.dismiss(appid)
        };
        if changed {
            self.emit_dm(true);
        }
        Value::Null
    }

    pub fn dm_dismiss(&self, appid: &str) -> Value {
        self.dismiss(appid)
    }

    /// `dm:setPriority` — devolve `true`/`false` como o handler.
    pub fn set_priority(&self, appid: &str, priority: &Value) -> Value {
        let changed = {
            let mut core = lock(&self.core);
            core.manager.set_priority(appid, priority)
        };
        if changed {
            self.emit_dm(true);
        }
        Value::Bool(changed)
    }

    pub fn dm_set_priority(&self, appid: &str, priority: &Value) -> Value {
        self.set_priority(appid, priority)
    }

    /// Limpeza de disco dos itens concluídos (equivalente aos 6s do oráculo).
    pub fn cleanup_done(&self) -> Value {
        let removed = {
            let mut core = lock(&self.core);
            core.done_deadlines.clear();
            core.manager.cleanup_done()
        };
        self.emit_dm(true);
        Value::Array(removed.into_iter().map(Value::String).collect())
    }

    pub fn dm_cleanup_done(&self) -> Value {
        self.cleanup_done()
    }

    /// `killActive` do shutdown: marca pausado antes de matar e derruba a
    /// bridge torrent.
    pub fn shutdown(&self) -> Value {
        self.stop_monitor();
        let _ = {
            let mut core = lock(&self.core);
            core.manager.shutdown_active()
        };
        let processes: Vec<ProcessHandle> = {
            let mut active = lock(&self.active);
            active.drain().map(|(_, process)| process.handle).collect()
        };
        for process in processes {
            let _ = self.runner.kill(&process);
        }
        *lock(&self.bridge) = None;
        self.emit_dm(true);
        self.emit_torrent(true);
        json!({ "ok": true })
    }

    // -- consultas/ferramentas Steam ----------------------------------------

    /// `store:ensureDotnet` — cópia local, runtime 9+ do PATH ou erro idêntico.
    pub fn ensure_dotnet(&self) -> Value {
        if self
            .probe
            .is_regular_non_empty_file(&self.config.dotnet_local)
        {
            return json!({ "ok": true, "path": self.config.dotnet_local.to_string_lossy() });
        }
        if self
            .probe
            .dotnet_runtimes()
            .is_some_and(|text| arcadia_download::depot::dotnet_runtime_compatible(&text))
        {
            return json!({ "ok": true, "path": self.config.dotnet });
        }
        let spec = ProcessSpec::new(self.config.dotnet.clone(), ["--list-runtimes"]);
        if let Ok(handle) = self.runner.spawn(&spec) {
            let _ = handle.wait(Some(Duration::from_secs(15)));
            let stdout = handle.drain_stdout().join("\n");
            if arcadia_download::depot::dotnet_runtime_compatible(&stdout) {
                return json!({ "ok": true, "path": self.config.dotnet });
            }
        }
        json!({ "ok": false, "error": arcadia_download::depot::DOTNET_ERROR })
    }

    pub fn store_ensure_dotnet(&self) -> Value {
        self.ensure_dotnet()
    }

    /// `store:ensureDepotDownloader` — valida os arquivos obrigatórios do
    /// runtime. O download automático do zip fica para o provedor do host.
    pub fn ensure_depotdownloader(&self) -> Value {
        if depotdownloader_installed(&self.config.deps_dir, &ProbeRef(&*self.probe)) {
            return json!({
                "ok": true,
                "path": self.config.deps_dir.join(DLL_NAME).to_string_lossy(),
            });
        }
        json!({ "ok": false, "error": "DepotDownloader indisponível" })
    }

    pub fn store_ensure_depotdownloader(&self) -> Value {
        self.ensure_depotdownloader()
    }

    // -- torrent / IPC torrent:* --------------------------------------------

    pub fn torrent_snapshot(&self) -> Vec<Value> {
        let core = lock(&self.core);
        core.torrent.iter().map(torrent_item).collect()
    }

    /// `torrent:list` — `{ ok, downloads }`.
    pub fn torrent_list(&self) -> Value {
        json!({ "ok": true, "downloads": self.torrent_snapshot() })
    }

    /// `torrent:start` — validação de ID/URI, política de debrid e start no
    /// worker Python (JSON-lines). Envelopes iguais ao `torrent.js`.
    pub fn torrent_start(&self, payload: &Value) -> Value {
        let game_id_raw = payload.get("gameId").cloned().unwrap_or(Value::Null);
        let url_raw = payload.get("url").cloned().unwrap_or(Value::Null);
        if !truthy_of(Some(&game_id_raw)) || !truthy_of(Some(&url_raw)) {
            return error_envelope("missing_args");
        }
        let stable_id = normalize_torrent_id(&game_id_raw);
        if stable_id.is_empty() {
            return error_envelope("invalid_game_id");
        }
        let safe_url = normalize_download_uri(&url_raw);
        if safe_url.is_empty() {
            return error_envelope("invalid_download_uri");
        }
        let save_path = {
            let raw = text_of(payload.get("savePath"));
            let path = if raw.trim().is_empty() {
                self.config
                    .torrent_download_path
                    .clone()
                    .unwrap_or_else(|| self.config.data_dir.join("downloads").join("torrent"))
            } else {
                PathBuf::from(raw)
            };
            if let Err(error) = std::fs::create_dir_all(&path) {
                return error_envelope(error.to_string());
            }
            path
        };
        let previous = {
            let core = lock(&self.core);
            core.torrent
                .iter()
                .find(|item| item.get("gameId").and_then(Value::as_str) == Some(&stable_id))
                .cloned()
        };
        let same_url = previous
            .as_ref()
            .and_then(|item| item.get("url"))
            .and_then(Value::as_str)
            == Some(safe_url.as_str());
        let mut base = if same_url {
            previous.clone().unwrap_or_else(|| json!({}))
        } else {
            json!({})
        };
        let object = base.as_object_mut().expect("base sempre é objeto");
        object.insert("gameId".to_string(), Value::String(stable_id.clone()));
        object.insert("url".to_string(), Value::String(safe_url.clone()));
        object.insert(
            "savePath".to_string(),
            Value::String(save_path.to_string_lossy().into_owned()),
        );
        object.insert(
            "fileIndices".to_string(),
            match payload.get("fileIndices") {
                Some(Value::Array(items)) => Value::Array(items.clone()),
                Some(_) => Value::Null,
                None => previous
                    .as_ref()
                    .and_then(|item| item.get("fileIndices").cloned())
                    .unwrap_or(Value::Null),
            },
        );
        let title = text_of(payload.get("title"));
        let cover = text_of(payload.get("cover"));
        object.insert(
            "title".to_string(),
            Value::String(if title.is_empty() {
                previous
                    .as_ref()
                    .and_then(|item| item.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            } else {
                title
            }),
        );
        object.insert(
            "cover".to_string(),
            Value::String(if cover.is_empty() {
                previous
                    .as_ref()
                    .and_then(|item| item.get("cover"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            } else {
                cover
            }),
        );
        object.insert("pausado".to_string(), Value::Bool(false));
        object.insert("completo".to_string(), Value::Bool(false));
        object.insert("erro".to_string(), Value::String(String::new()));
        if !same_url {
            for key in ["engine", "fileName", "fileSize", "cacheando"] {
                object.remove(key);
            }
        }
        lock(&self.canceled_torrents).remove(&stable_id);
        if safe_url.to_ascii_lowercase().starts_with("http") {
            object.insert("engine".to_string(), Value::String("http".to_string()));
            object.insert("cacheando".to_string(), Value::Bool(false));
            object.insert(
                "erro".to_string(),
                Value::String("download HTTP não portado".to_string()),
            );
            object.insert("pausado".to_string(), Value::Bool(true));
            self.upsert_torrent(base);
            return queued_error("download HTTP não portado");
        }
        if !self.config.debrid_configured {
            object.insert("erro".to_string(), Value::String(ERRO_DEBRID.to_string()));
            self.upsert_torrent(base);
            return queued_error(ERRO_DEBRID);
        }
        self.upsert_torrent(base);
        let file_indices = payload
            .get("fileIndices")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_i64).collect::<Vec<_>>());
        let result = self.torrent_rpc(|client| {
            client.start(
                &stable_id,
                &safe_url,
                &save_path.to_string_lossy(),
                file_indices.as_deref(),
                None,
            )
        });
        match result {
            Ok(()) => {
                self.patch_torrent(
                    &stable_id,
                    json!({ "erro": "", "pausado": false, "completo": false }),
                );
                self.emit_torrent(true);
                ok_envelope()
            }
            Err(error) => {
                self.mark_torrent_error(&stable_id, &error);
                queued_error(error)
            }
        }
    }

    pub fn torrent_pause(&self, game_id: &Value) -> Value {
        let Some(id) = self.normalized_torrent_arg(game_id) else {
            return error_envelope("invalid_game_id");
        };
        let current = {
            let core = lock(&self.core);
            core.torrent
                .iter()
                .find(|item| item.get("gameId").and_then(Value::as_str) == Some(&id))
                .cloned()
        };
        if let Some(current) = current {
            let engine = current
                .get("engine")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if current
                .get("erro")
                .and_then(Value::as_str)
                .is_some_and(|error| !error.is_empty())
                || matches!(engine, "http" | "debrid")
                || current
                    .get("cacheando")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                self.patch_torrent(&id, json!({ "pausado": true }));
                self.emit_torrent(true);
                return ok_envelope();
            }
        }
        if let Err(error) = self.torrent_rpc(|client| client.pause(&id)) {
            return error_envelope(error);
        }
        self.patch_torrent(&id, json!({ "pausado": true }));
        self.emit_torrent(true);
        ok_envelope()
    }

    pub fn torrent_resume(&self, game_id: &Value) -> Value {
        let Some(id) = self.normalized_torrent_arg(game_id) else {
            return error_envelope("invalid_game_id");
        };
        let item = {
            let core = lock(&self.core);
            core.torrent
                .iter()
                .find(|item| item.get("gameId").and_then(Value::as_str) == Some(&id))
                .cloned()
        };
        let Some(item) = item else {
            return error_envelope("download não encontrado");
        };
        if item.get("engine").and_then(Value::as_str) == Some("aria2")
            && !self.config.debrid_configured
        {
            return error_envelope(ERRO_DEBRID);
        }
        self.torrent_start(&item)
    }

    pub fn torrent_cancel(&self, game_id: &Value) -> Value {
        let Some(id) = self.normalized_torrent_arg(game_id) else {
            return error_envelope("invalid_game_id");
        };
        lock(&self.canceled_torrents).insert(id.clone());
        let item = {
            let core = lock(&self.core);
            core.torrent
                .iter()
                .find(|item| item.get("gameId").and_then(Value::as_str) == Some(&id))
                .cloned()
        };
        if let Some(item) = &item {
            let engine = item
                .get("engine")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(engine, "http" | "debrid")
                && !item
                    .get("cacheando")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                let _ = self.torrent_rpc(|client| client.cancel(&id));
            }
            if engine == "http" {
                remove_http_partials(item);
            }
        }
        {
            let mut core = lock(&self.core);
            core.torrent
                .retain(|entry| entry.get("gameId").and_then(Value::as_str) != Some(&id));
            self.write_torrent_state(&core.torrent);
        }
        self.emit_torrent(true);
        ok_envelope()
    }

    /// `torrent:files` — `{ ok, infoHash, name, totalSize, files }`.
    pub fn torrent_files(&self, payload: &Value) -> Value {
        let magnet = payload.get("magnet").cloned().unwrap_or(Value::Null);
        let safe = normalize_download_uri(&magnet);
        if safe.is_empty() || !safe.to_ascii_lowercase().starts_with("magnet:") {
            return error_envelope("invalid_download_uri");
        }
        if !self.config.libtorrent_available {
            return error_envelope(if cfg!(windows) {
                ERRO_LIBTORRENT_WINDOWS
            } else {
                ERRO_LIBTORRENT
            });
        }
        let timeout_ms = payload.get("timeoutMs").and_then(Value::as_i64);
        match self.torrent_rpc(|client| client.torrent_files(&safe, timeout_ms, None)) {
            Ok(files) => {
                let mut result = serde_json::to_value(files).unwrap_or_else(|_| json!({}));
                if let Some(object) = result.as_object_mut() {
                    object.insert("ok".to_string(), Value::Bool(true));
                }
                result
            }
            Err(error) => error_envelope(error),
        }
    }

    pub fn torrent_set_limit(&self, bytes: &Value) -> Value {
        let limit = bytes.as_u64().or_else(|| {
            bytes
                .as_f64()
                .filter(|value| *value >= 0.0)
                .map(|value| value as u64)
        });
        match self.torrent_rpc(|client| client.set_download_limit(limit)) {
            Ok(()) => ok_envelope(),
            Err(error) => error_envelope(error),
        }
    }

    /// Relê o estado vivo do worker para os itens ativos e emite
    /// `torrent:progress` quando algo mudou.
    pub fn torrent_poll(&self) {
        let (active, current) = {
            let core = lock(&self.core);
            let active = core.torrent.iter().any(torrent_is_active);
            (active, core.torrent.clone())
        };
        if !active {
            return;
        }
        let statuses = match self.torrent_rpc(|client| client.status()) {
            Ok(statuses) => statuses,
            Err(_) => return,
        };
        let canceled = lock(&self.canceled_torrents).clone();
        let mut changed = false;
        let mut next = current;
        let before = next.len();
        next.retain(|item| {
            item.get("gameId")
                .and_then(Value::as_str)
                .is_none_or(|id| !canceled.contains(id))
        });
        if next.len() != before {
            changed = true;
        }
        for item in &mut next {
            let Some(id) = item.get("gameId").and_then(Value::as_str) else {
                continue;
            };
            if !torrent_is_active(item) {
                continue;
            }
            if let Some(status) = statuses.get(id) {
                let merged = merge_progress(item, status);
                *item = merged;
                changed = true;
            }
        }
        if changed {
            let mut core = lock(&self.core);
            core.torrent = next;
            self.write_torrent_state(&core.torrent);
            let remaining: HashSet<String> = core
                .torrent
                .iter()
                .filter_map(|item| item.get("gameId").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            drop(core);
            lock(&self.canceled_torrents).retain(|id| remaining.contains(id));
        }
        self.emit_torrent(false);
    }

    /// `torrent.retomar()`: reenvia `start` para itens que estavam ativos.
    pub fn torrent_resume_all(&self) -> Value {
        let items: Vec<Value> = {
            let core = lock(&self.core);
            core.torrent
                .iter()
                .filter(|item| {
                    !item
                        .get("completo")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                        && !item
                            .get("pausado")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        && item
                            .get("erro")
                            .and_then(Value::as_str)
                            .is_none_or(str::is_empty)
                })
                .cloned()
                .collect()
        };
        for item in items {
            let _ = self.torrent_start(&item);
        }
        ok_envelope()
    }

    // -- dispatch opcional ---------------------------------------------------

    /// Roteia um canal exatamente como o host fará. Retorna `None` para canal
    /// não pertencente a este serviço.
    pub fn handle(&self, channel: &str, args: &[Value]) -> Option<Value> {
        let payload = args.first().cloned().unwrap_or_else(|| json!({}));
        match channel {
            "dm:queue" => Some(self.dm_queue()),
            "dm:install" => Some(self.dm_install(&payload)),
            "dm:pause" => {
                Some(self.dm_pause(args.first().and_then(Value::as_str).unwrap_or_default()))
            }
            "dm:resume" => {
                Some(self.dm_resume(args.first().and_then(Value::as_str).unwrap_or_default()))
            }
            "dm:cancel" => {
                Some(self.dm_cancel(args.first().and_then(Value::as_str).unwrap_or_default()))
            }
            "dm:retry" => {
                Some(self.dm_retry(args.first().and_then(Value::as_str).unwrap_or_default()))
            }
            "dm:dismiss" => {
                Some(self.dm_dismiss(args.first().and_then(Value::as_str).unwrap_or_default()))
            }
            "dm:setPriority" => Some(self.dm_set_priority(
                args.first().and_then(Value::as_str).unwrap_or_default(),
                args.get(1).unwrap_or(&Value::Null),
            )),
            "store:install" => Some(self.store_install(&payload)),
            "store:ensureDotnet" => Some(self.store_ensure_dotnet()),
            "store:ensureDepotDownloader" => Some(self.store_ensure_depotdownloader()),
            "torrent:start" => Some(self.torrent_start(&payload)),
            "torrent:pause" => Some(self.torrent_pause(&payload)),
            "torrent:resume" => Some(self.torrent_resume(&payload)),
            "torrent:cancel" => Some(self.torrent_cancel(&payload)),
            "torrent:files" => Some(self.torrent_files(&payload)),
            "torrent:setLimit" => Some(self.torrent_set_limit(&payload)),
            "torrent:list" => Some(self.torrent_list()),
            _ => None,
        }
    }

    // -- monitor/tick -------------------------------------------------------

    pub fn start_monitor(&self) {
        if self.monitor_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = self.clone();
        let interval = service.config.monitor_interval;
        let handle = thread::Builder::new()
            .name("arcadia-downloads".to_string())
            .spawn(move || {
                while service.monitor_running.load(Ordering::SeqCst) {
                    service.tick();
                    thread::sleep(interval);
                }
            })
            .ok();
        *lock(&self.monitor) = handle;
    }

    pub fn stop_monitor(&self) {
        self.monitor_running.store(false, Ordering::SeqCst);
        if let Some(handle) = lock(&self.monitor).take() {
            let _ = handle.join();
        }
    }

    pub fn monitor_running(&self) -> bool {
        self.monitor_running.load(Ordering::SeqCst)
    }

    /// Um passo do monitor: drena progresso, processa saídas, retries vencidos
    /// e a remoção de `done`.
    pub fn tick(&self) {
        self.pump();
        let now = Instant::now();
        let retry_due: Vec<String> = {
            let core = lock(&self.core);
            core.retry_deadlines
                .iter()
                .filter(|(_, deadline)| **deadline <= now)
                .map(|(appid, _)| appid.clone())
                .collect()
        };
        if !retry_due.is_empty() {
            {
                let mut core = lock(&self.core);
                for appid in &retry_due {
                    core.retry_deadlines.remove(appid);
                }
            }
            self.advance_queue();
        }
        let done_due: Vec<String> = {
            let core = lock(&self.core);
            core.done_deadlines
                .iter()
                .filter(|(_, deadline)| **deadline <= now)
                .map(|(appid, _)| appid.clone())
                .collect()
        };
        if !done_due.is_empty() {
            let mut core = lock(&self.core);
            for appid in &done_due {
                core.done_deadlines.remove(appid);
                core.manager.dismiss(appid);
            }
            drop(core);
            self.emit_dm(true);
        }
        self.torrent_poll();
    }

    /// Drena stdout/stderr de todos os processos e aplica saídas concluídas.
    pub fn pump(&self) {
        if self.pumping.swap(true, Ordering::SeqCst) {
            return;
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _reset = Reset(&self.pumping);

        let entries: Vec<(String, ProcessHandle, DownloadPhase)> = {
            let active = lock(&self.active);
            active
                .iter()
                .map(|(appid, process)| (appid.clone(), process.handle.clone(), process.phase))
                .collect()
        };
        for (appid, handle, phase) in entries {
            let stdout = handle.drain_stdout();
            let stderr = handle.drain_stderr();
            if !stdout.is_empty() || !stderr.is_empty() {
                self.apply_process_output(&appid, phase, &stdout, &stderr);
            }
            if self.config.steam_progress_poll {
                self.update_steam_disk_progress(&appid);
            }
            if handle.status() != ProcessState::Exited {
                continue;
            }
            let outcome = handle.wait(None).unwrap_or_else(|error| ProcessOutcome {
                code: None,
                signal: None,
                error: Some(error),
            });
            let stdout = handle.drain_stdout();
            let stderr = handle.drain_stderr();
            if !stdout.is_empty() || !stderr.is_empty() {
                self.apply_process_output(&appid, phase, &stdout, &stderr);
            }
            let Some(process) = lock(&self.active).remove(&appid) else {
                continue;
            };
            let Some(item) = ({
                let core = lock(&self.core);
                core.manager.queue().get(&appid).cloned()
            }) else {
                continue;
            };
            let error = if !process.last_error.is_empty() {
                Some(process.last_error)
            } else {
                outcome.error
            };
            let exit = ProcessExit {
                phase: process.phase,
                code: outcome.code,
                signal: outcome.signal,
                error,
                output_failed: process.phase == DownloadPhase::Verify
                    && verification_output_looks_failed(&process.verify_output),
                verified_at: Some(now_iso()),
            };
            let transition = {
                let mut core = lock(&self.core);
                core.manager.process_exit(&appid, exit)
            };
            if let Some(transition) = transition {
                self.after_transition(&appid, transition, true);
            }
            drop(item);
        }
        self.emit_dm(false);
    }

    fn apply_process_output(
        &self,
        appid: &str,
        phase: DownloadPhase,
        stdout: &[String],
        stderr: &[String],
    ) {
        let mut patch = Map::new();
        {
            let mut active = lock(&self.active);
            let Some(process) = active.get_mut(appid) else {
                return;
            };
            for line in stdout.iter().chain(stderr.iter()) {
                if is_error_line(line) && !line.trim().is_empty() {
                    process.last_error = truncate_chars(line.trim(), 300);
                }
                if phase == DownloadPhase::Verify {
                    process.verify_output.push_str(line);
                    process.verify_output.push('\n');
                    process.verify_output = tail_chars(&process.verify_output, 16_000);
                    continue;
                }
                if let Some(caps) = progress_regex().captures(line) {
                    add_number(&mut patch, "percent", &caps[1]);
                    add_number(&mut patch, "done", &caps[2]);
                    add_number(&mut patch, "total", &caps[3]);
                    patch.insert("eta".to_string(), Value::String(caps[4].to_string()));
                }
                if let Some(caps) = downloaded_regex().captures(line) {
                    add_number(&mut patch, "doneMiB", &caps[1]);
                }
                if let Some(caps) = speed_regex().captures(line) {
                    add_number(&mut patch, "speed", &caps[1]);
                }
                if let Some(caps) = install_path_regex().captures(line) {
                    let path = caps[1].trim();
                    if !path.is_empty() && process.install_dir.is_empty() {
                        process.install_dir = path.to_string();
                        patch.insert("installDir".to_string(), Value::String(path.to_string()));
                    }
                }
                if process.steam {
                    if let Some(caps) = depot_percent_regex().captures(line) {
                        if let Ok(value) = caps[1].parse::<f64>() {
                            let value = value.min(100.0);
                            if value > process.percent_max {
                                process.percent_max = value;
                                if process.steam_total <= 0.0 {
                                    patch.insert("percent".to_string(), json!(value));
                                }
                            }
                        }
                    }
                }
            }
        }
        if patch.is_empty() {
            return;
        }
        {
            let mut core = lock(&self.core);
            core.manager.update_progress(appid, &Value::Object(patch));
        }
        self.emit_dm(false);
    }

    fn update_steam_disk_progress(&self, appid: &str) {
        let (install_dir, total, should_sample) = {
            let mut active = lock(&self.active);
            let Some(process) = active.get_mut(appid) else {
                return;
            };
            if !process.steam || process.phase != DownloadPhase::Depot {
                return;
            }
            let now = Instant::now();
            let due = process
                .last_disk_sample
                .is_none_or(|(at, _)| now.duration_since(at) >= Duration::from_secs(3));
            if !due || process.install_dir.is_empty() {
                return;
            }
            let previous = process.last_disk_sample;
            process.last_disk_sample = Some((now, 0));
            (process.install_dir.clone(), process.steam_total, previous)
        };
        let bytes = dir_size_bytes(Path::new(&install_dir));
        let mib = bytes as f64 / (1024.0 * 1024.0);
        let mut patch = Map::new();
        patch.insert("done".to_string(), json!(mib.round()));
        if let Some((at, previous)) = should_sample {
            let seconds = Instant::now().duration_since(at).as_secs_f64().max(0.001);
            let speed =
                ((bytes.saturating_sub(previous)) as f64 / (1024.0 * 1024.0) / seconds).max(0.0);
            patch.insert("speed".to_string(), json!((speed * 10.0).round() / 10.0));
            if total > 0.0 {
                let percent = (mib / total * 1000.0).min(10_000.0).round() / 10.0;
                patch.insert("percent".to_string(), json!(percent));
                if speed > 0.1 {
                    patch.insert(
                        "eta".to_string(),
                        Value::String(fmt_eta((total - mib) / speed)),
                    );
                }
            }
        } else if total > 0.0 {
            let percent = (mib / total * 1000.0).min(10_000.0).round() / 10.0;
            patch.insert("percent".to_string(), json!(percent));
        }
        if let Some(process) = lock(&self.active).get_mut(appid) {
            process.last_disk_sample = Some((Instant::now(), bytes));
        }
        {
            let mut core = lock(&self.core);
            core.manager.update_progress(appid, &Value::Object(patch));
        }
        self.emit_dm(false);
    }

    fn after_transition(&self, appid: &str, transition: ManagerTransition, advance: bool) {
        match &transition {
            ManagerTransition::Done { .. } => {
                let item = {
                    let core = lock(&self.core);
                    core.manager
                        .queue()
                        .get(appid)
                        .cloned()
                        .map(|item| dm_item(&item))
                };
                {
                    let mut core = lock(&self.core);
                    core.done_deadlines
                        .insert(appid.to_string(), Instant::now() + self.config.done_grace);
                }
                if let Some(item) = item {
                    if let Some(sink) = lock(&self.done_events).clone() {
                        sink(item);
                    }
                }
            }
            ManagerTransition::Retry { delay_ms } if !self.config.immediate_retries => {
                let delay = Duration::from_millis((*delay_ms).max(0) as u64);
                lock(&self.core)
                    .retry_deadlines
                    .insert(appid.to_string(), Instant::now() + delay);
            }
            _ => {}
        }
        self.emit_dm(true);
        let advance = advance
            && !matches!(transition, ManagerTransition::Stopped)
            && (!matches!(transition, ManagerTransition::Retry { .. })
                || self.config.immediate_retries);
        if advance {
            self.advance_queue();
        }
    }

    /// Inicia o próximo item da fila (prioridade mais alta, FIFO no empate).
    /// Para Steam, prepara o keys file, o destino e a fila de comandos antes
    /// do spawn — comandos externos nunca são persistidos.
    fn advance_queue(&self) {
        for _ in 0..64 {
            let next = {
                let core = lock(&self.core);
                if !core.manager.has_free_slot() {
                    return;
                }
                next_queued(core.manager.items())
                    .and_then(|item| item.get("appid"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let Some(appid) = next else {
                return;
            };
            let item = {
                let core = lock(&self.core);
                core.manager.queue().get(&appid).cloned()
            };
            let Some(item) = item else {
                return;
            };
            if item.get("engine").and_then(Value::as_str) == Some("steam")
                && !item
                    .get("fila")
                    .and_then(Value::as_array)
                    .is_some_and(|commands| !commands.is_empty())
            {
                if let Err(error) = self.prepare_steam_item(&appid, &item) {
                    let integrity = json!({
                        "state": "failed",
                        "method": "depot-manifest",
                    });
                    {
                        let mut core = lock(&self.core);
                        core.manager.update(
                            &appid,
                            &json!({
                                "status": STATUS_ERROR,
                                "error": error,
                                "speed": 0,
                                "integrity": integrity,
                            }),
                        );
                    }
                    self.emit_dm(true);
                    continue;
                }
            }
            let outcome = {
                let mut core = lock(&self.core);
                core.manager.start_next()
            };
            match outcome {
                StartOutcome::Started { .. } => {
                    self.emit_dm(true);
                    return;
                }
                StartOutcome::NoSlot | StartOutcome::NoQueued => return,
                StartOutcome::SpawnFailed { transition, .. } => {
                    let stop = (matches!(transition, ManagerTransition::Retry { .. })
                        && !self.config.immediate_retries)
                        || matches!(transition, ManagerTransition::Stopped);
                    self.after_transition(&appid, transition, false);
                    if stop {
                        return;
                    }
                    continue;
                }
            }
        }
    }

    fn prepare_steam_item(&self, appid: &str, item: &Value) -> Result<(), String> {
        if !depotdownloader_installed(&self.config.deps_dir, &ProbeRef(&*self.probe)) {
            return Err("DepotDownloader indisponível".to_string());
        }
        let dotnet = self.ensure_dotnet();
        if dotnet.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = text_of(dotnet.get("error"));
            return Err(if error.trim().is_empty() {
                "dotnet indisponível".to_string()
            } else {
                error
            });
        }
        let depots_value = item
            .get("depots")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let depots: Vec<DepotSpec> = depots_value
            .iter()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect();
        if depots.is_empty() {
            return Err(NO_MANIFESTS_ERROR.to_string());
        }
        let steam_dir = item
            .get("steamDir")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| self.config.steam_dir.clone())
            .ok_or_else(|| "Steam não encontrada".to_string())?;
        let clean_appid = appid.strip_prefix("steam:").unwrap_or(appid);
        let dotnet = dotnet_binary(
            &self.config.dotnet_local,
            &self.config.dotnet,
            &ProbeRef(&*self.probe),
        );
        let mut request = DepotDownloadRequest::new(
            clean_appid,
            steam_dir,
            self.config.tmp_dir.clone(),
            self.config.deps_dir.clone(),
            dotnet,
        )
        .with_install_dir(text_of(item.get("installdir")))
        .with_depots(depots.clone());
        request.max_downloads = self.config.max_downloads;
        request.validate = true;
        let preparation = prepare_download(&request, &ProbeRef(&*self.probe))
            .map_err(|error| error.to_string())?;
        if let Some(parent) = preparation.keys_file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&preparation.keys_file, &preparation.keys_contents)
            .map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&preparation.dest).map_err(|error| error.to_string())?;
        let downloading: Vec<String> = preparation
            .commands
            .iter()
            .map(|command| command.depot_id.clone())
            .collect();
        let total_bytes: i64 = depots
            .iter()
            .filter(|depot| downloading.contains(&depot.depot_id))
            .map(|depot| depot.size.max(0))
            .sum();
        let commands: Vec<Value> = preparation
            .commands
            .iter()
            .map(|command| {
                json!({
                    "depotId": command.depot_id,
                    "cmd": command.cmd,
                    "args": command.args,
                })
            })
            .collect();
        let first = preparation
            .commands
            .first()
            .map(|command| command.depot_id.clone())
            .unwrap_or_default();
        let mut patch = json!({
            "fila": commands,
            "filaIdx": 0,
            "depotsOk": 0,
            "depotsFalhos": [],
            "depotsBaixando": downloading,
            "depotAtualId": first,
            "installDir": preparation.dest.to_string_lossy(),
        });
        let total_mib = total_bytes / (1024 * 1024);
        if total_mib > 0 {
            patch["total"] = json!(total_mib);
        }
        {
            let mut core = lock(&self.core);
            core.manager.update(appid, &patch);
        }
        Ok(())
    }

    // -- torrent internals --------------------------------------------------

    fn normalized_torrent_arg(&self, value: &Value) -> Option<String> {
        let id = normalize_torrent_id(value);
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    }

    fn upsert_torrent(&self, item: Value) {
        let id = item
            .get("gameId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        {
            let mut core = lock(&self.core);
            core.torrent
                .retain(|entry| entry.get("gameId").and_then(Value::as_str) != Some(id.as_str()));
            core.torrent.push(item);
            self.write_torrent_state(&core.torrent);
        }
        self.emit_torrent(true);
    }

    fn patch_torrent(&self, id: &str, patch: Value) {
        let mut core = lock(&self.core);
        if let Some(item) = core
            .torrent
            .iter_mut()
            .find(|item| item.get("gameId").and_then(Value::as_str) == Some(id))
        {
            if let (Some(object), Some(patch)) = (item.as_object_mut(), patch.as_object()) {
                for (key, value) in patch {
                    object.insert(key.clone(), value.clone());
                }
            }
            self.write_torrent_state(&core.torrent);
        }
    }

    fn mark_torrent_error(&self, id: &str, error: &str) {
        self.patch_torrent(
            id,
            json!({
                "erro": error,
                "pausado": true,
                "completo": false,
            }),
        );
        self.emit_torrent(true);
    }

    fn write_torrent_state(&self, items: &[Value]) {
        let text = serialize_state(items);
        if let Some(parent) = self.config.torrent_state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = arcadia_store::atomic::write_atomic_text(&self.config.torrent_state_file, &text);
    }

    fn torrent_rpc<T>(
        &self,
        operation: impl FnOnce(
            &mut TorrentRpcClient<BoxedTorrentTransport>,
        ) -> Result<T, TorrentRpcError>,
    ) -> Result<T, String> {
        let mut guard = lock(&self.bridge);
        if guard.is_none() {
            let transport = BoxedTorrentTransport(self.connector.connect()?);
            let mut client = TorrentRpcClient::new(transport);
            if let Err(error) = client.wait_ready() {
                return Err(torrent_error_text(&error));
            }
            *guard = Some(TorrentBridge { client });
        }
        let result = {
            let bridge = guard.as_mut().expect("bridge recém-criada");
            operation(&mut bridge.client)
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if error.code == "transport_error" {
                    *guard = None;
                }
                Err(torrent_error_text(&error))
            }
        }
    }

    // -- eventos ------------------------------------------------------------

    fn emit_dm(&self, force: bool) {
        let payload = {
            let mut core = lock(&self.core);
            let payload =
                Value::Array(core.manager.items().iter().map(dm_item).collect::<Vec<_>>());
            let text = payload.to_string();
            let now = Instant::now();
            if !force
                && (text == core.last_dm_emit
                    || now.duration_since(core.last_dm_at) < self.config.event_throttle)
            {
                return;
            }
            core.last_dm_emit = text;
            core.last_dm_at = now;
            payload
        };
        self.emit_event(EVENT_DM_PROGRESS, payload);
    }

    fn emit_torrent(&self, force: bool) {
        let payload = {
            let mut core = lock(&self.core);
            let payload = Value::Array(core.torrent.iter().map(torrent_item).collect::<Vec<_>>());
            let text = payload.to_string();
            let now = Instant::now();
            if !force
                && (text == core.last_torrent_emit
                    || now.duration_since(core.last_torrent_at) < self.config.event_throttle)
            {
                return;
            }
            core.last_torrent_emit = text;
            core.last_torrent_at = now;
            payload
        };
        self.emit_event(EVENT_TORRENT_PROGRESS, payload);
    }

    fn emit_event(&self, channel: &str, payload: Value) {
        let sink = lock(&self.events).clone();
        if let Some(sink) = sink {
            sink(channel, payload);
        }
    }
}

// ---------------------------------------------------------------------------
// Normalização de payloads da UI
// ---------------------------------------------------------------------------

fn dm_item(item: &Value) -> Value {
    let mut out = item.clone();
    let Some(object) = out.as_object_mut() else {
        return out;
    };
    let defaults = [
        ("appid", Value::String(String::new())),
        ("appName", Value::String(String::new())),
        ("title", Value::String(String::new())),
        ("cover", Value::String(String::new())),
        ("status", Value::String(STATUS_QUEUED.to_string())),
        ("percent", json!(0)),
        ("done", json!(0)),
        ("total", json!(0)),
        ("eta", Value::String(String::new())),
        ("speed", json!(0)),
        ("error", Value::String(String::new())),
    ];
    for (key, value) in defaults {
        object.entry(key.to_string()).or_insert(value);
    }
    out
}

fn torrent_item(item: &Value) -> Value {
    let mut out = item.clone();
    let Some(object) = out.as_object_mut() else {
        return out;
    };
    let defaults = [
        ("gameId", Value::String(String::new())),
        ("url", Value::String(String::new())),
        ("savePath", Value::String(String::new())),
        ("title", Value::String(String::new())),
        ("pausado", Value::Bool(false)),
        ("completo", Value::Bool(false)),
        ("erro", Value::String(String::new())),
        ("progress", json!(0)),
        ("downloadSpeed", json!(0)),
        ("uploadSpeed", json!(0)),
        ("numPeers", json!(0)),
        ("numSeeds", json!(0)),
        ("bytesDownloaded", json!(0)),
        ("fileSize", json!(0)),
    ];
    for (key, value) in defaults {
        object.entry(key.to_string()).or_insert(value);
    }
    out
}

fn torrent_is_active(item: &Value) -> bool {
    if item
        .get("completo")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || item
            .get("pausado")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || item
            .get("cacheando")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || item
            .get("erro")
            .and_then(Value::as_str)
            .is_some_and(|error| !error.is_empty())
    {
        return false;
    }
    !matches!(
        item.get("engine").and_then(Value::as_str),
        Some("http" | "debrid" | "aria2")
    )
}

fn torrent_error_text(error: &TorrentRpcError) -> String {
    if error.code == "transport_error" {
        error.message.clone()
    } else {
        error.code.clone()
    }
}

// ---------------------------------------------------------------------------
// Limpeza de disco segura
// ---------------------------------------------------------------------------

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn is_strict_child(target: &Path, root: &Path) -> bool {
    let target = normalized_path(target);
    let root = normalized_path(root);
    !root.is_empty()
        && target.len() > root.len()
        && target.starts_with(&root)
        && target.as_bytes().get(root.len()) == Some(&b'/')
}

fn remove_path(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        let _ = std::fs::remove_file(path);
    } else if metadata.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    }
}

fn slug_of(title: &str) -> String {
    title.chars().filter(char::is_ascii_alphanumeric).collect()
}

fn matching_install_dirs(base: &Path, title: &str) -> Vec<PathBuf> {
    let slug = slug_of(title);
    if slug.is_empty() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == slug {
                return Some(entry.path());
            }
            let rest = name.strip_prefix(&slug)?;
            if (4..=8).contains(&rest.chars().count())
                && rest
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect()
}

fn cleanup_item_paths(item: &Value, plan: &CleanupPlan, config: &DownloadServiceConfig) {
    let base = item
        .get("installPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| config.games_dir.clone());
    let steam_common = config
        .steam_dir
        .as_ref()
        .map(|steam| steam.join("steamapps").join("common"));
    let item_steam_common = item
        .get("steamDir")
        .and_then(Value::as_str)
        .map(|steam| PathBuf::from(steam).join("steamapps").join("common"));
    let mut roots: Vec<PathBuf> = vec![config.games_dir.clone(), base.clone()];
    if let Some(common) = &steam_common {
        roots.push(common.clone());
    }
    if let Some(common) = item_steam_common {
        roots.push(common);
    }

    let mut targets: Vec<PathBuf> = Vec::new();
    if plan.remove_partial {
        for key in ["partialPath", "partial_path"] {
            if let Some(path) = item.get(key).and_then(Value::as_str) {
                let path = PathBuf::from(path);
                if roots.iter().any(|root| is_strict_child(&path, root)) {
                    targets.push(path);
                }
            }
        }
    }
    if plan.remove_install_dir {
        for key in ["installDir", "install_dir"] {
            if let Some(path) = item.get(key).and_then(Value::as_str) {
                let path = PathBuf::from(path);
                if roots.iter().any(|root| is_strict_child(&path, root)) {
                    targets.push(path);
                }
            }
        }
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        targets.extend(matching_install_dirs(&base, title));
    }

    let mut seen = HashSet::new();
    for target in targets {
        let normalized = normalized_path(&target);
        if normalized.is_empty()
            || normalized == "/"
            || !seen.insert(normalized)
            || !roots.iter().any(|root| is_strict_child(&target, root))
        {
            continue;
        }
        remove_path(&target);
    }
}

fn remove_http_partials(item: &Value) {
    let save_path = item
        .get("savePath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let file_name = item
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if save_path.is_empty() || file_name.is_empty() {
        return;
    }
    let base = Path::new(save_path);
    let mut paths = vec![base.join(format!("{file_name}.part"))];
    for index in 0..8 {
        paths.push(base.join(format!("{file_name}.part{index}")));
    }
    for path in paths {
        if is_strict_child(&path, base) {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Diretório/ETA e regexes do Legendary/DepotDownloader
// ---------------------------------------------------------------------------

fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            total = total.saturating_add(dir_size_bytes(&entry.path()));
        } else if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}

fn fmt_eta(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

fn add_number(patch: &mut Map<String, Value>, key: &str, raw: &str) {
    if let Ok(value) = raw.parse::<f64>() {
        patch.insert(key.to_string(), json!(value));
    }
}

fn is_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "unable",
        "aborting",
        "401",
        "403",
        "denied",
        "not completely",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn progress_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)Progress:\s*([\d.]+)%\s*\(([\d.]+)/([\d.]+)\).*?ETA:?\s*([\d:]+)")
            .expect("regex de progresso")
    })
}

fn downloaded_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)Downloaded:\s*([\d.]+)\s*MiB").expect("regex de downloaded")
    })
}

fn speed_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)Download speed:\s*([\d.]+)\s*MiB/s").expect("regex de velocidade")
    })
}

fn install_path_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?im)Install path:\s*(.+)$").expect("regex de install path"))
}

fn depot_percent_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(\d{1,3}(?:\.\d+)?)%").expect("regex de percentual do depot"))
}

// ---------------------------------------------------------------------------
// aria2 (executor dormente)
// ---------------------------------------------------------------------------

/// argv do daemon aria2c — idêntico ao `aria2.js`, sem shell.
pub fn aria2_daemon_spec(
    program: impl Into<String>,
    work_dir: &Path,
    port: u16,
    secret: &str,
) -> ProcessSpec {
    let mut args = vec![
        "--enable-rpc=true".to_string(),
        "--rpc-listen-all=false".to_string(),
        format!("--rpc-listen-port={port}"),
        format!("--rpc-secret={secret}"),
        "--quiet=true".to_string(),
        "--console-log-level=warn".to_string(),
        "--summary-interval=0".to_string(),
        "--file-allocation=none".to_string(),
        "--continue=true".to_string(),
        "--allow-overwrite=true".to_string(),
        "--auto-file-renaming=false".to_string(),
        "--seed-time=0".to_string(),
        "--check-integrity=true".to_string(),
        "--no-conf=true".to_string(),
        "--dht-entry-point=dht.libtorrent.org:25401".to_string(),
    ];
    if !work_dir.as_os_str().is_empty() {
        args.push(format!(
            "--dht-file-path={}",
            work_dir.join("dht4.dat").display()
        ));
        args.push(format!(
            "--dht-file-path6={}",
            work_dir.join("dht6.dat").display()
        ));
        args.push(format!("--log={}", work_dir.join("aria2.log").display()));
        args.push("--log-level=warn".to_string());
    }
    ProcessSpec::new(program, args)
}

/// Executor mínimo do daemon aria2c: spawn pelo `ProcessRunner` e RPC JSON
/// local. Fica disponível para o host ligar no engine de torrent no futuro;
/// o caminho Python é o usado por `torrent:*` neste módulo.
pub struct Aria2Executor {
    runner: Arc<dyn ProcessRunner>,
    spec: ProcessSpec,
    client: Aria2Client<HttpTransport>,
    work_dir: PathBuf,
}

impl Aria2Executor {
    pub fn new(
        runner: Arc<dyn ProcessRunner>,
        program: impl Into<String>,
        work_dir: impl Into<PathBuf>,
        port: u16,
        secret: impl Into<String>,
    ) -> Self {
        let work_dir = work_dir.into();
        let secret = secret.into();
        let spec = aria2_daemon_spec(program, &work_dir, port, &secret);
        let client = Aria2Client::new(
            format!("http://127.0.0.1:{port}/jsonrpc"),
            secret,
            HttpTransport::new(Duration::from_secs(3)),
        );
        Self {
            runner,
            spec,
            client,
            work_dir,
        }
    }

    pub fn command(&self) -> &ProcessSpec {
        &self.spec
    }

    pub fn client(&self) -> &Aria2Client<HttpTransport> {
        &self.client
    }

    pub fn spawn(&self) -> Result<ProcessHandle, String> {
        let _ = std::fs::create_dir_all(&self.work_dir);
        self.runner.spawn(&self.spec)
    }

    pub fn wait_ready(&self, handle: &ProcessHandle, timeout: Duration) -> Result<(), String> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if handle.status() == ProcessState::Exited {
                return Err("aria2 não subiu".to_string());
            }
            if self.client.get_version().is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(250));
        }
        Err("aria2 não subiu".to_string())
    }

    pub fn stop(&self, handle: &ProcessHandle) {
        let _ = self.runner.kill(handle);
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod download_service_tests {
    use super::*;
    use arcadia_download::manager::MemoryPersistence;
    use arcadia_download::queue::{STATUS_DONE, STATUS_DOWNLOADING, STATUS_PAUSED};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "arcadia-download-service-{tag}-{}-{suffix}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn config(dir: &Path) -> DownloadServiceConfig {
        let mut config = DownloadServiceConfig::new(dir);
        config.games_dir = dir.join("games");
        config.legendary_bin = dir.join("runners").join("legendary.exe");
        config.steam_dir = Some(dir.join("Steam"));
        config.deps_dir = dir.join("deps").join("depotdownloader");
        config.tmp_dir = dir.join("tmp");
        config.dotnet_local = dir.join("dotnet").join("dotnet.exe");
        config.dotnet = "dotnet".to_string();
        config.python = "python3".to_string();
        config.torrent_worker = dir.join("torrent_rpc").join("main.py");
        config.torrent_download_path = Some(dir.join("torrents"));
        config.downloads_file = dir.join("downloads.json");
        config.torrent_state_file = dir.join("torrent_state.json");
        config.debrid_configured = true;
        config.slssteam_enabled = true;
        config.max_active = 1;
        config.max_downloads = 20;
        config.steam_progress_poll = false;
        config.immediate_retries = true;
        config.event_throttle = Duration::ZERO;
        config.done_grace = Duration::ZERO;
        config.monitor_interval = Duration::from_millis(5);
        config
    }

    #[derive(Debug, Clone, Default)]
    struct TestProbe {
        files: bool,
        runtimes: Option<String>,
    }

    impl DepotFsProbe for TestProbe {
        fn is_regular_non_empty_file(&self, _path: &Path) -> bool {
            self.files
        }

        fn dotnet_runtimes(&self) -> Option<String> {
            self.runtimes.clone()
        }
    }

    struct ScriptedTransport {
        lines: Arc<Mutex<VecDeque<String>>>,
    }

    impl TorrentTransport for ScriptedTransport {
        fn send_line(&mut self, _line: &str) -> Result<(), TorrentTransportError> {
            Ok(())
        }

        fn recv_line(&mut self) -> Result<Option<String>, TorrentTransportError> {
            Ok(lock(&self.lines).pop_front())
        }
    }

    struct ScriptedConnector {
        lines: Arc<Mutex<VecDeque<String>>>,
        connections: AtomicU64,
    }

    impl ScriptedConnector {
        fn new(lines: Vec<Value>) -> Self {
            Self {
                lines: Arc::new(Mutex::new(
                    lines.into_iter().map(|value| value.to_string()).collect(),
                )),
                connections: AtomicU64::new(0),
            }
        }

        fn push(&self, value: Value) {
            lock(&self.lines).push_back(value.to_string());
        }

        fn connections(&self) -> u64 {
            self.connections.load(Ordering::SeqCst)
        }
    }

    impl TorrentConnector for ScriptedConnector {
        fn connect(&self) -> Result<Box<dyn TorrentTransport + Send>, String> {
            self.connections.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(ScriptedTransport {
                lines: Arc::clone(&self.lines),
            }))
        }
    }

    struct Fixture {
        dir: PathBuf,
        config: DownloadServiceConfig,
        runner: Arc<FakeProcessRunner>,
        connector: Arc<ScriptedConnector>,
        probe: Arc<dyn DepotFsProbe + Send + Sync>,
        events: Arc<Mutex<Vec<(String, Value)>>>,
        done: Arc<Mutex<Vec<Value>>>,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            Self::with_probe(tag, TestProbe::default())
        }

        fn with_probe(tag: &str, probe: TestProbe) -> Self {
            let dir = temp_dir(tag);
            let config = config(&dir);
            let runner = Arc::new(FakeProcessRunner::new());
            let connector = Arc::new(ScriptedConnector::new(Vec::new()));
            let events = Arc::new(Mutex::new(Vec::new()));
            let done = Arc::new(Mutex::new(Vec::new()));
            Self {
                dir,
                config,
                runner,
                connector,
                probe: Arc::new(probe),
                events,
                done,
            }
        }

        fn open(&self) -> DownloadService<MemoryPersistence> {
            let service = DownloadService::with_persistence(
                MemoryPersistence::default(),
                self.config.clone(),
                Arc::clone(&self.runner) as Arc<dyn ProcessRunner>,
                Arc::clone(&self.connector) as Arc<dyn TorrentConnector>,
                Arc::clone(&self.probe),
            );
            service.set_event_sink(Arc::new({
                let events = Arc::clone(&self.events);
                move |channel, payload| {
                    lock(&events).push((channel.to_string(), payload));
                }
            }));
            service.set_done_sink(Arc::new({
                let done = Arc::clone(&self.done);
                move |item| lock(&done).push(item)
            }));
            service
        }

        fn open_with_persistence(
            &self,
            persistence: MemoryPersistence,
        ) -> DownloadService<MemoryPersistence> {
            DownloadService::with_persistence(
                persistence,
                self.config.clone(),
                Arc::clone(&self.runner) as Arc<dyn ProcessRunner>,
                Arc::clone(&self.connector) as Arc<dyn TorrentConnector>,
                Arc::clone(&self.probe),
            )
        }

        fn last_dm_event(&self) -> Option<Value> {
            lock(&self.events)
                .iter()
                .rev()
                .find(|(channel, _)| channel == EVENT_DM_PROGRESS)
                .map(|(_, payload)| payload.clone())
        }

        fn last_torrent_event(&self) -> Option<Value> {
            lock(&self.events)
                .iter()
                .rev()
                .find(|(channel, _)| channel == EVENT_TORRENT_PROGRESS)
                .map(|(_, payload)| payload.clone())
        }

        fn cleanup(self) {
            let _ = std::fs::remove_dir_all(self.dir);
        }
    }

    fn epic_payload(dir: &Path, appid: &str) -> Value {
        json!({
            "appid": appid,
            "title": "Fortnite",
            "cover": "cover.png",
            "installPath": dir.join("games").to_string_lossy(),
            "priority": 0,
        })
    }

    fn magnet() -> String {
        "magnet:?xt=urn:btih:481B6E3617BE4C88F96CB25E47C9D8272130071E&dn=repack".to_string()
    }

    #[test]
    fn download_service_install_snapshot_and_legendary_argv() {
        let fixture = Fixture::new("install");
        let service = fixture.open();
        let response = service.install(&epic_payload(&fixture.dir, "epic:Fortnite"));
        assert_eq!(response, ok_envelope());

        let queue = service.queue();
        assert_eq!(queue.len(), 1);
        let item = &queue[0];
        assert_eq!(item["appid"], "epic:Fortnite");
        assert_eq!(item["appName"], "Fortnite");
        assert_eq!(item["status"], STATUS_DOWNLOADING);
        assert_eq!(item["percent"], 0.0);
        assert_eq!(item["done"], 0.0);
        assert_eq!(item["total"], 0.0);
        assert_eq!(item["eta"], "");
        assert_eq!(item["speed"], 0.0);
        assert_eq!(item["error"], "");
        assert!(item["installPath"].as_str().is_some());

        let spec = fixture.runner.last_spec().expect("spawn do legendary");
        assert_eq!(spec.program, fixture.config.legendary_bin.to_string_lossy());
        assert_eq!(spec.args[0], "install");
        assert_eq!(spec.args[1], "Fortnite");
        assert_eq!(spec.args[2], "--base-path");
        assert_eq!(spec.args[4], "-y");

        let event = fixture.last_dm_event().expect("dm:progress");
        assert_eq!(event.as_array().unwrap().len(), 1);

        // Duplicata ativa não cria segundo card nem segundo processo.
        assert_eq!(
            service.install(&epic_payload(&fixture.dir, "epic:Fortnite")),
            ok_envelope()
        );
        assert_eq!(service.queue().len(), 1);
        assert_eq!(fixture.runner.spawn_count(), 1);
        assert!(!service
            .install(&json!({"appid": "Fortnite"}))
            .get("ok")
            .unwrap()
            .as_bool()
            .unwrap());
        fixture.cleanup();
    }

    #[test]
    fn download_service_progress_mapping_updates_dm_item() {
        let fixture = Fixture::new("progress");
        fixture.runner.set_auto_exit(false);
        let service = fixture.open();
        assert_eq!(
            service.install(&epic_payload(&fixture.dir, "epic:Fortnite")),
            ok_envelope()
        );
        let pid = fixture.runner.last_pid().unwrap();
        fixture.runner.push_stdout(
            pid,
            "[DLManager] INFO: = Progress: 15.13% (1500/761), Running for 00:05:32, ETA: 00:02:11\n\
             [DLManager] INFO:  - Downloaded: 150.20 MiB, Written: 300.10 MiB\n\
             [DLManager] INFO:  + Download speed: 15.23 MiB/s\n\
             [cli] INFO: Install path: C:/games/Fortnite\n",
        );
        service.pump();

        let item = &service.queue()[0];
        assert_eq!(item["percent"], 15.13);
        assert_eq!(item["done"], 1500.0);
        assert_eq!(item["total"], 761.0);
        assert_eq!(item["eta"], "00:02:11");
        assert_eq!(item["doneMiB"], 150.2);
        assert_eq!(item["speed"], 15.23);
        assert_eq!(item["installDir"], "C:/games/Fortnite");
        assert_eq!(item["status"], STATUS_DOWNLOADING);

        let event = fixture.last_dm_event().unwrap();
        assert_eq!(event[0]["percent"], 15.13);
        fixture.cleanup();
    }

    #[test]
    fn download_service_completes_epic_through_verify_and_done_sink() {
        let fixture = Fixture::new("done");
        let service = fixture.open();
        assert_eq!(
            service.install(&epic_payload(&fixture.dir, "epic:Fortnite")),
            ok_envelope()
        );
        service.pump();
        let spawns = fixture.runner.specs();
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[1].args[0], "verify");
        assert_eq!(spawns[1].args[1], "Fortnite");
        assert_eq!(service.queue()[0]["integrity"]["state"], "verifying");

        service.pump();
        let item = &service.queue()[0];
        assert_eq!(item["status"], STATUS_DONE);
        assert_eq!(item["integrity"]["state"], "verified");
        assert_eq!(item["integrity"]["method"], "legendary-verify");
        assert_eq!(lock(&fixture.done).len(), 1);
        assert_eq!(lock(&fixture.done)[0]["appid"], "epic:Fortnite");

        let removed = service.cleanup_done();
        assert_eq!(removed, json!(["epic:Fortnite"]));
        assert!(service.queue().is_empty());
        let persisted = service.read_persisted().unwrap().unwrap();
        assert!(persisted.trim() == "[]");
        fixture.cleanup();
    }

    #[test]
    fn download_service_pause_resume_cancel_and_disk_cleanup() {
        let fixture = Fixture::new("pause");
        let service = fixture.open();
        let payload = epic_payload(&fixture.dir, "epic:Fortnite");
        let install_path = PathBuf::from(payload["installPath"].as_str().unwrap());
        service.install(&payload);
        assert_eq!(service.queue()[0]["status"], STATUS_DOWNLOADING);

        service.pause("epic:Fortnite");
        assert_eq!(service.queue()[0]["status"], STATUS_PAUSED);
        assert_eq!(service.queue()[0]["speed"], 0.0);
        assert_eq!(service.active_count(), 0);

        service.resume("epic:Fortnite");
        assert_eq!(service.queue()[0]["status"], STATUS_DOWNLOADING);
        assert_eq!(fixture.runner.spawn_count(), 2);

        let game_dir = install_path.join("Fortnite");
        std::fs::create_dir_all(&game_dir).unwrap();
        std::fs::write(game_dir.join("data.bin"), b"partial").unwrap();
        service.cancel("epic:Fortnite");
        assert!(service.queue().is_empty());
        assert!(!game_dir.exists());
        assert_eq!(service.active_count(), 0);
        fixture.cleanup();
    }

    #[test]
    fn download_service_retry_dismiss_and_auto_recovery_errors() {
        let fixture = Fixture::new("retry");
        let error_item = |appid: &str| {
            json!({
                "appid": appid,
                "appName": appid,
                "title": appid,
                "status": "error",
                "error": "boom",
                "installPath": fixture.dir.join("games").to_string_lossy(),
            })
        };
        let persistence = MemoryPersistence::new(Some(
            json!([error_item("epic:BrokenA"), error_item("epic:BrokenB")]).to_string(),
        ));
        let service = fixture.open_with_persistence(persistence);
        assert_eq!(service.queue().len(), 2);

        service.dismiss("epic:BrokenA");
        assert_eq!(service.queue().len(), 1);
        service.retry("epic:BrokenB");
        assert_eq!(service.queue()[0]["status"], STATUS_DOWNLOADING);
        assert_eq!(service.queue()[0]["error"], "");
        assert_eq!(fixture.runner.spawn_count(), 1);

        // Falhas repetidas seguem a recuperação do oráculo até `error`.
        let fixture2 = Fixture::new("recovery");
        fixture2
            .runner
            .set_default_outcome(ProcessOutcome::failure(1));
        let service2 = fixture2.open();
        service2.install(&epic_payload(&fixture2.dir, "epic:Fortnite"));
        for _ in 0..4 {
            service2.pump();
        }
        let item = &service2.queue()[0];
        assert_eq!(item["status"], STATUS_ERROR);
        assert_eq!(item["integrity"]["state"], "failed");
        assert!(!item["error"].as_str().unwrap().is_empty());
        fixture.cleanup();
        fixture2.cleanup();
    }

    #[test]
    fn download_service_priority_reorders_queue() {
        let fixture = Fixture::new("priority");
        fixture.runner.set_auto_exit(false);
        let service = fixture.open();
        service.install(&epic_payload(&fixture.dir, "epic:First"));
        assert!(service
            .set_priority("epic:First", &json!(9))
            .as_bool()
            .unwrap());
        // Primeiro item já está ativo; instala outro e prioriza antes de o slot
        // liberar.
        service.pause("epic:First");
        service.install(&json!({
            "appid": "epic:Second",
            "title": "Second",
            "installPath": fixture.dir.join("games").to_string_lossy(),
            "priority": 10,
        }));
        assert_eq!(service.queue()[1]["status"], STATUS_DOWNLOADING);
        assert_eq!(service.queue()[1]["appid"], "epic:Second");
        assert_eq!(service.queue()[0]["priority"], 9);
        fixture.cleanup();
    }

    #[test]
    fn download_service_steam_depot_sequence_uses_builder_commands() {
        let fixture = Fixture::with_probe(
            "depot",
            TestProbe {
                files: true,
                runtimes: Some("Microsoft.NETCore.App 9.0.0 [C:\\dotnet]".to_string()),
            },
        );
        let service = fixture.open();
        let payload = json!({
            "appid": "440",
            "title": "Team Fortress 2",
            "cover": "tf2.jpg",
            "installdir": "../../Team Fortress 2",
            "steamDir": fixture.config.steam_dir.as_ref().unwrap().to_string_lossy(),
            "depots": [
                {"depotId": "111", "manifestId": "222", "key": "k1", "size": 104857600},
                {"depotId": "333", "manifestId": "444", "key": "k2", "size": 209715200},
            ],
        });
        assert_eq!(service.install_steam(&payload), ok_envelope());
        let item = &service.queue()[0];
        assert_eq!(item["appid"], "steam:440");
        assert_eq!(item["engine"], "steam");
        assert_eq!(item["installdir"], "Team Fortress 2");
        assert_eq!(item["total"], 300.0);
        assert_eq!(item["status"], STATUS_DOWNLOADING);
        assert_eq!(item["depotAtualId"], "111");

        let spawns = fixture.runner.specs();
        assert_eq!(spawns.len(), 1);
        let recorded = service.spawns();
        assert_eq!(recorded[0].phase, DownloadPhase::Depot);
        assert_eq!(
            spawns[0].program,
            fixture.config.dotnet_local.to_string_lossy().to_string()
        );
        assert_eq!(
            spawns[0].args[0],
            fixture
                .config
                .deps_dir
                .join(DLL_NAME)
                .to_string_lossy()
                .to_string()
        );
        assert!(spawns[0].args.iter().any(|arg| arg == "111"));
        assert!(spawns[0].args.iter().any(|arg| arg == "-validate"));
        let keys = std::fs::read_to_string(fixture.config.tmp_dir.join("keys_440.vdf")).unwrap();
        assert_eq!(keys, "111;k1\n333;k2");

        service.pump();
        assert_eq!(fixture.runner.specs().len(), 2);
        assert!(fixture.runner.specs()[1]
            .args
            .iter()
            .any(|arg| arg == "333"));
        assert_eq!(service.queue()[0]["filaIdx"], 1);
        assert_eq!(service.queue()[0]["depotAtualId"], "333");

        service.pump();
        let item = &service.queue()[0];
        assert_eq!(item["status"], STATUS_DONE);
        assert_eq!(item["depotsOk"], 2);
        assert_eq!(item["integrity"]["state"], "verified");
        assert_eq!(item["integrity"]["method"], "depot-manifest");
        fixture.cleanup();
    }

    #[test]
    fn download_service_torrent_wire_start_pause_resume_cancel() {
        let fixture = Fixture::new("torrent-wire");
        fixture
            .connector
            .push(json!({"event": "ready", "protocolVersion": 1}));
        fixture.connector.push(json!({"id": 1, "result": null}));
        fixture.connector.push(json!({"id": 2, "result": null}));
        fixture.connector.push(json!({"id": 3, "result": null}));
        fixture.connector.push(json!({"id": 4, "result": null}));
        let service = fixture.open();
        let payload = json!({
            "gameId": "abc",
            "url": magnet(),
            "savePath": fixture.dir.join("torrents").to_string_lossy(),
            "fileIndices": [0, 2],
            "title": "Repack",
            "cover": "cover.jpg",
        });
        assert_eq!(service.torrent_start(&payload), ok_envelope());
        let list = service.torrent_list();
        assert_eq!(list["ok"], true);
        let item = &list["downloads"][0];
        assert_eq!(item["gameId"], "tor:abc");
        assert_eq!(item["pausado"], false);
        assert_eq!(item["completo"], false);
        assert_eq!(item["erro"], "");

        assert_eq!(service.torrent_pause(&json!("tor:abc")), ok_envelope());
        assert_eq!(service.torrent_snapshot()[0]["pausado"], true);
        assert_eq!(service.torrent_resume(&json!("tor:abc")), ok_envelope());
        assert_eq!(service.torrent_snapshot()[0]["pausado"], false);
        assert_eq!(service.torrent_cancel(&json!("tor:abc")), ok_envelope());
        assert!(service.torrent_snapshot().is_empty());
        assert_eq!(fixture.connector.connections(), 1);
        assert_eq!(fixture.runner.spawn_count(), 0);

        // Sem processo real: o conector roteirizado é a única "bridge".
        fixture.cleanup();
    }

    #[test]
    fn download_service_torrent_progress_files_and_limit_wire() {
        let fixture = Fixture::new("torrent-rpc");
        fixture
            .connector
            .push(json!({"event": "ready", "protocolVersion": 1}));
        fixture.connector.push(json!({"id": 1, "result": null}));
        fixture.connector.push(json!({
            "id": 2,
            "result": {
                "tor:abc": {
                    "folderName": "Repack",
                    "fileSize": 1000,
                    "progress": 0.5,
                    "downloadSpeed": 50,
                    "uploadSpeed": 1,
                    "numPeers": 3,
                    "numSeeds": 2,
                    "state": "downloading",
                    "bytesDownloaded": 500
                }
            }
        }));
        fixture.connector.push(json!({
            "id": 3,
            "result": {
                "infoHash": "abc",
                "name": "Repack",
                "totalSize": 1000,
                "files": [{"index": 0, "path": "a.bin", "length": 1000}]
            }
        }));
        fixture.connector.push(json!({"id": 4, "result": null}));
        let service = fixture.open();
        assert_eq!(
            service.torrent_start(&json!({
                "gameId": "abc",
                "url": magnet(),
                "savePath": fixture.dir.join("torrents").to_string_lossy(),
            })),
            ok_envelope()
        );
        service.torrent_poll();
        let item = &service.torrent_snapshot()[0];
        assert_eq!(item["progress"], 0.5);
        assert_eq!(item["downloadSpeed"], 50.0);
        assert_eq!(item["bytesDownloaded"], 500.0);
        assert_eq!(item["completo"], false);
        assert_eq!(item["folderName"], "Repack");
        assert!(fixture
            .last_torrent_event()
            .is_some_and(|event| event[0]["progress"] == 0.5));

        let files = service.torrent_files(&json!({"magnet": magnet(), "timeoutMs": 60000}));
        assert_eq!(files["ok"], true);
        assert_eq!(files["infoHash"], "abc");
        assert_eq!(files["files"][0]["path"], "a.bin");
        assert_eq!(service.torrent_set_limit(&json!(1024)), ok_envelope());
        fixture.cleanup();
    }

    #[test]
    fn download_service_torrent_error_envelopes_match_oracle() {
        let fixture = Fixture::new("torrent-errors");
        let service = fixture.open();
        assert_eq!(
            service.torrent_start(&json!({})),
            error_envelope("missing_args")
        );
        assert_eq!(
            service.torrent_start(&json!({"gameId": "-x", "url": magnet()})),
            error_envelope("invalid_game_id")
        );
        assert_eq!(
            service.torrent_start(&json!({"gameId": "abc", "url": "file:///etc/passwd"})),
            error_envelope("invalid_download_uri")
        );
        assert_eq!(
            service.torrent_pause(&json!("-x")),
            error_envelope("invalid_game_id")
        );
        assert_eq!(
            service.torrent_resume(&json!("tor:missing")),
            error_envelope("download não encontrado")
        );
        assert_eq!(
            service.torrent_files(&json!({"magnet": "https://example.com/a"})),
            error_envelope("invalid_download_uri")
        );

        let mut no_debrid = config(&fixture.dir);
        no_debrid.debrid_configured = false;
        let disabled = DownloadService::with_persistence(
            MemoryPersistence::default(),
            no_debrid,
            Arc::clone(&fixture.runner) as Arc<dyn ProcessRunner>,
            Arc::clone(&fixture.connector) as Arc<dyn TorrentConnector>,
            Arc::clone(&fixture.probe),
        );
        assert_eq!(
            disabled.torrent_start(&json!({"gameId": "abc", "url": magnet()})),
            queued_error(ERRO_DEBRID)
        );
        assert_eq!(disabled.torrent_snapshot()[0]["erro"], ERRO_DEBRID);

        let mut no_libtorrent = config(&fixture.dir);
        no_libtorrent.libtorrent_available = false;
        let no_lt = DownloadService::with_persistence(
            MemoryPersistence::default(),
            no_libtorrent,
            Arc::clone(&fixture.runner) as Arc<dyn ProcessRunner>,
            Arc::clone(&fixture.connector) as Arc<dyn TorrentConnector>,
            Arc::clone(&fixture.probe),
        );
        let expected = if cfg!(windows) {
            ERRO_LIBTORRENT_WINDOWS
        } else {
            ERRO_LIBTORRENT
        };
        assert_eq!(
            no_lt.torrent_files(&json!({"magnet": magnet()})),
            error_envelope(expected)
        );

        // Worker sem ready: start vira erro enfileirado e marca o item.
        let empty = Fixture::new("torrent-noworker");
        let empty_service = empty.open();
        let response = empty_service.torrent_start(&json!({"gameId": "abc", "url": magnet()}));
        assert_eq!(response["ok"], false);
        assert_eq!(response["queued"], true);
        assert!(!response["error"].as_str().unwrap().is_empty());
        assert!(!empty_service.torrent_snapshot()[0]["erro"]
            .as_str()
            .unwrap()
            .is_empty());
        fixture.cleanup();
        empty.cleanup();
    }

    #[test]
    fn download_service_torrent_rpc_error_uses_worker_code() {
        let fixture = Fixture::new("torrent-rpc-error");
        fixture
            .connector
            .push(json!({"event": "ready", "protocolVersion": 1}));
        fixture.connector.push(json!({
            "id": 1,
            "error": {"code": "metadata_timeout", "message": "demorou"}
        }));
        let service = fixture.open();
        let response = service.torrent_start(&json!({"gameId": "abc", "url": magnet()}));
        assert_eq!(response["ok"], false);
        assert_eq!(response["queued"], true);
        assert_eq!(response["error"], "metadata_timeout");
        assert_eq!(service.torrent_snapshot()[0]["erro"], "metadata_timeout");
        assert_eq!(service.torrent_snapshot()[0]["pausado"], true);
        fixture.cleanup();
    }

    #[test]
    fn download_service_persistence_round_trip_memory_and_file() {
        let fixture = Fixture::new("persist");
        fixture.runner.set_auto_exit(false);
        let service = fixture.open();
        service.install(&epic_payload(&fixture.dir, "epic:Fortnite"));
        service.pause("epic:Fortnite");
        let saved = service.read_persisted().unwrap().unwrap();
        assert!(saved.contains("epic:Fortnite"));

        let reloaded = fixture.open_with_persistence(MemoryPersistence::new(Some(saved)));
        let item = &reloaded.queue()[0];
        assert_eq!(item["status"], STATUS_PAUSED);
        assert_eq!(item["integrity"]["state"], "downloading");
        assert_eq!(
            item["installPath"],
            fixture.dir.join("games").to_string_lossy().to_string()
        );

        // Persistência de arquivo no caminho real `<dataDir>/downloads.json`.
        let file_dir = temp_dir("persist-file");
        let mut file_config = config(&file_dir);
        file_config.downloads_file = file_dir.join("downloads.json");
        let file_service = DownloadService::with_persistence(
            FilePersistence::new(&file_config.downloads_file),
            file_config.clone(),
            Arc::clone(&fixture.runner) as Arc<dyn ProcessRunner>,
            Arc::clone(&fixture.connector) as Arc<dyn TorrentConnector>,
            Arc::clone(&fixture.probe),
        );
        file_service.install(&epic_payload(&file_dir, "epic:File"));
        let text = std::fs::read_to_string(&file_config.downloads_file).unwrap();
        assert!(text.contains("epic:File"));
        let reopened = DownloadService::with_persistence(
            FilePersistence::new(&file_config.downloads_file),
            file_config,
            Arc::clone(&fixture.runner) as Arc<dyn ProcessRunner>,
            Arc::clone(&fixture.connector) as Arc<dyn TorrentConnector>,
            Arc::clone(&fixture.probe),
        );
        assert_eq!(reopened.queue()[0]["appid"], "epic:File");
        assert_eq!(reopened.queue()[0]["status"], STATUS_PAUSED);
        let _ = std::fs::remove_dir_all(file_dir);
        fixture.cleanup();
    }

    #[test]
    fn download_service_ensure_dotnet_depot_and_aria2_builders() {
        let fixture = Fixture::with_probe(
            "ensure",
            TestProbe {
                files: true,
                runtimes: Some("Microsoft.NETCore.App 9.0.1 [C:\\dotnet]".to_string()),
            },
        );
        let service = fixture.open();
        let dotnet = service.ensure_dotnet();
        assert_eq!(dotnet["ok"], true);
        assert_eq!(
            dotnet["path"],
            fixture.config.dotnet_local.to_string_lossy().to_string()
        );
        let depot = service.ensure_depotdownloader();
        assert_eq!(depot["ok"], true);
        assert_eq!(
            depot["path"],
            fixture
                .config
                .deps_dir
                .join(DLL_NAME)
                .to_string_lossy()
                .to_string()
        );

        let spec = aria2_daemon_spec("aria2c.exe", &fixture.dir.join("aria2"), 6800, "segredo");
        assert_eq!(spec.program, "aria2c.exe");
        assert!(spec.args.iter().any(|arg| arg == "--enable-rpc=true"));
        assert!(spec.args.iter().any(|arg| arg == "--rpc-listen-port=6800"));
        assert!(spec.args.iter().any(|arg| arg == "--rpc-secret=segredo"));
        assert!(spec.args.iter().any(|arg| arg == "--no-conf=true"));
        fixture.cleanup();
    }

    #[test]
    fn download_service_shutdown_pauses_active_and_clears_processes() {
        let fixture = Fixture::new("shutdown");
        fixture.runner.set_auto_exit(false);
        let service = fixture.open();
        service.install(&epic_payload(&fixture.dir, "epic:Fortnite"));
        assert_eq!(service.queue()[0]["status"], STATUS_DOWNLOADING);
        assert_eq!(service.active_count(), 1);
        assert_eq!(service.shutdown(), json!({"ok": true}));
        assert_eq!(service.queue()[0]["status"], STATUS_PAUSED);
        assert_eq!(service.active_count(), 0);
        let persisted = service.read_persisted().unwrap().unwrap();
        assert!(persisted.contains("\"paused\""));
        fixture.cleanup();
    }

    #[test]
    fn download_service_handle_routes_all_owned_channels() {
        let fixture = Fixture::new("dispatch");
        fixture
            .connector
            .push(json!({"event": "ready", "protocolVersion": 1}));
        fixture.connector.push(json!({"id": 1, "result": null}));
        let service = fixture.open();
        assert_eq!(service.handle("dm:queue", &[]), Some(json!([])));
        assert_eq!(
            service.handle("dm:install", &[epic_payload(&fixture.dir, "epic:Game")]),
            Some(ok_envelope())
        );
        assert!(service
            .handle("dm:setPriority", &[json!("epic:Game"), json!(5)])
            .is_some());
        assert_eq!(
            service.handle(
                "torrent:start",
                &[json!({
                    "gameId": "abc",
                    "url": magnet(),
                    "savePath": fixture.dir.join("torrents").to_string_lossy(),
                })]
            ),
            Some(ok_envelope())
        );
        assert!(service.handle("torrent:list", &[]).is_some());
        assert!(service.handle("store:ensureDotnet", &[]).is_some());
        assert!(service.handle("store:ensureDepotDownloader", &[]).is_some());
        assert_eq!(service.handle("dm:unknown", &[]), None);
        fixture.cleanup();
    }
}
