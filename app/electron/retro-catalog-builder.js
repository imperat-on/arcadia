"use strict"

/**
 * Retro Catalog Builder
 *
 * Orchestrates the transformation of Hydra downloads into a canonical catalog:
 * 1. Processes offers from sources
 * 2. Matches offers to canonical games
 * 3. Consolidates duplicate games
 * 4. Generates catalog index with offer counts
 */

const { processSourceDownloads, updateOfferMatch, filterByReleaseKind, groupOffersByGame } = require("./retro-offers")
const { matchBatch, getMatchQuality } = require("./retro-matcher")
const { listSystems, getSystem } = require("./retro-systems")
const { extractSerial } = require("./retro-systems")

/**
 * Generate a stable game ID from identity data.
 * @param {string} systemId - System ID
 * @param {object} identity - Identity data (serial, hash, title)
 * @returns {string} - Stable game ID
 */
function generateGameId(systemId, identity) {
  if (identity.serial) {
    return `retro:${systemId}:${identity.serial}`
  }
  if (identity.sha1) {
    return `retro:${systemId}:sha1:${identity.sha1.slice(0, 16)}`
  }
  if (identity.crc32) {
    return `retro:${systemId}:crc32:${identity.crc32}`
  }
  // Fallback to title-based slug
  const crypto = require("node:crypto")
  const slug = crypto.createHash("sha256").update(identity.title || "unknown", "utf8").digest("hex").slice(0, 12)
  return `retro:${systemId}:slug:${slug}`
}

/**
 * Create a canonical game from an offer.
 * @param {object} offer - Matched offer
 * @param {number} offerCount - Number of offers for this game
 * @returns {object} - Canonical game
 */
function createCanonicalGame(offer, offerCount = 1) {
  const system = getSystem(offer.systemId)

  return {
    id: generateGameId(offer.systemId, {
      serial: offer.serials?.[0],
      title: offer.normalizedTitle,
    }),
    systemId: offer.systemId,
    title: offer.normalizedTitle,
    sortTitle: offer.normalizedTitle.toLowerCase(),
    aliases: [],
    serials: offer.serials || [],
    hashes: {},
    regions: offer.region ? [offer.region] : [],
    languages: offer.languages || [],
    releaseDate: null,
    developer: null,
    publisher: null,
    genres: [],
    summary: null,
    artwork: {
      cover: offer.cover || null,
      provider: offer.cover ? "source" : null,
    },
    offerCount,
    matchQuality: offer.match?.quality || "unmatched",
  }
}

/**
 * Merge multiple offers into consolidated game data.
 * @param {object[]} offers - Array of offers for the same game
 * @returns {object} - Merged game data
 */
function mergeOffers(offers) {
  if (!offers.length) return null

  const base = offers[0]
  const allSerials = new Set()
  const allRegions = new Set()
  const allLanguages = new Set()
  const allAliases = new Set()

  for (const offer of offers) {
    // Collect serials
    if (offer.serials) {
      offer.serials.forEach(s => allSerials.add(s))
    }

    // Collect regions
    if (offer.region) {
      allRegions.add(offer.region)
    }

    // Collect languages
    if (offer.languages) {
      offer.languages.forEach(l => allLanguages.add(l))
    }

    // Collect title variations as aliases
    if (offer.originalTitle && offer.originalTitle !== offer.normalizedTitle) {
      allAliases.add(offer.originalTitle)
    }
  }

  return {
    serials: Array.from(allSerials),
    regions: Array.from(allRegions),
    languages: Array.from(allLanguages),
    aliases: Array.from(allAliases),
    offerCount: offers.length,
  }
}

/**
 * Build canonical catalog from offers.
 * @param {object[]} offers - Array of all offers
 * @param {object} options - Build options
 * @returns {object} - { games: [], matches: [], unmatched: [], stats: {} }
 */
