//! Regras puras de integridade e recuperação de downloads.
//!
//! Port 1:1 de `app/electron/download-integrity.js`. O processo Electron fica
//! responsável por spawn/persistência; aqui só se decide quando uma saída é
//! íntegra, quando um download interrompido pode ser recuperado e qual é o
//! próximo passo de um processo de download/verificação.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js::{collapse_whitespace, js_number, js_string, js_trim, js_truthy, truncate_utf16};

pub const DEFAULT_MAX_RECOVERY_ATTEMPTS: i64 = 2;
pub const DEFAULT_RETRY_DELAY_MS: i64 = 1500;
pub const MAX_RETRY_DELAY_MS: i64 = 30000;
pub const MAX_ERROR_CHARS: usize = 300;
pub const MAX_RECOVERY_ATTEMPTS: i64 = 10;

pub const PHASE_DOWNLOAD: &str = "download";
pub const PHASE_DEPOT: &str = "depot";
pub const PHASE_VERIFY: &str = "verify";

pub const ENGINE_EPIC: &str = "epic";
pub const ENGINE_STEAM: &str = "steam";

/// Equivalente ao helper privado `asNonNegativeInt(value, fallback = 0)`.
///
/// `Number(value)` não finito ou negativo devolve o fallback; o restante é
/// truncado em direção a zero.
pub fn as_non_negative_int(value: &Value, fallback: i64) -> i64 {
    match js_number(value) {
        Some(n) if n.is_finite() && n >= 0.0 => crate::js::js_trunc(n),
        _ => fallback,
    }
}

/// `normalizeRecoveryAttempts(value)`.
pub fn normalize_recovery_attempts(value: &Value) -> i64 {
    as_non_negative_int(value, 0)
}

/// `normalizeMaxRecoveryAttempts(value = 2)`, limitado a 10.
///
/// A função JS aplica o default apenas quando o argumento é `undefined`. Um
/// `null` explícito vira `Number(null) === 0`; quem chama sem valor deve passar
/// [`DEFAULT_MAX_RECOVERY_ATTEMPTS`] como fallback (feito por
/// [`recovery_decision`]).
pub fn normalize_max_recovery_attempts(value: &Value) -> i64 {
    as_non_negative_int(value, DEFAULT_MAX_RECOVERY_ATTEMPTS).min(MAX_RECOVERY_ATTEMPTS)
}

/// `retryDelay(attempt, base = 1500)`.
pub fn retry_delay(attempt: &Value) -> i64 {
    retry_delay_with_base(attempt, &Value::from(DEFAULT_RETRY_DELAY_MS))
}

/// `retryDelay(attempt, base)` com o `base` vindo do oráculo.
pub fn retry_delay_with_base(attempt: &Value, base: &Value) -> i64 {
    let attempt = as_non_negative_int(attempt, 0);
    let base = match js_number(base) {
        Some(base) if base.is_finite() && base >= 0.0 => base,
        _ => return DEFAULT_RETRY_DELAY_MS,
    };
    let exponent = attempt.saturating_sub(1).clamp(0, 1023) as i32;
    let raw = base * 2f64.powi(exponent);
    if raw.is_nan() {
        return 0;
    }
    let rounded = raw.round();
    if rounded >= MAX_RETRY_DELAY_MS as f64 {
        MAX_RETRY_DELAY_MS
    } else {
        rounded.max(0.0) as i64
    }
}

