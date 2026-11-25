"use strict"

/**
 * Retro Repository
 *
 * Manages the local cache of canonical games, offers, matches, and overrides.
 * Handles persistence, migration from v1, and provides unified access to the catalog.
 */

const fs = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")

const CACHE_VERSION = 2
const MAX_CACHE_SIZE_MB = 500
const MAX_OFFERS = 100000

/**
 * Check if a path contains symlinks.
 * @param {string} target - Path to check
 * @param {object} fsImpl - Filesystem implementation
 * @returns {boolean} - True if any component is a symlink
 */
function pathHasSymlink(target, fsImpl = fs) {
  let current = path.resolve(target)
  const rootPath = path.parse(current).root
  while (current && current !== rootPath) {
    try {
      if (fsImpl.lstatSync(current).isSymbolicLink()) return true
    } catch (error) {
      if (error?.code !== "ENOENT") return true
    }
    const parent = path.dirname(current)
    if (parent === current) break
    current = parent
  }
  return false
}

/**
 * Safely read JSON from a file.
 * @param {string} filePath - Path to read
 * @param {object} fsImpl - Filesystem implementation
 * @returns {object|null} - Parsed JSON or null
 */
function safeReadJson(filePath, fsImpl = fs) {
  try {
    if (pathHasSymlink(filePath, fsImpl)) return null
    const stat = fsImpl.statSync(filePath)
    if (!stat.isFile() || stat.size > MAX_CACHE_SIZE_MB * 1024 * 1024) return null
    return JSON.parse(fsImpl.readFileSync(filePath, "utf8"))
  } catch {
    return null
  }
}

/**
 * Safely write JSON to a file with atomic rename.
 * @param {string} filePath - Path to write
 * @param {object} data - Data to serialize
 * @param {object} fsImpl - Filesystem implementation
 */
function safeWriteJson(filePath, data, fsImpl = fs) {
  try {
    const directory = path.dirname(filePath)
    if (pathHasSymlink(directory, fsImpl)) return
    fsImpl.mkdirSync(directory, { recursive: true })
    if (pathHasSymlink(directory, fsImpl) || pathHasSymlink(filePath, fsImpl)) return

    const tmp = `${filePath}.tmp-${process.pid}`
    fsImpl.writeFileSync(tmp, JSON.stringify(data, null, 2), { mode: 0o600 })
    fsImpl.renameSync(tmp, filePath)
  } catch (error) {
    // Silent failure - catalog continues in memory
  }
}

/**
 * Create a retro repository instance.
 * @param {object} options - Configuration options
 * @returns {object} - Repository instance
 */
