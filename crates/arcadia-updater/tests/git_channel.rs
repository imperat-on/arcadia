//! Testes do canal Git com probes injetadas (nenhum comando real).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::json;

use arcadia_updater::git_channel::{
    ApplyResult, CommandOutput, CommandSpec, CommitInfo, FsProbe, GitChannel, GitRunner, HttpProbe,
    HttpRequest, HttpResponse, RestartProbe, UpdateCheck, UpdatePhase, UpdateReason, UpdateState,
};

#[derive(Clone, Default)]
struct CallLog {
    calls: Rc<RefCell<Vec<CommandSpec>>>,
}

struct ScriptedRunner {
    log: CallLog,
    responses: HashMap<String, CommandOutput>,
}

impl ScriptedRunner {
    fn new(log: &CallLog, responses: &[(&str, CommandOutput)]) -> Self {
        Self {
            log: log.clone(),
            responses: responses
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        }
    }
}

impl GitRunner for ScriptedRunner {
    fn run(&mut self, command: &CommandSpec) -> CommandOutput {
        self.log.calls.borrow_mut().push(command.clone());
        self.responses
            .get(&command.command_line())
            .cloned()
            .unwrap_or_else(|| CommandOutput::failure("comando inesperado"))
    }
}

struct FakeFs {
    paths: Vec<PathBuf>,
}

impl FakeFs {
    fn with_git(root: &str) -> Self {
        Self {
            paths: vec![PathBuf::from(root).join(".git")],
        }
    }

    fn empty() -> Self {
        Self { paths: Vec::new() }
    }
}

impl FsProbe for FakeFs {
    fn exists(&self, path: &Path) -> bool {
        self.paths.iter().any(|candidate| candidate == path)
    }
}

#[derive(Clone, Default)]
struct HttpLog {
    requests: Rc<RefCell<Vec<HttpRequest>>>,
}

struct FakeHttp {
    log: HttpLog,
    response: Result<HttpResponse, String>,
}

impl HttpProbe for FakeHttp {
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, String> {
        self.log.requests.borrow_mut().push(request.clone());
        self.response.clone()
    }
}

#[derive(Clone)]
struct FakeRestart {
    calls: Rc<Cell<usize>>,
    result: Result<(), String>,
}

impl Default for FakeRestart {
    fn default() -> Self {
        Self {
            calls: Rc::new(Cell::new(0)),
            result: Ok(()),
        }
    }
}

impl RestartProbe for FakeRestart {
    fn request_restart(&mut self) -> Result<(), String> {
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

const ROOT: &str = "repo";

fn branch_cmd() -> &'static str {
    "git rev-parse --abbrev-ref HEAD"
}

fn status_cmd() -> &'static str {
    "git status --porcelain -uno"
}

fn unsent_cmd() -> &'static str {
    "git rev-list --count @{u}..HEAD"
}

fn sha_cmd() -> &'static str {
    "git rev-parse HEAD"
}

fn base_state_responses() -> Vec<(&'static str, CommandOutput)> {
    vec![
        (branch_cmd(), CommandOutput::success("master")),
        (status_cmd(), CommandOutput::success("")),
        (unsent_cmd(), CommandOutput::success("0")),
    ]
}

fn empty_http() -> FakeHttp {
    FakeHttp {
        log: HttpLog::default(),
        response: Err("http nao deveria ser chamado".into()),
    }
}

fn build_channel(
    runner: ScriptedRunner,
    fs: FakeFs,
    http: FakeHttp,
    restart: FakeRestart,
) -> GitChannel {
    GitChannel::new(runner, fs, http)
        .with_root(ROOT)
        .with_restart(restart)
}

fn dirty_responses() -> Vec<(&'static str, CommandOutput)> {
    vec![
        (branch_cmd(), CommandOutput::success("master")),
        (
            status_cmd(),
            CommandOutput::success(" M src/a.rs\n?? solto.txt"),
        ),
    ]
}

#[test]
fn no_git_dir_blocks_without_running_commands() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[]);
    let mut channel = build_channel(
        runner,
        FakeFs::empty(),
        empty_http(),
        FakeRestart::default(),
    );

    assert_eq!(
        channel.state(),
        UpdateState::blocked(UpdateReason::SemGit, None)
    );
    assert!(log.calls.borrow().is_empty(), "sem .git não roda git");
}

