"use strict"

// Unit/in-process HTTP tests: no PostgreSQL, network service, or external
// provider is needed. They exercise the boundary middleware and the injected
// readiness probe directly.

const test = require("node:test")
const assert = require("node:assert/strict")
const express = require("express")
const { after } = require("node:test")
const {
  REQUEST_ID_RE,
  requestContext,
  structuredErrors,
  sendError,
  handleError,
} = require("../src/api-observability")
const { registerHealthRoutes } = require("../src/health-routes")

function listen(app) {
  const listener = app.listen(0)
  after(() => new Promise((resolve) => listener.close(resolve)))
  return `http://127.0.0.1:${listener.address().port}`
}

const app = express()
app.use(requestContext)
app.use(structuredErrors)
app.get("/ok", (req, res) => res.json({ ok: true }))
app.get("/business-error", (req, res) => res.json({ ok: false, error: "senha_errada" }))
app.get("/bad", (req, res) => res.status(422).json({ error: "filtro_invalido" }))
app.get("/throws", () => {
  const error = new Error("client input")
  error.status = 400
  throw error
})
app.use((req, res) => sendError(req, res, 404, "rota_nao_encontrada"))
// eslint-disable-next-line no-unused-vars
app.use(handleError)
const base = listen(app)

test("request id e propagado no header e nos erros sem quebrar o campo legado", async () => {
  const response = await fetch(`${base}/bad`, { headers: { "x-request-id": "desktop-42" } })
  const body = await response.json()

  assert.equal(response.status, 422)
  assert.equal(response.headers.get("x-request-id"), "desktop-42")
  assert.deepEqual(body, {
    error: "filtro_invalido",
    code: "filtro_invalido",
    message: "A solicitacao nao pode ser concluida",
    request_id: "desktop-42",
  })
})

test("id invalido e substituido e sucessos permanecem com shape original", async () => {
  const response = await fetch(`${base}/ok`, { headers: { "x-request-id": "bad value" } })
  const body = await response.json()
  const id = response.headers.get("x-request-id")

  assert.equal(response.status, 200)
  assert.deepEqual(body, { ok: true })
  assert.ok(id && REQUEST_ID_RE.test(id))

  const businessError = await fetch(`${base}/business-error`)
  assert.deepEqual(await businessError.json(), { ok: false, error: "senha_errada" })
})

test("rotas desconhecidas e excecoes devolvem JSON estruturado", async () => {
  const missing = await fetch(`${base}/does-not-exist`)
  const missingBody = await missing.json()
  assert.equal(missing.status, 404)
  assert.equal(missingBody.error, "rota_nao_encontrada")
  assert.equal(missingBody.code, "rota_nao_encontrada")
  assert.equal(missingBody.request_id, missing.headers.get("x-request-id"))

  const thrown = await fetch(`${base}/throws`)
  const thrownBody = await thrown.json()
  assert.equal(thrown.status, 400)
  assert.deepEqual(thrownBody, {
    error: "erro_requisicao",
    code: "erro_requisicao",
    message: "A solicitacao nao pode ser concluida",
    request_id: thrown.headers.get("x-request-id"),
  })
})

let databaseUp = true
const healthApp = express()
healthApp.use(requestContext)
healthApp.use(structuredErrors)
registerHealthRoutes(healthApp, {
  checkDatabase: async () => {
    if (!databaseUp) throw new Error("database offline")
  },
  now: () => "2026-01-01T00:00:00.000Z",
})
// eslint-disable-next-line no-unused-vars
healthApp.use(handleError)
const healthBase = listen(healthApp)

test("readiness usa 503 quando o banco cai e health mantém contrato", async () => {
  const health = await fetch(`${healthBase}/health`)
  assert.equal(health.status, 200)
  assert.deepEqual(await health.json(), {
    ok: true,
    name: "arcadia-server",
    time: "2026-01-01T00:00:00.000Z",
  })

  const healthy = await fetch(`${healthBase}/ready`)
  assert.equal(healthy.status, 200)
  assert.deepEqual(await healthy.json(), {
    ok: true,
    ready: true,
    name: "arcadia-server",
    time: "2026-01-01T00:00:00.000Z",
  })

  databaseUp = false
  const notReady = await fetch(`${healthBase}/ready`)
  const body = await notReady.json()
  assert.equal(notReady.status, 503)
  assert.equal(body.error, "servico_indisponivel")
  assert.equal(body.code, "servico_indisponivel")
  assert.equal(body.request_id, notReady.headers.get("x-request-id"))
})
