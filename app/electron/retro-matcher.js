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
const SCORE_TITLE_STRONG = 80            // Título contém o canônico (ou tokens iguais) + sistema
const SCORE_TITLE_FUZZY_MAX = 94         // Fuzzy nunca alcança "exact" (>=95)
const SCORE_REGION_BONUS = 10            // Bônus quando região bate
const SCORE_YEAR_BONUS = 5               // Bônus quando ano bate

// Palavras vazias que não distinguem jogos ("The Legend of Zelda" == "Legend of Zelda, The")
const STOPWORDS = new Set(["the", "a", "an", "of", "and", "or", "in", "on", "with", "for", "to", "vs"])
// Numerais (palavra/romano/dígito) viram o mesmo valor: "Seven Blades" == "7 Blades",
// mas "Ace Combat Zero" != "Ace Combat" e "God of War II" != "God of War".
const NUMBER_WORDS = new Map([
  ["zero", "0"], ["one", "1"], ["two", "2"], ["three", "3"], ["four", "4"],
  ["five", "5"], ["six", "6"], ["seven", "7"], ["eight", "8"], ["nine", "9"], ["ten", "10"],
])
const ROMAN_NUMERALS = new Map([
  ["i", "1"], ["ii", "2"], ["iii", "3"], ["iv", "4"], ["v", "5"], ["vi", "6"],
  ["vii", "7"], ["viii", "8"], ["ix", "9"], ["x", "10"], ["xi", "11"], ["xii", "12"], ["xiii", "13"],
])

function numeralValue(token) {
  if (/^\d+$/.test(token)) return String(Number(token))
  return NUMBER_WORDS.get(token) || ROMAN_NUMERALS.get(token) || null
}

/**
 * Tokeniza um título para comparação: sem acento, sem pontuação, sem palavras vazias.
 * @param {string} title - Título original
 * @returns {string[]} - Tokens significativos
 */
function titleTokens(title) {
  return normalizeTitle(title)
    .split(" ")
    .filter((token) => token && !STOPWORDS.has(token))
}

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
 * Dice coefficient por bi-gramas (fallback para títulos de 1 token).
 * @param {string} normA - Título normalizado A
 * @param {string} normB - Título normalizado B
 * @returns {number} - Similaridade (0-100)
 */
function bigramDice(normA, normB) {
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

  return Math.round(((2 * intersection) / (bigramsA.size + bigramsB.size)) * 100)
}

function sameTokenSet(a, b) {
  if (a.size !== b.size) return false
  for (const token of a) if (!b.has(token)) return false
  return true
}

/**
 * Calcula similaridade entre dois títulos (0-100).
 *
 * Regras (medidas nos dados reais do usuário):
 * - números diferentes são jogos diferentes ("Spider-Man 2" != "Spider-Man 3");
 * - a oferta conter o título canônico inteiro é sinal forte
 *   ("James Bond 007: From Russia With Love" contém "007: From Russia with Love");
 * - tokens iguais em outra ordem também contam ("Legend of Zelda, The");
 * - título canônico de 1 token curto não vira associação automática.
 *
 * @param {string} a - Primeiro título
 * @param {string} b - Segundo título
 * @returns {number} - Similaridade (0-100)
 */
