"use strict"

const { catalogGet } = require("./catalog")
const { createRetroCatalogV2 } = require("./retro-catalog-v2")
const { getDataDir } = require("./runtime-paths")
const { normalizeRetroTitle } = require("./retro-title-parser")

function serverError(result) {
  return result?.error ? new Error(result.error.message || String(result.error)) : new Error("retro_server_unavailable")
}

function createRetroServerCatalog(options = {}) {
  const fallback = options.fallback || createRetroCatalogV2({ dataDir: options.dataDir || getDataDir() })
  const get = options.catalogGet || catalogGet
  const personalSources = options.sources || require("./sources")
  const personalOffers = new Map()

  async function withFallback(remote, local) {
    try {
      const result = await remote()
      if (result?.data?.ok) return result.data
      throw serverError(result)
    } catch {
      return local()
    }
  }

  async function list(params = {}) {
    const query = new URLSearchParams()
    query.set("offset", String(Math.max(0, Number(params.offset) || 0)))
    query.set("limit", String(Math.min(100, Math.max(1, Number(params.limit) || 24))))
    if (params.query) query.set("query", String(params.query).slice(0, 120))
    if (params.variants === "all") query.set("variants", "all")
    if (params.mode === "all") query.set("mode", "all")
    const system = Array.isArray(params.systems) ? params.systems[0] : params.system
    if (system) query.set("system", String(system).slice(0, 80))
    return withFallback(
      async () => get(`/catalog/v1/retro/games?${query}`),
      async () => fallback.list(params),
    )
  }

  async function getGame(gameId) {
    const id = encodeURIComponent(String(gameId || "").slice(0, 240))
    const response = await withFallback(
      async () => get(`/catalog/v1/retro/games/${id}`),
      async () => fallback.getGame(gameId),
    )
    if (!response?.ok || !response.game?.title || !personalSources?.search) return response
    try {
      const canonical = normalizeRetroTitle(response.game.title).toLowerCase()
      const candidates = await personalSources.search(response.game.title, 50)
      const local = candidates.filter((candidate) => normalizeRetroTitle(candidate.title).toLowerCase() === canonical)
      const summaries = local.map((candidate) => {
        const id = `personal:${candidate.ref}`
        personalOffers.set(id, candidate.ref)
        return {
          id,
          sourceId: String(candidate.ref).split(":")[0],
          sourceTitle: candidate.src || "Personal source",
          originalTitle: candidate.title,
          normalizedTitle: response.game.title,
          systemId: response.game.systemId,
          fileSize: candidate.fileSize || "",
          uploadDate: candidate.uploadDate || "",
          hasUris: true,
          uriCount: 1,
          personal: true,
        }
      })
      if (local.length) {
        response.offers = [...(response.offers || []), ...summaries]
        response.game.offerCount = Number(response.game.offerCount || 0) + local.length
      }
    } catch {}
    return response
  }

  async function getOffer(offerId) {
    const personalRef = personalOffers.get(String(offerId || ""))
    if (personalRef) {
      const resolved = await personalSources.getGame(personalRef)
      if (!resolved?.ok || !resolved.game) return { ok: false, error: resolved?.error || "personal_offer_unavailable" }
      const uris = Array.isArray(resolved.game.uris) ? resolved.game.uris : resolved.game.uri ? [resolved.game.uri] : []
      return {
        ok: true,
        offer: {
          id: offerId,
          sourceId: String(personalRef).split(":")[0],
          sourceTitle: resolved.source || "Personal source",
          originalTitle: resolved.game.title || "",
          normalizedTitle: normalizeRetroTitle(resolved.game.title || ""),
          fileSize: resolved.game.fileSize || "",
          uploadDate: resolved.game.uploadDate || "",
          hasUris: uris.length > 0,
          uriCount: uris.length,
          uris,
          personal: true,
        },
      }
    }
    const id = encodeURIComponent(String(offerId || "").slice(0, 80))
    // URIs are deliberately never written to catalog_espelho.
    return withFallback(
      async () => get(`/catalog/v1/retro/offers/${id}`, { noMirror: true }),
      async () => fallback.getOffer(offerId),
    )
  }

  async function audit(params = {}) {
    const query = new URLSearchParams()
    if (params.system) query.set("system", String(params.system).slice(0, 80))
    if (params.samples !== undefined) query.set("samples", String(Math.min(100, Math.max(0, Number(params.samples) || 0))))
    return withFallback(
      async () => get(`/catalog/v1/retro/audit?${query}`),
      async () => ({ ok: false, error: "retro_audit_unavailable" }),
    )
  }

  return {
    list,
    getGame,
    getOffer,
    audit,
    refresh: () => fallback.refresh(),
    migrateFromV1: () => fallback.migrateFromV1(),
    repository: fallback.repository,
  }
}

module.exports = { createRetroServerCatalog }
