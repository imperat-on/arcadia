"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  integrityMode,
  recoveryDecision,
  retryDelay,
  verificationCommand,
  verificationOutputLooksFailed,
} = require("../electron/download-integrity")

test("comando de verificação Epic usa o app e o destino, sem aceitar Steam", () => {
  assert.deepEqual(
    verificationCommand(
      { appName: "Fortnite", installPath: "/games" },
      "/bin/legendary",
      "/fallback",
    ),
    {
      cmd: "/bin/legendary",
      args: ["verify", "Fortnite", "--base-path", "/games"],
    },
  )
  assert.deepEqual(verificationCommand({ appName: "Fortnite" }, "/bin/legendary", "/fallback"), {
    cmd: "/bin/legendary",
    args: ["verify", "Fortnite", "--base-path", "/fallback"],
  })
  assert.equal(
    verificationCommand({ engine: "steam", appName: "440" }, "/bin/legendary", "/games"),
    null,
  )
  assert.equal(verificationCommand({ installPath: "/games" }, "/bin/legendary", "/games"), null)
})

test("código 0 Epic exige verify antes de permitir done", () => {
  assert.deepEqual(recoveryDecision({ phase: "download", engine: "epic", code: 0, attempts: 0 }), {
    action: "verify",
    attempts: 0,
  })
  assert.deepEqual(recoveryDecision({ phase: "verify", engine: "epic", code: 0, attempts: 0 }), {
    action: "done",
    attempts: 0,
  })
  assert.deepEqual(recoveryDecision({ phase: "depot", engine: "steam", code: 0, attempts: 0 }), {
    action: "done",
    attempts: 0,
  })
})

test("falha recuperável preserva tentativas e termina sem loop infinito", () => {
  assert.deepEqual(
    recoveryDecision({ phase: "download", code: 1, error: "network reset", attempts: 0 }),
    {
      action: "retry",
      attempts: 1,
      delayMs: 1500,
      error: "network reset",
    },
  )
  assert.equal(recoveryDecision({ phase: "verify", code: 1, attempts: 2 }).action, "error")
  assert.match(recoveryDecision({ phase: "verify", code: 1, attempts: 2 }).error, /verify falhou/)
  assert.equal(retryDelay(5), 24000)
  assert.equal(retryDelay(10), 30000)
})

test("pausa/cancelamento nunca viram retry e output de integridade não é sucesso", () => {
  assert.deepEqual(recoveryDecision({ code: 137, status: "paused", attempts: 0 }), {
    action: "stopped",
    attempts: 0,
  })
  assert.deepEqual(recoveryDecision({ code: 137, status: "canceled", attempts: 1 }), {
    action: "stopped",
    attempts: 1,
  })
  assert.equal(verificationOutputLooksFailed("ERROR: missing files"), true)
  assert.equal(verificationOutputLooksFailed("Verification complete"), false)
  assert.equal(verificationOutputLooksFailed("Verification complete: 0 errors"), false)
  assert.equal(integrityMode({ engine: "steam" }), "depot-manifest")
  assert.equal(integrityMode({ engine: "epic" }), "legendary-verify")
})
