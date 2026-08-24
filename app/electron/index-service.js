"use strict"

const { execFile: execFileDefault } = require("node:child_process")

const DEFAULT_TIMEOUT_MS = 5 * 60 * 1000

/**
 * Executa o indexador Python como um job deduplicado e observável.
 * O processo principal injeta somente o adaptador; nenhuma regra de provider
 * ou de UI entra aqui. Duas ações simultâneas compartilham o mesmo processo.
 */
function createIndexerService({
  indexPath,
  pythonPath = "python3",
  cwd,
  env = process.env,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  maxBuffer = 4 * 1024 * 1024,
  execFileImpl = execFileDefault,
  logger = () => {},
} = {}) {
  if (!indexPath) throw new Error("indexPath é obrigatório")
  let active = null
  let child = null

  function run() {
    if (active) return active
    active = new Promise((resolve) => {
      const finish = (error, stdout = "", stderr = "") => {
        child = null
        const out = String(stdout || "")
        const err = String(stderr || "")
        if (!error) return resolve({ ok: true, code: 0, stdout: out, stderr: err })

        const timedOut = error.code === "ETIMEDOUT" || error.killed === true
        const message = timedOut ? "indexador_timeout" : String(error.message || error)
        try {
          logger(`${message}${err ? `: ${err.trim().split("\n").pop()}` : ""}`)
        } catch {
          // Diagnóstico nunca pode alterar o resultado do job.
        }
        resolve({
          ok: false,
          code: error.code ?? null,
          signal: error.signal ?? null,
          error: message,
          stdout: out,
          stderr: err,
        })
      }

      try {
        child = execFileImpl(
          pythonPath,
          [indexPath],
          {
            cwd,
            env: { ...env },
            timeout: timeoutMs,
            maxBuffer,
          },
          finish,
        )
      } catch (error) {
        finish(error)
      }
    }).finally(() => {
      active = null
    })
    return active
  }

  function cancel() {
    if (!child || typeof child.kill !== "function") return false
    child.kill("SIGTERM")
    return true
  }

  return {
    run,
    cancel,
    isRunning: () => Boolean(active),
  }
}

module.exports = { DEFAULT_TIMEOUT_MS, createIndexerService }
