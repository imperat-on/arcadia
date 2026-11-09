//! Canal Git (quem roda da fonte).
//!
//! Port 1:1 de `app/electron/updater.js`: estado do clone, comparação com o
//! GitHub e aplicação (`git pull --ff-only` + `npm install` opcional + build).
//!
//! O crate não executa nada sozinho: comandos, existência de `.git`, HTTP e o
//! reinício entram por traits injetadas. As implementações de sistema existem
//! para a borda Tauri; os testes usam apenas probes falsos.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js;

pub const REPO: &str = "imperat-on/arcadia";
pub const BRANCH: &str = "master";
pub const GITHUB_API: &str = "https://api.github.com/repos";
pub const DEPS_FILE: &str = "app/package-lock.json";
pub const COMPARE_TIMEOUT_MS: u64 = 15_000;

/// Resultado cru de um comando (o oráculo faz `.trim()` em stdout/stderr).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub ok: bool,
    pub saida: String,
    pub erro: String,
}

impl CommandOutput {
    pub fn success(saida: impl Into<String>) -> Self {
        Self {
            ok: true,
            saida: saida.into(),
            erro: String::new(),
        }
    }

    pub fn failure(erro: impl Into<String>) -> Self {
        Self {
            ok: false,
            saida: String::new(),
            erro: erro.into(),
        }
    }
}

/// Comando a executar; `cwd` é o diretório de trabalho.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: &[&str], cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            cwd: cwd.into(),
        }
    }

    pub fn command_line(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

pub trait GitRunner {
    fn run(&mut self, command: &CommandSpec) -> CommandOutput;
}

pub trait FsProbe {
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait HttpProbe {
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

/// Efeito de reinício; o `main.js` spawna o processo novo depois do apply.
pub trait RestartProbe {
    fn request_restart(&mut self) -> Result<(), String>;
}

/// Implementação real de [`GitRunner`] via `std::process::Command`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGitRunner;

impl GitRunner for SystemGitRunner {
    fn run(&mut self, command: &CommandSpec) -> CommandOutput {
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .current_dir(&command.cwd)
            .output();
        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if output.status.success() {
                    CommandOutput {
                        ok: true,
                        saida: stdout,
                        erro: String::new(),
                    }
                } else {
                    let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    if stderr.is_empty() {
                        stderr = output.status.to_string();
                    }
                    CommandOutput {
                        ok: false,
                        saida: stdout,
                        erro: stderr,
                    }
                }
            }
            Err(error) => CommandOutput::failure(error.to_string()),
        }
    }
}

/// Implementação real de [`FsProbe`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFs;

impl FsProbe for SystemFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Reinício sem efeito; útil em testes e no modo headless.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRestart;

impl RestartProbe for NoopRestart {
    fn request_restart(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Por que este clone não pode se atualizar sozinho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateReason {
    #[serde(rename = "sem-git")]
    SemGit,
    #[serde(rename = "branch")]
    Branch,
    #[serde(rename = "sujo")]
    Sujo,
    #[serde(rename = "nao-enviado")]
    NaoEnviado,
}

impl UpdateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemGit => "sem-git",
            Self::Branch => "branch",
            Self::Sujo => "sujo",
            Self::NaoEnviado => "nao-enviado",
        }
    }
}

impl std::fmt::Display for UpdateReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// `estado()` do updater git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub pode_atualizar: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<UpdateReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detalhe: Option<String>,
}

impl UpdateState {
    pub fn can_update() -> Self {
        Self {
            pode_atualizar: true,
            motivo: None,
            detalhe: None,
        }
    }

    pub fn blocked(reason: UpdateReason, detail: Option<String>) -> Self {
        Self {
            pode_atualizar: false,
            motivo: Some(reason),
            detalhe: detail,
        }
    }
}

/// Um commit do `compare` do GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub titulo: String,
}

/// `verificar()` do updater git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atrasado: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<CommitInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps_mudaram: Option<bool>,
}

impl UpdateCheck {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            local: None,
            atrasado: None,
            commits: None,
            deps_mudaram: None,
        }
    }

    pub fn success(
        local: String,
        atrasado: i64,
        commits: Vec<CommitInfo>,
        deps_changed: bool,
    ) -> Self {
        Self {
            ok: true,
            error: None,
            local: Some(local),
            atrasado: Some(atrasado),
            commits: Some(commits),
            deps_mudaram: Some(deps_changed),
        }
    }
}

