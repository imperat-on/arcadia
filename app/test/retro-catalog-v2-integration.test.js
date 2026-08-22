"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createRetroCatalogV2 } = require("../electron/retro-catalog-v2")
const { buildCanonicalCatalog } = require("../electron/retro-catalog-builder")

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-retro-v2-"))
}

test("migração V1 importa mais de uma página, preserva URI somente no offer", async () => {
  const dir = tempDir()
  const source = { id: "classic", title: "Classics" }
  const games = Array.from({ length: 120 }, (_, index) => ({
    id: `classic:${index}`,
    title: `Game ${index}`,
    sourceId: source.id,
    sourceTitle: source.title,
    platform: "nes",
    uris: [`magnet:?xt=urn:btih:HASH${index}`],
  }))
  let listCalled = false
  const v1Catalog = {
    list() {
      listCalled = true
      throw new Error("migration must not use the paginated/public list")
    },
    async exportAll() {
      return { sources: [source], games, updatedAt: 1 }
    },
    async refresh() {
      return { sources: [source], games, updatedAt: 1 }
    },
  }

  try {
    const catalog = createRetroCatalogV2({ dataDir: dir, v1Catalog })
    const result = await catalog.migrateFromV1()
    assert.equal(listCalled, false)
    assert.equal(result.offersCount, 120)
    assert.equal(catalog.repository.getStats().offers, 120)

    const page = await catalog.list({ limit: 10 })
    assert.equal(page.ok, true)
    assert.equal(page.totalGames, 120)
    assert.equal(page.games.length, 10)
    assert.equal(page.games.some(game => "uris" in game), false)

    const detail = await catalog.getGame(page.games[0].id)
    assert.equal(detail.ok, true)
    assert.ok(detail.offers.length > 0)
    assert.equal(detail.offers.some(offer => "uris" in offer), false)

    const resolved = await catalog.getOffer(detail.offers[0].id)
    assert.equal(resolved.ok, true)
    assert.equal(resolved.offer.uris.length, 1)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("builder usa serial e fuzzy contra catálogo canônico sem cruzar sistema", () => {
  const canonicalGames = [
    {
      id: "retro:ps2:SLUS20312",
      systemId: "sony-playstation-2",
      title: "Example Adventure",
      aliases: [],
      serials: ["SLUS20312"],
      hashes: {},
      regions: ["USA"],
    },
  ]
  const catalog = buildCanonicalCatalog(
    [
      {
        id: "serial-offer",
        normalizedTitle: "Completely Different Release Name",
        systemId: "sony-playstation-2",
        serials: ["SLUS-20312"],
        releaseKind: "game",
      },
      {
        id: "wrong-system",
        normalizedTitle: "Example Adventure",
        systemId: "sony-playstation-3",
        serials: [],
        releaseKind: "game",
      },
    ],
    { canonicalGames },
  )
  assert.equal(catalog.matches.length, 1)
  assert.equal(catalog.matches[0].offerId, "serial-offer")
  assert.equal(catalog.matches[0].method, "serial")
  assert.equal(catalog.matches[0].score, 100)
  assert.equal(catalog.unmatched.length, 1)
})
