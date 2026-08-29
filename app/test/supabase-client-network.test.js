"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const { backendFetch, getClient, resolveBackendIpv4 } = require("../electron/supabase/client")
const { myProfile } = require("../electron/supabase/auth")

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


test("consultas IPv4 simultâneas compartilham a mesma consulta DoH", async () => {
  const originalFetch = global.fetch
  const hostname = `coalesce-${process.pid}-${Date.now()}.invalid`
  let chamadas = 0
  let liberar
  let iniciou
  const iniciouDoH = new Promise((resolve) => { iniciou = resolve })
  const bloqueio = new Promise((resolve) => { liberar = resolve })
  global.fetch = async () => {
    chamadas++
    iniciou()
    await bloqueio
    return {
      ok: true,
      status: 200,
      json: async () => ({ Answer: [{ type: 1, data: "192.0.2.10" }] }),
    }
  }
  try {
    const a = resolveBackendIpv4(hostname)
    const b = resolveBackendIpv4(hostname)
    await Promise.race([
      iniciouDoH,
      new Promise((_, reject) => setTimeout(() => reject(new Error("DoH não iniciou")), 1_500)),
    ])
    assert.equal(chamadas, 1)
    liberar()
    assert.deepEqual(await Promise.all([a, b]), [["192.0.2.10"], ["192.0.2.10"]])
  } finally {
    liberar?.()
    global.fetch = originalFetch
  }
})


test("getUser compartilha validação concorrente do mesmo token", async () => {
  const auth = getClient().auth
  const originalRequest = auth._request
  const originalSession = auth._session
  let chamadas = 0
  let liberar
  const bloqueio = new Promise((resolve) => { liberar = resolve })
  auth._session = { access_token: "token-single-flight", user: { id: "u-single-flight" } }
  auth._userValidatedAt = 0
  auth._userInFlight = null
  auth._request = async () => {
    chamadas++
    await bloqueio
    return { data: { user: auth._session.user }, error: null }
  }
  try {
    const a = auth.getUser()
    const b = auth.getUser()
    await new Promise((resolve) => setImmediate(resolve))
    assert.equal(chamadas, 1)
    liberar()
    assert.deepEqual(await Promise.all([a, b]), [
      { data: { user: { id: "u-single-flight" } }, error: null },
      { data: { user: { id: "u-single-flight" } }, error: null },
    ])
  } finally {
    liberar()
    auth._request = originalRequest
    auth._session = originalSession
    auth._resetUserCache()
  }
})


test("myProfile compartilha SELECT concorrente e reaproveita cache curto", async () => {
  const client = getClient()
  const auth = client.auth
  const originalFrom = client.from
  const originalGetUser = auth.getUser
  const originalSession = auth._session
  const token = `profile-token-${process.pid}-${Date.now()}`
  let chamadas = 0
  let iniciar
  let liberar
  const iniciou = new Promise((resolve) => { iniciar = resolve })
  const bloqueio = new Promise((resolve) => { liberar = resolve })
  auth._session = { access_token: token, user: { id: "u-profile-single-flight" } }
  auth.getUser = async () => ({ data: { user: auth._session.user }, error: null })
  client.from = () => ({
    select() { return this },
    eq() { return this },
    async maybeSingle() {
      chamadas++
      iniciar()
      await bloqueio
      return { data: { username: "perfil", avatar_url: null }, error: null }
    },
  })
  try {
    const a = myProfile()
    const b = myProfile()
    await Promise.race([
      iniciou,
      new Promise((_, reject) => setTimeout(() => reject(new Error("SELECT não iniciou")), 1_000)),
    ])
    assert.equal(chamadas, 1)
    liberar()
    const [pa, pb] = await Promise.all([a, b])
    assert.deepEqual(pa, pb)
    assert.equal((await myProfile()).profile.username, "perfil")
    assert.equal(chamadas, 1)
  } finally {
    liberar?.()
    client.from = originalFrom
    auth.getUser = originalGetUser
    auth._session = originalSession
    auth._resetUserCache()
  }
})
