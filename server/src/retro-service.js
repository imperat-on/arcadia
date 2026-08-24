"use strict"

const crypto = require("node:crypto")
const path = require("node:path")
const { loadLaunchboxIndex } = require("./launchbox-catalog")

const HYDRA_CLASSICS = "https://api.hydralibrary.com/sources?status=Classics&limit=100"
const MAX_JSON_BYTES = 64 * 1024 * 1024
const MAX_OFFERS = 100_000
const SYSTEMS = Object.freeze({
  ps1: ["sony-playstation", "Sony - PlayStation"],
  psx: ["sony-playstation", "Sony - PlayStation"],
  ps2: ["sony-playstation-2", "Sony - PlayStation 2"],
  ps3: ["sony-playstation-3", "Sony - PlayStation 3"],
  psp: ["sony-psp", "Sony - PlayStation Portable"],
  gc: ["nintendo-gamecube", "Nintendo - GameCube"],
  gcn: ["nintendo-gamecube", "Nintendo - GameCube"],
  gamecube: ["nintendo-gamecube", "Nintendo - GameCube"],
  wii: ["nintendo-wii", "Nintendo - Wii"],
  nds: ["nintendo-ds", "Nintendo - Nintendo DS"],
  ds: ["nintendo-ds", "Nintendo - Nintendo DS"],
  dsi: ["nintendo-dsi", "Nintendo - Nintendo DSi"],
  nes: ["nintendo-nes", "Nintendo - Nintendo Entertainment System"],
  snes: ["nintendo-snes", "Nintendo - Super Nintendo Entertainment System"],
  gb: ["nintendo-game-boy", "Nintendo - Game Boy"],
  gbc: ["nintendo-game-boy-color", "Nintendo - Game Boy Color"],
  gba: ["nintendo-game-boy-advance", "Nintendo - Game Boy Advance"],
  n64: ["nintendo-64", "Nintendo - Nintendo 64"],
})

function hash(value, length = 24) {
  return crypto.createHash("sha256").update(String(value)).digest("hex").slice(0, length)
}

function safeHttps(value) {
  try {
    const url = new URL(String(value || "").trim())
    if (url.protocol !== "https:" || url.username || url.password) return ""
    if (/^(localhost|127\.|10\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.)/i.test(url.hostname)) return ""
    return url.toString()
  } catch {
    return ""
  }
}

function safeArtworkList(value, limit = 8) {
  const values = Array.isArray(value) ? value : value ? [value] : []
  return [...new Set(values.map(safeHttps).filter(Boolean))].slice(0, limit)
}

function sourceArtwork(item) {
  const raw = item?.artwork && typeof item.artwork === "object" ? item.artwork : item
  const screenshots = safeArtworkList(raw?.screenshots || raw?.screens || raw?.images)
  const backgrounds = safeArtworkList(raw?.backgrounds || raw?.hero || raw?.backdrop)
  const logos = safeArtworkList(raw?.logos || raw?.logo, 4)
  const titleScreens = safeArtworkList(raw?.titleScreens || raw?.titleScreen, 4)
  const cover = safeHttps(raw?.cover || raw?.image || raw?.thumbnail)
  return {
    ...(cover ? { cover } : {}),
    ...(screenshots.length ? { screenshots } : {}),
    ...(backgrounds.length ? { backgrounds } : {}),
    ...(logos.length ? { logos } : {}),
    ...(titleScreens.length ? { titleScreens } : {}),
    ...(String(raw?.description || item?.description || "").trim() ? { description: String(raw?.description || item?.description).trim().slice(0, 4000) } : {}),
  }
}

function normalizeSystem(value) {
  const key = String(value || "").trim().toLowerCase()
  return SYSTEMS[key] || null
}

