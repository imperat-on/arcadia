"use strict"

// Teste da coluna background_url em profiles (Task 1 do plano sync-per-conta).

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bg-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")

const app = express()
app.use(express.json())
registerAuthRoutes(app)
registerRestRoutes(app)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
after(() => listener.close())

const { getClient } = require("../../app/electron/supabase/client.js")

let token, uid
test("setup: cria usuario e pega token", async () => {
  const { data, error } = await getClient().auth.signUp({
    email: "bg@x.com",
    password: "senha123",
    options: { data: { username: "bgtest" } },
  })
  assert.ifError(error)
  token = data.session.access_token
  uid = data.user.id
})

test("PATCH background_url da 200 e select devolve", async () => {
  const url = "https://cdn.exemplo.com/bg.png"
  const r = await fetch(`${base}/rest/v1/profiles`, {
    method: "PATCH",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: JSON.stringify({ background_url: url }),
  })
  assert.strictEqual(r.status, 200)

  const { data, error } = await getClient()
    .from("profiles")
    .select("background_url")
    .eq("id", uid)
    .maybeSingle()
  assert.ifError(error)
  assert.strictEqual(data.background_url, url)
})

test("PATCH password_hash da 400 sem_campos (nao esta na whitelist)", async () => {
  const r = await fetch(`${base}/rest/v1/profiles`, {
    method: "PATCH",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: JSON.stringify({ password_hash: "hackeado" }),
  })
  assert.strictEqual(r.status, 400)
  const body = await r.json()
  assert.strictEqual(body.error, "sem_campos")
})
