"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-cat-"))
process.env.ARCADIA_DATA_DIR = tmp
process.env.ARCADIA_SUPABASE_URL = "https://arcadia.test"

const fetchOriginal = global.fetch
const { catalogGet, catalogGetEspelho, espelhoPath } = require("../electron/catalog")

test.after(() => {
  global.fetch = fetchOriginal
  fs.rmSync(tmp, { recursive: true, force: true })
})

test("catalogGet grava o espelho da resposta do servidor", async () => {
  global.fetch = async (url, opts) => {
    assert.equal(url, "https://arcadia.test/catalog/v1/popular")
    assert.equal(opts.headers.accept, "application/json")
    return {
      ok: true,
      status: 200,
      json: async () => ({ ok: true, itens: [{ appid: "1" }] }),
    }
  }

  const r = await catalogGet("/catalog/v1/popular")

  assert.equal(r.error, null)
  assert.equal(r.fallback, false)
  assert.deepEqual(r.data.itens, [{ appid: "1" }])
  assert.equal(
    espelhoPath("/catalog/v1/popular"),
    path.join(tmp, "catalog_espelho", "popular.json"),
  )
  assert.deepEqual(catalogGetEspelho("/catalog/v1/popular"), r.data)
})

test("catalogGet usa o espelho quando o servidor esta fora", async () => {
  global.fetch = async () => {
    throw new Error("ECONNREFUSED")
  }

  const r = await catalogGet("/catalog/v1/popular")

  assert.equal(r.error, null)
  assert.equal(r.fallback, true)
  assert.deepEqual(r.data.itens, [{ appid: "1" }])
})

test("catalogGet noMirror nunca persiste payload sensível nem usa fallback", async () => {
  global.fetch = async () => ({
    ok: true,
    status: 200,
    json: async () => ({ ok: true, offer: { uris: ["magnet:?xt=urn:btih:SECRET"] } }),
  })
  const route = "/catalog/v1/retro/offers/private-offer"
  const online = await catalogGet(route, { noMirror: true })
  assert.equal(online.data.offer.uris.length, 1)
  assert.equal(catalogGetEspelho(route), null)

  global.fetch = async () => { throw new Error("offline") }
  const offline = await catalogGet(route, { noMirror: true })
  assert.equal(offline.data, null)
  assert.match(offline.error.message, /offline/)
})

test("espelho separa queries e permanece dentro do diretorio de catalogo", () => {
  const destino = espelhoPath("/catalog/v1/sources/../../passwd/games?q=x")
  assert.ok(destino.startsWith(path.join(tmp, "catalog_espelho") + path.sep))
  assert.ok(!destino.includes(".."))
  assert.notEqual(
    espelhoPath("/catalog/v1/items?appids=1"),
    espelhoPath("/catalog/v1/items?appids=2"),
  )
})

test("sources monta o indice e busca o jogo completo pelo catalogo", async () => {
  const id = "0123456789ab"
  fs.writeFileSync(
    path.join(tmp, "sources.json"),
    JSON.stringify([{ id, url: "https://hydra.test/source.json", name: "Teste" }]),
  )
  global.fetch = async (url) => {
    assert.equal(url, `https://arcadia.test/catalog/v1/sources/${id}/games`)
    return {
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          name: "Teste",
          downloads: [
            {
              title: "Jogo de Teste",
              fileSize: "1 GB",
              uris: ["magnet:?xt=urn:btih:teste"],
            },
          ],
        },
      }),
    }
  }
  const sources = require("../electron/sources")

  const resultados = await sources.search("jogo", 10)
  const completo = await sources.getGame(resultados[0].ref)

  assert.equal(resultados.length, 1)
  assert.equal(resultados[0].ref, `${id}:0`)
  assert.equal(completo.ok, true)
  assert.deepEqual(completo.game.uris, ["magnet:?xt=urn:btih:teste"])
})