#[test]
fn failing_branch_means_sem_git() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[]);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );

    assert_eq!(
        channel.state(),
        UpdateState::blocked(UpdateReason::SemGit, None)
    );
    assert_eq!(log.calls.borrow().len(), 1);
}

#[test]
fn wrong_branch_reports_branch_and_detail() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(branch_cmd(), CommandOutput::success("feature/x"))]);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );

    let state = channel.state();
    assert!(!state.pode_atualizar);
    assert_eq!(state.motivo, Some(UpdateReason::Branch));
    assert_eq!(state.detalhe.as_deref(), Some("feature/x"));
}

#[test]
fn dirty_tree_counts_porcelain_lines() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &dirty_responses());
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );

    let state = channel.state();
    assert_eq!(state.motivo, Some(UpdateReason::Sujo));
    assert_eq!(state.detalhe.as_deref(), Some("2"));
    // Só chegou até o status; rev-list não foi chamado.
    assert_eq!(log.calls.borrow().len(), 2);
}

#[test]
fn status_failure_is_ignored_and_unsent_still_blocks() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(
        &log,
        &[
            (branch_cmd(), CommandOutput::success("master")),
            (status_cmd(), CommandOutput::failure("boom")),
            (unsent_cmd(), CommandOutput::success(" 3 \n")),
        ],
    );
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );

    let state = channel.state();
    assert_eq!(state.motivo, Some(UpdateReason::NaoEnviado));
    assert_eq!(state.detalhe.as_deref(), Some("3"));
}

#[test]
fn unsent_zero_or_invalid_does_not_block() {
    for value in ["0", "", "abc", "-1"] {
        let log = CallLog::default();
        let mut responses = base_state_responses();
        responses.push((sha_cmd(), CommandOutput::success("abc")));
        responses[2] = (unsent_cmd(), CommandOutput::success(value));
        let runner = ScriptedRunner::new(&log, &responses);
        let mut channel = build_channel(
            runner,
            FakeFs::with_git(ROOT),
            empty_http(),
            FakeRestart::default(),
        );
        assert_eq!(
            channel.state(),
            UpdateState::can_update(),
            "valor {value:?}"
        );
    }
}

#[test]
fn sha_local_returns_trimmed_head_or_empty() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(
        &log,
        &[(sha_cmd(), CommandOutput::success(" abcdef123 \n"))],
    );
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    assert_eq!(channel.sha_local(), "abcdef123");

    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[]);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    assert_eq!(channel.sha_local(), "");
}

#[test]
fn check_without_git_returns_error_without_http() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[]);
    let http = empty_http();
    let http_log = http.log.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());

    assert_eq!(channel.check(), UpdateCheck::error("não é um clone git"));
    assert!(http_log.requests.borrow().is_empty());
}

#[test]
fn check_maps_compare_payload_like_the_oracle() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(
        &log,
        &[(sha_cmd(), CommandOutput::success("abcdef0123456789"))],
    );
    let body = json!({
        "ahead_by": 3,
        "commits": [
            {"sha": "1111111aaaa", "commit": {"message": "primeiro\ncorpo"}},
            {"sha": "2222222bbbb", "commit": {"message": "segundo"}}
        ],
        "files": [
            {"filename": "src/x.rs"},
            {"filename": "app/package-lock.json"}
        ]
    })
    .to_string();
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Ok(HttpResponse { status: 200, body }),
    };
    let http_log = http.log.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());

    let check = channel.check();
    assert_eq!(
        check,
        UpdateCheck::success(
            "abcdef0".into(),
            3,
            vec![
                CommitInfo {
                    sha: "2222222".into(),
                    titulo: "segundo".into()
                },
                CommitInfo {
                    sha: "1111111".into(),
                    titulo: "primeiro".into()
                }
            ],
            true
        )
    );

    let requests = http_log.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://api.github.com/repos/imperat-on/arcadia/compare/abcdef0123456789...master"
    );
    assert_eq!(
        requests[0].headers,
        vec![("User-Agent".to_string(), "arcadia".to_string())]
    );
    assert_eq!(requests[0].timeout_ms, 15_000);
}

#[test]
fn check_missing_arrays_defaults() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Ok(HttpResponse {
            status: 200,
            body: "{}".into(),
        }),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());

    let check = channel.check();
    assert!(check.ok);
    assert_eq!(check.atrasado, Some(0));
    assert_eq!(check.commits, Some(Vec::new()));
    assert_eq!(check.deps_mudaram, Some(false));
}

