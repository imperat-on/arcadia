"use strict"

// Repositório de ofertas com fingerprinting estável para persistência de IDs
// através de reordenações de feeds Hydra.

const crypto = require("node:crypto")
const { resolveSystem } = require("./retro-systems.js")
const { extractAllSerials } = require("./retro-matcher.js")
const { normalizeRetroTitle } = require("./retro-title-parser.js")

/**
 * Gera um fingerprint estável para uma oferta baseado em sourceId + título + URIs.
 * O fingerprint sobrevive a reordenações do array de downloads da source.
 * @param {string} sourceId - ID da source Hydra
 * @param {string} originalTitle - Título original do download
 * @param {string[]} uris - Array de URIs/magnets
 * @returns {string} - Fingerprint de 24 caracteres
 */
function generateOfferFingerprint(sourceId, originalTitle, uris) {
  const normalizedUris = Array.isArray(uris)
    ? [...new Set(uris.filter(Boolean).map(u => u.trim()))].sort().join("\0")
    : ""

  const content = [
    String(sourceId || "").trim(),
    String(originalTitle || "").trim(),
    normalizedUris,
  ].join("\0")

  return crypto.createHash("sha256").update(content, "utf8").digest("hex").slice(0, 24)
}

/**
 * Classifica o tipo de release baseado no título e conteúdo.
 * @param {string} title - Título normalizado
 * @param {string} originalTitle - Título original
 * @param {string} description - Descrição opcional
 * @returns {string} - Tipo: "game", "collection", "hack", "translation", "homebrew", "dlc", "update", "bios", "unknown"
 */
function classifyReleaseKind(title, originalTitle, description = "") {
  const text = `${originalTitle} ${title} ${description}`.toLowerCase()

  // BIOS deve ser ocultada e nunca oferecida como jogo
  if (/\bbios\b/i.test(text)) return "bios"

  // Collections/Packs
  if (/\b(\d+[\s-]?in[\s-]?1|collection|pack|bundle|anthology|compilation)\b/i.test(text)) {
    return "collection"
  }

  // Hacks e patches
  if (/\b(hack|mod|patch|romhack|rom\s*hack)\b/i.test(text)) return "hack"

  // Traduções
  if (/\b(translation|translated|fan[\s-]?translation|[+]\s*(?:eng|rus|pt|es|fr))\b/i.test(text)) {
    return "translation"
  }

  // Homebrew
  if (/\b(homebrew|indie|fangame|fan[\s-]?game)\b/i.test(text)) return "homebrew"

  // DLC
  if (/\b(dlc|downloadable[\s-]?content|expansion|add[\s-]?on)\b/i.test(text)) return "dlc"

  // Updates/Patches de sistema
  if (/\b(update|patch|hotfix|firmware)\b/i.test(text) && /\bv?\d+\.\d+/i.test(text)) {
    return "update"
  }

  // Jogo individual (padrão)
  return "game"
}

/**
 * Extrai região de um título.
 * @param {string} text - Texto para extrair região
 * @returns {string|null} - Código de região ou null
 */
function extractRegion(text) {
  const regionPatterns = [
    { pattern: /\b(usa|ntsc[\s-]?u|us)\b/i, region: "USA" },
    { pattern: /\b(europe|pal|eu|eur)\b/i, region: "EUR" },
    { pattern: /\b(japan|ntsc[\s-]?j|jp|jpn)\b/i, region: "JPN" },
    { pattern: /\b(asia|ntsc[\s-]?k|kor|korea)\b/i, region: "ASIA" },
    { pattern: /\b(brazil|br|brasil)\b/i, region: "BRA" },
    { pattern: /\b(world|ww|global)\b/i, region: "WORLD" },
  ]

  for (const { pattern, region } of regionPatterns) {
    if (pattern.test(text)) return region
  }

  return null
}

/**
 * Extrai idiomas de um título.
 * @param {string} text - Texto para extrair idiomas
 * @returns {string[]} - Array de códigos de idioma
 */
function extractLanguages(text) {
  const languages = []
  const languagePatterns = [
    { pattern: /\b(eng|english|en)\b/i, code: "en" },
    { pattern: /\b(rus|russian|ru)\b/i, code: "ru" },
    { pattern: /\b(por|portuguese|pt|ptbr|pt-br)\b/i, code: "pt" },
    { pattern: /\b(spa|spanish|es)\b/i, code: "es" },
    { pattern: /\b(fra|french|fr)\b/i, code: "fr" },
    { pattern: /\b(ger|german|de)\b/i, code: "de" },
    { pattern: /\b(ita|italian|it)\b/i, code: "it" },
    { pattern: /\b(jpn|japanese|ja)\b/i, code: "ja" },
    { pattern: /\b(multi\d*)\b/i, code: "multi" },
  ]

  for (const { pattern, code } of languagePatterns) {
    if (pattern.test(text) && !languages.includes(code)) {
      languages.push(code)
    }
  }

  return languages
}

/**
 * Normaliza uma oferta de download Hydra para o formato interno.
 * @param {object} download - Download da source Hydra
 * @param {object} source - Informações da source
 * @returns {object} - Oferta normalizada
 */