/// Etapas reportadas por `aplicar()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePhase {
    Pull,
    Deps,
    Build,
    Pronto,
}

impl UpdatePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Deps => "deps",
            Self::Build => "build",
            Self::Pronto => "pronto",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProgress {
    pub etapa: UpdatePhase,
}

impl UpdateProgress {
    pub fn new(etapa: UpdatePhase) -> Self {
        Self { etapa }
    }
}

/// Retorno de `aplicar()`, incluindo o reinício pedido após o sucesso.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reiniciou: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pode_atualizar: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<UpdateReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detalhe: Option<String>,
}

impl ApplyResult {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            sha: None,
            reiniciou: None,
            pode_atualizar: None,
            motivo: None,
            detalhe: None,
        }
    }

    pub fn blocked(message: impl Into<String>, state: &UpdateState) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            sha: None,
            reiniciou: None,
            pode_atualizar: Some(state.pode_atualizar),
            motivo: state.motivo,
            detalhe: state.detalhe.clone(),
        }
    }

    pub fn success(sha: String, restarted: bool) -> Self {
        Self {
            ok: true,
            error: None,
            sha: Some(sha),
            reiniciou: Some(restarted),
            pode_atualizar: None,
            motivo: None,
            detalhe: None,
        }
    }

    /// Sucesso do `aplicar()` puro (o `main` decide o reinício depois).
    pub fn success_without_restart(sha: String) -> Self {
        Self {
            ok: true,
            error: None,
            sha: Some(sha),
            reiniciou: None,
            pode_atualizar: None,
            motivo: None,
            detalhe: None,
        }
    }

    pub fn success_with_error(sha: String, restarted: bool, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            error: Some(message.into()),
            sha: Some(sha),
            reiniciou: Some(restarted),
            pode_atualizar: None,
            motivo: None,
            detalhe: None,
        }
    }
}

/// Canal Git com todos os efeitos injetados.
pub struct GitChannel {
    runner: Box<dyn GitRunner>,
    fs: Box<dyn FsProbe>,
    http: Box<dyn HttpProbe>,
    restart: Option<Box<dyn RestartProbe>>,
    root: PathBuf,
    app_dir: PathBuf,
    repo: String,
    branch: String,
}

