"use strict"

// Teste da coluna background_url em profiles (Task 1) e do bucket
// `backgrounds` de storage com imagem+video (Task 2), plano sync-per-conta.

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
const { registerStorageRoutes } = require("../src/storage-routes")

const app = express()
app.use(express.json())
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

// 1x1 PNG real (magic bytes validos)
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64"
)

// Cabecalho EBML (magic bytes do webm/mkv)
const WEBM = Buffer.from([0x1a, 0x45, 0xdf, 0xa3, 0x01, 0x02, 0x03, 0x04])

// Box `ftyp` do ISO BMFF (magic bytes do mp4/m4v/mov), brand "mp42"
const MP4 = Buffer.from([
  0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x6d, 0x70, 0x34, 0x32,
])

test("upload de background webm valido (EBML) da Key e serve 200", async () => {
  const { data, error } = await getClient()
    .storage.from("backgrounds")
    .upload(`${uid}/1.webm`, WEBM, { upsert: false, contentType: "video/webm" })
  assert.ifError(error)
  assert.ok(data.Key, "devolve Key")

  const r = await fetch(`${base}/storage/v1/object/public/backgrounds/${uid}/1.webm`)
  assert.strictEqual(r.status, 200)
})

test("upload de background mp4 valido (ftyp) da Key e serve 200", async () => {
  const { data, error } = await getClient()
    .storage.from("backgrounds")
    .upload(`${uid}/2.mp4`, MP4, { upsert: false, contentType: "video/mp4" })
  assert.ifError(error)
  assert.ok(data.Key, "devolve Key")

  const r = await fetch(`${base}/storage/v1/object/public/backgrounds/${uid}/2.mp4`)
  assert.strictEqual(r.status, 200)
})

test("upload de background PNG valido passa (imagem tambem aceita)", async () => {
  const { error } = await getClient()
    .storage.from("backgrounds")
    .upload(`${uid}/3.png`, PNG, { upsert: false, contentType: "image/png" })
  assert.ifError(error)
})

test("upload de background rejeita conteudo que nao e midia", async () => {
  const { error } = await getClient()
    .storage.from("backgrounds")
    .upload(`${uid}/4.png`, Buffer.from("isso nao e midia nenhuma"), {
      upsert: false,
      contentType: "image/png",
    })
  assert.ok(error, "rejeita nao-midia")
  assert.match(error.message, /background_nao_midia/)
})

test("upload de background rejeita > 25MB", async () => {
  const grande = Buffer.concat([WEBM, Buffer.alloc(26 * 1024 * 1024)])
  const { error } = await getClient()
    .storage.from("backgrounds")
    .upload(`${uid}/5.webm`, grande, { upsert: false, contentType: "video/webm" })
  assert.ok(error, "arquivo grande rejeitado")
})

test("upload de background rejeita owner errado", async () => {
  const { error } = await getClient()
    .storage.from("backgrounds")
    .upload(`00000000-0000-0000-0000-000000000000/6.webm`, WEBM, {
      upsert: false,
      contentType: "video/webm",
    })
  assert.ok(error, "path owner-scoped rejeitado")
})

test("regressao: avatars continua servindo apos generalizar storage-routes", async () => {
  const { data, error } = await getClient()
    .storage.from("avatars")
    .upload(`${uid}/12345.png`, PNG, { upsert: false, contentType: "image/png" })
  assert.ifError(error)
  assert.ok(data.Key)

  const r = await fetch(`${base}/storage/v1/object/public/avatars/${uid}/12345.png`)
  assert.strictEqual(r.status, 200)
})
