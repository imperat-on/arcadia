"use strict"

// E2E do sync de biblioteca entre MAQUINAS: servidor real + cliente real
// (app/electron/supabase/biblioteca.js). Modela cada maquina com seu proprio
// DATA_DIR/ARCADIA_DATA_DIR e conta (alice=PC, bob=notebook), alternando a
// conta ativa antes de cada operacao — igual aos 15 fix(sync) que precederam
// este teste:
//   - add numa maquina propaga pra outra (pull + realtime)
//   - remocao propaga (e o stub pending nao bloqueia mais)
//   - re-add do jogo removido volta ao servidor
//   - push-primeiro ressuscitava jogo removido em outra maquina (pull antes do push)

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DATA_PC = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bs-pc-"))
const DATA_NB = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bs-nb-"))

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bs-srv-"))
process.env.ARCADIA_DATA_DIR = DATA_PC

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerSyncRoutes } = require("../src/sync-routes")
const { registerRealtime } = require("../src/realtime")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
const listener = app.listen(0)
registerRealtime(listener)
const base = `http://127.0.0.1:${listener.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
after(() => {
  listener.closeAllConnections?.()
  listener.close()
})

const { getClient } = require("../../app/electron/supabase/client.js")
const contaMod = require("../../app/electron/supabase/conta.js")

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"))
  } catch {
    return null
  }
}
// caminho do arquivo via conta.js (honra ARCADIA_DATA_DIR ativo + conta ativa)
function contaArquivo(nome) {
  return contaMod.caminhoArquivoConta(nome)
}

let pc, nb
let DATA_NB2
async function signup(email, username) {
  const { data, error } = await getClient().auth.signUp({
    email,
    password: "senha123",
    options: { data: { username } },
  })
  assert.ifError(error)
  return data
}

test("preparar: cria alice (PC) e bob (notebook)", async () => {
  pc = await signup("alice-bs@x.com", "alicebs")
  nb = await signup("bob-bs@x.com", "bobbs")
})

test("maquina A adiciona jogo -> maquina B puxa no pull (reconcile)", async () => {
  const { push, pull } = require("../../app/electron/supabase/biblioteca.js")
  // Maquina A: alice adiciona jogo custom e sincroniza
  process.env.ARCADIA_DATA_DIR = DATA_PC
  contaMod.definirConta("alicebs")
  getClient().auth._session = pc.session
  fs.writeFileSync(contaArquivo("custom_games.json"), JSON.stringify([{ id: "custom:1", title: "Meu Jogo", launcher: "custom", platform: "windows", exe: "", installed: false }]))
  await push()
  const { data: srv } = await getClient().rpc("pull_library")
  assert.strictEqual(srv.length, 1, "servidor tem o jogo da maquina A")
  assert.strictEqual(srv[0].appid, "custom:1")

  // Maquina B: bob (conta diferente) NAO ve
  contaMod.definirConta("bobbs")
  getClient().auth._session = nb.session
  const { data: bobVazio } = await getClient().rpc("pull_library")
  assert.strictEqual(bobVazio.length, 0, "bob nao ve jogo de alice")

  // Mesma conta, outra maquina: bob e a SEGUNDA maquina de alice
  DATA_NB2 = path.join(DATA_NB, "nb")
  fs.mkdirSync(DATA_NB2, { recursive: true })
  process.env.ARCADIA_DATA_DIR = DATA_NB2
  contaMod.definirConta("alicebs")
  await pull()
  const custom = readJson(contaArquivo("custom_games.json"))
  assert.ok(custom && custom.some((g) => g.id === "custom:1"), "jogo chega na outra maquina via pull")
  // owned_games.json nao existia na maquina B (ainda nao migrada) — pull nao
  // materializa posse do zero (constraint: ausente = "possui tudo")
  assert.ok(!fs.existsSync(contaArquivo("owned_games.json")), "owned_games nao e materializado pelo pull")
})

test("remover na maquina B propaga: some do servidor e do pull da maquina A", async () => {
  const { pull: pullNb, push: pushNb, reconcile } = require("../../app/electron/supabase/biblioteca.js")
  // Maquina B remove o jogo (que veio via pull)
  process.env.ARCADIA_DATA_DIR = DATA_NB2
  contaMod.definirConta("alicebs")
  await pullNb()
  const customNb = readJson(contaArquivo("custom_games.json"))
  assert.ok(customNb && customNb.some((g) => g.id === "custom:1"), "maquina B tem o jogo antes de remover")
  fs.writeFileSync(contaArquivo("custom_games.json"), JSON.stringify([]))
  await pushNb()

  // Servidor: jogo sumiu
  const { data: srvAposRemocao } = await getClient().rpc("pull_library")
  assert.strictEqual(srvAposRemocao.length, 0, "remocao na maquina B tirou do servidor")

  // Maquina A (PC) faz reconcile (pull antes do push): jogo nao volta
  process.env.ARCADIA_DATA_DIR = DATA_PC
  contaMod.definirConta("alicebs")
  await reconcile()
  const customPc = readJson(contaArquivo("custom_games.json"))
  assert.ok(!customPc.some((g) => g.id === "custom:1"), "jogo removido nao volta no PC apos reconcile")
  const { data: srvFinal } = await getClient().rpc("pull_library")
  assert.strictEqual(srvFinal.length, 0, "servidor continua sem o jogo apos reconcile")
})

test("re-add: adicionar de novo em A propaga de novo pra B", async () => {
  const { push: pushPc, pull: pullNb2 } = require("../../app/electron/supabase/biblioteca.js")
  // Maquina A re-adiciona o jogo
  process.env.ARCADIA_DATA_DIR = DATA_PC
  contaMod.definirConta("alicebs")
  fs.writeFileSync(contaArquivo("custom_games.json"), JSON.stringify([{ id: "custom:1", title: "Meu Jogo", launcher: "custom", platform: "windows", exe: "", installed: false }]))
  await pushPc()
  const { data: srvReadd } = await getClient().rpc("pull_library")
  assert.strictEqual(srvReadd.length, 1, "jogo re-adicionado volta ao servidor")

  // Maquina B puxa e ve de novo
  process.env.ARCADIA_DATA_DIR = DATA_NB2
  contaMod.definirConta("alicebs")
  await pullNb2()
  const customNb = readJson(contaArquivo("custom_games.json"))
  assert.ok(customNb && customNb.some((g) => g.id === "custom:1"), "re-add chega na outra maquina")
})

test("realtime: push avisa canal library-<me> (sem boot) e pull traz na hora", async () => {
  const c = getClient()
  const me = pc.session.user.id
  c.auth._session = pc.session

  const recebidos = []
  const chan = c.channel(`library-${me}`)
  chan.on("postgres_changes", { event: "*", schema: "public", table: "user_library" }, (payload) =>
    recebidos.push(payload)
  )
  chan.subscribe()
  await new Promise((r) => setTimeout(r, 800))

  process.env.ARCADIA_DATA_DIR = DATA_PC
  contaMod.definirConta("alicebs")
  fs.writeFileSync(contaArquivo("custom_games.json"), JSON.stringify([{ id: "custom:2", title: "Jogo RT", launcher: "custom", platform: "windows", exe: "", installed: false }]))
  const { push: pushPc2 } = require("../../app/electron/supabase/biblioteca.js")
  await pushPc2()

  const limite = Date.now() + 5000
  while (recebidos.length === 0 && Date.now() < limite) {
    await new Promise((r) => setTimeout(r, 100))
  }
  assert.strictEqual(recebidos.length, 1, "recebeu 1 postgres_changes no canal library-<me>")

  chan.unsubscribe()
})
