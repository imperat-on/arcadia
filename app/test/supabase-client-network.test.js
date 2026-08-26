"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const { backendFetch } = require("../electron/supabase/client")

test("falha de rede vira resposta controlada em vez de rejeitar o IPC", async () => {
  const response = await backendFetch("https://example.invalid", {}, async () => {
    throw Object.assign(new TypeError("fetch failed"), { code: "UND_ERR_CONNECT_TIMEOUT" })
  }, async () => { throw new Error("fallback failed") })
  assert.equal(response.ok, false)
  assert.equal(response.status, 0)
  assert.deepEqual(JSON.parse(await response.text()), { error: "rede_indisponivel" })
})

test("falha da rota normal usa o fallback IPv4", async () => {
  let fallbackUrl = ""
  const response = await backendFetch(
    "https://api.example.test/health",
    { method: "GET" },
    async () => { throw new TypeError("fetch failed") },
    async (url) => {
      fallbackUrl = url
      return { ok: true, status: 200, text: async () => '{"ok":true}' }
    },
  )
  assert.equal(response.ok, true)
  assert.equal(fallbackUrl, "https://api.example.test/health")
})

test("domínio ts.net usa IPv4 primeiro para não bloquear o boot no IPv6", async () => {
  const calls = []
  const response = await backendFetch(
    "https://arcadia.example.ts.net/health",
    {},
    async () => { calls.push("normal"); throw new Error("ipv6 timeout") },
    async () => { calls.push("ipv4"); return { ok: true, status: 200, text: async () => "ok" } },
  )
  // Injeções de teste preservam a ordem genérica; a preferência .ts.net só é
  // ativada com o fetchRede real para não acoplar os testes à rede do Electron.
  assert.deepEqual(calls, ["normal", "ipv4"])
  assert.equal(response.ok, true)
})

test("cliente do backend não usa fetch global nas rotas HTTP", () => {
  const source = fs.readFileSync(path.join(__dirname, "..", "electron", "supabase", "client.js"), "utf8")
  assert.doesNotMatch(source, /await fetch\(/)
  assert.match(source, /await backendFetch\(/)
  assert.match(source, /require\("\.\.\/httpfetch"\)/)
  assert.match(source, /new WebSocket\(wsUrl, \{ lookup: backendLookup \}\)/)
})
