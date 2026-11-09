//! Runtime de launch: resolve o plano pelo crate `arcadia-launch`, spawna o
//! processo e mantém a UI informada com os mesmos eventos do Electron.
//!
//! O spawn real é a borda que o crate puro deixou de fora. Aqui ficam o
//! processo, o monitor de saída e o kill de árvore; a decisão de comando
//! continua 100% no resolver testado.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use arcadia_launch::platform::Platform;
use arcadia_launch::resolver::{
    plan_from_request, BuiltCommand, GameEntry, GameSettings, LaunchPayload, LaunchPlan,
    LaunchResolverDeps, PlanEnvironment,
};
use arcadia_store::accounts::account_file;
use arcadia_store::library::read_games;

pub type EventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct RunningGame {
    child: Child,
    pid: u32,
    game_id: String,
    token: u64,
    started: Instant,
}

/// Estado do runtime de launch. Compartilhado pelo AppState.
pub struct LaunchRuntime {
    running: Arc<Mutex<Option<RunningGame>>>,
    events: Arc<Mutex<Option<EventSink>>>,
    next_token: AtomicU64,
}

impl Default for LaunchRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LaunchRuntime {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(None)),
            next_token: AtomicU64::new(1),
        }
    }

    pub fn set_event_sink(&self, sink: EventSink) {
        *lock(&self.events) = Some(sink);
    }

    fn emit(&self, event: &str, payload: Value) {
        if let Some(sink) = lock(&self.events).clone() {
            sink(event, payload);
        }
    }

    pub fn is_running(&self) -> bool {
        let mut slot = lock(&self.running);
        match slot.as_mut() {
            Some(running) => match running.child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) => {
                    slot.take();
                    false
                }
                Err(_) => true,
            },
            None => false,
        }
    }

    /// `game:launch` — resolve, spawna e começa o monitor de saída.
    pub fn launch(&self, data_dir: &Path, account: Option<&str>, payload: &Value) -> Value {
        if self.is_running() {
            return json!({ "ok": false, "error": "Já existe um lançamento ou jogo em execução." });
        }

        let deps = RuntimeDeps {
            data_dir: data_dir.to_path_buf(),
            account: account.map(str::to_string),
        };
        let payload = LaunchPayload::from_json(payload);
        let platform = Platform::current();
        let environment = build_environment(platform, data_dir);
        let plan = match plan_from_request(&payload, &deps, &environment) {
            Ok(plan) => plan,
            Err(error) => return json!({ "ok": false, "error": error.to_string() }),
        };

        self.spawn_plan(&plan)
    }

    fn spawn_plan(&self, plan: &LaunchPlan) -> Value {
        let token = self.next_token.fetch_add(1, Ordering::SeqCst);
        let game_id = plan.game_id.clone();
        let mut command = Command::new(&plan.executable);
        command
            .args(&plan.args)
            .envs(&plan.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !plan.cwd.is_empty() {
            command.current_dir(&plan.cwd);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NEW_PROCESS_GROUP: permite o kill de árvore sem matar o app.
            command.creation_flags(0x0000_0200);
        }

        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.emit(
                    "game:launchError",
                    json!({ "gameId": game_id, "error": error.to_string() }),
                );
                return json!({ "ok": false, "error": error.to_string() });
            }
        };

        let pid = child.id();
        let game_events = plan.warnings.clone();
        {
            let mut slot = lock(&self.running);
            *slot = Some(RunningGame {
                child,
                pid,
                game_id: game_id.clone(),
                token,
                started: Instant::now(),
            });
        }

        self.emit(
            "game:launchState",
            json!({ "state": "starting", "gameId": game_id, "token": token }),
        );
        if !game_events.is_empty() {
            self.emit(
                "game:launchWarning",
                json!({ "gameId": game_id, "warnings": game_events }),
            );
        }
        self.emit("game:running", Value::Bool(true));
        self.emit(
            "game:active",
            json!({ "rodando": true, "gameId": game_id }),
        );
        self.emit(
            "game:launchState",
            json!({ "state": "running", "gameId": game_id, "token": token }),
        );

        self.spawn_monitor(token, pid, game_id.clone(), plan.track_session);

        json!({
            "ok": true,
            "pid": pid,
            "gameId": game_id,
            "token": token,
            "source": source_name(plan),
            "trackSession": plan.track_session
        })
    }

    fn spawn_monitor(&self, token: u64, pid: u32, game_id: String, track: bool) {
        let running = Arc::clone(&self.running);
        let events = Arc::clone(&self.events);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(1000));
            let finished = {
                let mut slot = lock(&running);
                let Some(current) = slot.as_mut() else {
                    return;
                };
                if current.token != token {
                    return;
                }
                match current.child.try_wait() {
                    Ok(Some(_)) => {
                        slot.take();
                        true
                    }
                    Ok(None) => false,
                    Err(_) => true,
                }
            };
            if finished {
                let elapsed_minutes = 0; // playtime é derivado do store/telemetria, não daqui
                let _ = elapsed_minutes;
                if let Some(sink) = lock(&events).clone() {
                    if track {
                        sink("game:running", Value::Bool(false));
                        sink(
                            "game:active",
                            json!({ "rodando": false, "gameId": game_id }),
                        );
                    }
                    sink(
                        "game:launchState",
                        json!({ "state": "idle", "gameId": game_id, "token": token }),
                    );
                }
                let _ = pid;
                return;
            }
        });
    }

    /// `game:close` — encerra o processo e a árvore.
    pub fn close(&self) -> Value {
        let running = {
            let mut slot = lock(&self.running);
            slot.take()
        };
        let Some(mut running) = running else {
            return json!({ "ok": true });
        };

        self.emit(
            "game:launchState",
            json!({ "state": "stopping", "gameId": running.game_id, "token": running.token }),
        );

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &running.pid.to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &running.pid.to_string()])
                .status();
        }
        let _ = running.child.kill();
        let _ = running.child.wait();

        self.emit("game:running", Value::Bool(false));
        self.emit(
            "game:active",
            json!({ "rodando": false, "gameId": running.game_id }),
        );
        json!({ "ok": true })
    }

    /// Estado atual para a UI (não há canal dedicado no preload; usado em testes).
    pub fn state_json(&self) -> Value {
        let mut slot = lock(&self.running);
        let Some(running) = slot.as_mut() else {
            return json!({ "running": false });
        };
        let alive = matches!(running.child.try_wait(), Ok(None));
        if alive {
            return json!({
                "running": true,
                "gameId": running.game_id,
                "pid": running.pid,
                "seconds": running.started.elapsed().as_secs()
            });
        }
        slot.take();
        json!({ "running": false })
    }
}

