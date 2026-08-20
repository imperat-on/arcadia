"use strict"

// Índice de busca local, pequeno e deliberadamente independente do Electron.
// O catálogo remoto é um cache de páginas (e não uma fonte de verdade para a
// UI): quando a rede cai, o índice permite continuar procurando os itens que
// já foram vistos. A biblioteca usa as mesmas regras de normalização/ranking,
// mas nunca é persistida neste arquivo.

const fsDefault = require("node:fs")
const pathDefault = require("node:path")

const SEARCH_INDEX_VERSION = 1
const DEFAULT_LIMIT = 40
const DEFAULT_MAX_ENTRIES = 200_000
const CATALOG_SOURCE = "catalog"
const LIBRARY_SOURCE = "library"

/**
 * Normaliza texto para busca humana e determinística.
 *
 * NFD + remoção de marcas faz "ação" casar com "acao". Pontuação vira
 * separador, em vez de ser removida ("Half-Life" também casa com "half life"),
 * e espaços repetidos não alteram o resultado.
 */
function normalizeSearchText(value) {
  return String(value == null ? "" : value)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase("en-US")
    .replace(/[^\p{Letter}\p{Number}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ")
}

function text(value) {
  if (typeof value !== "string" && typeof value !== "number") return ""
  return String(value).trim()
}

function arrayText(value) {
  if (!Array.isArray(value)) return []
  return value.flatMap((item) => {
    if (typeof item === "string" || typeof item === "number") return [String(item)]
    if (item && typeof item === "object") {
      return [item.name, item.title, item.description].filter(
        (part) => typeof part === "string" || typeof part === "number",
      ).map(String)
    }
    return []
  })
}

function valueForId(item, source) {
  if (!item || typeof item !== "object" || Array.isArray(item)) return ""
  if (source === LIBRARY_SOURCE) return text(item.id || item.appid || item.game_id)
  return text(item.appid || item.game_id || item.id)
}

function titleForItem(item) {
  return text(item?.title || item?.name || item?.game_name)
}

function sourceKey(source, id) {
  return `${source}:${id}`
}

function clone(value) {
  if (!value || typeof value !== "object") return value
  // structuredClone não existe em algumas versões suportadas pelo launcher;
  // os payloads do catálogo/biblioteca são JSON por contrato.
  try {
    return JSON.parse(JSON.stringify(value))
  } catch {
    return { ...value }
  }
}

function nonEmpty(value) {
  if (Array.isArray(value)) return value.length > 0
  if (typeof value === "string") return value.trim().length > 0
  return value !== undefined && value !== null
}

/**
 * Mescla duas cópias do mesmo jogo sem apagar metadados já cacheados com uma
 * resposta parcial. Chaves novas podem evoluir sem mudar o payload público.
 */
function stableValue(a, b) {
  if (!nonEmpty(a)) return b
  if (!nonEmpty(b)) return a
  if (Array.isArray(a) && Array.isArray(b)) {
    const values = [...a, ...b]
    const unique = []
    const seen = new Set()
    for (const value of values) {
      const key = typeof value === "object" ? JSON.stringify(value) : String(value)
      if (seen.has(key)) continue
      seen.add(key)
      unique.push(value)
    }
    return unique.sort((left, right) => {
      const aText = String(JSON.stringify(left))
      const bText = String(JSON.stringify(right))
      return aText === bText ? 0 : aText < bText ? -1 : 1
    })
  }
  if (typeof a === "string" && typeof b === "string") return a <= b ? a : b
  if (typeof a === "number" && typeof b === "number") return Math.min(a, b)
  const left = JSON.stringify(a)
  const right = JSON.stringify(b)
  return left <= right ? a : b
}

function mergeItems(previous, next) {
  const out = { ...(previous || {}) }
  for (const [key, value] of Object.entries(next || {})) {
    out[key] = Object.prototype.hasOwnProperty.call(out, key) ? stableValue(out[key], value) : value
  }
  return out
}

function buildEntry(item, source) {
  if (!item || typeof item !== "object" || Array.isArray(item)) return null
  const origem = source === LIBRARY_SOURCE ? LIBRARY_SOURCE : CATALOG_SOURCE
  const id = valueForId(item, origem)
  const title = titleForItem(item)
  if (!id || !title) return null

  const value = clone(item)
  if (origem === CATALOG_SOURCE) {
    // O índice da loja nunca deve virar um atalho para dados de download. Isso
    // também protege contra um cache Hydra passado por engano ao adaptador.
    for (const segredo of ["uri", "uris", "magnet", "download", "downloads", "download_url", "downloadUrl"])
      delete value[segredo]
    // O catálogo histórico teve game_id/game_name; normalizar só o índice não
    // basta: o resultado precisa manter o shape atual { appid, title, ... }.
    if (!text(value.appid)) value.appid = id
    if (!text(value.title)) value.title = title
  }
  const normalizedTitle = normalizeSearchText(value.title || title)
  if (!normalizedTitle) return null
  const fieldText = [
    normalizedTitle,
    value.launcher,
    ...arrayText(value.categories),
    ...arrayText(value.genres),
    ...arrayText(value.tags),
  ]
    .map(normalizeSearchText)
    .filter(Boolean)
    .join(" ")
  const identifier = normalizeSearchText(origem === LIBRARY_SOURCE ? value.id : value.appid)

  return {
    key: sourceKey(origem, id),
    source: origem,
    id,
    title: text(value.title || title),
    normalizedTitle,
    fieldText,
    identifier,
    value,
  }
}

function entryForPersisted(value) {
  if (!value || typeof value !== "object") return null
  const source = value.source === LIBRARY_SOURCE ? LIBRARY_SOURCE : CATALOG_SOURCE
  const item = value.value && typeof value.value === "object" ? value.value : value
  return buildEntry(item, source)
}

function unwrapCatalog(value) {
  if (!value || typeof value !== "object") return value
  // catalogGet espelha o envelope HTTP inteiro: { ok: true, data: ... }.
  if (value.data && typeof value.data === "object" && !Array.isArray(value.data)) return value.data
  return value
}

function candidatesFromArray(array) {
  if (!Array.isArray(array)) return []
  return array.filter((item) => item && typeof item === "object" && !Array.isArray(item))
}

/** Extrai apenas formatos de catálogo conhecidos; não indexa notícias/metadados. */
function extractCatalogItems(payload) {
  const value = unwrapCatalog(payload)
  if (Array.isArray(value)) return candidatesFromArray(value)
  if (!value || typeof value !== "object") return []

  for (const key of ["itens", "jogos", "completa"]) {
    if (Array.isArray(value[key])) return candidatesFromArray(value[key])
  }
  // Resposta de uma fonte Hydra: só indexe downloads que têm game_id/appid.
  if (Array.isArray(value.games)) return candidatesFromArray(value.games)
  // Alguns caches antigos guardavam cada lista sob uma chave própria
  // ({ "__all": { completa: [...] } }). Recorremos apenas ao JSON desse
  // arquivo; a validação de appid/título em buildEntry descarta metadados.
  const nested = []
  for (const child of Object.values(value)) {
    if (!child || typeof child !== "object") continue
    nested.push(...extractCatalogItems(child))
  }
  return nested
}

function rankEntry(entry, query, terms) {
  const title = entry.normalizedTitle
  const fields = entry.fieldText
  if (!title || !terms.length) return null

  const exact = title === query
  const exactIdentifier = entry.identifier === query
  const phrasePrefix = title.startsWith(query)
  const titleTokens = title.split(" ")
  const everyTerm = terms.every((term) => fields.split(" ").some((field) => field === term || field.startsWith(term)))
  const everyTitleTerm = terms.every((term) => titleTokens.some((token) => token === term || token.startsWith(term)))
  const phrase = title.includes(query)

  if (!exact && !exactIdentifier && !phrasePrefix && !everyTitleTerm && !everyTerm && !phrase) return null

  // Rank em faixas, depois por distância do título. O terceiro componente é
  // sempre estável mesmo quando dois jogos têm o mesmo nome.
  let bucket = 50
  if (exact) bucket = 0
  else if (exactIdentifier) bucket = 5
  else if (phrasePrefix) bucket = 10
  else if (everyTitleTerm) bucket = 20
  else if (phrase) bucket = 30
  else if (everyTerm) bucket = 40
  return [bucket, Math.max(0, title.length - query.length), entry.key]
}

function compareRank(a, b) {
  for (let i = 0; i < a.length; i++) {
    if (a[i] < b[i]) return -1
    if (a[i] > b[i]) return 1
  }
  return 0
}

function searchEntries(entries, query, { source, limit = DEFAULT_LIMIT } = {}) {
  const normalized = normalizeSearchText(query)
  if (!normalized) return []
  const terms = normalized.split(" ").filter(Boolean)
  const max = Number.isSafeInteger(Number(limit)) && Number(limit) > 0 ? Number(limit) : DEFAULT_LIMIT
  const hits = []
  for (const entry of entries || []) {
    if (!entry || (source && entry.source !== source)) continue
    const rank = rankEntry(entry, normalized, terms)
    if (rank) hits.push({ entry, rank })
  }
  hits.sort((a, b) => compareRank(a.rank, b.rank))
  return hits.slice(0, max).map(({ entry }) => clone(entry.value))
}

function compareLexical(a, b) {
  const left = String(a || "")
  const right = String(b || "")
  return left === right ? 0 : left < right ? -1 : 1
}

function sortEntries(entries) {
  return [...entries].sort(
    (a, b) => compareLexical(a.normalizedTitle, b.normalizedTitle) || compareLexical(a.key, b.key),
  )
}

class LocalSearchIndex {
  constructor({ indexPath, fsImpl = fsDefault, pathImpl = pathDefault, maxEntries = DEFAULT_MAX_ENTRIES } = {}) {
    this.indexPath = indexPath || ""
    this.fs = fsImpl
    this.path = pathImpl
    this.maxEntries =
      Number.isSafeInteger(Number(maxEntries)) && Number(maxEntries) > 0
        ? Number(maxEntries)
        : DEFAULT_MAX_ENTRIES
    this.entries = new Map()
    this.loaded = false
    this.changed = false
  }

  load() {
    if (this.loaded) return this
    this.loaded = true
    if (!this.indexPath) return this
    try {
      const parsed = JSON.parse(this.fs.readFileSync(this.indexPath, "utf8"))
      const list = Array.isArray(parsed) ? parsed : parsed?.version === SEARCH_INDEX_VERSION ? parsed.entries : []
      if (!Array.isArray(list)) return this
      for (const raw of list) {
        const entry = entryForPersisted(raw)
        if (entry) this.entries.set(entry.key, entry)
      }
    } catch {
      // Cache corrompido/ausente é equivalente a um índice vazio; a próxima
      // ingestão o substitui atomicamente.
    }
    return this
  }

  upsert(items, { source = CATALOG_SOURCE, persist = false } = {}) {
    this.load()
    let count = 0
    for (const item of Array.isArray(items) ? items : []) {
      const next = buildEntry(item, source)
      if (!next) continue
      const previous = this.entries.get(next.key)
      if (previous) {
        const merged = mergeItems(previous.value, next.value)
        const mergedEntry = buildEntry(merged, next.source)
        if (mergedEntry) this.entries.set(next.key, mergedEntry)
      } else {
        this.entries.set(next.key, next)
      }
      count++
    }
    this.prune()
    this.changed = this.changed || count > 0
    if (persist) this.save()
    return count
  }

  replaceSource(items, { source = LIBRARY_SOURCE, persist = false } = {}) {
    this.load()
    const keep = new Map([...this.entries].filter(([, entry]) => entry.source !== source))
    this.entries = keep
    const count = this.upsert(items, { source, persist: false })
    if (count) this.changed = true
    if (persist) this.save()
    return count
  }

  remove(id, source = CATALOG_SOURCE, { persist = false } = {}) {
    this.load()
    const removed = this.entries.delete(sourceKey(source, text(id)))
    this.changed = this.changed || removed
    if (persist && removed) this.save()
    return removed
  }

  search(query, options = {}) {
    this.load()
    return searchEntries(this.entries.values(), query, options)
  }

  page({ source = CATALOG_SOURCE, offset = 0, limit = DEFAULT_LIMIT } = {}) {
    this.load()
    const off = Number.isSafeInteger(Number(offset)) && Number(offset) >= 0 ? Number(offset) : 0
    const lim = Number.isSafeInteger(Number(limit)) && Number(limit) > 0 ? Number(limit) : DEFAULT_LIMIT
    const entries = sortEntries([...this.entries.values()].filter((entry) => entry.source === source))
    return {
      itens: entries.slice(off, off + lim).map((entry) => clone(entry.value)),
      total: entries.length,
      offset: off,
    }
  }

  hydrateCacheFiles(dataDir, { persist = false } = {}) {
    this.load()
    const root = dataDir || this.path.dirname(this.indexPath)
    const files = []
    const mirror = this.path.join(root, "catalog_espelho")
    try {
      for (const name of this.fs.readdirSync(mirror).sort()) {
        // Fontes Hydra carregam URIs e podem ter dezenas de MB; elas têm um
        // índice próprio em sources.js e não fazem parte do catálogo Steam.
        if (!/^\b(?:catalog|popular|steam250|genre|search)(?:_[^.]+)?\.json$/i.test(name)) continue
        files.push(this.path.join(mirror, name))
      }
    } catch {}
    for (const name of ["store_popular_cache.json", "store_genre_cache.json", "store_steam250_cache.json"]) {
      files.push(this.path.join(root, name))
    }

    let total = 0
    for (const file of [...new Set(files)].sort()) {
      try {
        const payload = JSON.parse(this.fs.readFileSync(file, "utf8"))
        total += this.upsert(extractCatalogItems(payload), { persist: false })
      } catch {
        // Um cache individual ruim não invalida os demais.
      }
    }
    if (persist && total) this.save()
    return total
  }

  stats() {
    this.load()
    const bySource = {}
    for (const entry of this.entries.values()) bySource[entry.source] = (bySource[entry.source] || 0) + 1
    return { version: SEARCH_INDEX_VERSION, total: this.entries.size, bySource }
  }

  prune() {
    if (this.entries.size <= this.maxEntries) return
    // Evicção determinística: não depende da ordem em que páginas chegaram.
    const kept = sortEntries([...this.entries.values()]).slice(0, this.maxEntries)
    this.entries = new Map(kept.map((entry) => [entry.key, entry]))
  }

  save() {
    this.load()
    if (!this.indexPath || !this.changed) return false
    try {
      this.fs.mkdirSync(this.path.dirname(this.indexPath), { recursive: true })
      const entries = sortEntries(this.entries.values()).map((entry) => ({
        source: entry.source,
        value: entry.value,
      }))
      const payload = {
        version: SEARCH_INDEX_VERSION,
        generated_at: Math.floor(Date.now() / 1000),
        entries,
      }
      const temporary = `${this.indexPath}.tmp`
      this.fs.writeFileSync(temporary, JSON.stringify(payload))
      this.fs.renameSync(temporary, this.indexPath)
      this.changed = false
      return true
    } catch {
      // O índice é uma otimização: resultados em memória continuam válidos.
      return false
    }
  }
}

function createLocalSearchIndex(options = {}) {
  return new LocalSearchIndex(options)
}

/** Busca a biblioteca já carregada, sem tocar no disco ou na rede. */
function searchLibrary(games, query, options = {}) {
  const entries = []
  for (const game of Array.isArray(games) ? games : []) {
    const entry = buildEntry(game, LIBRARY_SOURCE)
    if (entry) entries.push(entry)
  }
  return searchEntries(entries, query, { ...options, source: LIBRARY_SOURCE })
}

module.exports = {
  SEARCH_INDEX_VERSION,
  DEFAULT_LIMIT,
  CATALOG_SOURCE,
  LIBRARY_SOURCE,
  normalizeSearchText,
  buildEntry,
  extractCatalogItems,
  searchEntries,
  searchLibrary,
  createLocalSearchIndex,
}