#[test]
fn check_reports_http_status_and_transport_errors() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Ok(HttpResponse {
            status: 500,
            body: String::new(),
        }),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());
    assert_eq!(channel.check(), UpdateCheck::error("GitHub HTTP 500"));

    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Err("timeout".into()),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());
    assert_eq!(channel.check(), UpdateCheck::error("timeout"));
}

#[test]
fn check_rejects_invalid_json() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Ok(HttpResponse {
            status: 200,
            body: "isto nao e json".into(),
        }),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());
    let check = channel.check();
    assert!(!check.ok);
    assert!(check.error.is_some());
    assert!(check.local.is_none());
}

#[test]
fn check_rejects_non_array_commits_or_files() {
    for body in [
        json!({"commits": {"sha": "x"}}),
        json!({"files": "app/package-lock.json"}),
        json!(null),
        json!({"commits": [null]}),
    ] {
        let log = CallLog::default();
        let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
        let http = FakeHttp {
            log: HttpLog::default(),
            response: Ok(HttpResponse {
                status: 200,
                body: body.to_string(),
            }),
        };
        let mut channel =
            build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());
        let check = channel.check();
        assert!(!check.ok, "payload inválido: {body}");
        assert_eq!(check.error.as_deref(), Some("resposta_invalida"));
    }

    // Valores falsy seguem o `|| []` do JS e viram listas vazias.
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &[(sha_cmd(), CommandOutput::success("abcdef0"))]);
    let http = FakeHttp {
        log: HttpLog::default(),
        response: Ok(HttpResponse {
            status: 200,
            body: json!({"commits": null, "files": false}).to_string(),
        }),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), http, FakeRestart::default());
    let check = channel.check();
    assert!(check.ok);
    assert_eq!(check.commits, Some(Vec::new()));
    assert_eq!(check.deps_mudaram, Some(false));
}

#[test]
fn apply_blocked_reports_state_and_does_not_pull() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &dirty_responses());
    let restart = FakeRestart::default();
    let restart_calls = restart.calls.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), empty_http(), restart);

    let result = channel.apply(false);
    assert_eq!(result.error.as_deref(), Some("bloqueado: sujo"));
    assert_eq!(result.pode_atualizar, Some(false));
    assert_eq!(result.motivo, Some(UpdateReason::Sujo));
    assert_eq!(result.detalhe.as_deref(), Some("2"));
    assert!(result.sha.is_none());
    assert!(result.reiniciou.is_none());
    assert_eq!(restart_calls.get(), 0);
    assert!(log
        .calls
        .borrow()
        .iter()
        .all(|call| call.command_line() != "git pull --ff-only"));
}

fn apply_responses(
    extra: Vec<(&'static str, CommandOutput)>,
) -> Vec<(&'static str, CommandOutput)> {
    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::success("")));
    responses.push(("npm run build", CommandOutput::success("")));
    responses.push((sha_cmd(), CommandOutput::success("abcdef1234567890")));
    responses.extend(extra);
    responses
}

#[test]
fn apply_without_deps_runs_pull_and_build_then_requests_restart() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &apply_responses(vec![]));
    let restart = FakeRestart::default();
    let restart_calls = restart.calls.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), empty_http(), restart);
    let mut progress = Vec::new();

    let result = channel.apply_with(false, &mut |event| progress.push(event.etapa));
    assert_eq!(result, ApplyResult::success("abcdef1".into(), true));
    assert_eq!(
        progress,
        vec![UpdatePhase::Pull, UpdatePhase::Build, UpdatePhase::Pronto]
    );
    assert_eq!(restart_calls.get(), 1);

    let calls = log.calls.borrow();
    assert!(calls
        .iter()
        .any(|call| call.command_line() == "git pull --ff-only"));
    assert!(calls
        .iter()
        .any(|call| call.command_line() == "npm run build"));
    assert!(!calls
        .iter()
        .any(|call| call.command_line() == "npm install --no-audit --no-fund"));
    let build = calls
        .iter()
        .find(|call| call.command_line() == "npm run build")
        .expect("build");
    assert_eq!(build.cwd, PathBuf::from(ROOT).join("app"));
}

