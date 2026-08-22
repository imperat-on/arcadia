"use client"

import type { RetroGame } from "../../global"

const MAX_COVER_URL = 2048

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

/**
 * Artwork is resolved before a catalog version is published. Returning only
 * catalog URLs here prevents a page of cards from launching metadata searches;
 * Chromium's persistent HTTP cache supplies the hybrid on-disk layer.
 */
export function loadRetroCovers(game: RetroGame): Promise<string[]> {
  const native = [game.cover, game.capa, game.fallbackCover]
    .map(safeCoverUrl)
    .filter((url): url is string => Boolean(url))
  if (native.length) return Promise.resolve(native)

  return Promise.resolve([])
}

export function getRetroCover(game: RetroGame): string | null {
  const native = [game.cover, game.capa, game.fallbackCover]
    .map(safeCoverUrl)
    .find((url): url is string => Boolean(url))
  if (native) return native
  return null
}

export function loadRetroCover(game: RetroGame): Promise<string | null> {
  return loadRetroCovers(game).then((covers) => covers[0] || null)
}
