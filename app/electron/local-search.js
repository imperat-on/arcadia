"use strict"

// Índice de busca local, pequeno e deliberadamente independente do Electron.
// O catálogo remoto é um cache de páginas (e não uma fonte de verdade para a
// UI): quando a rede cai, o índice permite continuar procurando os itens que
// já foram vistos. A biblioteca usa as mesmas regras de normalização/ranking,
// mas nunca é persistida neste arquivo.

const fsDefault = require("node:fs")
const pathDefault = require("node:path")

const SEARCH_INDEX_VERSION = 2
const LEGACY_SEARCH_INDEX_VERSION = 1
const INDEX_SCHEMA = "arcadia.catalog-search"
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

const FACET_NAMES = ["launcher", "genre", "tag", "installed"]

// Facets are deliberately derived from the payload instead of from a fixed
  // provider schema. SteamSpy uses `genres`, catalog entries use `categories`,
// and older catalog snapshots occasionally have a singular `genre`/`tag`.
// Keeping this adapter here lets old and new pages participate in the same
// offline index without changing the public game payload.
function facetParts(value) {
  if (Array.isArray(value)) return value.flatMap(facetParts)
  if (typeof value === "string" || typeof value === "number") {
    return String(value)
      .split(/[,;|]/)
      .map((part) => part.trim())
      .filter(Boolean)
  }
  if (value && typeof value === "object") {
    return [value.name, value.title, value.description, value.value, value.id]
      .filter((part) => typeof part === "string" || typeof part === "number")
      .flatMap(facetParts)
  }
  return []
}

function installedValue(item) {
  if (!item || typeof item !== "object") return null
  for (const key of ["installed", "instalado", "is_installed", "isInstalled", "downloaded"]) {
    if (!Object.prototype.hasOwnProperty.call(item, key)) continue
    const value = item[key]
    if (typeof value === "boolean") return value
    if (typeof value === "number" && (value === 0 || value === 1)) return value === 1
    if (typeof value === "string") {
      const normalized = normalizeSearchText(value)
      if (["true", "yes", "sim", "installed", "instalado", "1"].includes(normalized)) return true
      if (["false", "no", "nao", "not installed", "nao instalado", "0"].includes(normalized))
        return false
    }
  }
  return null
}

function facetValues(item, facet) {
  if (!item || typeof item !== "object") return []
  if (facet === "launcher")
    return facetParts(item.launcher).map(normalizeSearchText).filter(Boolean)
  if (facet === "genre") {
    return [item.genres, item.genre, item.categories]
      .flatMap(facetParts)
      .map(normalizeSearchText)
      .filter(Boolean)
  }
  if (facet === "tag") {
    return [item.tags, item.tag].flatMap(facetParts).map(normalizeSearchText).filter(Boolean)
  }
  if (facet === "installed") {
    const installed = installedValue(item)
    return installed == null ? [] : [installed ? "true" : "false"]
  }
  return []
}

function uniqueFacetValues(values) {
  return [...new Set(values.filter(Boolean))].sort(compareLexical)
}

function normalizedFacetFilters(options = {}) {
  const nested = options.filters || options.filtros || options.facets || options.facetas || {}
  const valueFor = (name, aliases = []) => {
    for (const key of [name, ...aliases]) {
      if (Object.prototype.hasOwnProperty.call(options, key) && options[key] !== undefined)
        return options[key]
      if (Object.prototype.hasOwnProperty.call(nested, key) && nested[key] !== undefined)
        return nested[key]
    }
    return undefined
  }
  const out = {}
  for (const facet of ["launcher", "genre", "tag"]) {
    const raw = valueFor(
      facet,
      facet === "genre" ? ["genres", "genero"] : facet === "tag" ? ["tags"] : [],
    )
    if (raw === undefined || raw === null || raw === "") continue
    const values = facetParts(raw).map(normalizeSearchText).filter(Boolean)
    if (values.length) out[facet] = new Set(values)
  }
  const installed = valueFor("installed", ["instalado"])
  if (
    installed !== undefined &&
    installed !== null &&
    installed !== "" &&
    installed !== "all" &&
    installed !== "todos"
  ) {
    let value = null
    if (typeof installed === "boolean") value = installed
    else if (typeof installed === "number" && (installed === 0 || installed === 1))
      value = installed === 1
    else {
      const normalized = normalizeSearchText(installed)
      if (["true", "yes", "sim", "installed", "instalado", "1"].includes(normalized)) value = true
      if (["false", "no", "nao", "not installed", "nao instalado", "0"].includes(normalized))
        value = false
    }
    if (value !== null) out.installed = value
  }
  return out
}