function createRetroRepository(options = {}) {
  const {
    dataDir,
    fsImpl = fs,
    now = () => Date.now(),
  } = options

  const retroDir = path.join(dataDir, "retro")
  const manifestPath = path.join(retroDir, "catalog-manifest.json")
  const indexPath = path.join(retroDir, "catalog-index-v2.json")
  const offersPath = path.join(retroDir, "offers-v2.json")
  const matchesPath = path.join(retroDir, "matches-v2.json")
  const artworkPath = path.join(retroDir, "artwork-v1.json")
  const unmatchedPath = path.join(retroDir, "unmatched-v1.json")
  const overridesPath = path.join(retroDir, "overrides-v1.json")

  // In-memory state
  let manifest = null
  let catalogIndex = null
  let offers = null
  let matches = null
  let artwork = null
  let unmatched = null
  let overrides = null

  /**
   * Load manifest from disk.
   */
  function loadManifest() {
    if (manifest) return manifest

    const data = safeReadJson(manifestPath, fsImpl)
    if (!data || data.version !== CACHE_VERSION) {
      manifest = {
        version: CACHE_VERSION,
        generatedAt: null,
        catalogVersion: null,
        systems: [],
      }
      return manifest
    }

    manifest = data
    return manifest
  }

  /**
   * Save manifest to disk.
   */
  function saveManifest() {
    if (!manifest) return
    safeWriteJson(manifestPath, manifest, fsImpl)
  }

  /**
   * Load catalog index from disk.
   */
  function loadCatalogIndex() {
    if (catalogIndex) return catalogIndex

    const data = safeReadJson(indexPath, fsImpl)
    if (!data || data.version !== CACHE_VERSION) {
      catalogIndex = {
        version: CACHE_VERSION,
        updatedAt: null,
        games: [],
        systemsMap: {},
      }
      return catalogIndex
    }

    catalogIndex = data
    return catalogIndex
  }

  /**
   * Save catalog index to disk.
   */
  function saveCatalogIndex() {
    if (!catalogIndex) return
    safeWriteJson(indexPath, catalogIndex, fsImpl)
  }

  /**
   * Load offers from disk.
   */
  function loadOffers() {
    if (offers) return offers

    const data = safeReadJson(offersPath, fsImpl)
    if (!data || data.version !== CACHE_VERSION || !Array.isArray(data.offers)) {
      offers = {
        version: CACHE_VERSION,
        updatedAt: null,
        offers: [],
      }
      return offers
    }

    offers = data
    return offers
  }

  /**
   * Save offers to disk.
   */
  function saveOffers() {
    if (!offers) return
    safeWriteJson(offersPath, offers, fsImpl)
  }

  /**
   * Load matches from disk.
   */
  function loadMatches() {
    if (matches) return matches

    const data = safeReadJson(matchesPath, fsImpl)
    if (!data || data.version !== CACHE_VERSION || !Array.isArray(data.matches)) {
      matches = {
        version: CACHE_VERSION,
        updatedAt: null,
        matches: [],
      }
      return matches
    }

    matches = data
    return matches
  }

  /**
   * Save matches to disk.
   */
  function saveMatches() {
    if (!matches) return
    safeWriteJson(matchesPath, matches, fsImpl)
  }

  /**
   * Load artwork cache from disk.
   */
  function loadArtwork() {
    if (artwork) return artwork

    const data = safeReadJson(artworkPath, fsImpl)
    if (!data || data.version !== 1) {
      artwork = {
        version: 1,
        cache: {},
      }
      return artwork
    }

    artwork = data
    return artwork
  }

  /**
   * Save artwork cache to disk.
   */
  function saveArtwork() {
    if (!artwork) return
    safeWriteJson(artworkPath, artwork, fsImpl)
  }

  /**
   * Load unmatched offers from disk.
   */
  function loadUnmatched() {
    if (unmatched) return unmatched

    const data = safeReadJson(unmatchedPath, fsImpl)
    if (!data || data.version !== 1 || !Array.isArray(data.offers)) {
      unmatched = {
        version: 1,
        offers: [],
      }
      return unmatched
    }

    unmatched = data
    return unmatched
  }

  /**
   * Save unmatched offers to disk.
   */
  function saveUnmatched() {
    if (!unmatched) return
    safeWriteJson(unmatchedPath, unmatched, fsImpl)
  }

  /**
   * Load overrides from disk.
   */
  function loadOverrides() {
    if (overrides) return overrides

    const data = safeReadJson(overridesPath, fsImpl)
    if (!data || data.version !== 1) {
      overrides = {
        version: 1,
        overrides: {},
      }
      return overrides
    }

    overrides = data
    return overrides
  }

  /**
   * Save overrides to disk.
   */
  function saveOverrides() {
    if (!overrides) return
    safeWriteJson(overridesPath, overrides, fsImpl)
  }

  /**
   * Get all games from the catalog index.
   * @returns {object[]} - Array of canonical games
   */
  function getGames() {
    loadCatalogIndex()
    return catalogIndex.games || []
  }

  /**
   * Get a game by ID.
   * @param {string} gameId - Game ID
   * @returns {object|null} - Game or null
   */
  function getGame(gameId) {
    loadCatalogIndex()
    return catalogIndex.games.find(g => g.id === gameId) || null
  }

  /**
   * Get all offers.
   * @returns {object[]} - Array of offers
   */
  function getOffers() {
    loadOffers()
    return offers.offers || []
  }

  /**
   * Get offers for a specific game.
   * @param {string} gameId - Game ID
   * @returns {object[]} - Array of offers
   */
  function getOffersForGame(gameId) {
    loadOffers()
    loadMatches()

    const matchedOfferIds = matches.matches
      .filter(m => m.gameId === gameId)
      .map(m => m.offerId)

    return offers.offers.filter(o => matchedOfferIds.includes(o.id))
  }

  /**
   * Get an offer by ID.
   * @param {string} offerId - Offer ID
   * @returns {object|null} - Offer or null
   */
  function getOffer(offerId) {
    loadOffers()
    return offers.offers.find(o => o.id === offerId) || null
  }

  /**
   * Update offers.
   * @param {object[]} newOffers - New offers array
   */
  function setOffers(newOffers) {
    loadOffers()
    if (newOffers.length > MAX_OFFERS) {
      throw new Error(`Offers exceed maximum (${MAX_OFFERS})`)
    }
    offers.offers = newOffers
    offers.updatedAt = now()
    saveOffers()
  }

  /**
   * Update matches.
   * @param {object[]} newMatches - New matches array
   */
  function setMatches(newMatches) {
    loadMatches()
    matches.matches = newMatches
    matches.updatedAt = now()
    saveMatches()
  }

  /**
   * Update catalog index.
   * @param {object[]} games - New games array
   * @param {object} systemsMap - Systems map
   */
  function setCatalogIndex(games, systemsMap = {}) {
    loadCatalogIndex()
    catalogIndex.games = games
    catalogIndex.systemsMap = systemsMap
    catalogIndex.updatedAt = now()
    saveCatalogIndex()
  }

  /**
   * Get artwork for a game.
   * @param {string} gameId - Game ID
   * @returns {object|null} - Artwork data or null
   */
  function getArtwork(gameId) {
    loadArtwork()
    return artwork.cache[gameId] || null
  }

  /**
   * Set artwork for a game.
   * @param {string} gameId - Game ID
   * @param {object} artworkData - Artwork data
   */
  function setArtwork(gameId, artworkData) {
    loadArtwork()
    artwork.cache[gameId] = {
      ...artworkData,
      updatedAt: now(),
    }
    saveArtwork()
  }

  /**
   * Get override for an offer.
   * @param {string} offerFingerprint - Offer fingerprint
   * @returns {object|null} - Override or null
   */
  function getOverride(offerFingerprint) {
    loadOverrides()
    return overrides.overrides[offerFingerprint] || null
  }

  /**
   * Set override for an offer.
   * @param {string} offerFingerprint - Offer fingerprint
   * @param {object} overrideData - Override data
   */
  function setOverride(offerFingerprint, overrideData) {
    loadOverrides()
    overrides.overrides[offerFingerprint] = {
      ...overrideData,
      updatedAt: now(),
    }
    saveOverrides()
  }

  /**
   * Remove override for an offer.
   * @param {string} offerFingerprint - Offer fingerprint
   */
  function removeOverride(offerFingerprint) {
    loadOverrides()
    delete overrides.overrides[offerFingerprint]
    saveOverrides()
  }

  /**
   * Get unmatched offers.
   * @returns {object[]} - Array of unmatched offers
   */
  function getUnmatched() {
    loadUnmatched()
    return unmatched.offers || []
  }

  /**
   * Set unmatched offers.
   * @param {object[]} unmatchedOffers - Array of unmatched offers
   */
  function setUnmatched(unmatchedOffers) {
    loadUnmatched()
    unmatched.offers = unmatchedOffers
    saveUnmatched()
  }

  /**
   * Get repository stats.
   * @returns {object} - Repository statistics
   */
  function getStats() {
    loadCatalogIndex()
    loadOffers()
    loadMatches()
    loadUnmatched()

    const gameCount = catalogIndex.games.length
    const offerCount = offers.offers.length
    const matchedCount = matches.matches.length
    const unmatchedCount = unmatched.offers.length

    return {
      games: gameCount,
      offers: offerCount,
      matched: matchedCount,
      unmatched: unmatchedCount,
      catalogVersion: manifest?.catalogVersion || null,
      lastUpdated: offers.updatedAt || null,
    }
  }

  return {
    getGames,
    getGame,
    getOffers,
    getOffersForGame,
    getOffer,
    setOffers,
    setMatches,
    setCatalogIndex,
    getArtwork,
    setArtwork,
    getOverride,
    setOverride,
    removeOverride,
    getUnmatched,
    setUnmatched,
    getStats,
    loadManifest,
    saveManifest,
  }
}

module.exports = {
  createRetroRepository,
}