#[test]
fn apply_with_deps_runs_npm_install_before_build() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(
        &log,
        &apply_responses(vec![(
            "npm install --no-audit --no-fund",
            CommandOutput::success(""),
        )]),
    );
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    let mut progress = Vec::new();

    let result = channel.apply_with(true, &mut |event| progress.push(event.etapa));
    assert!(result.ok);
    assert_eq!(
        progress,
        vec![
            UpdatePhase::Pull,
            UpdatePhase::Deps,
            UpdatePhase::Build,
            UpdatePhase::Pronto
        ]
    );

    let calls = log.calls.borrow();
    let install = calls
        .iter()
        .position(|call| call.command_line() == "npm install --no-audit --no-fund")
        .expect("npm install");
    let build = calls
        .iter()
        .position(|call| call.command_line() == "npm run build")
        .expect("build");
    assert!(install < build);
    assert_eq!(calls[install].cwd, PathBuf::from(ROOT).join("app"));
}

#[test]
fn apply_stops_on_pull_failure_without_build_or_restart() {
    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::failure("divergiu")));
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &responses);
    let restart = FakeRestart::default();
    let restart_calls = restart.calls.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), empty_http(), restart);

    let result = channel.apply(false);
    assert_eq!(result, ApplyResult::error("divergiu"));
    assert_eq!(restart_calls.get(), 0);
    assert!(!log
        .calls
        .borrow()
        .iter()
        .any(|call| call.command_line() == "npm run build"));
}

#[test]
fn apply_uses_fallback_messages_when_stderr_is_empty() {
    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::failure("")));
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &responses);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    assert_eq!(channel.apply(false), ApplyResult::error("git pull falhou"));

    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::success("")));
    responses.push((
        "npm install --no-audit --no-fund",
        CommandOutput::failure(""),
    ));
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &responses);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    assert_eq!(
        channel.apply(true),
        ApplyResult::error("npm install falhou")
    );

    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::success("")));
    responses.push(("npm run build", CommandOutput::failure("")));
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &responses);
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    assert_eq!(channel.apply(false), ApplyResult::error("build falhou"));
}

#[test]
fn apply_build_failure_keeps_previous_dist_and_does_not_restart() {
    let mut responses = base_state_responses();
    responses.push(("git pull --ff-only", CommandOutput::success("")));
    responses.push(("npm run build", CommandOutput::failure("vite quebrou")));
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &responses);
    let restart = FakeRestart::default();
    let restart_calls = restart.calls.clone();
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), empty_http(), restart);

    assert_eq!(channel.apply(false), ApplyResult::error("vite quebrou"));
    assert_eq!(restart_calls.get(), 0);
}

#[test]
fn restart_failure_is_reported_but_update_stays_ok() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &apply_responses(vec![]));
    let restart = FakeRestart {
        calls: Rc::new(Cell::new(0)),
        result: Err("spawn falhou".into()),
    };
    let mut channel = build_channel(runner, FakeFs::with_git(ROOT), empty_http(), restart);

    let result = channel.apply(false);
    assert!(result.ok);
    assert_eq!(result.sha.as_deref(), Some("abcdef1"));
    assert_eq!(result.reiniciou, Some(false));
    assert_eq!(result.error.as_deref(), Some("spawn falhou"));
}

#[test]
fn apply_without_restart_probe_does_not_claim_a_restart() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &apply_responses(vec![]));
    let mut channel = GitChannel::new(runner, FakeFs::with_git(ROOT), empty_http()).with_root(ROOT);

    let result = channel.apply(false);
    assert!(result.ok);
    assert_eq!(
        result,
        ApplyResult::success_without_restart("abcdef1".into())
    );
    assert!(result.reiniciou.is_none());
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({ "ok": true, "sha": "abcdef1" })
    );
}

#[test]
fn apply_result_blocked_serializes_with_state_fields() {
    let log = CallLog::default();
    let runner = ScriptedRunner::new(&log, &dirty_responses());
    let mut channel = build_channel(
        runner,
        FakeFs::with_git(ROOT),
        empty_http(),
        FakeRestart::default(),
    );
    let value = serde_json::to_value(channel.apply(false)).unwrap();
    assert_eq!(
        value,
        json!({
            "ok": false,
            "error": "bloqueado: sujo",
            "podeAtualizar": false,
            "motivo": "sujo",
            "detalhe": "2"
        })
    );
}
