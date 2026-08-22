"use strict"

const { describe, it } = require("node:test")
const assert = require("node:assert")
const {
  buildCanonicalCatalog,
  generateGameId,
  createCanonicalGame,
  mergeOffers,
} = require("../electron/retro-catalog-builder")

describe("RetroCatalogBuilder", () => {
  describe("generateGameId", () => {
    it("should generate ID from serial", () => {
      const id = generateGameId("sony-playstation", { serial: "SLUS00312" })
      assert.strictEqual(id, "retro:sony-playstation:SLUS00312")
    })

    it("should generate ID from sha1", () => {
      const id = generateGameId("nintendo-nes", {
        sha1: "abcdef1234567890abcdef1234567890abcdef12",
      })
      assert.strictEqual(id, "retro:nintendo-nes:sha1:abcdef1234567890")
    })

    it("should generate ID from crc32", () => {
      const id = generateGameId("nintendo-nes", { crc32: "12345678" })
      assert.strictEqual(id, "retro:nintendo-nes:crc32:12345678")
    })

    it("should generate slug-based ID as fallback", () => {
      const id = generateGameId("nintendo-nes", { title: "Super Mario Bros" })
      assert.ok(id.startsWith("retro:nintendo-nes:slug:"))
    })
  })

  describe("createCanonicalGame", () => {
    it("should create canonical game from offer", () => {
      const offer = {
        systemId: "sony-playstation",
        normalizedTitle: "Final Fantasy VII",
        serials: ["SLUS00312"],
        region: "USA",
        languages: ["en"],
        cover: "https://example.com/cover.jpg",
      }

      const game = createCanonicalGame(offer, 1)

      assert.strictEqual(game.systemId, "sony-playstation")
      assert.strictEqual(game.title, "Final Fantasy VII")
      assert.strictEqual(game.serials[0], "SLUS00312")
      assert.strictEqual(game.regions[0], "USA")
      assert.strictEqual(game.offerCount, 1)
      assert.strictEqual(game.artwork.cover, "https://example.com/cover.jpg")
    })
  })

  describe("mergeOffers", () => {
    it("should merge multiple offers for the same game", () => {
      const offers = [
        {
          normalizedTitle: "Final Fantasy VII",
          originalTitle: "Final Fantasy VII [USA]",
          serials: ["SLUS00312"],
          region: "USA",
          languages: ["en"],
        },
        {
          normalizedTitle: "Final Fantasy VII",
          originalTitle: "Final Fantasy VII [EUR]",
          serials: ["SCES00867"],
          region: "EUR",
          languages: ["en", "fr", "de"],
        },
      ]

      const merged = mergeOffers(offers)

      assert.strictEqual(merged.offerCount, 2)
      assert.deepStrictEqual(merged.serials, ["SLUS00312", "SCES00867"])
      assert.deepStrictEqual(merged.regions, ["USA", "EUR"])
      assert.ok(merged.languages.includes("en"))
      assert.ok(merged.languages.includes("fr"))
    })
  })

  describe("buildCanonicalCatalog", () => {
    it("should build catalog from offers", () => {
      const offers = [
        {
          id: "offer1",
          sourceId: "source1",
          normalizedTitle: "Super Mario Bros",
          systemId: "nintendo-nes",
          releaseKind: "game",
          serials: [],
          uris: ["magnet:1"],
        },
        {
          id: "offer2",
          sourceId: "source2",
          normalizedTitle: "Super Mario Bros",
          systemId: "nintendo-nes",
          releaseKind: "game",
          serials: [],
          uris: ["magnet:2"],
        },
        {
          id: "offer3",
          sourceId: "source1",
          normalizedTitle: "The Legend of Zelda",
          systemId: "nintendo-nes",
          releaseKind: "game",
          serials: [],
          uris: ["magnet:3"],
        },
      ]

      const catalog = buildCanonicalCatalog(offers)

      assert.strictEqual(catalog.games.length, 2)
      assert.strictEqual(catalog.matches.length, 3)
      assert.strictEqual(catalog.stats.gamesCreated, 2)

      const marioGame = catalog.games.find(g => g.title === "Super Mario Bros")
      assert.strictEqual(marioGame.offerCount, 2)
    })

    it("should filter out non-game releases", () => {
      const offers = [
        {
          id: "offer1",
          normalizedTitle: "Game",
          systemId: "nintendo-nes",
          releaseKind: "game",
        },
        {
          id: "offer2",
          normalizedTitle: "BIOS Files",
          systemId: "nintendo-nes",
          releaseKind: "bios",
        },
        {
          id: "offer3",
          normalizedTitle: "100 in 1",
          systemId: "nintendo-nes",
          releaseKind: "collection",
        },
      ]

      const catalog = buildCanonicalCatalog(offers)

      assert.strictEqual(catalog.games.length, 1)
      assert.strictEqual(catalog.stats.gameOffers, 1)
      assert.strictEqual(catalog.stats.specialOffers, 2)
    })

    it("should mark offers without system as unmatched", () => {
      const offers = [
        {
          id: "offer1",
          normalizedTitle: "Unknown Game",
          systemId: null,
          releaseKind: "game",
        },
      ]

      const catalog = buildCanonicalCatalog(offers)

      assert.strictEqual(catalog.games.length, 0)
      assert.strictEqual(catalog.unmatched.length, 1)
      assert.strictEqual(catalog.unmatched[0].reason, "no system identified")
    })

    it("should count games by system", () => {
      const offers = [
        {
          id: "offer1",
          normalizedTitle: "Game 1",
          systemId: "nintendo-nes",
          releaseKind: "game",
        },
        {
          id: "offer2",
          normalizedTitle: "Game 2",
          systemId: "nintendo-nes",
          releaseKind: "game",
        },
        {
          id: "offer3",
          normalizedTitle: "Game 3",
          systemId: "sony-playstation",
          releaseKind: "game",
        },
      ]

      const catalog = buildCanonicalCatalog(offers)

      assert.strictEqual(catalog.stats.bySystem["nintendo-nes"], 2)
      assert.strictEqual(catalog.stats.bySystem["sony-playstation"], 1)
    })
  })
})
