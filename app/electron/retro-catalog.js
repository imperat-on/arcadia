"use strict"

// Catálogo da aba Retro. A lista é formada exclusivamente pelos registros que
// a Hydra Library marca com status "Classics"; nenhum jogo é inventado a
// partir de uma busca Steam. O processo principal apenas lê/faz cache de JSONs
// públicos e o renderer recebe os URIs somente ao abrir o detalhe.
const fs = require("node:fs")
const path = require("node:path")
const os = require("node:os")
const crypto = require("node:crypto")
const { fetchRede } = require("./httpfetch")
const { getDataDir } = require("./runtime-paths")

const HYDRA_API = "https://api.hydralibrary.com/sources?status=Classics&limit=100"
const CACHE_FILE_NAME = "retro-catalog.json"
const CACHE_VERSION = 1
const CACHE_TTL_MS = 24 * 60 * 60 * 1000
const MAX_SOURCES = 100
const MAX_GAMES = 50000
const MAX_URIS = 16
const MAX_TITLE = 300
const MAX_DESCRIPTION = 2400
const MAX_LIMIT = 48
const MAX_QUERY = 120
const ID_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/

function safePublicUrl(value, maxLength = 2048) {
  if (typeof value !== "string" || !value.trim() || value.length > maxLength) return ""
  try {
    const u = new URL(value.trim())
    if (u.hash) return ""
    if (u.protocol !== "https:" || u.username || u.password || (u.port && u.port !== "443"))
      return ""
    const host = u.hostname.toLowerCase()
    if (
      host === "localhost" ||
      host === "127.0.0.1" ||
      host === "::1" ||
      host.startsWith("[") ||
      host.includes(":") ||
      /^(10|127)\./.test(host) ||
      /^192\.168\./.test(host) ||
      /^172\.(1[6-9]|2\d|3[0-1])\./.test(host)
    )
      return ""
    return u.toString()
  } catch {
    return ""
  }
}

function sourceId(value) {
  const id = String(value ?? "").trim()
  return ID_RE.test(id) || /^\d{1,12}$/.test(id) ? id : ""
}

function cleanText(value, max = MAX_DESCRIPTION) {
  let text = String(value || "")
    .replace(/<br\s*\/?>(?=\s*)/gi, "\n")
    .replace(/<[^>]*>/g, " ")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/\s+/g, " ")
    .trim()
  return text.slice(0, max)
}

function normalizeSource(value) {
  if (!value || typeof value !== "object") return null
  const statusValues = [value.status, value.tags, value.statuses, value.tag].flatMap((raw) =>
    Array.isArray(raw) ? raw : [raw],
  )
  const status = statusValues
    .filter((s) => typeof s === "string")
    .flatMap((s) => s.split(/[;,|]/))
    .map((s) => s.trim())
    .filter(Boolean)
  if (!status.some((s) => s.toLowerCase() === "classics")) return null
  const id = sourceId(value.id ?? value.sourceId)
  const url = safePublicUrl(value.url || value.link)
  const title = cleanText(value.title || value.name, 160)
  if (!id || !url || !title) return null
  return {
    id,
    title,
    description: cleanText(value.description, 500),
    url,
    gamesCount: Number.isSafeInteger(Number(value.gamesCount))
      ? Math.max(0, Number(value.gamesCount))
      : 0,
    status,
    registryUrl: `https://library.hydra.wiki/sources/${encodeURIComponent(id)}`,
  }
}

function normalizeUri(value) {
  if (typeof value !== "string") return ""
  const uri = value.trim()
  if (!uri || uri.length > 8192 || uri.includes("\u0000")) return ""
  if (/^magnet:\?xt=urn:btih:[A-Za-z0-9]+/i.test(uri)) return uri
  return safePublicUrl(uri, 8192)
}

function normalizeGame(download, source, index) {
  if (!download || typeof download !== "object") return null
  const title = cleanText(download.title, MAX_TITLE)
  const rawUris = Array.isArray(download.uris) ? download.uris : download.uri ? [download.uri] : []
  const uris = [...new Set(rawUris.map(normalizeUri).filter(Boolean))].slice(0, MAX_URIS)
  if (!title || !uris.length) return null
  return {
    id: `${source.id}:${index}`,
    title,
    sourceId: source.id,
    sourceTitle: source.title,
    platform: cleanText(download.platform, 60),
    description: cleanText(download.descriptionHtml || download.description, MAX_DESCRIPTION),
    ...(safePublicUrl(
      download.cover ||
        download.image ||
        download.thumbnail ||
        download.artwork ||
        download.header_image,
    )
      ? {
          cover: safePublicUrl(
            download.cover ||
              download.image ||
              download.thumbnail ||
              download.artwork ||
              download.header_image,
          ),
        }
      : {}),
    ...(safePublicUrl(download.capa || download.portrait || download.coverPortrait)
      ? { capa: safePublicUrl(download.capa || download.portrait || download.coverPortrait) }
      : {}),
    ...(safePublicUrl(download.fallbackCover || download.fallback_cover)
      ? { fallbackCover: safePublicUrl(download.fallbackCover || download.fallback_cover) }
      : {}),
    fileSize: cleanText(download.fileSize, 80),
    uploadDate: cleanText(download.uploadDate, 80),
    uris,
  }
}