function matchesFilters(entry, options = {}, prepared = null) {
  const filters = prepared || normalizedFacetFilters(options)
  for (const facet of ["launcher", "genre", "tag"]) {
    const wanted = filters[facet]
    if (!wanted || !wanted.size) continue
    const values = entry?.facets?.[facet] || []
    if (!values.some((value) => wanted.has(value))) return false
  }
  if (filters.installed !== undefined) {
    const values = entry?.facets?.installed || []
    // Unknown installation state must not be advertised as installed (or as
    // explicitly not installed).  Callers can materialize a boolean in the
    // catalog/library payload when that distinction matters.
    if (!values.includes(filters.installed ? "true" : "false")) return false
  }
  return true
}

function facetCounts(entries, options = {}) {
  const filters = normalizedFacetFilters(options)
  const counts = { launcher: {}, genre: {}, tag: {}, installed: {} }
  const includeUnknown = options.includeUnknown === true || options.incluirDesconhecido === true
  for (const entry of entries || []) {
    if (
      !entry ||
      (options.source && entry.source !== options.source) ||
      !matchesFilters(entry, options, filters)
    )
      continue
    for (const facet of ["launcher", "genre", "tag"]) {
      for (const value of uniqueFacetValues(entry.facets?.[facet] || [])) {
        counts[facet][value] = (counts[facet][value] || 0) + 1
      }
    }
    const installed = entry.facets?.installed?.[0]
    if (installed) counts.installed[installed] = (counts.installed[installed] || 0) + 1
    else if (includeUnknown) counts.installed.unknown = (counts.installed.unknown || 0) + 1
  }
  for (const facet of FACET_NAMES) {
    const sorted = Object.entries(counts[facet]).sort(
      (a, b) => b[1] - a[1] || compareLexical(a[0], b[0]),
    )
    counts[facet] = Object.fromEntries(sorted)
  }
  return counts
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
  // The catalog itself is the Steam catalog.  Infer that source launcher for
  // indexing only (never mutate the legacy payload), while library entries
  // must carry their explicit launcher to be considered a match.
  const facetInput =
    origem === CATALOG_SOURCE && !value.launcher ? { ...value, launcher: "steam" } : value
  const facets = {
    launcher: uniqueFacetValues(facetValues(facetInput, "launcher")),
    genre: uniqueFacetValues(facetValues(facetInput, "genre")),
    tag: uniqueFacetValues(facetValues(facetInput, "tag")),
    installed: facetValues(facetInput, "installed"),
  }
  const fieldText = [
    normalizedTitle,
    value.launcher,
    ...arrayText(value.categories),
    ...arrayText(value.genres),
    ...arrayText(value.genre),
    ...arrayText(value.tags),
    ...arrayText(value.tag),
    ...facets.launcher,
    ...facets.genre,
    ...facets.tag,
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
    facets,
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

function positiveInteger(value, fallback) {
  return Number.isSafeInteger(Number(value)) && Number(value) > 0 ? Number(value) : fallback
}

function nonNegativeInteger(value, fallback = 0) {
  return Number.isSafeInteger(Number(value)) && Number(value) >= 0 ? Number(value) : fallback
}

function matchingEntries(entries, { source, query = "", filters, ...options } = {}) {
  const prepared =
    filters && filters.__normalized ? filters : normalizedFacetFilters({ ...options, filters })
  const normalized = normalizeSearchText(query)
  const terms = normalized.split(" ").filter(Boolean)
  const hits = []
  for (const entry of entries || []) {
    if (!entry || (source && entry.source !== source) || !matchesFilters(entry, options, prepared))
      continue
    // Empty query is useful to page a faceted catalog, but remains an empty
    // result for the historical search() API (handled by searchEntries).
    if (!normalized) {
      hits.push({ entry, rank: [0, 0, entry.key] })
      continue
    }
    const rank = rankEntry(entry, normalized, terms)
    if (rank) hits.push({ entry, rank })
  }
  hits.sort((a, b) => compareRank(a.rank, b.rank))
  return hits.map(({ entry }) => entry)
}

function filterEntries(entries, options = {}) {
  const source = options.source
  const prepared = normalizedFacetFilters(options)
  return sortEntries(
    [...(entries || [])].filter(
      (entry) =>
        entry && (!source || entry.source === source) && matchesFilters(entry, options, prepared),
    ),
  )
}

function searchEntries(entries, query, options = {}) {
  const normalized = normalizeSearchText(query)
  if (!normalized) return []
  const hits = matchingEntries(entries, { ...options, query })
  const offset = nonNegativeInteger(options.offset, 0)
  const limit = positiveInteger(options.limit, DEFAULT_LIMIT)
  return hits.slice(offset, offset + limit).map((entry) => clone(entry.value))
}

function searchPageEntries(entries, query, options = {}) {
  const normalized = normalizeSearchText(query)
  const hits = normalized
    ? matchingEntries(entries, { ...options, query })
    : filterEntries(entries, options)
  const offset = nonNegativeInteger(options.offset, 0)
  const limit = positiveInteger(options.limit, DEFAULT_LIMIT)
  const page = hits.slice(offset, offset + limit)
  const facets = facetCounts(hits, { includeUnknown: options.includeUnknown })
  return {
    itens: page.map((entry) => clone(entry.value)),
    total: hits.length,
    offset,
    limit,
    has_more: offset + page.length < hits.length,
    next_offset: offset + page.length < hits.length ? offset + page.length : null,
    facets,
    facetas: facets,
  }
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

function indexMetadata(entries, generatedAt = 0) {
  const sourceCounts = {}
  for (const entry of entries || [])
    sourceCounts[entry.source] = (sourceCounts[entry.source] || 0) + 1
  const sources = Object.fromEntries(
    Object.entries(sourceCounts).sort(([a], [b]) => compareLexical(a, b)),
  )
  return {
    schema: INDEX_SCHEMA,
    version: SEARCH_INDEX_VERSION,
    generated_at: Number(generatedAt) || 0,
    entry_count: (entries || []).length,
    sources,
    facets: facetCounts(entries),
  }
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
    this.generatedAt = 0
    this.persistedMetadata = null
  }

  load() {
    if (this.loaded) return this
    this.loaded = true
    if (!this.indexPath) return this
    try {
      const parsed = JSON.parse(this.fs.readFileSync(this.indexPath, "utf8"))
      // Version 1 used the same entries shape but had no metadata.  Keep it
      // readable in-place; the next write upgrades it atomically to v2.
      const list = Array.isArray(parsed)
        ? parsed
        : parsed &&
            (parsed.version === SEARCH_INDEX_VERSION ||
              parsed.version === LEGACY_SEARCH_INDEX_VERSION)
          ? parsed.entries
          : []
      if (!Array.isArray(list)) return this
      this.generatedAt = Number(parsed?.generated_at || parsed?.metadata?.generated_at) || 0
      this.persistedMetadata =
        parsed?.metadata && typeof parsed.metadata === "object" ? parsed.metadata : null
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

  upsert(items, { source = CATALOG_SOURCE, persist = false, installed } = {}) {
    this.load()
    let count = 0
    for (const item of Array.isArray(items) ? items : []) {
      if (!item || typeof item !== "object") continue
      const candidate =
        installed !== undefined && installed !== null && item.installed === undefined
          ? { ...item, installed: Boolean(installed) }
          : item
      const next = buildEntry(candidate, source)
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
    const before = this.entries.size
    this.prune()
    this.changed = this.changed || count > 0 || before !== this.entries.size
    if (count > 0 || before !== this.entries.size) this.generatedAt = Math.floor(Date.now() / 1000)
    if (persist) this.save()
    return count
  }

  replaceSource(items, { source = LIBRARY_SOURCE, persist = false, installed } = {}) {
    this.load()
    const before = this.entries.size
    const keep = new Map([...this.entries].filter(([, entry]) => entry.source !== source))
    this.entries = keep
    const count = this.upsert(items, { source, persist: false, installed })
    if (count || before !== this.entries.size) {
      this.changed = true
      this.generatedAt = Math.floor(Date.now() / 1000)
    }
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

  searchPage(query, options = {}) {
    this.load()
    return searchPageEntries(this.entries.values(), query, options)
  }

  filter(options = {}) {
    this.load()
    return filterEntries(this.entries.values(), options).map((entry) => clone(entry.value))
  }

  facets(options = {}) {
    this.load()
    return facetCounts(this.entries.values(), options)
  }

  page({
    source = CATALOG_SOURCE,
    offset = 0,
    limit = DEFAULT_LIMIT,
    query = "",
    ...options
  } = {}) {
    this.load()
    const off = nonNegativeInteger(offset, 0)
    const lim = positiveInteger(limit, DEFAULT_LIMIT)
    const entries = normalizeSearchText(query)
      ? matchingEntries(this.entries.values(), { ...options, source, query })
      : filterEntries(this.entries.values(), { ...options, source })
    const itens = entries.slice(off, off + lim).map((entry) => clone(entry.value))
    const generatedAt = this.generatedAt || this.persistedMetadata?.generated_at || 0
    const facets = facetCounts(entries, { includeUnknown: options.includeUnknown })
    const index = this.metadata({ generatedAt })
    return {
      itens,
      total: entries.length,
      offset: off,
      limit: lim,
      has_more: off + itens.length < entries.length,
      next_offset: off + itens.length < entries.length ? off + itens.length : null,
      facets,
      facetas: facets,
      index,
      indice: index,
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

  metadata({ generatedAt = this.generatedAt } = {}) {
    this.load()
    return indexMetadata([...this.entries.values()], generatedAt)
  }

  stats() {
    this.load()
    const metadata = this.metadata()
    return {
      version: SEARCH_INDEX_VERSION,
      total: this.entries.size,
      bySource: metadata.sources,
      facets: metadata.facets,
      metadata,
      metadados: metadata,
    }
  }

  prune() {
    if (this.entries.size <= this.maxEntries) return false
    // Evicção determinística: não depende da ordem em que páginas chegaram.
    const kept = sortEntries([...this.entries.values()]).slice(0, this.maxEntries)
    this.entries = new Map(kept.map((entry) => [entry.key, entry]))
    return true
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
      const generatedAt = Math.floor(Date.now() / 1000)
      const payload = {
        version: SEARCH_INDEX_VERSION,
        generated_at: generatedAt,
        metadata: indexMetadata([...this.entries.values()], generatedAt),
        entries,
      }
      const temporary = `${this.indexPath}.tmp`
      this.fs.writeFileSync(temporary, JSON.stringify(payload))
      this.fs.renameSync(temporary, this.indexPath)
      this.generatedAt = generatedAt
      this.persistedMetadata = payload.metadata
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

function entriesForGames(games, source = LIBRARY_SOURCE) {
  const entries = []
  for (const game of Array.isArray(games) ? games : []) {
    const entry = buildEntry(game, source)
    if (entry) entries.push(entry)
  }
  return entries
}

/** Busca a biblioteca já carregada, sem tocar no disco ou na rede. */
function searchLibrary(games, query, options = {}) {
  return searchEntries(entriesForGames(games, LIBRARY_SOURCE), query, {
    ...options,
    source: LIBRARY_SOURCE,
  })
}

function pageLibrary(games, query = "", options = {}) {
  return searchPageEntries(entriesForGames(games, LIBRARY_SOURCE), query, {
    ...options,
    source: LIBRARY_SOURCE,
  })
}

function facetsForItems(items, { source = CATALOG_SOURCE, ...options } = {}) {
  return facetCounts(entriesForGames(items, source), options)
}

module.exports = {
  SEARCH_INDEX_VERSION,
  DEFAULT_LIMIT,
  CATALOG_SOURCE,
  LIBRARY_SOURCE,
  FACET_NAMES,
  INDEX_SCHEMA,
  normalizeSearchText,
  buildEntry,
  extractCatalogItems,
  facetValues,
  normalizedFacetFilters,
  matchesFilters,
  facetCounts,
  filterEntries,
  searchEntries,
  searchPageEntries,
  searchLibrary,
  pageLibrary,
  facetsForItems,
  createLocalSearchIndex,
}