function normalizeTitle(value) {
  return String(value || "")
    .normalize("NFKC")
    .replace(/<[^>]*>/g, " ")
    .replace(/\([^)]*(?:password|senha|pass\s*:|psxroms\.pro)[^)]*\)/gi, " ")
    .replace(/\b(?:EUR|USA|JPN|RUS|ENG|MULTI\d*|PAL|NTSC(?:-[UJ])?)\b/gi, " ")
    .replace(/\s*[\[(][^\])]*(?:USA|Europe|Japan|World|PAL|NTSC|English|\b(?:En|Fr|De|Es|It)\b|Beta|Rev(?:ision)?)[^\])]*[\])]/gi, " ")
    .replace(/\[[^\]]*\]|\([^)]*(?:region|language|disc|disk|rev|version|edition|rutracker|рус|перевод)[^)]*\)/gi, " ")
    .replace(/[[(]\s*(?:USA|Europe|Japan|World|En|English|Rev[^\])]*|Disc[^\])]*|Disk[^\])]*)\s*[\])]/gi, " ")
    .replace(/\(\s*\)|\[\s*\]/g, " ")
    .replace(/\.(?:iso|chd|cue|bin|pkg|zip|7z|rar)\b/gi, " ")
    .replace(/[_]+/g, " ")
    .replace(/\s+/g, " ")
    .replace(/^[\s.,;:_-]+|[\s.,;:_-]+$/g, "")
    .trim()
    .slice(0, 240)
}

function matchKey(value) {
  return normalizeTitle(value)
    .normalize("NFKD")
    .replace(/\p{Mark}/gu, "")
    .replace(/\b(?:the|a|an)\b/gi, " ")
    .replace(/[^a-z0-9]+/gi, " ")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase()
}

function releaseTitleCandidates(value) {
  const original = normalizeTitle(value)
  const candidates = [original]
  let relaxed = original
  // Feeds No-Intro/Redump append region, languages, revision, beta and dump
  // qualifiers as trailing groups. LaunchBox stores the editorial title only.
  // Remove one group at a time so the first valid, platform-scoped alias wins.
  for (let index = 0; index < 12; index++) {
    const next = relaxed.replace(/\s*(?:\([^()[\]]{1,120}\)|\[[^()[\]]{1,120}\])\s*$/u, "").trim()
    if (!next || next === relaxed) break
    candidates.push(next)
    relaxed = next
  }
  return [...new Set(candidates.map(matchKey).filter(Boolean))]
}

