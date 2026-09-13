"use strict"

// Catraca do endereço do backend: primário + reserva.
//
// Um resolvedor ruim (operadora/roteador) ou uma queda do provedor não podem
// derrubar o app quando existe um segundo caminho. O contrato é: só falha de
// REDE troca de endereço; resposta HTTP com erro é resposta legítima e não se
// repete; URL externa (loja, CDN, SteamSpy) não é tocada.

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")

const APP = path.resolve(__dirname, "..")
const CONFIG = path.join(APP, "electron", "supabase", "config.js")
const HTTPFETCH = path.join(APP, "electron", "httpfetch.js")

// Módulos com estado (o endereço preferido é fixado em módulo): recarrega.
function modulo(arquivo) {
  delete require.cache[require.resolve(arquivo)]
  return require(arquivo)
}

/** Instala um fetch de mentira e devolve o log de URLs chamadas. */
function comFetchFalso(handler) {
  const chamadas = []
  const original = globalThis.fetch
  globalThis.fetch = async (url, opts) => {
    chamadas.push(String(url))
    return handler(String(url), opts)
  }
  return { chamadas, restaurar: () => (globalThis.fetch = original) }
}

test("o endereço público é o Worker, com o Funnel como reserva", () => {
  const cfg = modulo(CONFIG)
  assert.match(cfg.DEFAULT_API_URL, /^https:\/\/[a-z0-9.-]+\.workers\.dev$/)
  assert.equal(cfg.FALLBACK_API_URL, "https://zes.tail6e748d.ts.net")
  assert.deepEqual(cfg.urls, [cfg.DEFAULT_API_URL, cfg.FALLBACK_API_URL])
  assert.equal(cfg.url, cfg.DEFAULT_API_URL, "config.url continua sendo o primário")
})

test("com ARCADIA_API_URL (servidor próprio) não existe reserva", () => {
  const antes = process.env.ARCADIA_API_URL
  process.env.ARCADIA_API_URL = "http://127.0.0.1:3000"
  try {
    const cfg = modulo(CONFIG)
    assert.equal(cfg.url, "http://127.0.0.1:3000")
    assert.deepEqual(cfg.urls, ["http://127.0.0.1:3000"])
  } finally {
    if (antes === undefined) delete process.env.ARCADIA_API_URL
    else process.env.ARCADIA_API_URL = antes
  }
})

test("falha de rede cai para o endereço de reserva, preservando o caminho", async () => {
  const cfg = modulo(CONFIG)
  const { fetchRede } = modulo(HTTPFETCH)
  const falso = comFetchFalso((url) => {
    if (url.startsWith(cfg.DEFAULT_API_URL)) throw new Error("getaddrinfo ENOTFOUND")
    return new Response(JSON.stringify({ ok: true }), { status: 200 })
  })
  try {
    const r = await fetchRede(`${cfg.DEFAULT_API_URL}/rest/v1/profiles?select=username`)
    assert.equal(r.status, 200)
    assert.equal(falso.chamadas.length, 2, "tentou o primário e depois a reserva")
    assert.ok(falso.chamadas[1].startsWith(cfg.FALLBACK_API_URL))
    assert.match(falso.chamadas[1], /\/rest\/v1\/profiles\?select=username$/, "caminho e query intactos")
  } finally {
    falso.restaurar()
  }
})

test("depois da troca o app passa a usar o endereço que respondeu", async () => {
  const cfg = modulo(CONFIG)
  const { fetchRede } = modulo(HTTPFETCH)
  const falso = comFetchFalso((url) => {
    if (url.startsWith(cfg.DEFAULT_API_URL)) throw new Error("rede")
    return new Response("{}", { status: 200 })
  })
  try {
    await fetchRede(`${cfg.DEFAULT_API_URL}/health`) // troca e fixa
    falso.chamadas.length = 0
    await fetchRede(`${cfg.DEFAULT_API_URL}/health`) // o chamador ainda monta com o primário
    assert.equal(falso.chamadas.length, 1, "não bate de novo no endereço que falhou")
    assert.ok(falso.chamadas[0].startsWith(cfg.FALLBACK_API_URL))
  } finally {
    falso.restaurar()
  }
})

test("resposta HTTP com erro NÃO troca de endereço", async () => {
  const cfg = modulo(CONFIG)
  const { fetchRede } = modulo(HTTPFETCH)
  const falso = comFetchFalso(() => new Response("nope", { status: 401 }))
  try {
    const r = await fetchRede(`${cfg.DEFAULT_API_URL}/rest/v1/profiles`)
    assert.equal(r.status, 401)
    assert.equal(falso.chamadas.length, 1, "401 é resposta legítima do servidor")
  } finally {
    falso.restaurar()
  }
})

test("URL externa (loja/CDN) não é reescrita nem repetida", async () => {
  const { fetchRede } = modulo(HTTPFETCH)
  const falso = comFetchFalso(() => new Response("[]", { status: 200 }))
  try {
    await fetchRede("https://store.steampowered.com/api/appdetails?appids=440")
    assert.deepEqual(falso.chamadas, ["https://store.steampowered.com/api/appdetails?appids=440"])
  } finally {
    falso.restaurar()
  }
})

test("requisição abortada pelo chamador não é reenviada", async () => {
  const cfg = modulo(CONFIG)
  const { fetchRede } = modulo(HTTPFETCH)
  const ctrl = new AbortController()
  const falso = comFetchFalso(() => {
    ctrl.abort()
    throw new Error("The operation was aborted")
  })
  try {
    await assert.rejects(() => fetchRede(`${cfg.DEFAULT_API_URL}/health`, { signal: ctrl.signal }), /aborted/)
    assert.equal(falso.chamadas.length, 1, "abortar é decisão do chamador, não falha de rede")
  } finally {
    falso.restaurar()
  }
})
