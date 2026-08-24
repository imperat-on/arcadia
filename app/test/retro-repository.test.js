"use strict"

const { describe, it, beforeEach } = require("node:test")
const assert = require("node:assert")
const { createRetroRepository } = require("../electron/retro-repository")
const path = require("node:path")
const os = require("node:os")
const fs = require("node:fs")

describe("RetroRepository", () => {
  let tmpDir
  let repository
  let fsImpl

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "retro-repo-test-"))

    // Mock filesystem for testing
    const files = new Map()
    fsImpl = {
      mkdirSync: (dir, opts) => {
        files.set(dir, { type: "dir" })
      },
      statSync: (p) => {
        const file = files.get(p)
        if (!file) throw Object.assign(new Error("ENOENT"), { code: "ENOENT" })
        return {
          isFile: () => file.type === "file",
          size: file.content ? Buffer.byteLength(file.content) : 0,
        }
      },
      lstatSync: (p) => fsImpl.statSync(p),
      readFileSync: (p, encoding) => {
        const file = files.get(p)
        if (!file) throw Object.assign(new Error("ENOENT"), { code: "ENOENT" })
        return file.content || ""
      },
      writeFileSync: (p, content, opts) => {
        files.set(p, { type: "file", content })
      },
      renameSync: (from, to) => {
        const file = files.get(from)
        if (file) {
          files.set(to, file)
          files.delete(from)
        }
      },
    }

    repository = createRetroRepository({
      dataDir: tmpDir,
      fsImpl,
    })
  })

  it("should initialize with empty state", () => {
    const games = repository.getGames()
    assert.strictEqual(games.length, 0)

    const offers = repository.getOffers()
    assert.strictEqual(offers.length, 0)
  })

  it("should store and retrieve games", () => {
    const games = [
      {
        id: "retro:sony-playstation:SLUS-00312",
        systemId: "sony-playstation",
        title: "Final Fantasy VII",
        sortTitle: "final fantasy vii",
        offerCount: 2,
      },
    ]

    repository.setCatalogIndex(games)

    const retrieved = repository.getGames()
    assert.strictEqual(retrieved.length, 1)
    assert.strictEqual(retrieved[0].title, "Final Fantasy VII")
  })

  it("should store and retrieve offers", () => {
    const offers = [
      {
        id: "offer123",
        sourceId: "source1",
        normalizedTitle: "Final Fantasy VII",
        systemId: "sony-playstation",
        uris: ["magnet:?xt=urn:btih:abc123"],
      },
    ]

    repository.setOffers(offers)

    const retrieved = repository.getOffers()
    assert.strictEqual(retrieved.length, 1)
    assert.strictEqual(retrieved[0].id, "offer123")
  })

  it("should get offers for a specific game", () => {
    const offers = [
      {
        id: "offer1",
        sourceId: "source1",
        normalizedTitle: "Game 1",
        systemId: "sony-playstation",
        uris: ["magnet:1"],
      },
      {
        id: "offer2",
        sourceId: "source2",
        normalizedTitle: "Game 1",
        systemId: "sony-playstation",
        uris: ["magnet:2"],
      },
      {
        id: "offer3",
        sourceId: "source1",
        normalizedTitle: "Game 2",
        systemId: "sony-playstation",
        uris: ["magnet:3"],
      },
    ]

    const matches = [
      { offerId: "offer1", gameId: "game1", score: 85 },
      { offerId: "offer2", gameId: "game1", score: 85 },
      { offerId: "offer3", gameId: "game2", score: 85 },
    ]

    repository.setOffers(offers)
    repository.setMatches(matches)

    const game1Offers = repository.getOffersForGame("game1")
    assert.strictEqual(game1Offers.length, 2)
    assert.strictEqual(game1Offers[0].id, "offer1")
    assert.strictEqual(game1Offers[1].id, "offer2")
  })

  it("should store and retrieve artwork", () => {
    repository.setArtwork("game1", {
      cover: "https://example.com/cover.jpg",
      provider: "libretro",
    })

    const artwork = repository.getArtwork("game1")
    assert.strictEqual(artwork.cover, "https://example.com/cover.jpg")
    assert.strictEqual(artwork.provider, "libretro")
    assert.ok(artwork.updatedAt)
  })

  it("should store and retrieve overrides", () => {
    repository.setOverride("offer123", {
      gameId: "game1",
      artworkUrl: "https://example.com/custom.jpg",
    })

    const override = repository.getOverride("offer123")
    assert.strictEqual(override.gameId, "game1")
    assert.strictEqual(override.artworkUrl, "https://example.com/custom.jpg")
  })

  it("should remove overrides", () => {
    repository.setOverride("offer123", { gameId: "game1" })
    repository.removeOverride("offer123")

    const override = repository.getOverride("offer123")
    assert.strictEqual(override, null)
  })

  it("should track unmatched offers", () => {
    const unmatched = [
      { offerId: "offer1", reason: "no system", title: "Unknown Game" },
      { offerId: "offer2", reason: "no match", title: "Another Game" },
    ]

    repository.setUnmatched(unmatched)

    const retrieved = repository.getUnmatched()
    assert.strictEqual(retrieved.length, 2)
    assert.strictEqual(retrieved[0].offerId, "offer1")
  })

  it("should provide repository stats", () => {
    const games = [
      { id: "game1", title: "Game 1", systemId: "sony-playstation" },
      { id: "game2", title: "Game 2", systemId: "nintendo-nes" },
    ]

    const offers = [
      { id: "offer1", normalizedTitle: "Game 1" },
      { id: "offer2", normalizedTitle: "Game 1" },
      { id: "offer3", normalizedTitle: "Game 2" },
    ]

    const matches = [
      { offerId: "offer1", gameId: "game1" },
      { offerId: "offer2", gameId: "game1" },
      { offerId: "offer3", gameId: "game2" },
    ]

    const unmatched = [
      { offerId: "offer4", reason: "no match" },
    ]

    repository.setCatalogIndex(games)
    repository.setOffers(offers)
    repository.setMatches(matches)
    repository.setUnmatched(unmatched)

    const stats = repository.getStats()
    assert.strictEqual(stats.games, 2)
    assert.strictEqual(stats.offers, 3)
    assert.strictEqual(stats.matched, 3)
    assert.strictEqual(stats.unmatched, 1)
  })

  it("should reject offers exceeding maximum", () => {
    const largeOfferArray = new Array(200000).fill({
      id: "offer",
      normalizedTitle: "Game",
    })

    assert.throws(() => {
      repository.setOffers(largeOfferArray)
    }, /exceed maximum/)
  })
})
