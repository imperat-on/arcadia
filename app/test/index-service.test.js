"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { EventEmitter } = require("node:events")
const { createIndexerService } = require("../electron/index-service")

test("indexer deduplica execuções concorrentes e devolve stdout/stderr", async () => {
  const calls = []
  let finish
  const service = createIndexerService({
    indexPath: "/tmp/index.py",
    env: { ARCADIA_DATA_DIR: "/tmp/data" },
    execFileImpl: (command, args, options, callback) => {
      calls.push({ command, args, options })
      finish = () => callback(null, "ok\n", "")
      return { kill: () => {} }
    },
  })

  const first = service.run()
  const second = service.run()
  assert.strictEqual(first, second)
  assert.equal(service.isRunning(), true)
  finish()
  assert.deepEqual(await first, { ok: true, code: 0, stdout: "ok\n", stderr: "" })
  assert.equal(calls.length, 1)
  assert.equal(calls[0].command, "python3")
  assert.deepEqual(calls[0].args, ["/tmp/index.py"])
  assert.equal(calls[0].options.env.ARCADIA_DATA_DIR, "/tmp/data")
  assert.equal(service.isRunning(), false)
})

test("indexer transforma falha/timeout em diagnóstico e permite retry", async () => {
  const errors = []
  let attempt = 0
  const service = createIndexerService({
    indexPath: "/tmp/index.py",
    logger: (message) => errors.push(message),
    execFileImpl: (_command, _args, _options, callback) => {
      attempt++
      if (attempt === 1) return callback(Object.assign(new Error("timeout"), { code: "ETIMEDOUT" }), "", "timed out")
      callback(Object.assign(new Error("exit 2"), { code: 2 }), "partial", "provider failed")
      return { kill: () => {} }
    },
  })

  assert.equal((await service.run()).error, "indexador_timeout")
  assert.equal((await service.run()).ok, false)
  assert.equal(attempt, 2)
  assert.equal(errors.length, 2)
})

test("indexer cancel chama kill no processo ativo", async () => {
  const child = new EventEmitter()
  let callback
  child.kill = (signal) => {
    assert.equal(signal, "SIGTERM")
    callback(Object.assign(new Error("terminated"), { killed: true, signal }))
  }
  const service = createIndexerService({
    indexPath: "/tmp/index.py",
    execFileImpl: (_command, _args, _options, done) => {
      callback = done
      return child
    },
  })
  const running = service.run()
  assert.equal(service.cancel(), true)
  const result = await running
  assert.equal(result.ok, false)
  assert.equal(result.error, "indexador_timeout")
})
