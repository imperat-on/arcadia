// Testes do título real no push da biblioteca: jogos no library.json (global)
// subiam com o id feio quando não tinham stub no pending (ex.: Cyberpunk 2077
// indexado localmente). O tituloDe agora consulta o library.json também.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-titulo-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

function preparar({ owned, pending, library, rows }) {
  conta.definirConta("u1")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(owned))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify(pending))
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  // library.json GLOBAL (raiz do DATA_DIR)
  fs.writeFileSync(path.join(DIR, "library.json"), JSON.stringify(library))
  // sync_state com watermarks vazios (para o push reenviar tudo)
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u1" } }, error: null })
  client.rpc = async (fn, args) => {
    if (fn === "push_library") {
      ultimoPush = args.p_lib
      return { data: [], error: null }
    }
    if (fn === "pull_library") return { data: rows || [], error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }
}

let ultimoPush = null

test("push usa titulo do library.json quando jogo nao esta no pending", async () => {
  preparar({
    owned: ["steam:1091500"],
    pending: [], // Cyberpunk NÃO está no pending (indexado no library)
    library: [{ id: "steam:1091500", title: "Cyberpunk 2077" }],
    rows: [],
  })
  await biblioteca.push()
  assert.ok(ultimoPush, "push deve ter rodado")
  const cyberpunk = ultimoPush.find((x) => x.appid === "steam:1091500")
  assert.ok(cyberpunk, "cyberpunk deve estar no push")
  assert.equal(cyberpunk.title, "Cyberpunk 2077", "titulo deve vir do library.json, nao do id feio")
})

test("push prefere titulo do pending sobre o do library", async () => {
  preparar({
    owned: ["steam:1091500"],
    pending: [{ id: "steam:1091500", title: "Cyberpunk 2077 EDICAO" }],
    library: [{ id: "steam:1091500", title: "Cyberpunk 2077" }],
    rows: [],
  })
  ultimoPush = null
  await biblioteca.push()
  const cyberpunk = ultimoPush.find((x) => x.appid === "steam:1091500")
  assert.equal(cyberpunk.title, "Cyberpunk 2077 EDICAO", "pending tem prioridade")
})

test("push nao usa titulo feio do pending (Steam <appid>) quando library tem titulo real", async () => {
  preparar({
    owned: ["steam:1091500"],
    pending: [{ id: "steam:1091500", title: "Steam 1091500" }],
    library: [{ id: "steam:1091500", title: "Cyberpunk 2077" }],
    rows: [],
  })
  ultimoPush = null
  await biblioteca.push()
  const cyberpunk = ultimoPush.find((x) => x.appid === "steam:1091500")
  assert.equal(cyberpunk.title, "Cyberpunk 2077", "library vence o titulo feio do pending")
})
