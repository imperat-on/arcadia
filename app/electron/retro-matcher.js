"use strict"

// Motor de matching que associa ofertas (downloads Hydra) a jogos canônicos
// usando serial, hash, título e aliases com scoring de confiança.

const { normalizeSerial, extractSerial } = require("./retro-systems.js")

// Limiares de confiança para matching automático
const CONFIDENCE_THRESHOLD_AUTO = 80      // >=80: associar automaticamente
const CONFIDENCE_THRESHOLD_PROBABLE = 65  // 65-79: associar como "probable", sujeito a auditoria
// <65: manter unmatched

// Tabela de pontuação por tipo de evidência
const SCORE_SERIAL_EXACT = 100           // Serial exato no mesmo sistema
const SCORE_HASH_EXACT = 100             // SHA-1/MD5/CRC exato
const SCORE_TITLE_CANONICAL_EXACT = 85   // Título canônico exato + sistema
const SCORE_ALIAS_EXACT = 80             // Alias exato + sistema
const SCORE_TITLE_STRONG = 75            // Título normalizado forte + sistema + região
const SCORE_FUZZY_BASE = 50              // Base para fuzzy matching
const SCORE_REGION_BONUS = 10            // Bônus quando região bate
const SCORE_YEAR_BONUS = 5               // Bônus quando ano bate

/**
 * Normaliza um título para comparação (remove espaços extras, pontuação, case).
 * @param {string} title - Título original
 * @returns {string} - Título normalizado
 */
function normalizeTitle(title) {
  return String(title || "")
    .toLowerCase()
    .normalize("NFKC")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "") // Remove diacríticos
    .replace(/[^\w\s]/g, " ")         // Remove pontuação
    .replace(/\s+/g, " ")
    .trim()
}

/**
 * Calcula similaridade fuzzy entre dois títulos (0-100).
 * Usa Dice coefficient para bi-gramas.
 * @param {string} a - Primeiro título
 * @param {string} b - Segundo título
 * @returns {number} - Similaridade (0-100)
 */
function fuzzyMatch(a, b) {
  const normalize = (str) => normalizeTitle(str)
  const normA = normalize(a)
  const normB = normalize(b)

  if (normA === normB) return 100
  if (!normA || !normB) return 0

  // Dice coefficient usando bi-gramas
  const bigrams = (str) => {
    const pairs = new Set()
    for (let i = 0; i < str.length - 1; i++) {
      pairs.add(str.slice(i, i + 2))
    }
    return pairs
  }

  const bigramsA = bigrams(normA)
  const bigramsB = bigrams(normB)

  if (bigramsA.size === 0 && bigramsB.size === 0) return 100
  if (bigramsA.size === 0 || bigramsB.size === 0) return 0

  let intersection = 0
  for (const bigram of bigramsA) {
    if (bigramsB.has(bigram)) intersection++
  }

  const dice = (2 * intersection) / (bigramsA.size + bigramsB.size)
  return Math.round(dice * 100)
}

/**
 * Extrai todos os possíveis seriais de um texto.
 * @param {string} text - Texto para buscar seriais
 * @param {string} systemId - ID do sistema
 * @returns {string[]} - Array de serials encontrados (normalizados)
 */
function extractAllSerials(text, systemId) {
  if (!text || !systemId) return []

  const serials = []
  const direct = extractSerial(text, systemId)
  if (direct) serials.push(direct)

  // Busca adicional por padrões comuns em títulos/descrições
  const patterns = [
    /\b([A-Z]{4}[-_ ]?\d{5})\b/g,  // PlayStation, PSP
    /\b([A-Z]{4}\d{5})\b/g,        // PS3
    /\b(G[A-Z0-9]{3}\d{2})\b/g,    // GameCube
    /\b(R[A-Z0-9]{3}\d{2})\b/g,    // Wii
  ]

  for (const pattern of patterns) {
    const matches = text.matchAll(pattern)
    for (const match of matches) {
      const normalized = normalizeSerial(match[1])
      if (normalized && !serials.includes(normalized)) {
        serials.push(normalized)
      }
    }
  }

  return serials
}

/**
 * Calcula o score de match entre uma oferta e um jogo canônico.
 * @param {object} offer - Oferta (download Hydra)
 * @param {object} game - Jogo canônico
 * @returns {object} - { score, method, evidence }
 */