// A vitrine promete inglês. Sem um registro canônico inglês, não promovemos
// nomes cirílicos/CJK do feed a título editorial; eles continuam na oferta.
function isEnglishDisplayTitle(value) {
  const title = String(value || "")
  if (!title || /[\p{Script=Cyrillic}\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(title)) return false
  return /[A-Za-z]/.test(title)
}

function libretroFilename(title) {
  return String(title).replace(/[&*/:`<>?\\|]/g, "_")
}

function libretroCover(collection, title) {
  const repository = collection.replace(/ /g, "_")
  const filename = `${libretroFilename(title)}.png`
  return `https://raw.githubusercontent.com/libretro-thumbnails/${encodeURIComponent(repository)}/master/Named_Boxarts/${encodeURIComponent(filename)}`
}

function libretroThumbnail(collection, title, type) {
  const repository = collection.replace(/ /g, "_")
  const filename = `${libretroFilename(title)}.png`
  return `https://raw.githubusercontent.com/libretro-thumbnails/${encodeURIComponent(repository)}/master/${type}/${encodeURIComponent(filename)}`
}

function mergeCanonicalArtwork(primary = {}, secondary = {}) {
  const merged = { ...secondary, ...primary }
  for (const field of ["screenshots", "titleScreens", "backgrounds", "logos"]) {
    merged[field] = [...new Set([...(secondary[field] || []), ...(primary[field] || [])])].slice(0, 8)
  }
  if (!merged.cover) merged.cover = secondary.cover || primary.cover || ""
  return merged
}

function mergeCanonicalCandidate(existing, incoming) {
  if (!existing) return incoming
  const preferIncoming = incoming.provider === "launchbox" || existing.provider !== "launchbox"
  const selected = preferIncoming ? incoming : existing
  const other = preferIncoming ? existing : incoming
  return { ...other, ...selected, cover: selected.cover || other.cover || "", artwork: mergeCanonicalArtwork(selected.artwork, other.artwork) }
}

function normalizeUri(value) {
  const uri = String(value || "").trim()
  if (!uri || uri.length > 8192 || /[\u0000-\u001f\u007f]/.test(uri)) return ""
  if (/^magnet:\?/i.test(uri) && /(?:^|&)xt=urn:btih:[a-z0-9]+/i.test(uri.slice(uri.indexOf("?") + 1))) return uri
  return safeHttps(uri)
}

function isIndividualGame(title) {
  const value = String(title || "")
  return !(
    /\[\s*\d{1,3}\s+in\s+1\s*\]/i.test(value) ||
    /\b(?:bios|firmware|update|dlc|soundtrack|trainer|cheat\s*pack|rom\s*set|complete\s+collection|collection|anthology|anthologies|trilog(?:y|ia)|quadrilog(?:y|ia)|compilation|compil(?:e|ed)|bundle|game\s*pack|mega\s*pack|multi\s*pack|saga|best\s+of|duology|dilog(?:y|ia)|beta|prototype|proto|demo|alpha|preview|pre[- ]?release|test\s*(?:release|build)?|sample|debug|kiosk)\b/i.test(value) ||
    /(?:collection|recollection)/i.test(value) ||
    /(?:^|[\s/])(?:\d+\s*[-/]?\s*(?:in|en)\s*[-/]?\s*\d+|\d+\s+games?\s+in\s+\d+|\d+\s+games?\s+pack|multi\s*\d+)(?:$|[\s/])/i.test(value) ||
    /^\d+\s+.*\b(?:in|en)\s*[-/]?\s*\d+\b/i.test(value) ||
    /^\d+\s+.*\bgames?\b/i.test(value)
  )
}

function buildCatalog(sourcePayloads, options = {}) {
  const canonicalBySystem = options.canonicalBySystem || new Map()
  const groups = new Map()
  const unmatched = []
  let seen = 0
  for (const entry of sourcePayloads) {
    const source = entry.source || {}
    for (const item of entry.payload?.downloads || []) {
      if (++seen > MAX_OFFERS) break
      const originalTitle = String(item?.title || "").trim().slice(0, 1000)
      const title = normalizeTitle(originalTitle)
      const system = normalizeSystem(item?.platform)
      const uris = [...new Set((Array.isArray(item?.uris) ? item.uris : [item?.uri]).map(normalizeUri).filter(Boolean))].slice(0, 32)
      if (!system || !title || !uris.length || !isIndividualGame(originalTitle)) {
        unmatched.push({ title: originalTitle, platform: String(item?.platform || "").slice(0, 80), reason: !system ? "system" : !uris.length ? "uri" : "release-kind" })
        continue
      }
      const [systemId, collection] = system
      const normalizedKey = matchKey(title)
      const canonicalIndex = canonicalBySystem.get(systemId)
      let canonical = canonicalIndex?.get(normalizedKey)
      if (!canonical && canonicalIndex) {
        for (const candidateKey of releaseTitleCandidates(title).slice(1)) {
          canonical = canonicalIndex.get(candidateKey)
          if (canonical) break
        }
      }
      if (canonical && !isIndividualGame(canonical.title)) canonical = null
      if (!canonical && !isEnglishDisplayTitle(title)) {
        unmatched.push({ title: originalTitle, platform: String(item?.platform || "").slice(0, 80), systemId, reason: "english-title" })
        continue
      }
      const candidateSourceCover = safeHttps(item?.cover || item?.image || item?.thumbnail)
      const candidateSourceArtwork = sourceArtwork(item)
      if (options.requireArtwork && !canonical && !candidateSourceCover) {
        unmatched.push({ title: originalTitle, platform: String(item?.platform || "").slice(0, 80), systemId, reason: "canonical-artwork" })
        continue
      }
      const displayTitle = canonical?.title || title
      const canonicalIdentity = canonical?.provider && canonical?.providerId
        ? `${canonical.provider}:${canonical.providerId}`
        : `slug:${hash(displayTitle.toLowerCase(), 12)}`
      const key = `${systemId}\0${canonicalIdentity}`
      if (!groups.has(key)) groups.set(key, {
        systemId,
        collection,
        title: displayTitle,
        canonicalIdentity,
        canonicalProvider: canonical?.provider || (canonical ? "libretro" : "source"),
        canonicalCover: canonical?.cover || "",
        canonicalArtwork: canonical?.artwork || {},
        offers: [],
      })
      const sourceId = String(source.id || hash(source.url || source.title, 12))
      const offerId = hash(`${sourceId}\0${originalTitle}\0${[...uris].sort().join("\0")}`)
      groups.get(key).offers.push({
        id: offerId,
        sourceId,
        sourceTitle: String(source.title || source.name || "Hydra Classics").slice(0, 160),
        originalTitle,
        normalizedTitle: title,
        systemId,
        uris,
        metadata: {
          fileSize: String(item?.fileSize || "").slice(0, 80),
          uploadDate: String(item?.uploadDate || "").slice(0, 80),
          region: String(item?.region || "").slice(0, 40),
        },
        sourceCover: candidateSourceCover,
        sourceArtwork: candidateSourceArtwork,
      })
    }
  }

  const games = []
  const offers = []
  for (const group of groups.values()) {
    const gameId = `retro:${group.systemId}:${group.canonicalIdentity}`
    const sourceCover = group.offers.find((offer) => offer.sourceCover)?.sourceCover
    const sourceArtwork = group.offers.reduce((acc, offer) => mergeCanonicalArtwork(acc, offer.sourceArtwork || {}), {})
    const artwork = mergeCanonicalArtwork(group.canonicalArtwork, sourceArtwork)
    games.push({
      id: gameId,
      systemId: group.systemId,
      title: group.title,
      titleLocale: "en",
      sortTitle: group.title.toLocaleLowerCase("en-US"),
      aliases: [...new Set(group.offers.map((offer) => offer.originalTitle).filter((alias) => alias !== group.title))].slice(0, 30),
      artwork: group.canonicalCover || sourceCover || Object.keys(artwork).length
        ? {
            ...artwork,
            cover: group.canonicalCover || artwork.cover || sourceCover,
            provider: group.canonicalCover ? group.canonicalProvider : "source",
          }
        : { provider: null },
      offerCount: group.offers.length,
      matchQuality: "strong",
    })
    for (const offer of group.offers) offers.push({ ...offer, gameId })
  }
  games.sort((a, b) => a.sortTitle.localeCompare(b.sortTitle, "en") || a.id.localeCompare(b.id))
  return { games, offers, unmatched }
}

function titleFromThumbnailPath(pathname) {
  const match = /^Named_Boxarts\/(.+)\.png$/i.exec(String(pathname || ""))
  if (!match) return null
  try { return decodeURIComponent(match[1]) } catch { return match[1] }
}

function regionPriority(value) {
  const title = String(value || "")
  if (/\((?:USA|World)(?:,|\))/i.test(title)) return 5
  if (/\(Europe(?:,|\))/i.test(title)) return 4
  if (!/\([^)]*(?:Japan|Korea|China|Asia)[^)]*\)/i.test(title)) return 3
  if (/\(Japan(?:,|\))/i.test(title)) return 1
  return 0
}

async function fetchCanonicalIndexes(fetchImpl = fetch) {
  const unique = [...new Map(Object.values(SYSTEMS).map(([id, collection]) => [id, collection])).entries()]
  const result = new Map()
  let cursor = 0
  async function worker() {
    while (cursor < unique.length) {
      const [systemId, collection] = unique[cursor++]
      const repository = collection.replace(/ /g, "_")
      try {
        const tree = await fetchJson(`https://api.github.com/repos/libretro-thumbnails/${encodeURIComponent(repository)}/git/trees/master?recursive=1`, 60_000, fetchImpl)
        const index = new Map()
      for (const item of tree?.tree || []) {
          const match = /^(Named_Boxarts|Named_Snaps|Named_Titles|Named_Logos)\/(.+)\.png$/i.exec(String(item?.path || ""))
          if (!match) continue
          let title
          try { title = decodeURIComponent(match[2]) } catch { title = match[2] }
          if (!title || !isEnglishDisplayTitle(title)) continue
          if (!isIndividualGame(title)) continue
          const normalized = matchKey(title)
          if (!normalized) continue
          const previous = index.get(normalized)
          const priority = regionPriority(title)
          const candidate = previous || { title: normalizeTitle(title), cover: "", priority: -1, artwork: { provider: "libretro", screenshots: [], titleScreens: [], logos: [] } }
          if (match[1] === "Named_Boxarts" && (!candidate.cover || priority > candidate.priority)) {
            candidate.cover = libretroCover(collection, title)
            candidate.priority = priority
          }
          const artworkField = { Named_Snaps: "screenshots", Named_Titles: "titleScreens", Named_Logos: "logos" }[match[1]]
          if (artworkField) {
            const url = libretroThumbnail(collection, title, match[1])
            if (!candidate.artwork[artworkField].includes(url)) candidate.artwork[artworkField].push(url)
            candidate.artwork[artworkField] = candidate.artwork[artworkField].slice(0, 8)
          }
          index.set(normalized, candidate)
        }
        if (index.size) result.set(systemId, index)
      } catch {
        // The public GitHub API is an enrichment. Source artwork and the last
        // published catalog remain valid when it is unavailable/rate-limited.
      }
    }
  }
  await Promise.all(Array.from({ length: 4 }, worker))
  return result
}

async function fetchJson(url, timeoutMs = 30_000, fetchImpl = fetch) {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const response = await fetchImpl(url, { headers: { accept: "application/json", "user-agent": "Arcadia-Retro/2" }, redirect: "error", signal: controller.signal })
    if (!response.ok) throw new Error(`HTTP ${response.status}`)
    const length = Number(response.headers.get("content-length") || 0)
    if (length > MAX_JSON_BYTES) throw new Error("retro_payload_too_large")
    const text = await response.text()
    if (Buffer.byteLength(text) > MAX_JSON_BYTES) throw new Error("retro_payload_too_large")
    return JSON.parse(text)
  } finally {
    clearTimeout(timer)
  }
}

