// Testes do cache de conquistas de amigos (FriendProfileView + friends.js).
// Roda em Node puro; ARCADIA_DATA_DIR aponta pra pasta temporaria, definido
// ANTES do require (padrao dos outros testes da suite).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-achcache-"))
process.env.ARCADIA_DATA_DIR = DIR

const {
  achCacheFile,
  lerAchCache,
  gravarAchCache,
  canonicalPair,
  friendAchievements,
} = require("../electron/supabase/friends.js")
const { getClient } = require("../electron/supabase/client.js")
const { definirConta } = require("../electron/supabase/conta.js")

const AMIGO = "friend-1111-2222-3333"

test("gravarAchCache escreve e lerAchCache le de volta", () => {
  const itens = [{ appid: "1091500", apiname: "ach_01", unlocked_at: "2026-01-01" }]
  gravarAchCache(AMIGO, itens)
  const lido = lerAchCache(AMIGO)
  assert.ok(lido, "deve retornar os itens gravados")
  assert.equal(lido.length, 1)
  assert.equal(lido[0].appid, "1091500")
})

test("lerAchCache separa por amigo (nao vaza entre ids)", () => {
  const a = lerAchCache("amigo-outro-9999")
  assert.equal(a, null, "amigo sem cache deve retornar null")
  assert.ok(lerAchCache(AMIGO), "cache do amigo original intacto")
})

test("lerAchCache respeita TTL (dados velhos sao ignorados)", () => {
  const file = achCacheFile()
  const all = JSON.parse(fs.readFileSync(file, "utf8"))
  all["amigo-ttl"] = { ts: Date.now() - 120_000, data: [{ appid: "730" }] }
  fs.writeFileSync(file, JSON.stringify(all))
  assert.equal(lerAchCache("amigo-ttl"), null, "cache com TTL vencido deve ser ignorado")
})

test("canonicalPair continua consistente (regressao)", () => {
  assert.deepEqual(canonicalPair("a", "b"), { user_a: "a", user_b: "b" })
})

test("comparação recente não usa cache quando o servidor nega acesso", async (t) => {
  const client = getClient()
  t.mock.method(client.auth, "getUser", async () => ({ data: { user: { id: "owner" } } }))
  const rpc = t.mock.method(client, "rpc", async () => ({ error: { message: "not friends" } }))
  const friend = "removed-friend"
  gravarAchCache(friend, [{ appid: "400", apiname: "A" }])
  assert.deepEqual(await friendAchievements(friend, { fresh: true }), { ok: false, error: "not friends" })
  assert.deepEqual(rpc.mock.calls[0].arguments, ["friend_achievements", { p_friend: friend }])
})

test("resposta de comparação não preenche o cache de outra conta", async (t) => {
  definirConta("first")
  t.after(() => definirConta(null))
  const client = getClient()
  t.mock.method(client.auth, "getUser", async () => ({ data: { user: { id: "owner" } } }))
  t.mock.method(client, "rpc", async () => {
    definirConta("second")
    return { data: [{ appid: "400", apiname: "A" }] }
  })
  assert.deepEqual(await friendAchievements("old-account-friend", { fresh: true }), { ok: false, error: "conta_trocada" })
  assert.equal(lerAchCache("old-account-friend"), null)
})