function calculateMatchScore(offer, game) {
  // Regra fundamental: nunca cruzar sistemas
  if (offer.systemId && game.systemId && offer.systemId !== game.systemId) {
    return { score: 0, method: "none", evidence: "different systems" }
  }

  let score = 0
  let method = "none"
  let evidence = []

  // 1. Serial exato (100 pontos)
  if (offer.serials && offer.serials.length > 0 && game.serials && game.serials.length > 0) {
    const offerSerials = offer.serials.map(normalizeSerial)
    const gameSerials = game.serials.map(normalizeSerial)

    for (const offerSerial of offerSerials) {
      if (gameSerials.includes(offerSerial)) {
        score = SCORE_SERIAL_EXACT
        method = "serial"
        evidence.push(`serial:${offerSerial}`)
        return { score, method, evidence: evidence.join(", ") }
      }
    }
  }

  // 2. Hash exato (100 pontos)
  if (offer.hashes && game.hashes) {
    for (const hashType of ["sha1", "md5", "crc32"]) {
      if (offer.hashes[hashType] && game.hashes[hashType]) {
        const offerHashes = Array.isArray(offer.hashes[hashType])
          ? offer.hashes[hashType]
          : [offer.hashes[hashType]]
        const gameHashes = Array.isArray(game.hashes[hashType])
          ? game.hashes[hashType]
          : [game.hashes[hashType]]

        for (const offerHash of offerHashes) {
          if (gameHashes.includes(offerHash.toLowerCase())) {
            score = SCORE_HASH_EXACT
            method = "hash"
            evidence.push(`${hashType}:${offerHash.slice(0, 8)}`)
            return { score, method, evidence: evidence.join(", ") }
          }
        }
      }
    }
  }

  // 3. Título canônico exato (85 pontos)
  const offerTitle = normalizeTitle(offer.normalizedTitle || offer.title)
  const gameTitle = normalizeTitle(game.title)

  if (offerTitle && gameTitle && offerTitle === gameTitle) {
    score = SCORE_TITLE_CANONICAL_EXACT
    method = "exact-title"
    evidence.push(`title:exact`)

    // Bônus por região
    if (offer.region && game.regions && game.regions.includes(offer.region)) {
      score += SCORE_REGION_BONUS
      evidence.push(`region:${offer.region}`)
    }

    return { score, method, evidence: evidence.join(", ") }
  }

  // 4. Alias exato (80 pontos)
  if (game.aliases && game.aliases.length > 0) {
    const gameAliases = game.aliases.map(normalizeTitle)

    if (gameAliases.includes(offerTitle)) {
      score = SCORE_ALIAS_EXACT
      method = "alias"
      evidence.push(`alias:match`)

      if (offer.region && game.regions && game.regions.includes(offer.region)) {
        score += SCORE_REGION_BONUS
        evidence.push(`region:${offer.region}`)
      }

      return { score, method, evidence: evidence.join(", ") }
    }
  }

  // 5. Título normalizado forte com região (75 pontos)
  const similarity = fuzzyMatch(offerTitle, gameTitle)

  if (similarity >= 90 && offer.systemId && game.systemId && offer.systemId === game.systemId) {
    score = SCORE_TITLE_STRONG
    method = "fuzzy"
    evidence.push(`similarity:${similarity}`)

    if (offer.region && game.regions && game.regions.includes(offer.region)) {
      score += SCORE_REGION_BONUS
      evidence.push(`region:${offer.region}`)
    }

    if (offer.releaseYear && game.releaseDate) {
      const gameYear = new Date(game.releaseDate).getFullYear()
      if (offer.releaseYear === gameYear) {
        score += SCORE_YEAR_BONUS
        evidence.push(`year:${gameYear}`)
      }
    }

    return { score, method, evidence: evidence.join(", ") }
  }

  // 6. Fuzzy matching com limites (50-69 pontos)
  if (similarity >= 80 && offer.systemId && game.systemId && offer.systemId === game.systemId) {
    score = SCORE_FUZZY_BASE + Math.round((similarity - 80) / 2)
    method = "fuzzy"
    evidence.push(`similarity:${similarity}`)

    if (offer.region && game.regions && game.regions.includes(offer.region)) {
      score += SCORE_REGION_BONUS
      evidence.push(`region:${offer.region}`)
    }

    return { score, method, evidence: evidence.join(", ") }
  }

  // 7. Sem plataforma = baixa confiança máxima (45 pontos)
  if (!offer.systemId && similarity >= 95) {
    score = 45
    method = "fuzzy"
    evidence.push(`no-platform,similarity:${similarity}`)
    return { score, method, evidence: evidence.join(", ") }
  }

  return { score: 0, method: "none", evidence: "no match" }
}

/**
 * Determina a qualidade do match baseado no score.
 * @param {number} score - Score de confiança
 * @returns {string} - "exact", "strong", "probable", "unmatched"
 */
function getMatchQuality(score) {
  if (score >= 95) return "exact"
  if (score >= CONFIDENCE_THRESHOLD_AUTO) return "strong"
  if (score >= CONFIDENCE_THRESHOLD_PROBABLE) return "probable"
  return "unmatched"
}

/**
 * Encontra o melhor match para uma oferta em um catálogo de jogos.
 * @param {object} offer - Oferta para associar
 * @param {object[]} games - Array de jogos canônicos
 * @param {object} options - Opções de matching
 * @returns {object|null} - Match encontrado ou null
 */