impl GitChannel {
    pub fn new<R, F, H>(runner: R, fs: F, http: H) -> Self
    where
        R: GitRunner + 'static,
        F: FsProbe + 'static,
        H: HttpProbe + 'static,
    {
        Self {
            runner: Box::new(runner),
            fs: Box::new(fs),
            http: Box::new(http),
            restart: None,
            root: PathBuf::from("."),
            app_dir: PathBuf::from("app"),
            repo: REPO.to_string(),
            branch: BRANCH.to_string(),
        }
    }

    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self.app_dir = self.root.join("app");
        self
    }

    pub fn with_app_dir(mut self, app_dir: impl Into<PathBuf>) -> Self {
        self.app_dir = app_dir.into();
        self
    }

    /// Injeta o efeito de reinício. Sem ele, `ApplyResult.reiniciou` fica
    /// ausente (o `main` do Electron decide o reinício fora do `aplicar`).
    pub fn with_restart<R: RestartProbe + 'static>(mut self, restart: R) -> Self {
        self.restart = Some(Box::new(restart));
        self
    }

    pub fn with_repo(mut self, repo: impl Into<String>) -> Self {
        self.repo = repo.into();
        self
    }

    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = branch.into();
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    fn run(&mut self, program: &str, args: &[&str], cwd: &Path) -> CommandOutput {
        let command = CommandSpec::new(program, args, cwd);
        let output = self.runner.run(&command);
        CommandOutput {
            ok: output.ok,
            saida: output.saida.trim().to_string(),
            erro: output.erro.trim().to_string(),
        }
    }

    fn git(&mut self, args: &[&str]) -> CommandOutput {
        let root = self.root.clone();
        self.run("git", args, &root)
    }

    /// `estado()`: sem `.git`, branch errada, árvore suja ou commit não enviado.
    pub fn state(&mut self) -> UpdateState {
        if !self.fs.exists(&self.root.join(".git")) {
            return UpdateState::blocked(UpdateReason::SemGit, None);
        }

        let branch = self.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
        if !branch.ok {
            return UpdateState::blocked(UpdateReason::SemGit, None);
        }
        if branch.saida != self.branch {
            return UpdateState::blocked(UpdateReason::Branch, Some(branch.saida));
        }

        // `-uno`: arquivo não versionado que o usuário deixou na pasta não
        // bloqueia o fast-forward; só alteração versionada bloqueia.
        let dirty = self.git(&["status", "--porcelain", "-uno"]);
        if dirty.ok && !dirty.saida.is_empty() {
            let files = dirty.saida.split('\n').count();
            return UpdateState::blocked(UpdateReason::Sujo, Some(files.to_string()));
        }

        let unsent = self.git(&["rev-list", "--count", "@{u}..HEAD"]);
        if unsent.ok {
            let count = unsent.saida.parse::<f64>().ok().unwrap_or(f64::NAN);
            if count > 0.0 {
                return UpdateState::blocked(UpdateReason::NaoEnviado, Some(unsent.saida));
            }
        }

        UpdateState::can_update()
    }

    /// `shaLocal()`: SHA completo do HEAD ou string vazia.
    pub fn sha_local(&mut self) -> String {
        let result = self.git(&["rev-parse", "HEAD"]);
        if result.ok {
            result.saida
        } else {
            String::new()
        }
    }

    /// `verificar()`: uma chamada ao `compare` do GitHub resolve commits,
    /// atraso e mudança de dependências.
    pub fn check(&mut self) -> UpdateCheck {
        let local = self.sha_local();
        if local.is_empty() {
            return UpdateCheck::error("não é um clone git");
        }

        let url = format!(
            "{GITHUB_API}/{}/compare/{}...{}",
            self.repo, local, self.branch
        );
        let request = HttpRequest {
            url,
            headers: vec![("User-Agent".to_string(), "arcadia".to_string())],
            timeout_ms: COMPARE_TIMEOUT_MS,
        };
        let response = match self.http.get(&request) {
            Ok(response) => response,
            Err(error) => return UpdateCheck::error(error),
        };
        if !(200..=299).contains(&response.status) {
            return UpdateCheck::error(format!("GitHub HTTP {}", response.status));
        }

        let data: Value = match serde_json::from_str(&response.body) {
            Ok(data) => data,
            Err(error) => return UpdateCheck::error(error.to_string()),
        };
        if data.is_null() {
            return UpdateCheck::error("resposta_invalida");
        }

        let atrasado = data.get("ahead_by").and_then(js::number).unwrap_or(0.0) as i64;

        // `(d.commits || [])`: null/false/0/"" caem no array vazio; qualquer
        // outro valor "truthy" que não seja array faz o `.map` lançar no JS.
        let commit_items = match data.get("commits") {
            Some(Value::Array(items)) => items.as_slice(),
            Some(value) if js::truthy(value) => {
                return UpdateCheck::error("resposta_invalida");
            }
            _ => &[],
        };
        let mut commits = Vec::with_capacity(commit_items.len());
        for commit in commit_items {
            if commit.is_null() {
                return UpdateCheck::error("resposta_invalida");
            }
            let sha = js::string_or_empty(commit.get("sha"));
            let message = js::string_or_empty(
                commit
                    .get("commit")
                    .and_then(|commit| commit.get("message")),
            );
            commits.push(CommitInfo {
                sha: sha.chars().take(7).collect(),
                titulo: message.split('\n').next().unwrap_or_default().to_string(),
            });
        }
        commits.reverse();

        let file_items = match data.get("files") {
            Some(Value::Array(items)) => Some(items.as_slice()),
            Some(value) if js::truthy(value) => {
                return UpdateCheck::error("resposta_invalida");
            }
            _ => None,
        };
        let deps_changed = file_items.is_some_and(|files| {
            files
                .iter()
                .any(|file| file.get("filename").and_then(Value::as_str) == Some(DEPS_FILE))
        });

        UpdateCheck::success(
            local.chars().take(7).collect(),
            atrasado,
            commits,
            deps_changed,
        )
    }

    /// `aplicar()` sem callback de progresso.
    pub fn apply(&mut self, deps_changed: bool) -> ApplyResult {
        let mut noop = |_progress: UpdateProgress| {};
        self.apply_with(deps_changed, &mut noop)
    }

    /// `aplicar(onProgresso, depsMudaram)`.
    pub fn apply_with(
        &mut self,
        deps_changed: bool,
        on_progress: &mut dyn FnMut(UpdateProgress),
    ) -> ApplyResult {
        let state = self.state();
        if !state.pode_atualizar {
            let reason = state.motivo.unwrap_or(UpdateReason::SemGit);
            return ApplyResult::blocked(format!("bloqueado: {reason}"), &state);
        }

        on_progress(UpdateProgress::new(UpdatePhase::Pull));
        // `--ff-only` de propósito: se não avançar limpo, é para PARAR.
        let pull = self.git(&["pull", "--ff-only"]);
        if !pull.ok {
            return ApplyResult::error(if pull.erro.is_empty() {
                "git pull falhou".to_string()
            } else {
                pull.erro
            });
        }

        if deps_changed {
            on_progress(UpdateProgress::new(UpdatePhase::Deps));
            let app_dir = self.app_dir.clone();
            let npm = self.run("npm", &["install", "--no-audit", "--no-fund"], &app_dir);
            if !npm.ok {
                return ApplyResult::error(if npm.erro.is_empty() {
                    "npm install falhou".to_string()
                } else {
                    npm.erro
                });
            }
        }

        on_progress(UpdateProgress::new(UpdatePhase::Build));
        let app_dir = self.app_dir.clone();
        let build = self.run("npm", &["run", "build"], &app_dir);
        if !build.ok {
            return ApplyResult::error(if build.erro.is_empty() {
                "build falhou".to_string()
            } else {
                build.erro
            });
        }

        on_progress(UpdateProgress::new(UpdatePhase::Pronto));
        let sha: String = self.sha_local().chars().take(7).collect();
        match self.restart.as_mut() {
            Some(restart) => match restart.request_restart() {
                Ok(()) => ApplyResult::success(sha, true),
                Err(error) => ApplyResult::success_with_error(sha, false, error),
            },
            None => ApplyResult::success_without_restart(sha),
        }
    }

    // Aliases com os nomes do oráculo.
    pub fn estado(&mut self) -> UpdateState {
        self.state()
    }

    pub fn verificar(&mut self) -> UpdateCheck {
        self.check()
    }

    pub fn aplicar(&mut self, deps_mudaram: bool) -> ApplyResult {
        self.apply(deps_mudaram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_serialized_like_the_oracle() {
        let value = serde_json::to_value(UpdateState::can_update()).unwrap();
        assert_eq!(value, serde_json::json!({ "podeAtualizar": true }));

        let value =
            serde_json::to_value(UpdateState::blocked(UpdateReason::Sujo, Some("2".into())))
                .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "podeAtualizar": false,
                "motivo": "sujo",
                "detalhe": "2"
            })
        );

        let value = serde_json::to_value(UpdateState::blocked(
            UpdateReason::NaoEnviado,
            Some("3".into()),
        ))
        .unwrap();
        assert_eq!(value["motivo"], serde_json::json!("nao-enviado"));
    }

    #[test]
    fn check_and_progress_envelopes_match() {
        let value = serde_json::to_value(UpdateCheck::error("sem rede")).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "ok": false, "error": "sem rede" })
        );

        let value = serde_json::to_value(UpdateProgress::new(UpdatePhase::Deps)).unwrap();
        assert_eq!(value, serde_json::json!({ "etapa": "deps" }));

        let value = serde_json::to_value(ApplyResult::error("build falhou")).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "ok": false, "error": "build falhou" })
        );

        let value = serde_json::to_value(ApplyResult::success("abc1234".into(), true)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "ok": true, "sha": "abc1234", "reiniciou": true })
        );
    }

    #[test]
    fn command_spec_builds_expected_line() {
        let spec = CommandSpec::new("npm", &["run", "build"], "app");
        assert_eq!(spec.command_line(), "npm run build");
        assert_eq!(spec.cwd, PathBuf::from("app"));
    }
}
