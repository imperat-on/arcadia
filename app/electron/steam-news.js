"use strict"

const STEAM_NEWS_HOSTS = new Set([
  "steamstore-a.akamaihd.net",
  "store.steampowered.com",
  "steamcommunity.com",
])

function decodeAttribute(value) {
  return String(value || "")
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
}

function normalizeSteamImageUrl(value) {
  const raw = decodeAttribute(value).trim()
  if (!raw) return ""
  const expanded = raw.replace(
    /^\{STEAM_CLAN_IMAGE\}\/+(\d+)\/(.+)$/i,
    "https://clan.cloudflare.steamstatic.com/images/$1/$2",
  )
  const absolute = expanded.startsWith("//") ? `https:${expanded}` : expanded
  try {
    const url = new URL(absolute)
    return url.protocol === "https:" ? url.href : ""
  } catch {
    return ""
  }
}

function extractAttribute(tag, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
  const match = new RegExp(`\\b${escaped}\\s*=\\s*(?:["']([^"']*)["']|([^\\s>]+))`, "i").exec(tag)
  return decodeAttribute(match?.[1] || match?.[2] || "")
}

function extractSteamNewsImage(markup) {
  const text = String(markup || "")
  const bbcode =
    /\[img(?:\s+src\s*=\s*["']?|\s*=\s*["']?)?([^\]"']+)["']?\]\s*\[\/img\]/i.exec(text)?.[1] ||
    /\[previewimg(?:\s*=\s*["']?)?([^\]"']+)["']?\]/i.exec(text)?.[1]
  const normalizedBbcode = normalizeSteamImageUrl(bbcode)
  if (normalizedBbcode) return normalizedBbcode

  for (const tag of text.match(/<(?:meta|link)\b[^>]*>/gi) || []) {
    const type = (extractAttribute(tag, "property") || extractAttribute(tag, "name") || extractAttribute(tag, "rel")).toLowerCase()
    if (!["og:image", "og:image:url", "twitter:image", "twitter:image:src", "image_src"].includes(type)) continue
    const image = normalizeSteamImageUrl(extractAttribute(tag, "content") || extractAttribute(tag, "href"))
    if (image) return image
  }

  const htmlImage = /<img\b[^>]*\bsrc\s*=\s*(?:["']([^"']+)["']|([^\s>]+))/i.exec(text)
  return normalizeSteamImageUrl(htmlImage?.[1] || htmlImage?.[2] || "")
}

function isSteamNewsUrl(value) {
  try {
    const url = new URL(String(value || ""))
    return url.protocol === "https:" && STEAM_NEWS_HOSTS.has(url.hostname.toLowerCase())
  } catch {
    return false
  }
}

function createSteamNewsImageResolver({ fetchImpl, ttlMs = 6 * 60 * 60 * 1000 } = {}) {
  const cache = new Map()
  const inFlight = new Map()

  return async function resolveSteamNewsImage(item) {
    const inline = extractSteamNewsImage(item?.contents)
    if (inline) return inline

    const pageUrl = String(item?.url || "")
    if (!isSteamNewsUrl(pageUrl) || typeof fetchImpl !== "function") return ""
    const cached = cache.get(pageUrl)
    if (cached && cached.expires > Date.now()) return cached.image
    if (inFlight.has(pageUrl)) return inFlight.get(pageUrl)

    const request = Promise.resolve()
      .then(async () => {
        const options = { headers: { "User-Agent": "arcadia", Accept: "text/html" } }
        if (typeof AbortSignal?.timeout === "function") options.signal = AbortSignal.timeout(8000)
        const response = await fetchImpl(pageUrl, options)
        if (!response?.ok) return ""
        return extractSteamNewsImage(await response.text())
      })
      .catch(() => "")
      .then((image) => {
        cache.set(pageUrl, { image, expires: Date.now() + ttlMs })
        return image
      })
      .finally(() => inFlight.delete(pageUrl))

    inFlight.set(pageUrl, request)
    return request
  }
}

module.exports = {
  createSteamNewsImageResolver,
  extractSteamNewsImage,
  isSteamNewsUrl,
  normalizeSteamImageUrl,
}
