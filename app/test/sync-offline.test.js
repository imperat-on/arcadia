"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-sync-offline-"))
process.env.ARCADIA_DATA_DIR = DIR
const sync = require("../electron/supabase/sync.js")
const { getClient } = require("../electron/supabase/client.js")
const conta = require("../electron/supabase/conta.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

test("fila persiste quando o RPC falha e drena somente após sucesso", async () => {
  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u1" } }, error: null })
  let calls = 0
  client.rpc = async () => {
    calls++
    if (calls === 1) return { data: null, error: { message: "Failed to fetch" } }
    return { data: [], error: null }
  }
  sync.enqueue([{ appid: "730", apiname: "offline", unlocked_at: 1700000000 }])

  const failed = await sync.pushDelta()
  assert.equal(failed.ok, false)
  assert.equal(failed.retryable, true)
  assert.equal(sync.queueLength(), 1)

  const sent = await sync.pushDelta()
  assert.equal(sent.ok, true)
  assert.equal(sent.pushed, 1)
  assert.equal(sync.queueLength(), 0)
  assert.equal(calls, 2)
  assert.ok(fs.existsSync(path.join(DIR, "sync_queue.json")))
})


test("syncNow registra erro quando a sessão não pode ser obtida", async () => {
  const client = getClient()
  const originalGetUser = client.auth.getUser
  conta.definirConta("sessao-expirada")
  client.auth.getUser = async () => ({
    data: { user: null },
    error: { message: "token invalido", status: 401 },
  })
  try {
    const result = await sync.syncNow()
    assert.deepEqual(result, { ok: false, error: "nao_logado", retryable: false })
    const state = JSON.parse(
      fs.readFileSync(path.join(DIR, "contas", "sessao-expirada", "sync_state.json"), "utf8"),
    )
    assert.equal(state.lastError, "nao_logado")
  } finally {
    client.auth.getUser = originalGetUser
    conta.definirConta(null)
  }
})
