"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { performance } = require("node:perf_hooks")
const { CATALOG_SOURCE, createLocalSearchIndex } = require("../electron/local-search")

const SYNTHETIC_COUNT = 8192
const PAGE_LIMIT = 32
const SEARCH_BUDGET_MS = 1500
const FACETS_BUDGET_MS = 1500
const LAUNCHERS = ["steam", "epic", "gog", "heroic"]
const GENRES = ["rpg", "strategy", "action", "simulation"]
const TAGS = ["indie", "story", "co-op", "offline"]

function deterministicCatalog() {
  const catalog = [
    {
      appid: "fixture-neon-1",
      title: "Néon Harbor",
      launcher: "steam",
      genres: ["Action"],
      tags: ["Indie", "Offline"],
      installed: true,
    },
    {
      appid: "fixture-neon-2",
      title: "Neon Harbor: Afterlight",
      launcher: "steam",
      genres: ["Action"],
      tags: ["Story"],
      installed: true,
    },
    {
      appid: "fixture-neon-3",
      title: "Neon Harbour",
      launcher: "epic",
      genres: ["Adventure"],
      tags: ["Offline"],
      installed: false,
    },
  ]

  for (let index = 0; index < SYNTHETIC_COUNT; index++) {
    catalog.push({
      appid: String(100000 + index),
      title: `Catalog Game ${String(index).padStart(4, "0")}`,
      launcher: LAUNCHERS[index % LAUNCHERS.length],
      genres: [GENRES[Math.floor(index / LAUNCHERS.length) % GENRES.length]],
      tags: [TAGS[index % TAGS.length], TAGS[(index + 1) % TAGS.length]],
      installed: index % 2 === 0,
    })
  }
  return catalog
}

function measured(fn) {
  const started = performance.now()
  const value = fn()
  return { value, elapsedMs: performance.now() - started }
}

test("índice local offline mantém corretude em catálogo determinístico", () => {
  const index = createLocalSearchIndex()
  index.upsert(deterministicCatalog(), { source: CATALOG_SOURCE })

  const exact = index.search("neon harbor", { limit: 10 })
  assert.deepEqual(
    exact.map((game) => game.appid),
    ["fixture-neon-1", "fixture-neon-2"],
  )
  assert.equal(exact[0].title, "Néon Harbor")

  const firstPage = index.page({
    source: CATALOG_SOURCE,
    query: "neon harbor",
    offset: 0,
    limit: 1,
  })
  assert.deepEqual(
    firstPage.itens.map((game) => game.appid),
    ["fixture-neon-1"],
  )
  assert.equal(firstPage.total, 2)
  assert.equal(firstPage.has_more, true)
  assert.equal(firstPage.next_offset, 1)
  assert.deepEqual(firstPage.facets, {
    launcher: { steam: 2 },
    genre: { action: 2 },
    tag: { indie: 1, offline: 1, story: 1 },
    installed: { true: 2 },
  })

  const secondPage = index.page({
    source: CATALOG_SOURCE,
    query: "neon harbor",
    offset: firstPage.next_offset,
    limit: 1,
  })
  assert.deepEqual(
    secondPage.itens.map((game) => game.appid),
    ["fixture-neon-2"],
  )
  assert.equal(secondPage.has_more, false)
  assert.equal(secondPage.next_offset, null)

  const filtered = index.page({
    source: CATALOG_SOURCE,
    query: "catalog",
    launcher: "gog",
    genre: "strategy",
    offset: 510,
    limit: PAGE_LIMIT,
  })
  assert.equal(filtered.total, SYNTHETIC_COUNT / 16)
  assert.equal(filtered.itens.length, 2)
  assert.deepEqual(
    filtered.itens.map((game) => game.appid),
    ["108166", "108182"],
  )
  assert.equal(filtered.facets.launcher.gog, SYNTHETIC_COUNT / 16)
  assert.equal(filtered.facets.genre.strategy, SYNTHETIC_COUNT / 16)
  assert.equal(filtered.facets.installed.true, SYNTHETIC_COUNT / 16)
  assert.equal(filtered.facets.installed.false, undefined)
  assert.deepEqual(filtered.facets.tag, { "co op": 512, offline: 512 })
})

test("benchmark local offline mede busca, paginação e facets sem limite instável", () => {
  const index = createLocalSearchIndex()
  index.upsert(deterministicCatalog(), { source: CATALOG_SOURCE })

  const measuredPages = measured(() => {
    let pages = 0
    for (let iteration = 0; iteration < 6; iteration++) {
      const page = index.page({
        source: CATALOG_SOURCE,
        query: "catalog",
        launcher: "gog",
        genre: "strategy",
        offset: iteration * PAGE_LIMIT,
        limit: PAGE_LIMIT,
      })
      assert.equal(page.total, SYNTHETIC_COUNT / 16)
      assert.equal(page.itens.length, PAGE_LIMIT)
      assert.equal(page.facets.launcher.gog, SYNTHETIC_COUNT / 16)
      assert.equal(page.facets.genre.strategy, SYNTHETIC_COUNT / 16)
      assert.equal(page.has_more, true)
      assert.equal(page.next_offset, (iteration + 1) * PAGE_LIMIT)
      pages += page.itens.length
    }
    return pages
  })
  assert.equal(measuredPages.value, PAGE_LIMIT * 6)
  assert.ok(
    measuredPages.elapsedMs < SEARCH_BUDGET_MS,
    `busca/paginação levou ${measuredPages.elapsedMs.toFixed(1)}ms (limite ${SEARCH_BUDGET_MS}ms)`,
  )

  const measuredFacets = measured(() => {
    let total = 0
    for (let iteration = 0; iteration < 3; iteration++) {
      const facets = index.facets({ source: CATALOG_SOURCE, launcher: LAUNCHERS[iteration] })
      assert.equal(
        facets.launcher[LAUNCHERS[iteration]],
        SYNTHETIC_COUNT / 4 + [2, 1, 0][iteration],
      )
      total += Object.values(facets.genre).reduce((sum, count) => sum + count, 0)
    }
    return total
  })
  assert.equal(measuredFacets.value, 2050 + 2049 + 2048)
  assert.ok(
    measuredFacets.elapsedMs < FACETS_BUDGET_MS,
    `facets levou ${measuredFacets.elapsedMs.toFixed(1)}ms (limite ${FACETS_BUDGET_MS}ms)`,
  )
})
