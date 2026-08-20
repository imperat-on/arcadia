"use strict"

// Regras puras do download manager. O processo Electron fica responsável por
// spawn/persistência; este módulo só decide quando uma saída é íntegra e quando
// um download interrompido pode ser recuperado. Assim as decisões críticas
// podem ser testadas sem Electron, rede ou processos reais.

const DEFAULT_MAX_RECOVERY_ATTEMPTS = 2
const DEFAULT_RETRY_DELAY_MS = 1500
const MAX_RETRY_DELAY_MS = 30000
const VERIFY_FAILURE =
  /(?:error|fail(?:ed|ure)?|unable|aborting|denied|forbidden|missing|mismatch|corrupt|invalid|not[ ._-]+complete|incomplete)/i

function asNonNegativeInt(value, fallback = 0) {
  const n = Number(value)
  if (!Number.isFinite(n) || n < 0) return fallback
  return Math.trunc(n)
}

function normalizeRecoveryAttempts(value) {
  return asNonNegativeInt(value)
}

function normalizeMaxRecoveryAttempts(value = DEFAULT_MAX_RECOVERY_ATTEMPTS) {
  const n = asNonNegativeInt(value, DEFAULT_MAX_RECOVERY_ATTEMPTS)
  return Math.min(10, n)
}

function retryDelay(attempt, base = DEFAULT_RETRY_DELAY_MS) {
  const n = asNonNegativeInt(attempt)
  const b = Number(base)
  if (!Number.isFinite(b) || b < 0) return DEFAULT_RETRY_DELAY_MS
  return Math.min(MAX_RETRY_DELAY_MS, Math.round(b * 2 ** Math.max(0, n - 1)))
}

function failureMessage({ code, signal, error, phase = "download" } = {}) {
  const detail = String(error || "")
    .trim()
    .replace(/\s+/g, " ")
    .slice(0, 300)
  if (detail) return detail
  if (signal) return `${phase} interrompido por ${signal}`
  if (code == null) return `${phase} falhou ao iniciar`
  return `${phase} falhou (código ${code})`
}

/**
 * Decide o próximo passo de um processo de download/verificação.
 *
 * `phase` é "download", "depot" ou "verify". Só um download Epic bem
 * sucedido pede a segunda fase de verificação; o DepotDownloader já executa
 * validação de blocos quando recebe `-validate`, portanto o manager pode
 * concluir um depot Steam com código 0.
 */
function recoveryDecision({
  phase = "download",
  engine = "epic",
  code,
  signal,
  status = "downloading",
  attempts = 0,
  maxAttempts = DEFAULT_MAX_RECOVERY_ATTEMPTS,
  error = "",
} = {}) {
  const current = normalizeRecoveryAttempts(attempts)
  const limit = normalizeMaxRecoveryAttempts(maxAttempts)

  // SIGSTOP não gera close; esta guarda cobre também cancelamento/pausa que
  // chega junto com o close de um processo já sinalizado.
  if (status === "paused" || status === "canceled") {
    return { action: "stopped", attempts: current }
  }

  const succeeded = Number(code) === 0 && !signal
  if (succeeded) {
    if (phase === "download" && engine !== "steam") {
      return { action: "verify", attempts: current }
    }
    return { action: "done", attempts: current }
  }

  const reason = failureMessage({ code, signal, error, phase })
  if (current < limit) {
    const nextAttempts = current + 1
    return {
      action: "retry",
      attempts: nextAttempts,
      delayMs: retryDelay(nextAttempts),
      error: reason,
    }
  }
  return { action: "error", attempts: current, error: reason }
}

/**
 * Comando de verificação nativo do Legendary. Null significa que o item não
 * possui dados suficientes para uma verificação Epic (itens Steam são
 * validados pelo `-validate` do DepotDownloader).
 */
function verificationCommand(item, bin, defaultBasePath) {
  if (!item || item.engine === "steam") return null
  const appName = String(item.appName || "").trim()
  if (!appName || !bin) return null
  const basePath = String(item.installPath || defaultBasePath || "").trim()
  if (!basePath) return null
  return {
    cmd: bin,
    args: ["verify", appName, "--base-path", basePath],
  }
}

function verificationOutputLooksFailed(output) {
  return VERIFY_FAILURE.test(String(output || ""))
}

function integrityMode(item) {
  return item?.engine === "steam" ? "depot-manifest" : "legendary-verify"
}

module.exports = {
  DEFAULT_MAX_RECOVERY_ATTEMPTS,
  DEFAULT_RETRY_DELAY_MS,
  normalizeRecoveryAttempts,
  normalizeMaxRecoveryAttempts,
  retryDelay,
  failureMessage,
  recoveryDecision,
  verificationCommand,
  verificationOutputLooksFailed,
  integrityMode,
}
