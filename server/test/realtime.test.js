"use strict"

// Testes da Fase 6: realtime WS (badge de amigos).
// Alice se inscreve no canal friends-<me>. Bob insere uma friendships.
// Alice recebe `postgres_changes` com payload.new.requester_id.

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-rt-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerRealtime } = require("../src/realtime")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
const server = app.listen(0)
registerRealtime(server)
const base = `http://127.0.0.1:${server.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
after(() => {
  server.closeAllConnections?.()
  server.close()
})

const { getClient } = require("../../app/electron/supabase/client.js")

let alice, bob
async function signup(email, username) {
  const { data, error } = await getClient().auth.signUp({
    email,
    password: "senha123",
    options: { data: { username } },
  })
  assert.ifError(error)
  return data
}

test("prepara alice e bob", async () => {
  alice = await signup("alice@rt.com", "alice")
  bob = await signup("bob@rt.com", "bob")
})

test("realtime: quem e user_b recebe postgres_changes no INSERT", async () => {
  const c = getClient()
  const a = alice.session.user.id
  const b = bob.session.user.id
  // Par canonico: user_a < user_b. O app notifica quem e user_b
  // (watchRequests filtra user_b=eq.me). Independe de alice/bob ser o maior.
  const [ua, ub] = a < b ? [a, b] : [b, a]
  const requester = a < b ? a : b // quem envia (o menor, user_a)
  const addressee = a < b ? b : a // quem recebe (o maior, user_b)
  const sessaoAddressee = a < b ? bob.session : alice.session

  // addressee se inscreve no canal friends-<me>
  c.auth._session = sessaoAddressee
  const recebidos = []
  const chan = c.channel(`friends-${addressee}`)
  chan.on(
    "postgres_changes",
    { event: "INSERT", schema: "public", table: "friendships", filter: `user_b=eq.${addressee}` },
    (payload) => recebidos.push(payload)
  )
  chan.subscribe()
  // espera o join completar no servidor (o open/join do WS e async)
  await new Promise((r) => setTimeout(r, 800))

  // requester insere o pedido
  c.auth._session = a < b ? alice.session : bob.session
  const { error } = await c.from("friendships").insert({
    user_a: ua,
    user_b: ub,
    requester_id: requester,
    status: "pending",
  })
  assert.ifError(error)

  // espera o evento chegar (poll com timeout)
  const limite = Date.now() + 5000
  while (recebidos.length === 0 && Date.now() < limite) {
    await new Promise((r) => setTimeout(r, 100))
  }
  assert.strictEqual(recebidos.length, 1, "recebeu 1 postgres_changes")
  assert.strictEqual(recebidos[0].new.requester_id, requester, "payload traz requester_id")

  chan.unsubscribe()
})