function fuzzyMatch(a, b) {
  const normA = normalizeTitle(a)
  const normB = normalizeTitle(b)

  if (normA === normB) return 100
  if (!normA || !normB) return 0

  const tokensA = titleTokens(normA)
  const tokensB = titleTokens(normB)
  if (!tokensA.length || !tokensB.length) return bigramDice(normA, normB)

  const digitsA = new Set(tokensA.filter((token) => /^\d+$/.test(token)).map((token) => String(Number(token))))
  const digitsB = new Set(tokensB.filter((token) => /^\d+$/.test(token)).map((token) => String(Number(token))))
  if (!sameTokenSet(digitsA, digitsB)) return 40

  const setA = new Set(tokensA)
  const setB = new Set(tokensB)
  let shared = 0
  for (const token of setA) if (setB.has(token)) shared++
  const dice = Math.round(((2 * shared) / (setA.size + setB.size)) * 100)

  const aInsideB = [...setA].every((token) => setB.has(token))
  const bInsideA = [...setB].every((token) => setA.has(token))

  let score
  if (aInsideB && bInsideA) {
    score = 95
  } else if (aInsideB || bInsideA) {
    // Um título contém o outro inteiro (com sufixos de release no meio).
    const shorter = aInsideB ? setA : setB
    const shorterTokens = aInsideB ? tokensA : tokensB
    score = shorter.size >= 2 ? 90 : shorterTokens[0].length >= 4 ? 88 : 70
  } else {
    score = Math.min(dice, 88)
  }

  const numeralsA = new Set(tokensA.map(numeralValue).filter(Boolean))
  const numeralsB = new Set(tokensB.map(numeralValue).filter(Boolean))
  if (!sameTokenSet(numeralsA, numeralsB)) {
    // Numeral faltando/sobrando não é associação automática.
    score = Math.min(score, CONFIDENCE_THRESHOLD_PROBABLE - 1)
  }

  return score
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

  // Busca adicional por padrões comuns em títulos/descrições (case-insensitive:
  // feeds Hydra aparecem com serial em minúsculas)
  const patterns = [
    /\b([A-Z]{4}[-_ ]?\d{5})\b/gi,  // PlayStation, PSP
    /\b([A-Z]{4}\d{5})\b/gi,        // PS3
    /\b(G[A-Z0-9]{3}\d{2})\b/gi,    // GameCube
    /\b(R[A-Z0-9]{3}\d{2})\b/gi,    // Wii
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

  // 5. Similaridade de título (canônico e aliases) com regras de contenção.
  // A similaridade é o melhor resultado entre o título do jogo e seus aliases.
  let similarity = 0
  let bestName = ""
  for (const name of [game.title, ...(game.aliases || [])]) {
    const value = fuzzyMatch(offerTitle, name)
    if (value > similarity) {
      similarity = value
      bestName = name
    }
  }

  // Oferta é subconjunto estrito do nome canônico (jogo mais específico que o
  // título da oferta): pode ser o mesmo jogo, mas nunca associação automática.
  const offerTokens = new Set(titleTokens(offerTitle))
  const nameTokens = new Set(titleTokens(bestName))
  const offerSubset =
    offerTokens.size > 0 &&
    offerTokens.size < nameTokens.size &&
    [...offerTokens].every((token) => nameTokens.has(token))

  // Sem plataforma na oferta: título sozinho não sustenta associação automática.
  const noPlatform = !offer.systemId

  if (similarity >= 90) {
    // Título contém o canônico (ou tokens idênticos): forte.
    score = noPlatform ? 45 : SCORE_TITLE_STRONG
    method = "fuzzy"
    evidence.push(`${noPlatform ? "no-platform," : ""}similarity:${similarity}`)

    if (!noPlatform) {
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
    }

    if (offerSubset) {
      score = Math.min(score, CONFIDENCE_THRESHOLD_AUTO - 1)
      evidence.push("subset")
    }

    return { score: noPlatform ? score : Math.min(score, SCORE_TITLE_FUZZY_MAX), method, evidence: evidence.join(", ") }
  }

  // 6. Fuzzy provável (65-69): título encurtado/variante, sujeito a auditoria.
  if (similarity >= 80 && !noPlatform) {
    score = CONFIDENCE_THRESHOLD_PROBABLE + Math.round((similarity - 80) / 3)
    method = "fuzzy"
    evidence.push(`similarity:${similarity}`)

    if (offer.region && game.regions && game.regions.includes(offer.region)) {
      score += SCORE_REGION_BONUS
      evidence.push(`region:${offer.region}`)
    }

    return { score: Math.min(score, CONFIDENCE_THRESHOLD_AUTO - 1), method, evidence: evidence.join(", ") }
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
    // Piso de aceitação: 80 (forte) para associação automática; 65-79 é
    // "probable" e só entra com allowProbable (auditoria na UI).
    minConfidence = CONFIDENCE_THRESHOLD_PROBABLE,
    allowProbable = false,
  } = options

  const offerTitle = offer.normalizedTitle || offer.title
  const scored = []
  for (const game of games) {
    const result = calculateMatchScore(offer, game)
    if (result.score <= 0) continue
    const sameSystem = Boolean(offer.systemId && game.systemId && offer.systemId === game.systemId)
    let titleSimilarity = 0
    for (const name of [game.title, ...(game.aliases || [])]) {
      const value = fuzzyMatch(offerTitle, name)
      if (value > titleSimilarity) titleSimilarity = value
    }
    scored.push({ game, result, sameSystem, titleSimilarity })
  }

  if (!scored.length) return null

  // Ordem estável: score desc, mesmo sistema, similaridade de título e, por fim,
  // id. Um empate nunca vira "melhor" por acaso da ordem do catálogo.
  scored.sort(
    (a, b) =>
      b.result.score - a.result.score ||
      Number(b.sameSystem) - Number(a.sameSystem) ||
      b.titleSimilarity - a.titleSimilarity ||
      String(a.game.id).localeCompare(String(b.game.id)),
  )

  const top = scored[0]
  const tieCount = scored.filter(
    (entry) =>
      entry.result.score === top.result.score &&
      entry.sameSystem === top.sameSystem &&
      entry.titleSimilarity === top.titleSimilarity,
  ).length
  const tied = tieCount > 1

  if (top.result.score < minConfidence) return null

  let quality = getMatchQuality(top.result.score)
  // Empate rebaixa para "probable": nunca associa automaticamente em silêncio.
  if (tied && quality !== "unmatched") quality = "probable"
  if (!allowProbable && quality === "probable") return null

  return {
    gameId: top.game.id,
    game: top.game,
    score: top.result.score,
    confidence: top.result.score,
    method: top.result.method,
    evidence: tied ? `${top.result.evidence}, tie:${tieCount}` : top.result.evidence,
    quality,
    tied,
  }
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
  const byFirstToken = new Map()
  const firstTokenOf = (value) => titleTokens(value)[0] || ""
  for (const game of games) {
    const title = normalizeTitle(game.title)
    add(byTitle, scopedKey(game.systemId, title), game)
    add(byPrefix, scopedKey(game.systemId, title.slice(0, 3)), game)
    add(byFirstToken, scopedKey(game.systemId, firstTokenOf(title)), game)
    for (const alias of game.aliases || []) {
      add(byAlias, scopedKey(game.systemId, normalizeTitle(alias)), game)
      add(byFirstToken, scopedKey(game.systemId, firstTokenOf(alias)), game)
    }
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
    // Fuzzy bounded por tokens: qualquer token da oferta que seja o primeiro
    // token de um título/alias canônico entra como candidato. Cobre variações
    // com prefixo diferente ("James Bond 007: ..." -> "007: ...") sem O(n²).
    for (const token of titleTokens(title))
      include(byFirstToken.get(scopedKey(offer.systemId, token)))
    // Último recurso: prefixo de 3 caracteres (typos no começo do título).
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
        confidence: match.confidence,
        method: match.method,
        evidence: match.evidence,
        quality: match.quality,
        tied: match.tied,
      })

      stats.matched++
      stats.byMethod[match.method] = (stats.byMethod[match.method] || 0) + 1
      stats.byQuality[match.quality] = (stats.byQuality[match.quality] || 0) + 1
      if (match.tied) stats.tied = (stats.tied || 0) + 1

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