let syncInFlight
async function syncRetroCatalog({ fetchImpl = fetch, registryUrl = HYDRA_CLASSICS } = {}) {
  if (syncInFlight) return syncInFlight
  syncInFlight = (async () => {
    const { db, withTransaction } = require("./db")
    const { DATA_DIR } = require("./db")
    const now = Math.floor(Date.now() / 1000)
    const launchboxPromise = loadLaunchboxIndex({
      cacheDir: path.join(DATA_DIR, "retro", "launchbox"),
      matchKey,
      fetchImpl,
    }).catch((error) => ({ gamesBySystem: new Map(), stats: {}, stale: true, error: error.message }))
    const libretroCachePrefix = "retro:libretro:v3:"
    const cachedRows = (await db.query("SELECT key, data, at FROM catalog_cache WHERE key LIKE $1", [`${libretroCachePrefix}%`])).rows
    const cachedCanonical = new Map()
    let canonicalFresh = cachedRows.length >= new Set(Object.values(SYSTEMS).map(([, collection]) => collection)).size
    for (const row of cachedRows) {
      try {
        const systemId = row.key.slice(libretroCachePrefix.length)
        const entries = JSON.parse(row.data)
        if (Array.isArray(entries) && entries.length) cachedCanonical.set(systemId, new Map(entries))
        if (now - Number(row.at || 0) > 7 * 24 * 60 * 60) canonicalFresh = false
      } catch { canonicalFresh = false }
    }

    const registry = await fetchJson(registryUrl, 20_000, fetchImpl)
    const rawSources = Array.isArray(registry) ? registry : registry?.sources || []
    const sources = rawSources.map((source) => ({ ...source, url: safeHttps(source?.url || source?.link) })).filter((source) => source.url).slice(0, 100)
    const settled = await Promise.allSettled(sources.map(async (source) => {
      const sourceId = String(source.id || hash(source.url, 12)).slice(0, 80)
      const cacheKey = `retro:source:${sourceId}`
      let lastError
      for (let attempt = 0; attempt < 2; attempt++) {
        try {
          const payload = await fetchJson(source.url, 60_000, fetchImpl)
          await db.query(
            `INSERT INTO catalog_cache (key, data, at) VALUES ($1,$2,$3)
             ON CONFLICT (key) DO UPDATE SET data = excluded.data, at = excluded.at`,
            [cacheKey, JSON.stringify(payload), now],
          )
          return { source, payload, mode: "live" }
        } catch (error) {
          lastError = error
        }
      }
      const cached = (await db.query("SELECT data FROM catalog_cache WHERE key = $1", [cacheKey])).rows[0]
      if (cached) return { source, payload: JSON.parse(cached.data), mode: "cache" }
      throw lastError || new Error("retro_source_unavailable")
    }))
    const payloads = settled.filter((result) => result.status === "fulfilled").map((result) => result.value)
    if (!payloads.length) throw new Error("retro_sources_unavailable")
    let canonicalBySystem = cachedCanonical
    if (!canonicalFresh) {
      const fetched = await fetchCanonicalIndexes(fetchImpl)
      canonicalBySystem = new Map(cachedCanonical)
      for (const [systemId, index] of fetched) canonicalBySystem.set(systemId, index)
      if ([...canonicalBySystem.values()].reduce((sum, index) => sum + index.size, 0) < 10_000) {
        throw new Error("retro_canonical_index_incomplete")
      }
      for (const [systemId, index] of fetched) {
        await db.query(
          `INSERT INTO catalog_cache (key, data, at) VALUES ($1,$2,$3)
           ON CONFLICT (key) DO UPDATE SET data = excluded.data, at = excluded.at`,
          [`${libretroCachePrefix}${systemId}`, JSON.stringify([...index.entries()]), now],
        )
      }
    }
    const launchbox = await launchboxPromise
    for (const [systemId, launchboxIndex] of launchbox.gamesBySystem) {
      const combined = new Map(canonicalBySystem.get(systemId) || [])
      for (const [key, candidate] of launchboxIndex) {
        if (!isIndividualGame(candidate?.title)) continue
        combined.set(key, mergeCanonicalCandidate(combined.get(key), candidate))
      }
      canonicalBySystem.set(systemId, combined)
    }
    const catalog = buildCatalog(payloads, { canonicalBySystem })
    if (!catalog.games.length) throw new Error("retro_catalog_empty")
    const version = `${Date.now()}-${hash(JSON.stringify(catalog.games.map((game) => [game.id, game.offerCount])), 12)}`

    await withTransaction(async (client) => {
      await client.query("INSERT INTO retro_catalog_versions (version, status, source_meta) VALUES ($1, 'building', $2)", [version, JSON.stringify({
        sources: sources.length,
        succeeded: payloads.length,
        live: payloads.filter((entry) => entry.mode === "live").length,
        cached: payloads.filter((entry) => entry.mode === "cache").length,
        failed: settled.length - payloads.length,
        launchbox: {
          games: Number(launchbox.stats?.games) || 0,
          aliases: Number(launchbox.stats?.aliases) || 0,
          artwork: Number(launchbox.stats?.artwork) || 0,
          stale: Boolean(launchbox.stale),
          error: launchbox.error || null,
        },
      })])
      await client.query(
        `INSERT INTO retro_games (version, game_id, system_id, title, title_locale, sort_title, search_text, aliases, artwork, offer_count, match_quality)
         SELECT $1, x.game_id, x.system_id, x.title, x.title_locale, x.sort_title,
                x.search_text, x.aliases, x.artwork, x.offer_count, x.match_quality
           FROM jsonb_to_recordset($2::jsonb) AS x(
             game_id text, system_id text, title text, title_locale text,
             sort_title text, search_text text, aliases jsonb, artwork jsonb,
             offer_count integer, match_quality text)`,
        [version, JSON.stringify(catalog.games.map((game) => ({
          game_id: game.id,
          system_id: game.systemId,
          title: game.title,
          title_locale: game.titleLocale,
          sort_title: game.sortTitle,
          search_text: `${game.title} ${game.aliases.join(" ")}`,
          aliases: game.aliases,
          artwork: game.artwork,
          offer_count: game.offerCount,
          match_quality: game.matchQuality,
        })))],
      )
      await client.query(
        `INSERT INTO retro_offers (version, offer_id, game_id, source_id, source_title, original_title, normalized_title, system_id, metadata, uris)
         SELECT $1, x.offer_id, x.game_id, x.source_id, x.source_title,
                x.original_title, x.normalized_title, x.system_id, x.metadata, x.uris
           FROM jsonb_to_recordset($2::jsonb) AS x(
             offer_id text, game_id text, source_id text, source_title text,
             original_title text, normalized_title text, system_id text,
             metadata jsonb, uris jsonb)`,
        [version, JSON.stringify(catalog.offers.map((offer) => ({
          offer_id: offer.id,
          game_id: offer.gameId,
          source_id: offer.sourceId,
          source_title: offer.sourceTitle,
          original_title: offer.originalTitle,
          normalized_title: offer.normalizedTitle,
          system_id: offer.systemId,
          metadata: offer.metadata,
          uris: offer.uris,
        })))],
      )
      const unmatchedByReason = {}
      const unmatchedBySystem = {}
      for (const item of catalog.unmatched) {
        unmatchedByReason[item.reason] = (unmatchedByReason[item.reason] || 0) + 1
        const system = item.systemId || String(item.platform || "unknown")
        unmatchedBySystem[system] = (unmatchedBySystem[system] || 0) + 1
      }
      const unmatchedSamples = catalog.unmatched.slice(0, 200).map((item) => ({
        title: String(item.title || "").slice(0, 240),
        platform: String(item.platform || "").slice(0, 80),
        systemId: item.systemId || null,
        reason: item.reason,
      }))
      const stats = { games: catalog.games.length, offers: catalog.offers.length, unmatched: catalog.unmatched.length, unmatchedByReason, unmatchedBySystem, unmatchedSamples }
      await client.query("UPDATE retro_catalog_versions SET status = 'active', stats = $2 WHERE version = $1", [version, JSON.stringify(stats)])
      await client.query("UPDATE retro_catalog_versions SET status = 'superseded' WHERE status = 'active' AND version <> $1", [version])
      await client.query("UPDATE retro_catalog_state SET active_version = $1, updated_at = now() WHERE singleton", [version])
    })
    // Keep one rollback version and bound database growth.
    await db.query(`DELETE FROM retro_catalog_versions WHERE version IN (SELECT version FROM retro_catalog_versions WHERE version <> $1 ORDER BY generated_at DESC OFFSET 1)`, [version])
    return { ok: true, version, games: catalog.games.length, offers: catalog.offers.length, unmatched: catalog.unmatched.length }
  })().finally(() => { syncInFlight = null })
  return syncInFlight
}

async function activeVersion() {
  const { db } = require("./db")
  return (await db.query("SELECT active_version FROM retro_catalog_state WHERE singleton")).rows[0]?.active_version || null
}

module.exports = { SYSTEMS, normalizeSystem, normalizeTitle, matchKey, releaseTitleCandidates, isEnglishDisplayTitle, isIndividualGame, libretroFilename, libretroCover, libretroThumbnail, titleFromThumbnailPath, fetchCanonicalIndexes, buildCatalog, syncRetroCatalog, activeVersion }
