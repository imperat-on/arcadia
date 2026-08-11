"use strict"

// Testes da Fase 4: friends (via shim REST) + RPCs de conquistas/biblioteca.
// Sobe o servidor numa porta efemera e usa o shim do client.js, como o app.

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-fs-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerSyncRoutes } = require("../src/sync-routes")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
const listener = app.listen(0)
process.env.ARCADIA_SUPABASE_URL = `http://127.0.0.1:${listener.address().port}`
after(() => listener.close())

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

test("preparar: cria alice e bob", async () => {
  alice = await signup("alice@x.com", "alice")
  bob = await signup("bob@x.com", "bob")
  assert.ok(alice.session.access_token)
  assert.ok(bob.session.access_token)
})

test("amigos: bob envia pedido para alice, alice aceita", async () => {
  // bob envia (sessao atual = bob)
  const c = getClient()
  // alterna sessao para bob
  c.auth._session = bob.session
  const a = alice.session.user.id
  const b = bob.session.user.id
  const [ua, ub] = a < b ? [a, b] : [b, a]
  const { error: eSend } = await c.from("friendships").insert({
    user_a: ua,
    user_b: ub,
    requester_id: b,
    status: "pending",
  })
  assert.ifError(eSend)

  // alice aceita
  c.auth._session = alice.session
  const { error: eAccept } = await c
    .from("friendships")
    .update({ status: "accepted" })
    .or(`and(user_a.eq.${a},user_b.eq.${b}),and(user_a.eq.${b},user_b.eq.${a})`)
    .eq("status", "pending")
    .eq("requester_id", b)
  assert.ifError(eAccept)
})

test("amigos: buscarFresco devolve o shape correto", async () => {
  const c = getClient()
  c.auth._session = alice.session
  const me = alice.session.user.id
  const { data: rels, error } = await c
    .from("friendships")
    .select(
      `user_a, user_b, status, requester_id, created_at,
       pa:profiles!friendships_user_a_fkey(id, username, avatar_url, display_name),
       pb:profiles!friendships_user_b_fkey(id, username, avatar_url, display_name)`
    )
    .or(`user_a.eq.${me},user_b.eq.${me}`)
  assert.ifError(error)
  assert.ok(Array.isArray(rels))
  // monta o shape como o friends.js faz
  const friends = []
  for (const r of rels) {
    const pa = Array.isArray(r.pa) ? r.pa?.[0] : r.pa
    const pb = Array.isArray(r.pb) ? r.pb?.[0] : r.pb
    const other = r.user_a === me ? r.user_b : r.user_a
    const p = (r.user_a === me ? pb : pa) || null
    if (r.status === "accepted") friends.push({ id: other, username: p?.username })
  }
  assert.strictEqual(friends.length, 1)
  assert.strictEqual(friends[0].username, "bob")
})

test("search: alice procura bob e acha", async () => {
  const c = getClient()
  c.auth._session = alice.session
  const { data, error } = await c
    .from("profiles")
    .select("id, username, avatar_url, display_name")
    .ilike("username", "bo%")
    .limit(20)
  assert.ifError(error)
  const nomes = data.map((p) => p.username)
  assert.ok(nomes.includes("bob"))
})

test("sync_achievements: push e pull com earliest-wins", async () => {
  const c = getClient()
  c.auth._session = alice.session

  // push 1 conquista com ts antigo
  const r1 = await c.rpc("sync_achievements", {
    p_items: [{ appid: "440", apiname: "ach_a", unlocked_at: 1000, title: "A", icon: "i1", percent: 50 }],
  })
  assert.ifError(r1.error)

  // push a MESMA com ts mais novo → nao altera (earliest wins)
  const r2 = await c.rpc("sync_achievements", {
    p_items: [{ appid: "440", apiname: "ach_a", unlocked_at: 5000, title: "A", icon: "i1", percent: 50 }],
  })
  assert.ifError(r2.error)
  // r2 deve retornar 0 alteradas (ts maior que a existente)
  assert.ok(Array.isArray(r2.data))
  assert.strictEqual(r2.data.length, 0, "ts maior nao altera")

  // pull desde 0 traz 1
  const p = await c.rpc("pull_achievements", { p_since: "1970-01-01T00:00:00Z" })
  assert.ifError(p.error)
  assert.strictEqual(p.data.length, 1)
  assert.strictEqual(p.data[0].unlocked_at, 1000, "earliest-wins mantem 1000")
})

test("friend_achievements: guardas (nao-amigo → vazio)", async () => {
  const c = getClient()
  // carol cria conta, nao e amiga de alice
  const carol = await signup("carol@x.com", "carol")
  c.auth._session = alice.session
  const r = await c.rpc("friend_achievements", { p_friend: carol.session.user.id })
  assert.ifError(r.error)
  assert.strictEqual(r.data.length, 0, "nao-amigo retorna vazio")
})

test("friend_achievements: amigo ve conquistas", async () => {
  const c = getClient()
  c.auth._session = alice.session
  const r = await c.rpc("friend_achievements", { p_friend: bob.session.user.id })
  assert.ifError(r.error)
  // bob ainda nao tem conquistas, mas e amigo → array (vazio ok)
  assert.ok(Array.isArray(r.data))
})

test("push_library e pull_library: lib + playtime acumula", async () => {
  const c = getClient()
  c.auth._session = alice.session

  // push lib + 30min
  const r1 = await c.rpc("push_library", {
    p_lib: [{ appid: "10", title: "Half-Life", platform: "windows" }],
    p_playtime: [{ appid: "10", minutes: 30 }],
  })
  assert.ifError(r1.error)

  // push +20min → total 50
  const r2 = await c.rpc("push_library", {
    p_lib: [],
    p_playtime: [{ appid: "10", minutes: 20 }],
  })
  assert.ifError(r2.error)

  const r3 = await c.rpc("pull_library")
  assert.ifError(r3.error)
  assert.strictEqual(r3.data.length, 1)
  assert.strictEqual(r3.data[0].minutes, 50, "playtime acumula (30+20)")
  assert.strictEqual(r3.data[0].title, "Half-Life")
})

test("push_library: removed deleta", async () => {
  const c = getClient()
  c.auth._session = alice.session
  const r = await c.rpc("push_library", {
    p_lib: [{ appid: "10", removed: true }],
    p_playtime: [],
  })
  assert.ifError(r.error)
  const pull = await c.rpc("pull_library")
  assert.strictEqual(pull.data.length, 0, "jogo removido")
})
