"use strict"

/**
 * Retro Catalog V2
 *
 * New canonical catalog implementation that:
 * - Separates games from offers
 * - Uses stable IDs based on serial/hash/title
 * - Consolidates duplicates across sources
 * - Maintains URIs only in local offers file
 */

const { createRetroCatalog: createV1Catalog } = require("./retro-catalog")
const { createRetroRepository } = require("./retro-repository")
const { rebuildCatalogFromSources, applyOverridesToCatalog } = require("./retro-catalog-builder")
const { getDataDir } = require("./runtime-paths")

const CACHE_TTL_MS = 24 * 60 * 60 * 1000

/**
 * Create retro catalog V2 instance.
 * @param {object} options - Configuration options
 * @returns {object} - Catalog instance
 */
function createRetroCatalogV2(options = {}) {
  const {
    dataDir = getDataDir(),
    v1Catalog = createV1Catalog({ dataDir }),
    now = () => Date.now(),
  } = options

  const repository = createRetroRepository({ dataDir })
  let inFlight = null

  const publicGame = (game) => ({
    ...game,
    platform: game.systemId,
    ...(game.artwork?.cover ? { cover: game.artwork.cover } : {}),
    ...(game.artwork?.titleScreen ? { fallbackCover: game.artwork.titleScreen } : {}),
  })

  /**
   * Migrate V1 catalog to V2 format.
   */
  async function migrateFromV1() {
    try {
      const v1Data = typeof v1Catalog.exportAll === "function"
        ? await v1Catalog.exportAll()
        : null
      if (!v1Data?.games?.length) {
        return null
      }

      // The V1 "games" are actually offers (downloads)
      // We need to rebuild them through the catalog builder
      const { processSourceDownloads } = require("./retro-offers")

      const offers = []
      const sourceMap = new Map()

      // Group by source
      for (const v1Game of v1Data.games) {
        if (!sourceMap.has(v1Game.sourceId)) {
          const source = v1Data.sources?.find(s => s.id === v1Game.sourceId)
          if (source) {
            sourceMap.set(v1Game.sourceId, source)
          }
        }
      }

      // Convert V1 games back to download format, then process as offers
      for (const v1Game of v1Data.games) {
        const source = sourceMap.get(v1Game.sourceId)
        if (!source) continue

        const download = {
          title: v1Game.originalTitle || v1Game.title,
          platform: v1Game.platform,
          description: v1Game.description,
          fileSize: v1Game.fileSize,
          uploadDate: v1Game.uploadDate,
          uris: v1Game.uris || [],
          cover: v1Game.cover || v1Game.capa,
        }

        const processed = processSourceDownloads([download], source)
        offers.push(...processed)
      }

      // Build canonical catalog
      const { buildCanonicalCatalog } = require("./retro-catalog-builder")
      const catalog = buildCanonicalCatalog(offers)

      // Save to repository
      repository.setOffers(offers)
      repository.setMatches(catalog.matches)
      repository.setCatalogIndex(catalog.games)
      repository.setUnmatched(catalog.unmatched)

      return {
        migrated: true,
        offersCount: offers.length,
        gamesCount: catalog.games.length,
        matchedCount: catalog.matches.length,
        unmatchedCount: catalog.unmatched.length,
      }
    } catch (error) {
      console.error("[retro-catalog-v2] Migration failed:", error)
      return null
    }
  }

  /**
   * Refresh catalog from sources.
   */
  async function refresh() {
    try {
      // Use V1 catalog to fetch sources
      const v1Data = await v1Catalog.refresh()

      if (!v1Data || !v1Data.sources?.length) {
        throw new Error("No sources available")
      }

      // Convert V1 data to source payloads
      const sourcePayloads = {}
      const sourceMap = new Map()

      for (const source of v1Data.sources) {
        sourceMap.set(source.id, source)
        sourcePayloads[source.id] = {
          name: source.title,
          downloads: [],
        }
      }

      // Group V1 games by source
      for (const v1Game of v1Data.games) {
        if (!sourcePayloads[v1Game.sourceId]) continue

        sourcePayloads[v1Game.sourceId].downloads.push({
          title: v1Game.originalTitle || v1Game.title,
          platform: v1Game.platform,
          description: v1Game.description,
          fileSize: v1Game.fileSize,
          uploadDate: v1Game.uploadDate,
          uris: v1Game.uris || [],
          cover: v1Game.cover || v1Game.capa,
        })
      }

      // Build canonical catalog
      const catalog = rebuildCatalogFromSources(v1Data.sources, sourcePayloads)

      // Apply any existing overrides
      const overridesMap = {}
      // TODO: Load from repository when overrides are implemented

      const finalCatalog = applyOverridesToCatalog(catalog, overridesMap)

      // Save to repository
      const allOffers = []
      for (const source of v1Data.sources) {
        if (!sourcePayloads[source.id]) continue
        const { processSourceDownloads } = require("./retro-offers")
        const offers = processSourceDownloads(sourcePayloads[source.id].downloads, source)
        allOffers.push(...offers)
      }

      repository.setOffers(allOffers)
      repository.setMatches(finalCatalog.matches)
      repository.setCatalogIndex(finalCatalog.games)
      repository.setUnmatched(finalCatalog.unmatched)

      return {
        ok: true,
        games: finalCatalog.games,
        stats: finalCatalog.stats,
        updatedAt: now(),
      }
    } catch (error) {
      console.error("[retro-catalog-v2] Refresh failed:", error)
      throw error
    }
  }

  /**
   * Ensure catalog is loaded and fresh.
   */
  async function ensure(force = false) {
    const stats = repository.getStats()

    // Check if we have data
    if (!stats.games && !force) {
      // Try migration first
      const migrated = await migrateFromV1()
      if (migrated) {
        return repository
      }
    }

    // Check if data is stale
    const age = stats.lastUpdated ? now() - stats.lastUpdated : Infinity
    if (age > CACHE_TTL_MS || force) {
      if (!inFlight) {
        inFlight = refresh().finally(() => {
          inFlight = null
        })
      }

      if (force || !stats.games) {
        await inFlight
      }
    }

    return repository
  }

  /**
   * List games with pagination and filtering.
   */
  async function list(params = {}) {
    const raw = params && typeof params === "object" && !Array.isArray(params) ? params : {}
    const {
      query = "",
      systems = [],
      sources = [],
      offset = 0,
      limit = 24,
      refresh: forceRefresh = false,
    } = raw
    const safeOffset = Math.max(0, Number.isSafeInteger(Number(offset)) ? Number(offset) : 0)
    const safeLimit = Math.min(48, Math.max(1, Number.isSafeInteger(Number(limit)) ? Number(limit) : 24))
    const safeSystems = Array.isArray(systems) ? systems.filter(value => typeof value === "string").slice(0, 32) : []
    const safeQuery = String(query || "").trim().slice(0, 120)

    try {
      await ensure(forceRefresh)

      const allGames = repository.getGames()
      let filtered = allGames

      // Filter by system
      if (safeSystems.length > 0) {
        filtered = filtered.filter(g => safeSystems.includes(g.systemId))
      }

      // Filter by query
      if (safeQuery) {
        const q = safeQuery.toLowerCase()
        filtered = filtered.filter(g =>
          g.title.toLowerCase().includes(q) ||
          (g.aliases || []).some(a => a.toLowerCase().includes(q))
        )
      }

      // TODO: Filter by sources (requires offer lookup)

      // Paginate
      const total = filtered.length
      const games = filtered.slice(safeOffset, safeOffset + safeLimit).map(publicGame)

      // Build facets
      const facets = {
        systems: {},
        releaseKinds: {},
      }

      for (const game of allGames) {
        facets.systems[game.systemId] = (facets.systems[game.systemId] || 0) + 1
      }

      const stats = repository.getStats()

      return {
        ok: true,
        games,
        total,
        totalGames: total,
        totalOffers: stats.offers,
        unmatchedOffers: stats.unmatched,
        facets,
        offset: safeOffset,
        limit: safeLimit,
        hasMore: safeOffset + games.length < total,
        updatedAt: stats.lastUpdated,
      }
    } catch (error) {
      return {
        ok: false,
        games: [],
        totalGames: 0,
        totalOffers: 0,
        unmatchedOffers: 0,
        facets: { systems: {}, releaseKinds: {} },
        offset: safeOffset,
        limit: safeLimit,
        hasMore: false,
        error: String(error?.message || error),
      }
    }
  }

  /**
   * Get game details.
   */
  async function getGame(gameId) {
    try {
      await ensure(false)

      const game = repository.getGame(gameId)
      if (!game) {
        return {
          ok: false,
          error: "Game not found",
        }
      }

      const offers = repository.getOffersForGame(gameId)

      return {
        ok: true,
        game: publicGame(game),
        offers: offers.map(o => {
          const { uris, ...summary } = o
          return {
            ...summary,
            hasUris: uris && uris.length > 0,
            uriCount: uris ? uris.length : 0,
          }
        }),
      }
    } catch (error) {
      return {
        ok: false,
        error: String(error?.message || error),
      }
    }
  }

  /**
   * Get offer details with URIs.
   */
  async function getOffer(offerId) {
    try {
      await ensure(false)

      const offer = repository.getOffer(offerId)
      if (!offer) {
        return {
          ok: false,
          error: "Offer not found",
        }
      }

      return {
        ok: true,
        offer,
      }
    } catch (error) {
      return {
        ok: false,
        error: String(error?.message || error),
      }
    }
  }

  return {
    list,
    getGame,
    getOffer,
    refresh,
    migrateFromV1,
    repository,
  }
}

module.exports = {
  createRetroCatalogV2,
}