function buildCanonicalCatalog(offers, options = {}) {
  const {
    minConfidence = 65,
    allowProbable = true,
  } = options

  // Filter out non-game releases by default
  const gameOffers = filterByReleaseKind(offers, ["game", "unknown"])
  const specialOffers = offers.filter(o => !gameOffers.includes(o))

  // Until the Libretro index is supplied, offers seed one candidate per exact
  // system/title. Matching still runs through the real scoring engine, making
  // serial/hash/alias/fuzzy behavior effective as soon as canonicalGames are
  // provided without changing this builder's contract.
  const seedMap = new Map()
  const noSystem = []

  for (const offer of gameOffers) {
    if (!offer.systemId) {
      noSystem.push({
        offerId: offer.id,
        reason: "no system identified",
        title: offer.normalizedTitle,
        platformRaw: offer.platformRaw,
      })
      continue
    }
    const gameKey = `${offer.systemId}:${offer.normalizedTitle.toLowerCase()}`
    if (!seedMap.has(gameKey)) seedMap.set(gameKey, [])
    seedMap.get(gameKey).push(offer)
  }

  const seededGames = options.canonicalGames || []
  if (!options.canonicalGames) {
    for (const groupedOffers of seedMap.values()) {
      const firstOffer = groupedOffers[0]
      const merged = mergeOffers(groupedOffers)
      const game = createCanonicalGame(firstOffer, groupedOffers.length)
      // A title can have region-specific serials. Use a title-stable ID for a
      // derived seed while retaining every serial as matching evidence.
      game.id = generateGameId(firstOffer.systemId, { title: firstOffer.normalizedTitle })
      game.serials = merged.serials
      game.regions = merged.regions
      game.languages = merged.languages
      game.aliases = merged.aliases
      seededGames.push(game)
    }
  }

  const batch = matchBatch(gameOffers.filter(offer => offer.systemId), seededGames, {
    minConfidence,
    allowProbable,
  })
  const matches = batch.matches
  const unmatched = [...noSystem, ...batch.unmatched]
  const offersById = new Map(gameOffers.map(offer => [offer.id, offer]))
  const matchesByGame = new Map()
  for (const match of matches) {
    if (!matchesByGame.has(match.gameId)) matchesByGame.set(match.gameId, [])
    const offer = offersById.get(match.offerId)
    if (offer) matchesByGame.get(match.gameId).push(offer)
  }

  const games = []
  for (const seed of seededGames) {
    const groupedOffers = matchesByGame.get(seed.id) || []
    if (!groupedOffers.length) continue
    const merged = mergeOffers(groupedOffers)
    const game = { ...seed }
    game.serials = merged.serials
    game.regions = merged.regions
    game.languages = merged.languages
    game.aliases = merged.aliases
    game.offerCount = merged.offerCount
    games.push(game)
  }

  // Sort games by title
  games.sort((a, b) => a.sortTitle.localeCompare(b.sortTitle, "en"))

  // Generate stats
  const stats = {
    totalOffers: offers.length,
    gameOffers: gameOffers.length,
    specialOffers: specialOffers.length,
    gamesCreated: games.length,
    matched: matches.length,
    unmatched: unmatched.length,
    bySystem: {},
    byReleaseKind: {},
    byMatchQuality: {
      exact: 0,
      strong: 0,
      probable: 0,
      unmatched: unmatched.length,
    },
  }

  // Count by system
  for (const game of games) {
    stats.bySystem[game.systemId] = (stats.bySystem[game.systemId] || 0) + 1
  }

  // Count by release kind
  for (const offer of offers) {
    stats.byReleaseKind[offer.releaseKind] = (stats.byReleaseKind[offer.releaseKind] || 0) + 1
  }

  // Count by match quality
  for (const match of matches) {
    if (match.quality === "exact" || match.score >= 95) {
      stats.byMatchQuality.exact++
    } else if (match.quality === "strong" || match.score >= 80) {
      stats.byMatchQuality.strong++
    } else if (match.quality === "probable" || match.score >= 65) {
      stats.byMatchQuality.probable++
    }
  }

  return {
    games,
    matches,
    unmatched,
    specialOffers,
    stats,
  }
}

/**
 * Rebuild catalog from Hydra sources.
 * @param {object[]} sources - Array of Hydra sources
 * @param {object} sourcePayloads - Map of sourceId -> payload data
 * @param {object} options - Build options
 * @returns {object} - Built catalog
 */
function rebuildCatalogFromSources(sources, sourcePayloads, options = {}) {
  const allOffers = []

  for (const source of sources) {
    if (!sourcePayloads[source.id]) continue

    const payload = sourcePayloads[source.id]
    const downloads = payload.downloads || []

    const offers = processSourceDownloads(downloads, source)
    allOffers.push(...offers)
  }

  return buildCanonicalCatalog(allOffers, options)
}

/**
 * Apply manual overrides to catalog.
 * @param {object} catalog - Built catalog
 * @param {object} overrides - Map of offerFingerprint -> override
 * @returns {object} - Catalog with overrides applied
 */
function applyOverridesToCatalog(catalog, overrides) {
  const { games, matches, unmatched } = catalog
  const updatedMatches = []
  const updatedUnmatched = []

  for (const match of matches) {
    const override = overrides[match.offerId]

    if (override) {
      if (override.gameId === null) {
        // Ignore this offer
        continue
      } else if (override.gameId) {
        // Manual match
        updatedMatches.push({
          ...match,
          gameId: override.gameId,
          method: "manual",
          quality: "exact",
          score: 100,
        })
        continue
      }
    }

    updatedMatches.push(match)
  }

  for (const item of unmatched) {
    const override = overrides[item.offerId]

    if (override && override.gameId) {
      // Promote to matched
      updatedMatches.push({
        offerId: item.offerId,
        gameId: override.gameId,
        method: "manual",
        quality: "exact",
        score: 100,
      })
      continue
    }

    if (override && override.gameId === null) {
      // Keep ignored
      continue
    }

    updatedUnmatched.push(item)
  }

  return {
    ...catalog,
    matches: updatedMatches,
    unmatched: updatedUnmatched,
  }
}

module.exports = {
  buildCanonicalCatalog,
  rebuildCatalogFromSources,
  applyOverridesToCatalog,
  generateGameId,
  createCanonicalGame,
  mergeOffers,
}