fn source_name(plan: &LaunchPlan) -> &'static str {
    use arcadia_launch::resolver::LaunchSource::*;
    match plan.source {
        Steam => "steam",
        Proton => "proton",
        Wine => "wine",
        Native => "native",
        Emulator => "emulator",
        Custom => "custom",
    }
}

fn build_environment<'a>(platform: Platform, data_dir: &'a Path) -> PlanEnvironment<'a> {
    let mut environment = PlanEnvironment::new(platform);
    environment.base_env = std::env::vars().collect();
    environment.default_prefix = Box::new(move |game_id: &str| {
        let safe = game_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>();
        data_dir
            .join("prefixes")
            .join(if safe.is_empty() { "game".to_string() } else { safe })
            .to_string_lossy()
            .into_owned()
    });
    environment.bin_exists = Box::new(|name: &str| {
        let candidate = PathBuf::from(name);
        if candidate.is_absolute() {
            return candidate.is_file();
        }
        let Some(path) = std::env::var_os("PATH") else {
            return false;
        };
        for dir in std::env::split_paths(&path) {
            let direct = dir.join(name);
            if direct.is_file() {
                return true;
            }
            #[cfg(windows)]
            {
                if !name.to_ascii_lowercase().ends_with(".exe") {
                    if dir.join(format!("{name}.exe")).is_file() {
                        return true;
                    }
                }
            }
        }
        false
    });
    environment.fs_exists = Box::new(|path: &str| Path::new(path).exists());
    environment
}

