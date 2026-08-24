"use strict"

// Route-level guard tests stop before any SQL, so this file also runs without a
// PostgreSQL service. Database-backed behavior is exercised by the integration
// suite when TEST_DATABASE_URL is configured.
process.env.NODE_ENV = "test"
process.env.TEST_DATABASE_URL = process.env.TEST_DATABASE_URL || "postgres://unused"
process.env.JWT_SECRET = process.env.JWT_SECRET || "test-only-community-jwt-secret-32-chars"

const test = require("node:test")
const assert = require("node:assert/strict")
const express = require("express")
const { registerCommunityRoutes } = require("../src/community-routes")
const { requestContext, structuredErrors } = require("../src/api-observability")

const app = express()
app.use(requestContext)
app.use(structuredErrors)
app.use(express.json())
registerCommunityRoutes(app)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
test.after(() => listener.close())

async function request(path, options = {}) {
  const response = await fetch(base + path, {
    ...options,
    headers: { "content-type": "application/json", ...options.headers },
  })
  return { status: response.status, headers: response.headers, body: await response.json() }
}

test("community rejects unauthenticated writes with structured error", async () => {
  const result = await request("/community/v1/collections", {
    method: "POST",
    body: JSON.stringify({ title: "privada" }),
  })
  assert.equal(result.status, 401)
  assert.equal(result.body.error, "nao_autenticado")
  assert.equal(result.body.code, "nao_autenticado")
  assert.ok(result.body.request_id)
  assert.equal(result.headers.get("x-request-id"), result.body.request_id)
})

test("community validates appid before opening the database", async () => {
  const result = await request("/community/v1/reviews?appid=../etc")
  assert.equal(result.status, 400)
  assert.equal(result.body.error, "appid_invalido")
  assert.equal(result.body.code, "appid_invalido")
})

test("community validates pagination limits before opening the database", async () => {
  const result = await request("/community/v1/listas?limit=not-a-number")
  assert.equal(result.status, 400)
  assert.equal(result.body.error, "limit_invalido")
})