/// `failureMessage({ code, signal, error, phase = "download" })`.
pub fn failure_message(input: &Value) -> String {
    let error = input.get("error");
    let signal = input.get("signal");
    let code = input.get("code");
    let phase = input
        .get("phase")
        .map(js_string)
        .unwrap_or_else(|| PHASE_DOWNLOAD.to_string());

    let raw = match error {
        Some(value) if js_truthy(value) => js_string(value),
        _ => String::new(),
    };
    let detail = truncate_utf16(&collapse_whitespace(&js_trim(&raw)), MAX_ERROR_CHARS);
    if !detail.is_empty() {
        return detail;
    }

    if let Some(signal) = signal.filter(|value| js_truthy(value)) {
        return format!("{phase} interrompido por {}", js_string(signal));
    }
    match code {
        None | Some(Value::Null) => format!("{phase} falhou ao iniciar"),
        Some(code) => format!("{phase} falhou (código {})", js_string(code)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryAction {
    Stopped,
    Verify,
    Done,
    Retry,
    Error,
}

impl RecoveryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Verify => "verify",
            Self::Done => "done",
            Self::Retry => "retry",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resultado de `recoveryDecision`. Os campos opcionais só existem nas ações
/// que os produzem no oráculo (`delayMs` em retry, `error` em retry/error).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDecision {
    pub action: RecoveryAction,
    pub attempts: i64,
    #[serde(rename = "delayMs", skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RecoveryDecision {
    fn stopped(attempts: i64) -> Self {
        Self {
            action: RecoveryAction::Stopped,
            attempts,
            delay_ms: None,
            error: None,
        }
    }

    fn verify(attempts: i64) -> Self {
        Self {
            action: RecoveryAction::Verify,
            attempts,
            delay_ms: None,
            error: None,
        }
    }

    fn done(attempts: i64) -> Self {
        Self {
            action: RecoveryAction::Done,
            attempts,
            delay_ms: None,
            error: None,
        }
    }

    fn retry(attempts: i64, delay_ms: i64, error: String) -> Self {
        Self {
            action: RecoveryAction::Retry,
            attempts,
            delay_ms: Some(delay_ms),
            error: Some(error),
        }
    }

    fn error(attempts: i64, error: String) -> Self {
        Self {
            action: RecoveryAction::Error,
            attempts,
            delay_ms: None,
            error: Some(error),
        }
    }
}

/// Decide o próximo passo de um processo de download/verificação.
///
/// `phase` é `"download"`, `"depot"` ou `"verify"`. Só um download Epic bem
/// sucedido pede a segunda fase de verificação; o DepotDownloader já executa
/// validação de blocos quando recebe `-validate`, portanto o manager pode
/// concluir um depot Steam com código 0.
pub fn recovery_decision(input: &Value) -> RecoveryDecision {
    let current = input
        .get("attempts")
        .map(normalize_recovery_attempts)
        .unwrap_or(0);
    let limit = input
        .get("maxAttempts")
        .map(normalize_max_recovery_attempts)
        .unwrap_or(DEFAULT_MAX_RECOVERY_ATTEMPTS);

    let status = input
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("downloading");
    // SIGSTOP não gera close; esta guarda cobre também cancelamento/pausa que
    // chega junto com o close de um processo já sinalizado.
    if status == "paused" || status == "canceled" {
        return RecoveryDecision::stopped(current);
    }

    let code = input.get("code");
    let signal = input.get("signal");
    let succeeded =
        code.and_then(js_number).is_some_and(|code| code == 0.0) && !signal.is_some_and(js_truthy);
    if succeeded {
        let phase = input
            .get("phase")
            .map(js_string)
            .unwrap_or_else(|| PHASE_DOWNLOAD.to_string());
        let engine = input
            .get("engine")
            .map(js_string)
            .unwrap_or_else(|| ENGINE_EPIC.to_string());
        if phase == PHASE_DOWNLOAD && engine != ENGINE_STEAM {
            return RecoveryDecision::verify(current);
        }
        return RecoveryDecision::done(current);
    }

    let reason = failure_message(input);
    if current < limit {
        let next_attempts = current + 1;
        let delay = retry_delay(&Value::from(next_attempts));
        return RecoveryDecision::retry(next_attempts, delay, reason);
    }
    RecoveryDecision::error(current, reason)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCommand {
    pub cmd: String,
    pub args: Vec<String>,
}

/// Comando de verificação nativo do Legendary. `None` significa que o item
/// não possui dados suficientes para uma verificação Epic (itens Steam são
/// validados pelo `-validate` do DepotDownloader).
pub fn verification_command(
    item: &Value,
    bin: &str,
    default_base_path: &str,
) -> Option<VerificationCommand> {
    if item.get("engine").and_then(Value::as_str) == Some(ENGINE_STEAM) {
        return None;
    }
    let app_name = match item.get("appName") {
        Some(value) if js_truthy(value) => js_trim(&js_string(value)),
        _ => String::new(),
    };
    if app_name.is_empty() || bin.is_empty() {
        return None;
    }
    let base_path = match item.get("installPath") {
        Some(value) if js_truthy(value) => js_string(value),
        _ => default_base_path.to_string(),
    };
    let base_path = js_trim(&base_path);
    if base_path.is_empty() {
        return None;
    }
    Some(VerificationCommand {
        cmd: bin.to_string(),
        args: vec![
            "verify".to_string(),
            app_name,
            "--base-path".to_string(),
            base_path,
        ],
    })
}

const VERIFY_FAILURE_WORDS: &[&str] = &[
    "error",
    "fail",
    "failed",
    "failure",
    "unable",
    "aborting",
    "denied",
    "forbidden",
    "missing",
    "mismatch",
    "corrupt",
    "invalid",
];

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    haystack.match_indices(needle).any(|(start, _)| {
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let end = start + needle.len();
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        before_ok && after_ok
    })
}

fn contains_not_complete(haystack: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = haystack[cursor..].find("not") {
        let start = cursor + offset;
        let mut index = start + 3;
        let mut separators = 0;
        while index < bytes.len() && matches!(bytes[index], b' ' | b'.' | b'_' | b'-') {
            index += 1;
            separators += 1;
        }
        if separators > 0 && haystack[index..].starts_with("complete") {
            return true;
        }
        cursor = start + 3;
    }
    false
}

/// `verificationOutputLooksFailed(output)`.
pub fn verification_output_looks_failed(output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    if output.contains("incomplete") {
        return true;
    }
    if contains_not_complete(&output) {
        return true;
    }
    VERIFY_FAILURE_WORDS
        .iter()
        .any(|word| contains_ascii_word(&output, word))
}

/// Variante que aplica `String(output || "")` antes da verificação.
pub fn verification_output_value_looks_failed(output: &Value) -> bool {
    let text = if js_truthy(output) {
        js_string(output)
    } else {
        String::new()
    };
    verification_output_looks_failed(&text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrityMode {
    LegendaryVerify,
    DepotManifest,
}

impl IntegrityMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegendaryVerify => "legendary-verify",
            Self::DepotManifest => "depot-manifest",
        }
    }
}

impl std::fmt::Display for IntegrityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `integrityMode(item)`.
pub fn integrity_mode(item: &Value) -> IntegrityMode {
    if item.get("engine").and_then(Value::as_str) == Some(ENGINE_STEAM) {
        IntegrityMode::DepotManifest
    } else {
        IntegrityMode::LegendaryVerify
    }
}

// ---------------------------------------------------------------------------
// Validação de arquivo/parcial (decisões puras usadas pelo staging)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityProblem {
    InvalidExpectedSize,
    InvalidActualSize,
    SizeExceedsExpected { expected: u64, actual: u64 },
    HashMismatch { expected: String, actual: String },
    MissingHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIntegrity {
    /// Tamanho (e hash, quando esperado) conferem; o parcial pode ser aceito
    /// como arquivo final.
    Complete,
    /// Parcial dentro do tamanho esperado: mantenha e retome deste offset.
    Resume { offset: u64 },
    /// Parcial vazio/inutilizável ou servidor sem Range: recomece do zero.
    Restart,
    /// Parcial inválido: descarte/limpe antes de tentar de novo.
    Reject(IntegrityProblem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeOffset {
    Complete,
    ResumeFrom(u64),
    Restart,
    Reject(IntegrityProblem),
}

fn normalize_hash(hash: Option<&str>) -> Option<String> {
    hash.map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(str::to_ascii_lowercase)
}

/// Valida um arquivo/parcial contra tamanho e SHA-256 esperados.
///
/// Regras:
/// - tamanho negativo é inválido (não vira 0 silenciosamente);
/// - parcial maior que o esperado é rejeitado (não é um prefixo válido);
/// - parcial menor que o esperado é aceito para resume no offset atual;
/// - parcial vazio pede restart;
/// - no tamanho final, hash esperado exige hash real e compara sem diferenciar
///   maiúsculas/minúsculas;
/// - hash ausente no arquivo real, quando há hash esperado, é rejeitado.
pub fn check_file(
    expected_size: i64,
    expected_sha256: Option<&str>,
    actual_size: i64,
    actual_sha256: Option<&str>,
) -> FileIntegrity {
    match resume_offset(expected_size, actual_size, true) {
        ResumeOffset::Complete => {
            if let Some(expected) = normalize_hash(expected_sha256) {
                let Some(actual) = normalize_hash(actual_sha256) else {
                    return FileIntegrity::Reject(IntegrityProblem::MissingHash);
                };
                if !actual.eq_ignore_ascii_case(&expected) {
                    return FileIntegrity::Reject(IntegrityProblem::HashMismatch {
                        expected,
                        actual,
                    });
                }
            }
            FileIntegrity::Complete
        }
        ResumeOffset::ResumeFrom(offset) => FileIntegrity::Resume { offset },
        ResumeOffset::Restart => FileIntegrity::Restart,
        ResumeOffset::Reject(problem) => FileIntegrity::Reject(problem),
    }
}

/// Decide se um parcial pode ser retomado e a partir de qual offset.
///
/// `range_supported` representa suporte do servidor a `Range: bytes=N-`.
/// Sem suporte, um parcial menor que o esperado precisa recomeçar do zero.
pub fn resume_offset(expected_size: i64, partial_size: i64, range_supported: bool) -> ResumeOffset {
    if expected_size < 0 {
        return ResumeOffset::Reject(IntegrityProblem::InvalidExpectedSize);
    }
    if partial_size < 0 {
        return ResumeOffset::Reject(IntegrityProblem::InvalidActualSize);
    }
    let expected = expected_size as u64;
    let partial = partial_size as u64;
    if partial > expected {
        return ResumeOffset::Reject(IntegrityProblem::SizeExceedsExpected {
            expected,
            actual: partial,
        });
    }
    if partial == expected {
        return ResumeOffset::Complete;
    }
    if partial == 0 || !range_supported {
        return ResumeOffset::Restart;
    }
    ResumeOffset::ResumeFrom(partial)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupReason {
    Completed,
    Canceled,
    IntegrityFailed,
    Retry,
    Paused,
}

/// O que apagar depois de cada desfecho. O download manager Electron apaga a
/// pasta/instalação no cancelamento, mantém parciais em retry/pausa e só remove
/// o `.part` quando o arquivo final foi aceito ou corrompeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupPlan {
    pub remove_partial: bool,
    pub remove_install_dir: bool,
    pub keep_partial: bool,
}

pub fn cleanup_plan(reason: CleanupReason) -> CleanupPlan {
    match reason {
        CleanupReason::Completed => CleanupPlan {
            remove_partial: true,
            remove_install_dir: false,
            keep_partial: false,
        },
        CleanupReason::Canceled => CleanupPlan {
            remove_partial: true,
            remove_install_dir: true,
            keep_partial: false,
        },
        CleanupReason::IntegrityFailed => CleanupPlan {
            remove_partial: true,
            remove_install_dir: false,
            keep_partial: false,
        },
        CleanupReason::Retry | CleanupReason::Paused => CleanupPlan {
            remove_partial: false,
            remove_install_dir: false,
            keep_partial: true,
        },
    }
}

/// Caminhos explícitos que o chamador deve remover, preservando os nomes de
/// campo usados pelo restante do app (`partialPath`/`installDir`).
pub fn cleanup_paths(item: &Value, reason: CleanupReason) -> Vec<String> {
    let plan = cleanup_plan(reason);
    if plan.keep_partial {
        return Vec::new();
    }
    let mut paths: Vec<String> = Vec::new();
    let keys: &[&str] = if plan.remove_install_dir {
        &["partialPath", "partial_path", "installDir", "install_dir"]
    } else {
        &["partialPath", "partial_path"]
    };
    for key in keys {
        if let Some(path) = item.get(*key).and_then(Value::as_str) {
            let path = path.trim();
            if !path.is_empty() && !paths.iter().any(|known| known == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comando_de_verificacao_epic_usa_app_e_destino_sem_aceitar_steam() {
        assert_eq!(
            verification_command(
                &json!({ "appName": "Fortnite", "installPath": "/games" }),
                "/bin/legendary",
                "/fallback",
            ),
            Some(VerificationCommand {
                cmd: "/bin/legendary".to_string(),
                args: vec![
                    "verify".to_string(),
                    "Fortnite".to_string(),
                    "--base-path".to_string(),
                    "/games".to_string(),
                ],
            })
        );
        assert_eq!(
            verification_command(
                &json!({ "appName": "Fortnite" }),
                "/bin/legendary",
                "/fallback"
            ),
            Some(VerificationCommand {
                cmd: "/bin/legendary".to_string(),
                args: vec![
                    "verify".to_string(),
                    "Fortnite".to_string(),
                    "--base-path".to_string(),
                    "/fallback".to_string(),
                ],
            })
        );
        assert_eq!(
            verification_command(
                &json!({ "engine": "steam", "appName": "440" }),
                "/bin/legendary",
                "/games",
            ),
            None
        );
        assert_eq!(
            verification_command(
                &json!({ "installPath": "/games" }),
                "/bin/legendary",
                "/games",
            ),
            None
        );
        assert_eq!(
            verification_command(&json!({ "appName": "   " }), "/bin/legendary", "/games"),
            None
        );
        assert_eq!(
            verification_command(&json!({ "appName": "X" }), "", "/games"),
            None
        );
    }

    #[test]
    fn codigo_zero_epic_exige_verify_antes_de_permitir_done() {
        assert_eq!(
            recovery_decision(&json!({
                "phase": "download", "engine": "epic", "code": 0, "attempts": 0
            })),
            RecoveryDecision {
                action: RecoveryAction::Verify,
                attempts: 0,
                delay_ms: None,
                error: None,
            }
        );
        assert_eq!(
            recovery_decision(&json!({
                "phase": "verify", "engine": "epic", "code": 0, "attempts": 0
            })),
            RecoveryDecision {
                action: RecoveryAction::Done,
                attempts: 0,
                delay_ms: None,
                error: None,
            }
        );
        assert_eq!(
            recovery_decision(&json!({
                "phase": "depot", "engine": "steam", "code": 0, "attempts": 0
            })),
            RecoveryDecision {
                action: RecoveryAction::Done,
                attempts: 0,
                delay_ms: None,
                error: None,
            }
        );
        // String "0" também é Number(code) === 0 no oráculo.
        assert_eq!(
            recovery_decision(&json!({ "code": "0", "attempts": 0 })).action,
            RecoveryAction::Verify
        );
    }

    #[test]
    fn falha_recuperavel_preserva_tentativas_e_termina_sem_loop_infinito() {
        assert_eq!(
            recovery_decision(&json!({
                "phase": "download", "code": 1, "error": "network reset", "attempts": 0
            })),
            RecoveryDecision {
                action: RecoveryAction::Retry,
                attempts: 1,
                delay_ms: Some(1500),
                error: Some("network reset".to_string()),
            }
        );
        let decision = recovery_decision(&json!({
            "phase": "verify", "code": 1, "attempts": 2
        }));
        assert_eq!(decision.action, RecoveryAction::Error);
        assert!(decision
            .error
            .as_deref()
            .is_some_and(|error| error.contains("verify falhou")));
        assert_eq!(retry_delay(&json!(5)), 24000);
        assert_eq!(retry_delay(&json!(10)), 30000);
    }

    #[test]
    fn pausa_cancelamento_nunca_viram_retry_e_output_nao_e_sucesso() {
        assert_eq!(
            recovery_decision(&json!({ "code": 137, "status": "paused", "attempts": 0 })),
            RecoveryDecision {
                action: RecoveryAction::Stopped,
                attempts: 0,
                delay_ms: None,
                error: None,
            }
        );
        assert_eq!(
            recovery_decision(&json!({ "code": 137, "status": "canceled", "attempts": 1 })),
            RecoveryDecision {
                action: RecoveryAction::Stopped,
                attempts: 1,
                delay_ms: None,
                error: None,
            }
        );
        assert!(verification_output_looks_failed("ERROR: missing files"));
        assert!(!verification_output_looks_failed("Verification complete"));
        assert!(!verification_output_looks_failed(
            "Verification complete: 0 errors"
        ));
        assert_eq!(
            integrity_mode(&json!({ "engine": "steam" })),
            IntegrityMode::DepotManifest
        );
        assert_eq!(
            integrity_mode(&json!({ "engine": "epic" })),
            IntegrityMode::LegendaryVerify
        );
        assert_eq!(integrity_mode(&json!({})), IntegrityMode::LegendaryVerify);
    }

    #[test]
    fn normalizacao_de_tentativas_e_delay_com_limites() {
        assert_eq!(normalize_recovery_attempts(&json!(-3)), 0);
        assert_eq!(normalize_recovery_attempts(&json!("2.9")), 2);
        assert_eq!(normalize_recovery_attempts(&json!("nan")), 0);
        assert_eq!(normalize_max_recovery_attempts(&json!(-1)), 2);
        assert_eq!(normalize_max_recovery_attempts(&json!(99)), 10);
        assert_eq!(normalize_max_recovery_attempts(&json!(2.9)), 2);
        assert_eq!(normalize_max_recovery_attempts(&json!(null)), 0);
        assert_eq!(retry_delay(&json!(0)), 1500);
        assert_eq!(retry_delay(&json!(-4)), 1500);
        assert_eq!(retry_delay(&json!("abc")), 1500);
        assert_eq!(retry_delay_with_base(&json!(3), &json!(-1)), 1500);
        assert_eq!(retry_delay_with_base(&json!(3), &json!("nan")), 1500);
        assert_eq!(retry_delay_with_base(&json!(3), &json!(100)), 400);
        assert_eq!(retry_delay_with_base(&json!(2), &json!(100)), 200);
        assert_eq!(retry_delay_with_base(&json!(10), &json!(100)), 30000);
    }

    #[test]
    fn failure_message_normaliza_espacos_sinal_codigo_e_tamanho() {
        assert_eq!(
            failure_message(&json!({ "error": "  network\n\t reset  " })),
            "network reset"
        );
        assert_eq!(
            failure_message(&json!({ "signal": "SIGKILL", "phase": "verify" })),
            "verify interrompido por SIGKILL"
        );
        assert_eq!(failure_message(&json!({})), "download falhou ao iniciar");
        assert_eq!(
            failure_message(&json!({ "code": null, "phase": "depot" })),
            "depot falhou ao iniciar"
        );
        assert_eq!(
            failure_message(&json!({ "code": 0 })),
            "download falhou (código 0)"
        );
        let long = "x".repeat(400);
        assert_eq!(
            failure_message(&json!({ "error": long })).chars().count(),
            300
        );
    }

    #[test]
    fn output_de_verificacao_usa_palavras_com_limite_e_not_complete() {
        for output in [
            "error",
            "failed",
            "failure",
            "unable to continue",
            "aborting",
            "denied",
            "forbidden",
            "missing files",
            "size mismatch",
            "corrupt",
            "invalid manifest",
            "not complete",
            "not_complete",
            "not - _ . complete",
            "incomplete",
            "cannot complete",
        ] {
            assert!(verification_output_looks_failed(output), "{output}");
        }
        for output in [
            "Verification complete",
            "0 errors",
            "terror",
            "disabled",
            "notcomplete",
            "uninstall",
        ] {
            assert!(!verification_output_looks_failed(output), "{output}");
        }
        assert!(!verification_output_value_looks_failed(&json!(null)));
    }

    #[test]
    fn arquivo_completo_aceita_hash_case_insensitive_e_rejeita_divergencia() {
        assert_eq!(
            check_file(10, Some("ABCDEF"), 10, Some("abcdef")),
            FileIntegrity::Complete
        );
        assert_eq!(
            check_file(10, None, 10, Some("qualquer")),
            FileIntegrity::Complete
        );
        assert_eq!(
            check_file(10, Some("aaaa"), 10, Some("bbbb")),
            FileIntegrity::Reject(IntegrityProblem::HashMismatch {
                expected: "aaaa".to_string(),
                actual: "bbbb".to_string(),
            })
        );
        assert_eq!(
            check_file(10, Some("aaaa"), 10, None),
            FileIntegrity::Reject(IntegrityProblem::MissingHash)
        );
        // Hash vazio no esperado equivale a "sem hash".
        assert_eq!(
            check_file(10, Some("  "), 10, None),
            FileIntegrity::Complete
        );
    }

    #[test]
    fn parcial_truncado_retoma_e_parcial_maior_e_rejeitado() {
        assert_eq!(
            check_file(100, Some("aaaa"), 40, None),
            FileIntegrity::Resume { offset: 40 }
        );
        assert_eq!(check_file(100, None, 0, None), FileIntegrity::Restart);
        assert_eq!(
            check_file(100, None, 120, None),
            FileIntegrity::Reject(IntegrityProblem::SizeExceedsExpected {
                expected: 100,
                actual: 120,
            })
        );
    }

    #[test]
    fn tamanhos_zero_e_negativos_sao_tratados_sem_panico() {
        assert_eq!(check_file(0, None, 0, None), FileIntegrity::Complete);
        assert_eq!(
            check_file(0, None, 1, None),
            FileIntegrity::Reject(IntegrityProblem::SizeExceedsExpected {
                expected: 0,
                actual: 1,
            })
        );
        assert_eq!(
            check_file(-1, None, 0, None),
            FileIntegrity::Reject(IntegrityProblem::InvalidExpectedSize)
        );
        assert_eq!(
            check_file(0, None, -1, None),
            FileIntegrity::Reject(IntegrityProblem::InvalidActualSize)
        );
    }

    #[test]
    fn resume_offset_respeita_range_e_tamanho_esperado() {
        assert_eq!(resume_offset(100, 40, true), ResumeOffset::ResumeFrom(40));
        assert_eq!(resume_offset(100, 40, false), ResumeOffset::Restart);
        assert_eq!(resume_offset(100, 0, true), ResumeOffset::Restart);
        assert_eq!(resume_offset(100, 100, true), ResumeOffset::Complete);
        assert_eq!(
            resume_offset(100, 101, true),
            ResumeOffset::Reject(IntegrityProblem::SizeExceedsExpected {
                expected: 100,
                actual: 101,
            })
        );
        assert_eq!(
            resume_offset(-1, 0, true),
            ResumeOffset::Reject(IntegrityProblem::InvalidExpectedSize)
        );
    }

    #[test]
    fn limpeza_mantem_parcial_em_retry_e_remove_no_cancelamento() {
        assert_eq!(
            cleanup_plan(CleanupReason::Retry),
            CleanupPlan {
                remove_partial: false,
                remove_install_dir: false,
                keep_partial: true,
            }
        );
        assert_eq!(
            cleanup_plan(CleanupReason::Paused),
            cleanup_plan(CleanupReason::Retry)
        );
        assert_eq!(
            cleanup_plan(CleanupReason::Canceled),
            CleanupPlan {
                remove_partial: true,
                remove_install_dir: true,
                keep_partial: false,
            }
        );
        assert_eq!(
            cleanup_plan(CleanupReason::Completed),
            CleanupPlan {
                remove_partial: true,
                remove_install_dir: false,
                keep_partial: false,
            }
        );

        let item = json!({
            "partialPath": "/games/a.part",
            "partial_path": "/games/a.part",
            "installDir": "/games/a",
        });
        assert!(cleanup_paths(&item, CleanupReason::Retry).is_empty());
        assert_eq!(
            cleanup_paths(&item, CleanupReason::IntegrityFailed),
            vec!["/games/a.part".to_string()]
        );
        assert_eq!(
            cleanup_paths(&item, CleanupReason::Canceled),
            vec!["/games/a.part".to_string(), "/games/a".to_string()]
        );
    }

    #[test]
    fn decision_serializa_com_os_nomes_do_oraculo() {
        let decision = recovery_decision(&json!({ "code": 1, "attempts": 0, "error": "x" }));
        assert_eq!(
            serde_json::to_value(decision).unwrap(),
            json!({
                "action": "retry",
                "attempts": 1,
                "delayMs": 1500,
                "error": "x",
            })
        );
    }
}
