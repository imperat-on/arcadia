"use client"

import type { RetroGame } from "../../global"

type CoverCandidate = { url?: unknown; thumb?: unknown }

const MAX_COVER_URL = 2048
const MAX_ACTIVE_LOOKUPS = 3
const coverCache = new Map<string, Promise<string[]>>()
const resolvedCoverCache = new Map<string, string[]>()
const queue: Array<{
  run: () => Promise<string[]>
  resolve: (value: string[]) => void
}> = []
let activeLookups = 0

function drainQueue() {
  while (activeLookups < MAX_ACTIVE_LOOKUPS && queue.length) {
    const job = queue.shift()
    if (!job) break
    activeLookups++
    job
      .run()
      .catch(() => [])
      .then(job.resolve)
      .finally(() => {
        activeLookups--
        drainQueue()
      })
  }
}

function scheduleCover(run: () => Promise<string[]>) {
  return new Promise<string[]>((resolve) => {
    queue.push({ run, resolve })
    drainQueue()
  })
}

function isPrivateCoverHost(hostname: string) {
  const host = hostname.toLowerCase().replace(/^\[|\]$/g, "")
  if (!host || host === "localhost" || host.endsWith(".localhost") || host === "::1") return true
  if (host.includes(":")) return true
  if (/^(0|10|127)\./.test(host) || /^192\.168\./.test(host)) return true
  if (/^169\.254\./.test(host) || /^172\.(1[6-9]|2\d|3[0-1])\./.test(host)) return true
  return false
}

function safeCoverUrl(value: unknown) {
  if (typeof value !== "string" || value.length > MAX_COVER_URL) return null
  try {
    const url = new URL(value.trim())
    if (
      url.protocol !== "https:" ||
      url.username ||
      url.password ||
      url.hash ||
      isPrivateCoverHost(url.hostname)
    )
      return null
    return url.toString()
  } catch {
    return null
  }
}

function coverKey(game: RetroGame) {
  return [game.title, game.platform || ""].join("\u001f").toLowerCase()
}

/**
 * Busca arte apenas quando o feed não trouxe uma capa. A busca usa a bridge
 * existente (IGDB/Xbox/PS Store/Steam), nunca rede no renderer; cache e fila
 * impedem que uma página de 24 cards dispare dezenas de requisições simultâneas.
 */
export function loadRetroCovers(game: RetroGame) {
  const native = [game.cover, game.capa, game.fallbackCover]
    .map(safeCoverUrl)
    .filter((url): url is string => Boolean(url))
  if (native.length) return Promise.resolve(native)

  const key = coverKey(game)
  const cached = coverCache.get(key)
  if (cached) return cached

  const pending = scheduleCover(async () => {
    const search = window.launcherAPI?.searchArt
    if (!search) return []
    const response = await search(game.id, game.title, "cover", ["600x900"])
    const candidates: string[] = []
    for (const candidate of response?.candidatos || []) {
      const url =
        safeCoverUrl((candidate as CoverCandidate).url) ||
        safeCoverUrl((candidate as CoverCandidate).thumb)
      if (url && !candidates.includes(url)) candidates.push(url)
    }
    return candidates
  })
  const tracked = pending.then((covers) => {
    resolvedCoverCache.set(key, covers)
    return covers
  })
  coverCache.set(key, tracked)
  return tracked
}

export function getRetroCover(game: RetroGame) {
  const native = [game.cover, game.capa, game.fallbackCover]
    .map(safeCoverUrl)
    .find((url): url is string => Boolean(url))
  if (native) return native
  return resolvedCoverCache.get(coverKey(game))?.[0] || null
}

export function loadRetroCover(game: RetroGame) {
  return loadRetroCovers(game).then((covers) => covers[0] || null)
}