function normalizeOffer(download, source) {
  const originalTitle = String(download.title || "").trim()
  const normalizedTitle = normalizeRetroTitle(originalTitle)
  const uris = Array.isArray(download.uris)
    ? download.uris.filter(Boolean)
    : download.uri
    ? [download.uri]
    : []

  const id = generateOfferFingerprint(source.id, originalTitle, uris)
  const platformRaw = String(download.platform || "").trim().toLowerCase()
  const systemId = resolveSystem(platformRaw)

  const text = `${originalTitle} ${download.description || ""}`
  const releaseKind = classifyReleaseKind(normalizedTitle, originalTitle, download.description)
  const region = extractRegion(text)
  const languages = extractLanguages(text)

  return {
    id,
    sourceId: source.id,
    sourceTitle: source.title,
    originalTitle,
    normalizedTitle,
    systemId,
    platformRaw,
    serials: systemId ? extractAllSerials(text, systemId) : [],
    region,
    languages,
    releaseKind,
    fileSize: download.fileSize || null,
    uploadDate: download.uploadDate || null,
    description: download.description || null,
    uris,
    cover: download.cover || download.capa || null,
    match: {
      method: "none",
      confidence: 0,
      catalogVersion: 2,
      gameId: null,
    },
  }
}

/**
 * Processa downloads de uma source em ofertas normalizadas.
 * @param {object[]} downloads - Array de downloads
 * @param {object} source - Informações da source
 * @returns {object[]} - Array de ofertas
 */
function processSourceDownloads(downloads, source) {
  if (!Array.isArray(downloads)) return []

  const offers = []
  const seen = new Set()

  for (const download of downloads) {
    if (!download || typeof download !== "object") continue

    const offer = normalizeOffer(download, source)

    // Deduplica por fingerprint
    if (seen.has(offer.id)) continue
    seen.add(offer.id)

    offers.push(offer)
  }

  return offers
}

/**
 * Atualiza o match de uma oferta.
 * @param {object} offer - Oferta
 * @param {object} matchResult - Resultado do matcher
 * @returns {object} - Oferta atualizada
 */
function updateOfferMatch(offer, matchResult) {
  return {
    ...offer,
    match: {
      method: matchResult.method || "none",
      confidence: matchResult.score || 0,
      catalogVersion: 2,
      gameId: matchResult.gameId || null,
      evidence: matchResult.evidence || null,
      quality: matchResult.quality || "unmatched",
    },
  }
}

/**
 * Filtra ofertas por tipo de release.
 * @param {object[]} offers - Array de ofertas
 * @param {string|string[]} kinds - Tipo(s) para filtrar
 * @returns {object[]} - Ofertas filtradas
 */
function filterByReleaseKind(offers, kinds) {
  const allowedKinds = Array.isArray(kinds) ? kinds : [kinds]
  return offers.filter(offer => allowedKinds.includes(offer.releaseKind))
}

/**
 * Agrupa ofertas por jogo canônico.
 * @param {object[]} offers - Array de ofertas matched
 * @returns {Map<string, object[]>} - Map de gameId -> ofertas
 */
function groupOffersByGame(offers) {
  const grouped = new Map()

  for (const offer of offers) {
    if (!offer.match || !offer.match.gameId) continue

    const gameId = offer.match.gameId
    if (!grouped.has(gameId)) {
      grouped.set(gameId, [])
    }
    grouped.get(gameId).push(offer)
  }

  return grouped
}

/**
 * Cria um sumário de oferta (sem URIs) para listagem.
 * @param {object} offer - Oferta completa
 * @returns {object} - Sumário sem URIs
 */
function createOfferSummary(offer) {
  const { uris, ...summary } = offer
  return {
    ...summary,
    hasUris: Array.isArray(uris) && uris.length > 0,
    uriCount: Array.isArray(uris) ? uris.length : 0,
  }
}

/**
 * Serializa ofertas para cache.
 * @param {object[]} offers - Array de ofertas
 * @param {object} metadata - Metadados do cache
 * @returns {object} - Objeto serializável
 */
function serializeOffers(offers, metadata = {}) {
  return {
    version: 2,
    updatedAt: Date.now(),
    ...metadata,
    offers: offers.map(offer => ({
      ...offer,
      // Garante que arrays estejam presentes
      uris: offer.uris || [],
      serials: offer.serials || [],
      languages: offer.languages || [],
    })),
  }
}

/**
 * Desserializa ofertas do cache.
 * @param {object} cached - Objeto do cache
 * @returns {object[]|null} - Array de ofertas ou null se inválido
 */
function deserializeOffers(cached) {
  if (!cached || cached.version !== 2 || !Array.isArray(cached.offers)) {
    return null
  }

  return cached.offers.map(offer => ({
    ...offer,
    uris: offer.uris || [],
    serials: offer.serials || [],
    languages: offer.languages || [],
  }))
}

module.exports = {
  generateOfferFingerprint,
  normalizeOffer,
  processSourceDownloads,
  updateOfferMatch,
  classifyReleaseKind,
  extractRegion,
  extractLanguages,
  filterByReleaseKind,
  groupOffersByGame,
  createOfferSummary,
  serializeOffers,
  deserializeOffers,
}
