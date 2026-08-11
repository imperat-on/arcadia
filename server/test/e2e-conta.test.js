"use strict"

// E2E: isolamento por conta (biblioteca, background, sources).
// Sobe o servidor completo + shim client.js. Verifica que dados de uma conta
// nao vazam pra outra e que background/sources sincronizam por conta.

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-e2e-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerSyncRoutes } = require("../src/sync-routes")
const { registerStorageRoutes } = require("../src/storage-routes")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
registerStorageRoutes(app)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
process.env.ARCADIA_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-e2e-data-"))
after(() => listener.close())

const { getClient } = require("../../app/electron/supabase/client.js")
const { filtrarPorPosse, OWNED_GAMES } = require("../../app/electron/owned.js")
const contaMod = require("../../app/electron/supabase/conta.js")

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

// T12: biblioteca nao vaza entre contas
test("T12: biblioteca de alice nao vaza pra bob", async () => {
  alice = await signup("alice@e2e.com", "alice")
  bob = await signup("bob@e2e.com", "bob")

  const c = getClient()
  c.auth._session = alice.session
  const { error: pushErr } = await c.rpc("push_library", {
    p_lib: [{ appid: "steam:10", title: "Half-Life", platform: "windows" }],
    p_playtime: [],
  })
  assert.ifError(pushErr)

  // bob pull vazio
  c.auth._session = bob.session
  const { data: bobPull } = await c.rpc("pull_library")
  assert.strictEqual(bobPull.length, 0, "bob nao ve a biblioteca de alice")

  // readLibrary/filtrarPorPosse: alice ve 1, bob ve 0, guest ve todos
  const globais = [{ id: "steam:10", title: "A" }, { id: "epic:x", title: "B" }]
  contaMod.definirConta("alice")
  fs.writeFileSync(contaMod.caminhoArquivoConta(OWNED_GAMES), JSON.stringify(["steam:10"]))
  const aliceLib = filtrarPorPosse(globais)
  assert.strictEqual(aliceLib.length, 1, "alice ve so o que possui")

  contaMod.definirConta("bob")
  fs.writeFileSync(contaMod.caminhoArquivoConta(OWNED_GAMES), JSON.stringify([]))
  const bobLib = filtrarPorPosse(globais)
  assert.strictEqual(bobLib.length, 0, "bob ve 0")

  contaMod.definirConta(null)
  const guestLib = filtrarPorPosse(globais)
  assert.strictEqual(guestLib.length, 2, "guest ve tudo")
})

// T13: background sincroniza
test("T13: background sincroniza entre maquinas", async () => {
  const PNG = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
    "base64"
  )
  const tmp = path.join(os.tmpdir(), "bg-e2e-" + Date.now() + ".png")
  fs.writeFileSync(tmp, PNG)

  const c = getClient()
  c.auth._session = alice.session
  const { setBackground } = require("../../app/electron/supabase/auth.js")
  const r = await setBackground(tmp)
  fs.unlinkSync(tmp)
  assert.ok(r.ok, "setBackground ok: " + JSON.stringify(r))
  assert.ok(r.background_url.includes("/public/backgrounds/"), "URL backgrounds")

  // serve publico
  const url = r.background_url.replace(base, "")
  const fetchRes = await fetch(base + url)
  assert.strictEqual(fetchRes.status, 200, "background servido")
  const bytes = Buffer.from(await fetchRes.arrayBuffer())
  assert.ok(bytes.length > 0, "bytes do background")

  // nova sessao (maquina 2) ve o background_url
  const { data: perfil } = await c.auth.getUser()
  const { data: meu } = await c.from("profiles").select("background_url").eq("id", perfil.user.id).maybeSingle()
  assert.ok(meu.background_url && meu.background_url.includes("/public/backgrounds/"), "maquina 2 ve")

  // bob nao ve
  c.auth._session = bob.session
  const { data: perfilBob } = await c.auth.getUser()
  const { data: perfilBobRow } = await c.from("profiles").select("background_url").eq("id", perfilBob.user.id).maybeSingle()
  assert.strictEqual(perfilBobRow.background_url, null, "bob sem background")
})

// T14: sources publicas sincronizam, chaves nao
test("T14: sources sincronizam por conta, sem chaves", async () => {
  const c = getClient()
  c.auth._session = alice.session
  const { error: pushErr } = await c.rpc("push_sources", {
    p_sources: [
      { source_id: "59e6a31484ce", url: "https://hydralinks.cloud/sources/fitgirl.json", name: "FitGirl" },
      { source_id: "118d12535cb4", url: "https://hydralinks.cloud/sources/steamrip.json", name: "SteamRip" },
    ],
  })
  assert.ifError(pushErr)

  // bob nao ve
  c.auth._session = bob.session
  const { data: bobPull } = await c.rpc("pull_sources")
  assert.strictEqual(bobPull.length, 0, "bob nao ve sources de alice")

  // alice ve 2, sem etag/count
  c.auth._session = alice.session
  const { data: alicePull } = await c.rpc("pull_sources")
  assert.strictEqual(alicePull.length, 2, "alice ve 2 sources")
  for (const s of alicePull) {
    assert.ok(!("etag" in s) && !("count" in s), "sem campos locais")
  }

  // source_id invalido ignorado
  await c.rpc("push_sources", { p_sources: [{ source_id: "invalido!!", url: "https://x.com/a.json", name: "X" }] })
  const { data: aposInvalido } = await c.rpc("pull_sources")
  assert.strictEqual(aposInvalido.length, 2, "invalido nao entra")

  // removed some, re-add volta
  await c.rpc("push_sources", { p_sources: [{ source_id: "59e6a31484ce", removed: true }] })
  const { data: aposRemove } = await c.rpc("pull_sources")
  assert.strictEqual(aposRemove.length, 1, "removida some")
  await c.rpc("push_sources", { p_sources: [{ source_id: "59e6a31484ce", url: "https://hydralinks.cloud/sources/fitgirl.json", name: "FitGirl" }] })
  const { data: aposReadd } = await c.rpc("pull_sources")
  assert.strictEqual(aposReadd.length, 2, "re-add volta")
})