function findBestMatch(offer, games, options = {}) {
  const {
    minConfidence = CONFIDENCE_THRESHOLD_PROBABLE,
    allowProbable = true,
  } = options

  let bestMatch = null
  let bestScore = 0

  for (const game of games) {
    const result = calculateMatchScore(offer, game)

    if (result.score > bestScore) {
      bestScore = result.score
      bestMatch = {
        gameId: game.id,
        game,
        score: result.score,
        method: result.method,
        evidence: result.evidence,
        quality: getMatchQuality(result.score),
      }
    }
  }

  if (!bestMatch) return null

  // Aplicar limiar de confiança
  if (bestMatch.score < minConfidence) return null

  // Verificar se probable é permitido
  if (!allowProbable && bestMatch.quality === "probable") return null

  return bestMatch
}

/**
 * Processa um lote de ofertas contra um catálogo.
 * @param {object[]} offers - Array de ofertas
 * @param {object[]} games - Array de jogos canônicos
 * @param {object} options - Opções de matching
 * @returns {object} - { matches: [], unmatched: [], stats: {} }
 */
function matchBatch(offers, games, options = {}) {
  const matches = []
  const unmatched = []
  const stats = {
    total: offers.length,
    matched: 0,
    unmatched: 0,
    byMethod: {},
    byQuality: {},
    byConfidence: {
      exact: 0,      // 95-100
      strong: 0,     // 80-94
      probable: 0,   // 65-79
      weak: 0,       // <65
    },
  }

  const scopedKey = (systemId, value) => `${systemId || ""}\u001f${String(value || "")}`
  const add = (map, key, game) => {
    if (!key) return
    if (!map.has(key)) map.set(key, [])
    map.get(key).push(game)
  }
  const bySerial = new Map()
  const byHash = new Map()
  const byTitle = new Map()
  const byAlias = new Map()
  const byPrefix = new Map()
  for (const game of games) {
    const title = normalizeTitle(game.title)
    add(byTitle, scopedKey(game.systemId, title), game)
    add(byPrefix, scopedKey(game.systemId, title.slice(0, 3)), game)
    for (const alias of game.aliases || [])
      add(byAlias, scopedKey(game.systemId, normalizeTitle(alias)), game)
    for (const serial of game.serials || [])
      add(bySerial, scopedKey(game.systemId, normalizeSerial(serial)), game)
    for (const type of ["sha1", "md5", "crc32"])
      for (const hash of game.hashes?.[type] || [])
        add(byHash, scopedKey(game.systemId, `${type}:${String(hash).toLowerCase()}`), game)
  }

  const candidatesFor = (offer) => {
    const found = new Map()
    const include = (items) => {
      for (const game of items || []) found.set(game.id, game)
    }
    for (const serial of offer.serials || [])
      include(bySerial.get(scopedKey(offer.systemId, normalizeSerial(serial))))
    for (const type of ["sha1", "md5", "crc32"])
      for (const hash of offer.hashes?.[type] || [])
        include(byHash.get(scopedKey(offer.systemId, `${type}:${String(hash).toLowerCase()}`)))
    const title = normalizeTitle(offer.normalizedTitle || offer.title)
    include(byTitle.get(scopedKey(offer.systemId, title)))
    include(byAlias.get(scopedKey(offer.systemId, title)))
    // Fuzzy matching is deliberately bounded to a same-system title prefix.
    // Exact identifiers above cover the normal hot path without an O(n²) scan.
    if (!found.size) include(byPrefix.get(scopedKey(offer.systemId, title.slice(0, 3))))
    return [...found.values()].slice(0, 2000)
  }

  for (const offer of offers) {
    const match = findBestMatch(offer, candidatesFor(offer), options)

    if (match) {
      matches.push({
        offerId: offer.id,
        gameId: match.gameId,
        score: match.score,
        method: match.method,
        evidence: match.evidence,
        quality: match.quality,
      })

      stats.matched++
      stats.byMethod[match.method] = (stats.byMethod[match.method] || 0) + 1
      stats.byQuality[match.quality] = (stats.byQuality[match.quality] || 0) + 1

      if (match.score >= 95) stats.byConfidence.exact++
      else if (match.score >= 80) stats.byConfidence.strong++
      else if (match.score >= 65) stats.byConfidence.probable++
      else stats.byConfidence.weak++
    } else {
      unmatched.push({
        offerId: offer.id,
        reason: "no match found",
        title: offer.title,
        systemId: offer.systemId,
      })
      stats.unmatched++
    }
  }

  return { matches, unmatched, stats }
}

module.exports = {
  calculateMatchScore,
  findBestMatch,
  matchBatch,
  getMatchQuality,
  normalizeTitle,
  fuzzyMatch,
  extractAllSerials,
  CONFIDENCE_THRESHOLD_AUTO,
  CONFIDENCE_THRESHOLD_PROBABLE,
}