function normalizePayload(data, source) {
  if (!data || typeof data !== "object" || Array.isArray(data)) return []
  if (typeof data.name !== "string" || !data.name.trim()) return []
  const downloads = data.downloads
  if (!Array.isArray(downloads)) return []
  const games = []
  for (let i = 0; i < downloads.length && games.length < MAX_GAMES; i++) {
    const game = normalizeGame(downloads[i], source, i)
    if (game) games.push(game)
  }
  return games
}

const MAX_JSON_BYTES = 64 * 1024 * 1024

async function readResponseJson(response) {
  const reader = response?.body?.getReader?.()
  if (!reader) return response.json()
  const chunks = []
  let total = 0
  try {
    while (true) {
      const part = await reader.read()
      if (part.done) break
      total += part.value?.byteLength || 0
      if (total > MAX_JSON_BYTES) {
        await reader.cancel().catch(() => {})
        throw new Error("JSON da fonte excede o limite")
      }
      chunks.push(part.value)
    }
  } finally {
    reader.releaseLock?.()
  }
  const bytes = Buffer.concat(chunks.map((chunk) => Buffer.from(chunk)))
  return JSON.parse(bytes.toString("utf8"))
}

async function readJson(fetchImpl, url, timeoutMs = 30000) {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const response = await fetchImpl(url, {
      headers: { Accept: "application/json", "User-Agent": "Arcadia-Retro/1" },
      redirect: "error",
      signal: controller.signal,
    })
    if (!response?.ok) throw new Error(`HTTP ${response?.status || 0}`)
    const length = Number(response.headers?.get?.("content-length") || 0)
    if (length > MAX_JSON_BYTES) throw new Error("JSON da fonte excede o limite")
    return await readResponseJson(response)
  } finally {
    clearTimeout(timer)
  }
}

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

function readCache(file, fsImpl = fs) {
  try {
    if (pathHasSymlink(file, fsImpl)) return null
    const value = JSON.parse(fsImpl.readFileSync(file, "utf8"))
    if (
      value?.version !== CACHE_VERSION ||
      !Array.isArray(value.games) ||
      !Array.isArray(value.sources)
    )
      return null
    return value
  } catch {
    return null
  }
}

function writeCache(file, value, fsImpl = fs) {
  try {
    const directory = path.dirname(file)
    if (pathHasSymlink(directory, fsImpl)) return
    fsImpl.mkdirSync(directory, { recursive: true })
    if (pathHasSymlink(directory, fsImpl) || pathHasSymlink(file, fsImpl)) return
    const tmp = `${file}.tmp-${process.pid}`
    fsImpl.writeFileSync(tmp, JSON.stringify(value), { mode: 0o600 })
    fsImpl.renameSync(tmp, file)
  } catch {
    // O catálogo continua utilizável em memória mesmo sem disco gravável.
  }
}

function sourceCachePath(dataDir, url) {
  const id = crypto.createHash("sha256").update(String(url)).digest("hex").slice(0, 12)
  return path.join(dataDir, "sources", `${id}.json`)
}

function readLocalSourcePayload(dataDir, source, fsImpl = fs) {
  const file = sourceCachePath(dataDir, source.url)
  try {
    if (pathHasSymlink(file, fsImpl)) return null
    const stat = fsImpl.statSync(file)
    if (!stat.isFile() || stat.size > MAX_JSON_BYTES) return null
    return JSON.parse(fsImpl.readFileSync(file, "utf8"))
  } catch {
    return null
  }
}

