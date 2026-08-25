"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { createRetroServerCatalog } = require("../electron/retro-server-catalog")

test("server-first usa resposta pronta e não consulta fallback", async () => {
  let fallbackCalls = 0
  const catalog = createRetroServerCatalog({
    catalogGet: async (path, options) => ({ data: { ok: true, path, noMirror: options?.noMirror } }),
    fallback: {
      list: async () => { fallbackCalls++; return { ok: false } },
      getGame: async () => { fallbackCalls++; return { ok: false } },
      getOffer: async () => { fallbackCalls++; return { ok: false } },
      refresh: async () => {}, migrateFromV1: async () => {}, repository: {},
    },
  })
  const page = await catalog.list({ query: "Mario", offset: 24, limit: 24 })
  assert.match(page.path, /retro\/games\?offset=24&limit=24&query=Mario/)
  const offer = await catalog.getOffer("offer-1")
  assert.equal(offer.noMirror, true)
  assert.equal(fallbackCalls, 0)
})

test("server-first cai no V2 local quando backend está indisponível", async () => {
  const catalog = createRetroServerCatalog({
    catalogGet: async () => ({ data: null, error: { message: "offline" } }),
    fallback: {
      list: async () => ({ ok: true, games: [{ id: "local" }] }),
      getGame: async () => ({ ok: true, game: { id: "local" } }),
      getOffer: async () => ({ ok: true, offer: { id: "local" } }),
      refresh: async () => {}, migrateFromV1: async () => {}, repository: {},
    },
  })
  assert.equal((await catalog.list()).games[0].id, "local")
  assert.equal((await catalog.getGame("local")).game.id, "local")
})

test("detalhe não espera fontes pessoais lentas", async () => {
  const catalog = createRetroServerCatalog({
    catalogGet: async () => ({
      data: { ok: true, game: { title: "Mario", offerCount: 1 }, offers: [] },
    }),
    sources: {
      search: () => new Promise(() => {}),
      getGame: async () => ({ ok: false }),
    },
    fallback: {
      list: async () => ({ ok: false }),
      getGame: async () => ({ ok: false }),
      getOffer: async () => ({ ok: false }),
      refresh: async () => {}, migrateFromV1: async () => {}, repository: {},
    },
  })
  const inicio = Date.now()
  const response = await catalog.getGame("retro:nes:mario")
  assert.equal(response.ok, true)
  assert.ok(Date.now() - inicio < 1000, "getGame deve responder sem esperar as fontes")
  assert.equal(response.offers.length, 0)
})

test("ofertas pessoais entram quando respondem a tempo", async () => {
  const catalog = createRetroServerCatalog({
    catalogGet: async () => ({
      data: {
        ok: true,
        game: { title: "Mario", systemId: "nintendo-nes", offerCount: 1 },
        offers: [],
      },
    }),
    sources: {
      search: async () => [{
        ref: "fonte:mario",
        src: "Minha fonte",
        title: "Mario",
        fileSize: "4 MB",
      }],
      getGame: async () => ({ ok: false }),
    },
    fallback: {
      list: async () => ({ ok: false }),
      getGame: async () => ({ ok: false }),
      getOffer: async () => ({ ok: false }),
      refresh: async () => {}, migrateFromV1: async () => {}, repository: {},
    },
  })
  const response = await catalog.getGame("retro:nes:mario")
  assert.equal(response.offers.length, 1)
  assert.equal(response.offers[0].id, "personal:fonte:mario")
  assert.equal(response.game.offerCount, 2)
})
