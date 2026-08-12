"use strict"

// Realtime WS do sync de biblioteca (canal library-<me>): quando um push_library
// com jogos novos/removidos acontece numa maquina, as outras logadas na mesma
// conta recebem postgres_changes na hora — sem isto, so pegavam a novidade no
// proximo boot/login (o sintoma era "demora pra sincronizar").

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-libr-"))

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
const server = app.listen(0)
registerRealtime(server)
const base = `http://127.0.0.1:${server.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
after(() => {
  server.closeAllConnections?.()
  server.close()
})

const { getClient } = require("../../app/electron/supabase/client.js")

let alice

test("prepara alice", async () => {
  const { data, error } = await getClient().auth.signUp({
    email: "alice@libr.com",
    password: "senha123",
    options: { data: { username: "alice_libr" } },
  })
  assert.ifError(error)
  alice = data
})

test("realtime: push_library com jogo novo avisa o canal library-<me>", async () => {
  const c = getClient()
  const me = alice.session.user.id
  c.auth._session = alice.session

  const recebidos = []
  const chan = c.channel(`library-${me}`)
  chan.on("postgres_changes", { event: "*", schema: "public", table: "user_library" }, (payload) =>
    recebidos.push(payload)
  )
  chan.subscribe()
  await new Promise((r) => setTimeout(r, 800))

  const { error } = await c.rpc("push_library", {
    p_lib: [{ appid: "steam:123", title: "Jogo Teste", platform: "windows" }],
    p_playtime: [],
  })
  assert.ifError(error)

  const limite = Date.now() + 5000
  while (recebidos.length === 0 && Date.now() < limite) {
    await new Promise((r) => setTimeout(r, 100))
  }
  assert.strictEqual(recebidos.length, 1, "recebeu 1 postgres_changes")

  chan.unsubscribe()
})

test("realtime: push_library so com playtime AVISA o canal (display de horas atualiza sem reiniciar)", async () => {
  const c = getClient()
  const me = alice.session.user.id
  c.auth._session = alice.session

  const recebidos = []
  const chan = c.channel(`library-${me}`)
  chan.on("postgres_changes", { event: "*", schema: "public", table: "user_library" }, (payload) =>
    recebidos.push(payload)
  )
  chan.subscribe()
  await new Promise((r) => setTimeout(r, 800))

  const { error } = await c.rpc("push_library", {
    p_lib: [],
    p_playtime: [{ appid: "steam:123", minutes: 5 }],
  })
  assert.ifError(error)

  const limite = Date.now() + 5000
  while (recebidos.length === 0 && Date.now() < limite) {
    await new Promise((r) => setTimeout(r, 100))
  }
  assert.strictEqual(recebidos.length, 1, "playtime sozinho dispara postgres_changes")

  chan.unsubscribe()
})

test("realtime: sync_achievements com conquista nova avisa o canal achievements-<me>", async () => {
  const c = getClient()
  const me = alice.session.user.id
  c.auth._session = alice.session

  const recebidos = []
  const chan = c.channel(`achievements-${me}`)
  chan.on("postgres_changes", { event: "*", schema: "public", table: "user_achievements" }, (payload) =>
    recebidos.push(payload)
  )
  chan.subscribe()
  await new Promise((r) => setTimeout(r, 800))

  const { error } = await c.rpc("sync_achievements", {
    p_items: [{ appid: "1091500", apiname: "ACH_TESTE", unlocked_at: 1700000000, title: "Teste" }],
  })
  assert.ifError(error)

  const limite = Date.now() + 5000
  while (recebidos.length === 0 && Date.now() < limite) {
    await new Promise((r) => setTimeout(r, 100))
  }
  assert.strictEqual(recebidos.length, 1, "recebeu 1 postgres_changes")

  // Reenviar a MESMA conquista (sem mudanca no servidor) NAO avisa de novo:
  // so notifica quando algo de fato desbloqueou.
  const { error: e2 } = await c.rpc("sync_achievements", {
    p_items: [{ appid: "1091500", apiname: "ACH_TESTE", unlocked_at: 1700000000, title: "Teste" }],
  })
  assert.ifError(e2)
  await new Promise((r) => setTimeout(r, 1500))
  assert.strictEqual(recebidos.length, 1, "reenvio idempotente nao dispara evento novo")

  chan.unsubscribe()
})