function createRetroCatalog({
  dataDir = getDataDir(),
  fetchImpl = fetchRede,
  apiUrl = HYDRA_API,
  fsImpl = fs,
  now = () => Date.now(),
  sourcePayloads,
} = {}) {
  const cacheFile = path.join(path.resolve(String(dataDir || os.tmpdir())), CACHE_FILE_NAME)
  let memory = readCache(cacheFile, fsImpl)
  let inFlight = null

  const fetchSource = async (source) => {
    if (sourcePayloads && Object.prototype.hasOwnProperty.call(sourcePayloads, source.id)) {
      return normalizePayload(sourcePayloads[source.id], source)
    }
    try {
      const games = normalizePayload(await readJson(fetchImpl, source.url, 60000), source)
      if (games.length) return games
      const cached = readLocalSourcePayload(dataDir, source, fsImpl)
      return cached ? normalizePayload(cached, source) : games
    } catch (error) {
      const cached = readLocalSourcePayload(dataDir, source, fsImpl)
      if (cached) {
        const games = normalizePayload(cached, source)
        if (games.length) return games
      }
      throw error
    }
  }

  const refresh = async () => {
    const listing = await readJson(fetchImpl, apiUrl, 20000)
    const candidates = (Array.isArray(listing) ? listing : listing?.sources || [])
      .map(normalizeSource)
      .filter(Boolean)
      .slice(0, MAX_SOURCES)
    if (!candidates.length) throw new Error("Nenhuma fonte Classics encontrada")

    const results = await Promise.allSettled(
      candidates.map(async (source) => ({
        source,
        games: await fetchSource(source),
      })),
    )
    // Keep the metadata list even when one provider is temporarily offline;
    // the UI can still explain which tagged source was skipped.
    const sources = candidates
    const games = []
    for (const result of results) {
      if (result.status !== "fulfilled") continue
      games.push(...result.value.games)
      if (games.length >= MAX_GAMES) break
    }
    if (!games.length) {
      const error = new Error("As fontes Classics não retornaram jogos")
      error.sources = candidates
      throw error
    }
    games.sort((left, right) => {
      const title = left.title.toLowerCase().localeCompare(right.title.toLowerCase(), "en")
      return (
        title ||
        left.sourceId.localeCompare(right.sourceId, "en") ||
        left.id.localeCompare(right.id, "en")
      )
    })
    const catalog = {
      version: CACHE_VERSION,
      updatedAt: now(),
      sources,
      games: games.slice(0, MAX_GAMES),
    }
    memory = catalog
    writeCache(cacheFile, catalog, fsImpl)
    return catalog
  }

  const ensure = async (force = false) => {
    const fresh = memory && now() - Number(memory.updatedAt || 0) < CACHE_TTL_MS
    if (!force && fresh) return memory
    if (!force && memory) {
      if (!inFlight)
        inFlight = refresh()
          .catch(() => memory)
          .finally(() => {
            inFlight = null
          })
      return memory
    }
    if (!inFlight)
      inFlight = refresh().finally(() => {
        inFlight = null
      })
    return inFlight
  }

  async function list({ query = "", offset = 0, limit = 24, refresh: force = false } = {}) {
    const off = Math.max(0, Number.isSafeInteger(Number(offset)) ? Number(offset) : 0)
    const lim = Math.min(
      MAX_LIMIT,
      Math.max(1, Number.isSafeInteger(Number(limit)) ? Number(limit) : 24),
    )
    let catalog
    try {
      catalog = await ensure(Boolean(force))
    } catch (cause) {
      return {
        ok: false,
        games: [],
        sources: Array.isArray(cause?.sources) ? cause.sources : [],
        total: 0,
        offset: off,
        limit: lim,
        hasMore: false,
        error: String(cause?.message || cause),
      }
    }
    const q = String(query || "")
      .trim()
      .toLowerCase()
      .slice(0, MAX_QUERY)
    const all = q
      ? catalog.games.filter((game) => game.title.toLowerCase().includes(q))
      : catalog.games
    const games = all.slice(off, off + lim).map(({ uris, ...game }) => game)
    return {
      ok: true,
      games,
      sources: catalog.sources,
      total: all.length,
      offset: off,
      limit: lim,
      hasMore: off + games.length < all.length,
      updatedAt: catalog.updatedAt,
    }
  }

  async function getGame(id) {
    let catalog
    try {
      catalog = await ensure(false)
    } catch (cause) {
      return {
        ok: false,
        error: String(cause?.message || cause),
        sources: Array.isArray(cause?.sources) ? cause.sources : [],
      }
    }
    const key = String(id || "").trim()
    const game = catalog.games.find((candidate) => candidate.id === key)
    if (!game) return { ok: false, error: "jogo retro não encontrado", sources: catalog.sources }
    return { ok: true, game, sources: catalog.sources }
  }

  return { list, getGame, refresh, cacheFile }
}

const singleton = createRetroCatalog()
module.exports = {
  HYDRA_API,
  createRetroCatalog,
  list: singleton.list,
  getGame: singleton.getGame,
  refresh: singleton.refresh,
  normalizeSource,
  normalizeGame,
  normalizePayload,
}
