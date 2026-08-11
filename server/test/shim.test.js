"use strict"

// Teste de integracao do shim do client.js contra o servidor.
// Sobe o servidor numa porta efemera, aponta config.url para ele via env,
// e exercita getClient() (auth + from + rpc) como os modulos do app fazem.

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-shim-"))

// Sobe o servidor completo (auth + rest) numa porta efemera
const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`

// Aponta o config do app para este servidor, antes de importar o shim
process.env.ARCADIA_SUPABASE_URL = base
const client = require("../../app/electron/supabase/client.js")
const { getClient } = client

test("shim: signup devolve sessao e meu perfil via from('profiles')", async () => {
  const auth = getClient().auth
  const { data, error } = await auth.signUp({
    email: "shim@test.com",
    password: "senha123",
    options: { data: { username: "shimuser" } },
  })
  assert.ifError(error)
  assert.ok(data.session.access_token)
  assert.strictEqual(data.user.user_metadata.username, "shimuser")

  // getClient().auth.getUser() → /auth/v1/user
  const { data: ud, error: ue } = await auth.getUser()
  assert.ifError(ue)
  assert.strictEqual(ud.user.id, data.user.id)

  // from('profiles').select().eq('id', me).maybeSingle()
  const { data: perfil, error: pe } = await getClient()
    .from("profiles")
    .select("username, avatar_url, display_name")
    .eq("id", data.user.id)
    .maybeSingle()
  assert.ifError(pe)
  assert.strictEqual(perfil.username, "shimuser")
})

test("shim: rpc login_check e username_available", async () => {
  const r = await getClient().rpc("username_available", { p_username: "shimuser" })
  assert.strictEqual(r.data, false)
  const livre = await getClient().rpc("username_available", { p_username: "novo_user" })
  assert.strictEqual(livre.data, true)

  const lc = await getClient().rpc("login_check", {
    p_username: "shimuser",
    p_password: "senha123",
  })
  assert.strictEqual(lc.data.ok, true)
  assert.strictEqual(lc.data.email, "shim@test.com")
})

test("shim: signInWithPassword e getUser", async () => {
  const auth = getClient().auth
  const { data, error } = await auth.signInWithPassword({
    email: "shim@test.com",
    password: "senha123",
  })
  assert.ifError(error)
  assert.ok(data.session.access_token)

  const { data: ud } = await auth.getUser()
  assert.strictEqual(ud.user.user_metadata.username, "shimuser")
})

test("shim: onAuthStateChange emite SIGNED_IN e SIGNED_OUT", async () => {
  const auth = getClient().auth
  const eventos = []
  auth.onAuthStateChange((ev, _s) => eventos.push(ev))
  await auth.signInWithPassword({ email: "shim@test.com", password: "senha123" })
  assert.ok(eventos.includes("SIGNED_IN"))
  await auth.signOut()
  assert.ok(eventos.includes("SIGNED_OUT"))
})

after(() => listener.close())
