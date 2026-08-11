"use strict"

// E2E do catalogo: sobe o backend em porta efemera, autentica pelo cliente
// usado no app e consulta /catalog/v1/popular pelo catalogGet real.
const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const serverData = fs.mkdtempSync(
  path.join(os.tmpdir(), "arcadia-e2e-catalog-server-"),
)
const appData = fs.mkdtempSync(
  path.join(os.tmpdir(), "arcadia-e2e-catalog-app-"),
)
process.env.DATA_DIR = serverData
process.env.ARCADIA_DATA_DIR = appData

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")
const { registerRestRoutes } = require("../src/rest-routes")
const { registerSyncRoutes } = require("../src/sync-routes")
const { registerStorageRoutes } = require("../src/storage-routes")
const { registerCatalogRoutes } = require("../src/catalog-routes")
const { db, nowEpochS } = require("../src/db")

const app = express()
app.use(express.json({ limit: "1mb" }))
app.use(require("compression")())
registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
registerStorageRoutes(app)
registerCatalogRoutes(app)

const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
process.env.ARCADIA_SUPABASE_URL = base

test.after(async () => {
  await new Promise((resolve) => listener.close(resolve))
  fs.rmSync(serverData, { recursive: true, force: true })
  fs.rmSync(appData, { recursive: true, force: true })
})

const { getClient } = require("../../app/electron/supabase/client")
const { catalogGet, catalogGetEspelho } = require("../../app/electron/catalog")

test("E2E: app autenticado consulta catalogo e grava espelho", async () => {
  const semToken = await fetch(`${base}/catalog/v1/popular`)
  assert.equal(semToken.status, 401)

  const { error } = await getClient().auth.signUp({
    email: "catalogo@e2e.test",
    password: "senha123",
    options: { data: { username: "catalogoe2e" } },
  })
  assert.equal(error, null)

  db.prepare(
    "INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)",
  ).run(
    "popular",
    JSON.stringify({
      completa: [
        {
          appid: "10",
          title: "Counter-Strike",
          cover: "capa-10",
          manifest: false,
        },
        { appid: "70", title: "Half-Life", cover: "capa-70", manifest: true },
      ],
    }),
    nowEpochS(),
  )

  const resposta = await catalogGet("/catalog/v1/popular?limite=1&offset=1")

  assert.equal(resposta.error, null)
  assert.equal(resposta.fallback, false)
  assert.equal(resposta.data.ok, true)
  assert.equal(resposta.data.total, 2)
  assert.deepEqual(resposta.data.itens, [
    { appid: "70", title: "Half-Life", cover: "capa-70", manifest: true },
  ])
  assert.deepEqual(
    catalogGetEspelho("/catalog/v1/popular?limite=1&offset=1"),
    resposta.data,
  )
})
