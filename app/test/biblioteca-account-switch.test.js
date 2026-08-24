"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-biblioteca-account-switch-"))
process.env.ARCADIA_DATA_DIR = DIR
const conta = require("../electron/supabase/conta.js")
const biblioteca = require("../electron/supabase/biblioteca.js")
const { getClient } = require("../electron/supabase/client.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

function arquivo(username, nome) {
  conta.definirConta(username)
  return conta.caminhoArquivoConta(nome)
}

test("push não grava watermark da conta nova após troca durante RPC", async () => {
  conta.definirConta("alice")
  fs.writeFileSync(
    conta.caminhoArquivoConta("custom_games.json"),
    JSON.stringify([{ id: "custom:1", title: "Jogo", launcher: "custom" }]),
  )
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({}))
  const aliceState = conta.caminhoArquivoConta("sync_state.json")

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "alice-id" } }, error: null })
  client.rpc = async (fn) => {
    assert.equal(fn, "push_library")
    conta.definirConta("bob")
    return { data: null, error: null }
  }

  const result = await biblioteca.push()
  assert.deepEqual(result, { ok: false, error: "conta_trocada", retryable: false })
  assert.deepEqual(JSON.parse(fs.readFileSync(aliceState, "utf8")), {})
  assert.equal(fs.existsSync(arquivo("bob", "sync_state.json")), false)
})

test("pull descarta resposta antiga e não cria jogos na conta nova", async () => {
  conta.definirConta("alice")
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  const aliceCustom = conta.caminhoArquivoConta("custom_games.json")

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "alice-id" } }, error: null })
  client.rpc = async (fn) => {
    assert.equal(fn, "pull_library")
    conta.definirConta("bob")
    return {
      data: [{ appid: "custom:remote", title: "Jogo remoto", platform: "windows", minutes: 0 }],
      error: null,
    }
  }

  const result = await biblioteca.pull()
  assert.equal(result, false)
  assert.deepEqual(JSON.parse(fs.readFileSync(aliceCustom, "utf8")), [])
  assert.equal(fs.existsSync(arquivo("bob", "custom_games.json")), false)
})
