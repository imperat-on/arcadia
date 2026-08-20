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
  assert.deepEqual(
    result.map((game) => game.appid),
    ["30", "10"],
  )
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

test("filtros reais combinam facets e preservam o payload", () => {
  const index = createLocalSearchIndex()
  const jogos = [
    {
      appid: "1",
      title: "Zeta",
      launcher: "Steam",
      genres: ["RPG"],
      tags: ["Indie", "Co-op"],
      installed: true,
      cover: "z",
    },
    {
      appid: "2",
      title: "Alpha",
      launcher: "Epic",
      genres: [{ description: "Ação" }],
      tags: ["Indie"],
      installed: false,
      cover: "a",
    },
    {
      appid: "3",
      title: "Beta",
      launcher: "steam",
      genres: ["RPG"],
      tags: ["Co-op"],
      installed: true,
      cover: "b",
    },
  ]
  index.upsert(jogos)

  assert.deepEqual(index.search("", { launcher: "steam" }), [])
  assert.deepEqual(
    index
      .filter({ launcher: "steam", genre: "rpg", tag: "co-op", installed: true })
      .map((g) => g.appid),
    ["3", "1"],
  )
  assert.deepEqual(
    index.search("beta", { filters: { launcher: ["steam"], genre: "RPG" } }).map((g) => g.appid),
    ["3"],
  )
  assert.deepEqual(index.facets(), {
    launcher: { steam: 2, epic: 1 },
    genre: { rpg: 2, acao: 1 },
    tag: { "co op": 2, indie: 2 },
    installed: { true: 2, false: 1 },
  })
  assert.equal(index.search("zeta", { installed: false }).length, 0)
  assert.equal(index.search("zeta")[0].cover, "z")
})

test("paginação e metadados do índice são determinísticos", () => {
  const primeiros = createLocalSearchIndex()
  const segundos = createLocalSearchIndex()
  const jogos = [
    { appid: "30", title: "Mesmo Nome", launcher: "steam" },
    { appid: "10", title: "Alpha", launcher: "steam" },
    { appid: "20", title: "Mesmo Nome", launcher: "epic" },
  ]
  primeiros.upsert(jogos)
  segundos.upsert([...jogos].reverse())
  const a = primeiros.page({ offset: 1, limit: 1 })
  const b = segundos.page({ offset: 1, limit: 1 })
  assert.deepEqual(a.itens, b.itens)
  assert.equal(a.itens[0].appid, "20")
  assert.equal(a.total, 3)
  assert.equal(a.limit, 1)
  assert.equal(a.has_more, true)
  assert.equal(a.next_offset, 2)
  assert.equal(a.index.schema, "arcadia.catalog-search")
  assert.equal(a.index.entry_count, 3)
  assert.equal(a.index.facets.launcher.steam, 2)

  const busca = primeiros.searchPage("", { offset: 1, limit: 1, launcher: "steam" })
  assert.deepEqual(
    busca.itens.map((g) => g.appid),
    ["30"],
  )
  assert.equal(busca.total, 2)
  assert.equal(busca.facets.launcher.steam, 2)
})

test("índice v1 legado carrega e sobe para envelope v2 ao salvar", () => {
  const dir = tempDir()
  const file = path.join(dir, "index.json")
  try {
    fs.writeFileSync(
      file,
      JSON.stringify({
        version: 1,
        entries: [{ source: "catalog", value: { appid: "7", title: "Legado" } }],
      }),
    )
    const index = createLocalSearchIndex({ indexPath: file })
    assert.equal(index.search("legado")[0].appid, "7")
    index.upsert([{ appid: "8", title: "Novo", tags: ["tag"] }], { persist: true })
    const saved = JSON.parse(fs.readFileSync(file, "utf8"))
    assert.equal(saved.version, SEARCH_INDEX_VERSION)
    assert.equal(saved.metadata.entry_count, 2)
    assert.deepEqual(saved.metadata.facets.tag, { tag: 1 })
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
