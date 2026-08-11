"use strict"

// Testes do RPC push_sources/pull_sources: registro de fontes publicas
// (source_id, url, name), sem etag/lastMod/count (estado local, nao sincroniza).

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-src-"))

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
  alice = await signup("alice-src@x.com", "alicesrc")
  bob = await signup("bob-src@x.com", "bobsrc")
  assert.ok(alice.session.access_token)
  assert.ok(bob.session.access_token)
})

test("push_sources e pull_sources: 2 fontes ida e volta, sem campos locais", async () => {
  const c = getClient()
  c.auth._session = alice.session

  const r1 = await c.rpc("push_sources", {
    p_sources: [
      { source_id: "aaaaaaaaaaaa", url: "https://a.example/repo.json", name: "Fonte A" },
      { source_id: "bbbbbbbbbbbb", url: "http://b.example/repo.json", name: "Fonte B" },
    ],
  })
  assert.ifError(r1.error)

  const r2 = await c.rpc("pull_sources")
  assert.ifError(r2.error)
  assert.strictEqual(r2.data.length, 2)
  const ids = r2.data.map((s) => s.source_id).sort()
  assert.deepStrictEqual(ids, ["aaaaaaaaaaaa", "bbbbbbbbbbbb"])
  for (const s of r2.data) {
    assert.strictEqual(Object.prototype.hasOwnProperty.call(s, "removed_at"), false, "sem removed_at")
    assert.strictEqual(Object.prototype.hasOwnProperty.call(s, "etag"), false, "sem etag")
    assert.strictEqual(Object.prototype.hasOwnProperty.call(s, "count"), false, "sem count")
    assert.ok(Object.prototype.hasOwnProperty.call(s, "url"))
    assert.ok(Object.prototype.hasOwnProperty.call(s, "name"))
  }
})

test("push_sources: source_id invalido e ignorado", async () => {
  const c = getClient()
  c.auth._session = alice.session

  const r1 = await c.rpc("push_sources", {
    p_sources: [{ source_id: "nao-hex-valido", url: "https://c.example/repo.json", name: "Fonte C" }],
  })
  assert.ifError(r1.error)

  const r2 = await c.rpc("pull_sources")
  assert.ifError(r2.error)
  const ids = r2.data.map((s) => s.source_id)
  assert.ok(!ids.includes("nao-hex-valido"), "id invalido nao entra")
  assert.strictEqual(r2.data.length, 2, "continua so com as 2 validas")
})

test("push_sources: url nao-http e ignorada", async () => {
  const c = getClient()
  c.auth._session = alice.session

  const r1 = await c.rpc("push_sources", {
    p_sources: [{ source_id: "cccccccccccc", url: "ftp://c.example/repo.json", name: "Fonte C" }],
  })
  assert.ifError(r1.error)

  const r2 = await c.rpc("pull_sources")
  assert.ifError(r2.error)
  const ids = r2.data.map((s) => s.source_id)
  assert.ok(!ids.includes("cccccccccccc"), "url nao-http nao entra")
  assert.strictEqual(r2.data.length, 2)
})

test("push_sources: removed:true faz a fonte sumir do pull", async () => {
  const c = getClient()
  c.auth._session = alice.session

  const r1 = await c.rpc("push_sources", {
    p_sources: [{ source_id: "aaaaaaaaaaaa", removed: true }],
  })
  assert.ifError(r1.error)

  const r2 = await c.rpc("pull_sources")
  assert.ifError(r2.error)
  assert.strictEqual(r2.data.length, 1)
  assert.strictEqual(r2.data[0].source_id, "bbbbbbbbbbbb")
})

test("push_sources: re-push da fonte removida faz ela voltar", async () => {
  const c = getClient()
  c.auth._session = alice.session

  const r1 = await c.rpc("push_sources", {
    p_sources: [{ source_id: "aaaaaaaaaaaa", url: "https://a.example/repo.json", name: "Fonte A" }],
  })
  assert.ifError(r1.error)

  const r2 = await c.rpc("pull_sources")
  assert.ifError(r2.error)
  assert.strictEqual(r2.data.length, 2)
  const ids = r2.data.map((s) => s.source_id).sort()
  assert.deepStrictEqual(ids, ["aaaaaaaaaaaa", "bbbbbbbbbbbb"])
})

test("pull_sources: bob nao ve as fontes de alice", async () => {
  const c = getClient()
  c.auth._session = bob.session

  const r = await c.rpc("pull_sources")
  assert.ifError(r.error)
  assert.strictEqual(r.data.length, 0, "bob nao tem fontes proprias")
})
