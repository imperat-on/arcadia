"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-sync-account-switch-"))
process.env.ARCADIA_DATA_DIR = DIR
const conta = require("../electron/supabase/conta.js")
const sync = require("../electron/supabase/sync.js")
const { getClient } = require("../electron/supabase/client.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

test("push não drena nem grava na conta nova se a sessão troca durante o RPC", async () => {
  conta.definirConta("alice")
  sync.enqueue([{ appid: "730", apiname: "ach", unlocked_at: 1700000000 }])
  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "alice-id" } }, error: null })
  client.rpc = async () => {
    conta.definirConta("bob")
    return { data: [], error: null }
  }

  const result = await sync.pushDelta()
  assert.equal(result.ok, false)
  assert.equal(result.error, "conta_trocada")
  assert.deepEqual(
    JSON.parse(fs.readFileSync(path.join(DIR, "contas", "alice", "sync_queue.json"), "utf8")),
    [{ appid: "730", apiname: "ach", unlocked_at: 1700000000, achieved: true }],
  )
  assert.equal(fs.existsSync(path.join(DIR, "contas", "bob", "sync_queue.json")), false)
})
