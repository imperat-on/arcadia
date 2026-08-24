"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-store-local-"))
process.env.ARCADIA_DATA_DIR = dir
process.env.ARCADIA_API_URL = "https://offline.arcadia.test"
const mirror = path.join(dir, "catalog_espelho")
fs.mkdirSync(mirror, { recursive: true })
fs.writeFileSync(
  path.join(mirror, "search_abc.json"),
  JSON.stringify({
    ok: true,
    data: {
      itens: [
        { appid: "1091500", title: "Cyberpunk 2077", cover: "https://cdn/cyber.jpg" },
        { appid: "730", title: "Counter-Strike 2", cover: "https://cdn/cs.jpg" },
      ],
    },
  }),
)
fs.writeFileSync(
  path.join(mirror, "catalog_facets.json"),
  JSON.stringify({
    ok: true,
    data: {
      itens: [
        { appid: "900", title: "Facet RPG", genres: ["RPG"], tags: ["Indie"], installed: false },
        {
          appid: "901",
          title: "Facet Action",
          genres: ["Action"],
          tags: ["Indie"],
          installed: true,
        },
      ],
    },
  }),
)

let chamadas = 0
const fetchOriginal = global.fetch
global.fetch = async () => {
  chamadas++
  throw new Error("rede desligada")
}
const steamstore = require("../electron/steamstore")

test.after(() => {
  global.fetch = fetchOriginal
  fs.rmSync(dir, { recursive: true, force: true })
})

test("store:search usa espelho local sem esperar rede e mantém payload de jogos", async () => {
  const result = await steamstore.search("cyberpunk")
  assert.equal(result.ok, true)
  assert.equal(result.fonte, "local")
  assert.equal(result.cache, true)
  assert.deepEqual(result.jogos, [
    { appid: "1091500", title: "Cyberpunk 2077", cover: "https://cdn/cyber.jpg" },
  ])
  assert.equal(chamadas, 0)
})

test("store:suggest reaproveita o mesmo índice local", async () => {
  const result = await steamstore.suggest("counter")
  assert.equal(result.ok, true)
  assert.deepEqual(result.jogos, [
    { appid: "730", title: "Counter-Strike 2", cover: "https://cdn/cs.jpg" },
  ])
  assert.equal(chamadas, 0)
})

test("store:search aplica facets locais e devolve metadados sem rede", async () => {
  const result = await steamstore.search("facet", {
    genre: "rpg",
    tag: "indie",
    installed: false,
  })
  assert.equal(result.ok, true)
  assert.equal(result.fonte, "local")
  assert.deepEqual(result.jogos, [
    { appid: "900", title: "Facet RPG", genres: ["RPG"], tags: ["Indie"], installed: false },
  ])
  assert.equal(result.facets.genre.rpg, 1)
  assert.equal(result.facets.tag.indie, 1)
  assert.equal(result.facets.installed.false, 1)
  assert.equal(result.index.schema, "arcadia.catalog-search")
  assert.equal(chamadas, 0)
})
