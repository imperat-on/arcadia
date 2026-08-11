"use strict"

// Testes da Fase 5: storage de avatares (upload/serve/remove, magic bytes).

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-store-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerStorageRoutes } = require("../src/storage-routes")

const app = express()
app.use(express.json({ limit: "1mb" }))
registerAuthRoutes(app)
registerRestRoutes(app)
registerStorageRoutes(app)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
process.env.ARCADIA_SUPABASE_URL = base
after(() => listener.close())

const { getClient } = require("../../app/electron/supabase/client.js")

let token, uid
test("setup: cria usuario e pega token", async () => {
  const { data, error } = await getClient().auth.signUp({
    email: "avatar@x.com",
    password: "senha123",
    options: { data: { username: "avatartest" } },
  })
  assert.ifError(error)
  token = data.session.access_token
  uid = data.user.id
})

// 1x1 PNG real (magic bytes validos)
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64"
)

test("upload de avatar valido (PNG)", async () => {
  const { data, error } = await getClient()
    .storage.from("avatars")
    .upload(`${uid}/12345.png`, PNG, { upsert: false, contentType: "image/png" })
  assert.ifError(error)
  assert.ok(data.Key, "devolve Key")

  // serve
  const r = await fetch(`${base}/storage/v1/object/public/avatars/${uid}/12345.png`)
  assert.strictEqual(r.status, 200)
  const buf = Buffer.from(await r.arrayBuffer())
  assert.ok(buf.length > 0, "arquivo servido")
})

test("upload rejeita nao-imagem (magic bytes)", async () => {
  const { error } = await getClient()
    .storage.from("avatars")
    .upload(`${uid}/99999.png`, Buffer.from("not an image at all"), {
      upsert: false,
      contentType: "image/png",
    })
  assert.ok(error, "rejeita nao-imagem")
})

test("upload rejeita owner errado (outro uid no path)", async () => {
  const { error } = await getClient()
    .storage.from("avatars")
    .upload(`00000000-0000-0000-0000-000000000000/12345.png`, PNG, {
      upsert: false,
      contentType: "image/png",
    })
  assert.ok(error, "path owner-scoped rejeitado")
})

test("upload rejeita > 5MB", async () => {
  const grande = Buffer.concat([PNG, Buffer.alloc(6 * 1024 * 1024)])
  const { error } = await getClient()
    .storage.from("avatars")
    .upload(`${uid}/big.png`, grande, { upsert: false, contentType: "image/png" })
  assert.ok(error, "arquivo grande rejeitado")
})

test("remove apaga o avatar", async () => {
  const { error } = await getClient().storage.from("avatars").remove([`${uid}/12345.png`])
  assert.ifError(error)
  const r = await fetch(`${base}/storage/v1/object/public/avatars/${uid}/12345.png`)
  assert.strictEqual(r.status, 404, "sumiu apos remove")
})
