"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  SEARCH_INDEX_VERSION,
  normalizeSearchText,
  extractCatalogItems,
  searchLibrary,
  createLocalSearchIndex,
} = require("../electron/local-search")

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-local-search-"))
}

test("normalização é acento/pontuação-insensível e estável", () => {
  assert.equal(normalizeSearchText("  Ação: Half-Life  "), "acao half life")
  assert.equal(normalizeSearchText("Hades Ⅱ"), "hades ⅱ")
  assert.equal(normalizeSearchText(""), "")
})

test("índice deduplica appid, preserva payload e ordena ranking deterministicamente", () => {
  const index = createLocalSearchIndex()
  index.upsert([
    { appid: "10", title: "The Witcher 3", cover: "header", uris: ["magnet:?secret"] },
    { appid: "20", title: "Witcher", cover: "short" },
    { appid: "10", title: "The Witcher 3", heroi: "hero" },
    { appid: "30", title: "Witcher 3: Wild Hunt" },
  ])

  const result = index.search("witcher 3", { limit: 10 })
  assert.deepEqual(result.map((game) => game.appid), ["30", "10"])
  const witcher = result.find((game) => game.appid === "10")
  assert.equal(witcher.cover, "header")
  assert.equal(witcher.heroi, "hero")
  assert.equal("uris" in witcher, false)
  // Não há score/normalização vazando no payload IPC.
  assert.equal("normalizedTitle" in witcher, false)
})

test("índice persiste envelope versionado e recupera busca sem rede", () => {
  const dir = tempDir()
  const file = path.join(dir, "catalog_search_index.json")
  try {
    const first = createLocalSearchIndex({ indexPath: file })
    first.upsert(
      [
        { appid: "1", title: "Portal 2", cover: "cover" },
        { appid: "2", title: "Portal", cover: "cover2" },
      ],
      { persist: true },
    )
    const saved = JSON.parse(fs.readFileSync(file, "utf8"))
    assert.equal(saved.version, SEARCH_INDEX_VERSION)
    assert.equal(saved.entries.length, 2)
    assert.equal(fs.existsSync(`${file}.tmp`), false)

    const second = createLocalSearchIndex({ indexPath: file })
    assert.deepEqual(second.search("portal 2"), [{ appid: "1", title: "Portal 2", cover: "cover" }])
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("hidrata páginas do espelho e cache legado, sem ler fontes Hydra", () => {
  const dir = tempDir()
  const mirror = path.join(dir, "catalog_espelho")
  fs.mkdirSync(mirror)
  try {
    fs.writeFileSync(
      path.join(mirror, "catalog_a.json"),
      JSON.stringify({ ok: true, data: { itens: [{ appid: "100", title: "Jogo Catalogado" }] } }),
    )
    fs.writeFileSync(
      path.join(mirror, "sources_abc_games.json"),
      JSON.stringify({ downloads: [{ title: "Jogo Hydra", uris: ["magnet:?secret"] }] }),
    )
    fs.writeFileSync(
      path.join(dir, "store_genre_cache.json"),
      JSON.stringify({ __all: { completa: [{ appid: "200", title: "Jogo Antigo" }] } }),
    )
    const index = createLocalSearchIndex({ indexPath: path.join(dir, "index.json") })
    index.hydrateCacheFiles(dir, { persist: true })
    assert.equal(index.stats().total, 2)
    assert.equal(index.search("catalogado")[0].appid, "100")
    assert.equal(index.search("hydra").length, 0)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("busca da biblioteca mantém jogo inteiro e permite launcher/categoria", () => {
  const games = [
    { id: "steam:440", title: "Team Fortress 2", launcher: "steam", categories: ["Ação"] },
    { id: "custom:1", title: "Meu Jogo", launcher: "custom", categories: ["RPG"] },
  ]
  assert.deepEqual(searchLibrary(games, "acao"), [games[0]])
  assert.deepEqual(searchLibrary(games, "custom:1"), [games[1]])
  assert.deepEqual(searchLibrary(games, "inexistente"), [])
})
