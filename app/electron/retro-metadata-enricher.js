"use strict"

/**
 * Retro Metadata Enricher
 *
 * Enriquece o catálogo retro V2 com metadados detalhados de múltiplas fontes:
 * - IGDB (metadados ricos)
 * - Libretro Thumbnails (capas)
 * - Cache local com TTL
 */

const fs = require("fs")
const path = require("path")
const crypto = require("crypto")
const igdb = require("./retro-metadata-igdb")

const METADATA_CACHE_VERSION = 1
const METADATA_TTL_DAYS = 30
const ARTWORK_TTL_DAYS = 7
const MAX_CACHE_SIZE_MB = 200
const RATE_LIMIT_DELAY_MS = 250 // 4 requests/segundo

/**
 * Cria instância do enricher.
 * @param {object} options - Opções de configuração
 * @returns {object} - Enricher instance
 */
function createMetadataEnricher(options = {}) {
  const {
    dataDir,
    igdbCredentials = null,
    now = () => Date.now(),
  } = options

  const cacheDir = path.join(dataDir, "retro", "metadata")
  const artworkCacheDir = path.join(dataDir, "retro", "artwork")

  // Criar diretórios se não existirem
  ensureDir(cacheDir)
  ensureDir(artworkCacheDir)

  const manifestPath = path.join(cacheDir, "manifest.json")
  let manifest = loadManifest()
  let requestQueue = Promise.resolve()

  /**
   * Carrega manifest do cache.
   */
  function loadManifest() {
    try {
      if (fs.existsSync(manifestPath)) {
        const data = fs.readFileSync(manifestPath, "utf8")
        return JSON.parse(data)
      }
    } catch (error) {
      console.warn("[metadata-enricher] Failed to load manifest:", error)
    }

    return {
      version: METADATA_CACHE_VERSION,
      created: now(),
      entries: {},
    }
  }

  /**
   * Salva manifest atomicamente.
   */
  function saveManifest() {
    try {
      const tmpPath = `${manifestPath}.tmp`
      fs.writeFileSync(tmpPath, JSON.stringify(manifest, null, 2), "utf8")
      fs.renameSync(tmpPath, manifestPath)
    } catch (error) {
      console.error("[metadata-enricher] Failed to save manifest:", error)
    }
  }

  /**
   * Gera ID de cache para um jogo.
   * @param {string} systemId - ID do sistema
   * @param {string} identifier - Serial ou título normalizado
   * @returns {string} - Cache ID
   */
  function getCacheId(systemId, identifier) {
    const data = `${systemId}:${identifier}`
    return crypto.createHash("sha256").update(data).digest("hex").substring(0, 24)
  }

  /**
   * Verifica se entrada do cache é válida.
   * @param {object} entry - Entrada do manifest
   * @param {number} ttlMs - TTL em milissegundos
   * @returns {boolean}
   */
  function isCacheValid(entry, ttlMs) {
    if (!entry || !entry.fetchedAt) return false
    return (now() - entry.fetchedAt) < ttlMs
  }

  /**
   * Carrega metadados do cache.
   * @param {string} cacheId - ID do cache
   * @returns {object|null} - Metadados ou null
   */
  function loadFromCache(cacheId) {
    try {
      const filePath = path.join(cacheDir, `${cacheId}.json`)
      if (fs.existsSync(filePath)) {
        const data = fs.readFileSync(filePath, "utf8")
        return JSON.parse(data)
      }
    } catch (error) {
      console.warn(`[metadata-enricher] Failed to load cache ${cacheId}:`, error)
    }
    return null
  }

  /**
   * Salva metadados no cache.
   * @param {string} cacheId - ID do cache
   * @param {object} metadata - Metadados
   */
  function saveToCache(cacheId, metadata) {
    try {
      const filePath = path.join(cacheDir, `${cacheId}.json`)
      const tmpPath = `${filePath}.tmp`

      const envelope = {
        version: METADATA_CACHE_VERSION,
        cacheId,
        fetchedAt: now(),
        metadata,
      }

      fs.writeFileSync(tmpPath, JSON.stringify(envelope, null, 2), "utf8")
      fs.renameSync(tmpPath, filePath)

      // Atualizar manifest
      manifest.entries[cacheId] = {
        fetchedAt: envelope.fetchedAt,
        size: Buffer.byteLength(JSON.stringify(envelope)),
      }
      saveManifest()
    } catch (error) {
      console.error(`[metadata-enricher] Failed to save cache ${cacheId}:`, error)
    }
  }

  /**
   * Enfileira request com rate limiting.
   * @param {Function} fn - Função assíncrona
   * @returns {Promise<any>}
   */
  function enqueueRequest(fn) {
    const promise = requestQueue
      .then(() => fn())
      .then(result => {
        // Delay para rate limiting
        return new Promise(resolve =>
          setTimeout(() => resolve(result), RATE_LIMIT_DELAY_MS)
        )
      })

    requestQueue = promise.catch(() => {}) // Ignorar erros no queue
    return promise
  }

  /**
   * Busca metadados de um jogo.
   * @param {object} game - Jogo do catálogo V2
   * @param {object} options - Opções de busca
   * @returns {Promise<object|null>} - Metadados enriquecidos
   */
  async function fetchMetadata(game, options = {}) {
    const { force = false, source = "auto" } = options

    // Sistemas suportados
    const supportedSystems = ["sony-playstation-2", "sony-playstation-3", "sony-playstation", "sony-psp"]
    if (!supportedSystems.includes(game.systemId)) {
      return null
    }

    // Tentar usar serial primeiro, depois título
    const identifier = game.serials?.[0] || game.sortTitle
    if (!identifier) {
      return null
    }

    const cacheId = getCacheId(game.systemId, identifier)
    const ttl = METADATA_TTL_DAYS * 24 * 60 * 60 * 1000

    // Verificar cache
    if (!force && manifest.entries[cacheId]) {
      if (isCacheValid(manifest.entries[cacheId], ttl)) {
        const cached = loadFromCache(cacheId)
        if (cached?.metadata) {
          return cached.metadata
        }
      }
    }

    // Buscar da fonte
    if (!igdbCredentials || !igdbCredentials.clientId || !igdbCredentials.clientSecret) {
      return null
    }

    try {
      let metadata = null

      // Tentar por serial primeiro
      if (game.serials?.length > 0) {
        metadata = await enqueueRequest(() =>
          igdb.searchBySerial(game.serials[0], game.systemId, igdbCredentials)
        )
      }

      // Fallback para busca por título
      if (!metadata && game.title) {
        metadata = await enqueueRequest(() =>
          igdb.searchByTitle(game.title, game.systemId, igdbCredentials)
        )
      }

      if (metadata) {
        saveToCache(cacheId, metadata)
        return metadata
      }

      // Salvar cache negativo (evita requests repetidos)
      saveToCache(cacheId, { notFound: true })
      return null
    } catch (error) {
      console.error(`[metadata-enricher] Failed to fetch metadata for ${game.id}:`, error)
      return null
    }
  }

  /**
   * Enriquece um jogo do catálogo com metadados.
   * @param {object} game - Jogo do catálogo V2
   * @param {object} options - Opções
   * @returns {Promise<object>} - Jogo enriquecido
   */
  async function enrichGame(game, options = {}) {
    const metadata = await fetchMetadata(game, options)

    if (!metadata || metadata.notFound) {
      return game
    }

    // Merge metadados com o jogo existente
    return {
      ...game,
      // Preservar título original, mas adicionar alternativo da IGDB
      titleIgdb: metadata.title !== game.title ? metadata.title : undefined,
      summary: metadata.summary,
      releaseDate: metadata.releaseDate || game.releaseDate,
      developer: metadata.developer,
      publisher: metadata.publisher,
      genres: mergeUnique(game.genres, metadata.genres),
      themes: metadata.themes,
      gameModes: metadata.gameModes,
      perspectives: metadata.perspectives,
      rating: metadata.rating,
      ratingCount: metadata.ratingCount,
      artwork: {
        ...game.artwork,
        // IGDB tem prioridade se não houver artwork local
        cover: game.artwork?.cover || metadata.artwork?.cover,
        coverHD: metadata.artwork?.coverHD,
        artworks: metadata.artwork?.artworks,
        screenshots: metadata.artwork?.screenshots,
        provider: metadata.artwork?.cover ? "igdb" : game.artwork?.provider,
      },
      metadataSource: "igdb",
      metadataFetchedAt: now(),
    }
  }

  /**
   * Enriquece catálogo completo em batch.
   * @param {object[]} games - Lista de jogos
   * @param {object} options - Opções
   * @returns {Promise<object[]>} - Jogos enriquecidos
   */
  async function enrichCatalog(games, options = {}) {
    const {
      maxGames = null,
      onProgress = null,
      systemFilter = null,
    } = options

    const filteredGames = systemFilter
      ? games.filter(g => systemFilter.includes(g.systemId))
      : games

    const gamesToEnrich = maxGames
      ? filteredGames.slice(0, maxGames)
      : filteredGames

    const enrichedGames = []
    let processed = 0

    for (const game of gamesToEnrich) {
      const enriched = await enrichGame(game, options)
      enrichedGames.push(enriched)
      processed++

      if (onProgress && processed % 10 === 0) {
        onProgress({ processed, total: gamesToEnrich.length })
      }
    }

    if (onProgress) {
      onProgress({ processed, total: gamesToEnrich.length, done: true })
    }

    return enrichedGames
  }

  /**
   * Limpa cache antigo.
   * @param {number} maxAgeDays - Idade máxima em dias
   * @returns {number} - Número de entradas removidas
   */
  function cleanCache(maxAgeDays = METADATA_TTL_DAYS * 2) {
    const maxAgeMs = maxAgeDays * 24 * 60 * 60 * 1000
    const cutoff = now() - maxAgeMs
    let removed = 0

    for (const [cacheId, entry] of Object.entries(manifest.entries)) {
      if (entry.fetchedAt < cutoff) {
        try {
          const filePath = path.join(cacheDir, `${cacheId}.json`)
          if (fs.existsSync(filePath)) {
            fs.unlinkSync(filePath)
          }
          delete manifest.entries[cacheId]
          removed++
        } catch (error) {
          console.warn(`[metadata-enricher] Failed to remove cache ${cacheId}:`, error)
        }
      }
    }

    if (removed > 0) {
      saveManifest()
    }

    return removed
  }

  /**
   * Estatísticas do cache.
   * @returns {object}
   */
  function getCacheStats() {
    const entries = Object.values(manifest.entries)
    const totalSize = entries.reduce((sum, e) => sum + (e.size || 0), 0)
    const validCount = entries.filter(e =>
      isCacheValid(e, METADATA_TTL_DAYS * 24 * 60 * 60 * 1000)
    ).length

    return {
      totalEntries: entries.length,
      validEntries: validCount,
      expiredEntries: entries.length - validCount,
      totalSizeMB: (totalSize / (1024 * 1024)).toFixed(2),
      cacheDir,
    }
  }

  return {
    fetchMetadata,
    enrichGame,
    enrichCatalog,
    cleanCache,
    getCacheStats,
  }
}

/**
 * Merge arrays removendo duplicatas.
 */
function mergeUnique(arr1 = [], arr2 = []) {
  return [...new Set([...arr1, ...arr2])]
}

/**
 * Garante que diretório existe.
 */
function ensureDir(dirPath) {
  if (!fs.existsSync(dirPath)) {
    fs.mkdirSync(dirPath, { recursive: true })
  }
}

module.exports = {
  createMetadataEnricher,
}