struct RuntimeDeps {
    data_dir: PathBuf,
    account: Option<String>,
}

impl RuntimeDeps {
    fn find(&self, id: &str) -> Option<GameEntry> {
        read_games(&self.data_dir, self.account.as_deref())
            .into_iter()
            .find(|game| game.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|game| serde_json::from_value::<GameEntry>(game).ok())
    }

    fn settings(&self, id: &str) -> GameSettings {
        let path = account_file(
            &self.data_dir,
            self.account.as_deref(),
            "game_settings.json",
        );
        let Ok(raw) = std::fs::read_to_string(path) else {
            return GameSettings::default();
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            return GameSettings::default();
        };
        value
            .get(id)
            .cloned()
            .and_then(|entry| serde_json::from_value::<GameSettings>(entry).ok())
            .unwrap_or_default()
    }
}

impl LaunchResolverDeps for RuntimeDeps {
    fn find_game(&self, id: &str) -> Option<GameEntry> {
        self.find(id)
    }

    fn custom_launch_cmd(&self, id: &str) -> Option<BuiltCommand> {
        let game = self.find(id)?;
        if game.launch_cmd.is_empty() {
            return None;
        }
        Some(BuiltCommand::new(game.launch_cmd))
    }

    fn get_game_settings(&self, id: &str) -> GameSettings {
        self.settings(id)
    }

    fn exe_launch_cmd(&self, id: &str, exe: &str) -> Option<BuiltCommand> {
        if exe.trim().is_empty() {
            return None;
        }
        let mut command = BuiltCommand::new([exe]);
        let settings = self.settings(id);
        if let Some(cwd) = settings
            .exe_path
            .as_deref()
            .and_then(|path| Path::new(path).parent())
        {
            command.cwd = cwd.to_string_lossy().into_owned();
        }
        Some(command)
    }

    fn emulator_launch(
        &self,
        _id: &str,
        _game: &GameEntry,
        _settings: &GameSettings,
    ) -> Option<arcadia_launch::resolver::EmulatorLaunchResult> {
        // O registro de emuladores ainda não foi portado; o resolver recebe
        // `None` e devolve o erro claro de "sem comando" em vez de adivinhar.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_rejects_empty_payload_without_spawning() {
        let runtime = LaunchRuntime::new();
        let result = runtime.launch(
            &std::env::temp_dir(),
            None,
            &json!({ "gameId": "nao-existe" }),
        );
        assert_eq!(result["ok"], false);
        assert!(!runtime.is_running());
    }

    #[test]
    fn state_is_idle_when_nothing_runs() {
        let runtime = LaunchRuntime::new();
        assert_eq!(runtime.state_json()["running"], false);
    }

    #[test]
    fn spawns_and_closes_real_process() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-launch-runtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let (program, args) = if cfg!(windows) {
            ("cmd.exe", vec!["/c", "ping", "-n", "6", "127.0.0.1"])
        } else {
            ("sleep", vec!["5"])
        };
        let library = json!({
            "version": 1,
            "generated_at": 1,
            "sources": {},
            "games": [{
                "id": "launch-test",
                "title": "Launch Test",
                "launcher": "custom",
                "launch_cmd": std::iter::once(program.to_string())
                    .chain(args.iter().map(|a| a.to_string()))
                    .collect::<Vec<_>>()
            }]
        });
        std::fs::write(
            dir.join("library.json"),
            serde_json::to_string(&library).unwrap(),
        )
        .unwrap();

        let runtime = LaunchRuntime::new();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink_events = Arc::clone(&events);
        runtime.set_event_sink(Arc::new(move |event, _| {
            lock(&sink_events).push(event.to_string());
        }));

        let result = runtime.launch(&dir, None, &json!({ "gameId": "launch-test" }));
        assert_eq!(result["ok"], true, "launch falhou: {result}");
        assert!(runtime.is_running());
        assert!(lock(&events).iter().any(|event| event == "game:running"));

        std::thread::sleep(std::time::Duration::from_millis(250));
        let closed = runtime.close();
        assert_eq!(closed["ok"], true);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        assert!(!runtime.is_running());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
