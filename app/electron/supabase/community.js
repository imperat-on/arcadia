"use strict"

// Cliente da API de reviews/listas da comunidade no processo principal.
// A rede nunca é chamada pelo renderer: o módulo reaproveita o token mantido
// pelo shim de autenticação e devolve apenas dados serializáveis e sem paths.
// Leituras têm cache por conta para o launcher continuar útil offline.

const fs = require("node:fs")
const path = require("node:path")
const config = require("./config")
const { getClient } = require("./client")
const { caminhoArquivoConta } = require("./conta")

const CACHE_VERSION = 1
const DEFAULT_PAGE_SIZE = 20
const MAX_PAGE_SIZE = 100

function asPositiveInt(value, fallback, max = Number.MAX_SAFE_INTEGER) {
  const number = Number(value)
  if (!Number.isInteger(number) || number < 0) return fallback
  return Math.min(number, max)
}

function pageOptions(options = {}) {
  const limit = asPositiveInt(options.limit, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE) || DEFAULT_PAGE_SIZE
  const offset = asPositiveInt(options.offset, 0)
  return { limit, offset }
}

function queryString(values) {
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(values || {})) {
    if (value === undefined || value === null || value === "") continue
    if (typeof value === "boolean") query.set(key, value ? "true" : "false")
    else query.set(key, String(value))
  }
  const text = query.toString()
  return text ? `?${text}` : ""
}

function safeJson(value, fallback = null) {
  try {
    return JSON.parse(value)
  } catch {
    return fallback
  }
}

function readCache(file) {
  try {
    const parsed = safeJson(fs.readFileSync(file, "utf8"), null)
    if (!parsed || parsed.version !== CACHE_VERSION || typeof parsed !== "object") return {}
    return parsed
  } catch {
    return {}
  }
}

function writeCache(file, cache) {
  const dir = path.dirname(file)
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 })
  const temporary = `${file}.tmp-${process.pid}-${Date.now()}`
  try {
    fs.writeFileSync(temporary, JSON.stringify({ ...cache, version: CACHE_VERSION }), {
      encoding: "utf8",
      mode: 0o600,
    })
    fs.renameSync(temporary, file)
  } finally {
    try {
      fs.rmSync(temporary, { force: true })
    } catch {
      // A successful rename already removed the temporary file.
    }
  }
}

function normalizeError(data, status) {
  const error = typeof data?.error === "string" ? data.error : `http_${status}`
  return {
    error,
    code: typeof data?.code === "string" ? data.code : error,
    message: typeof data?.message === "string" ? data.message : error,
    request_id: typeof data?.request_id === "string" ? data.request_id : undefined,
    status,
  }
}

function reviewList(data) {
  const reviews = Array.isArray(data?.reviews)
    ? data.reviews
    : Array.isArray(data?.items)
      ? data.items
      : []
  return {
    reviews,
    items: reviews,
    pagination: data?.pagination || { limit: reviews.length, offset: 0, has_more: false },
  }
}

function collectionList(data) {
  const collections = Array.isArray(data?.collections)
    ? data.collections
    : Array.isArray(data?.lists)
      ? data.lists
      : Array.isArray(data?.items)
        ? data.items
        : []
  return {
    collections,
    lists: collections,
    items: collections,
    pagination: data?.pagination || { limit: collections.length, offset: 0, has_more: false },
  }
}

function collectionCacheKey(options) {
  const page = pageOptions(options)
  return JSON.stringify({
    limit: page.limit,
    offset: page.offset,
    mine: Boolean(options?.mine),
    owner: options?.owner || options?.user_id || "",
    visibility: options?.visibility || "",
  })
}

/**
 * Cria o cliente com dependências injetáveis para testes e para instalações
 * sem rede. A instância exportada abaixo usa o backend real do Arcadia.
 */
function createCommunityClient({
  fetchImpl = globalThis.fetch,
  baseUrl = config.url,
  authHeaders = () => getClient()._authHeaders(),
  cachePath = () => caminhoArquivoConta("community_cache.json"),
  timeoutMs = 30_000,
} = {}) {
  if (typeof fetchImpl !== "function") {
    throw new TypeError("fetch indisponivel")
  }

  function currentCachePath() {
    return typeof cachePath === "function" ? cachePath() : cachePath
  }

  function load(file = currentCachePath()) {
    return readCache(file)
  }

  function save(cache, file = currentCachePath()) {
    writeCache(file, cache)
  }

  // A request may outlive an auth event. Never write a private collection
  // response into the next account's cache (and never return stale account
  // data as a successful mutation result).
  function scopeChanged(file) {
    return file !== currentCachePath()
  }

  function accountChanged() {
    return { ok: false, error: "conta_trocada", code: "conta_trocada" }
  }

  async function request(route, { method = "GET", query, body } = {}) {
    const url = `${String(baseUrl).replace(/\/+$/, "")}${route}${queryString(query)}`
    let headers = { accept: "application/json" }
    try {
      headers = { ...headers, ...(authHeaders() || {}) }
    } catch {
      // A public list remains readable while the session is being restored.
    }
    const options = { method, headers }
    if (body !== undefined) {
      options.headers = { ...headers, "content-type": "application/json" }
      options.body = JSON.stringify(body)
    }
    if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
      options.signal = AbortSignal.timeout(timeoutMs)
    }
    let response
    try {
      response = await fetchImpl(url, options)
    } catch (error) {
      return {
        ok: false,
        offline: true,
        ...normalizeError({ error: "offline", message: String(error?.message || "rede indisponivel") }, 0),
      }
    }
    const text = await response.text()
    const data = text ? safeJson(text, { error: text }) : null
    if (!response.ok) return { ok: false, ...normalizeError(data, response.status) }
    return { ok: true, data, status: response.status }
  }

  function cachedReviews(appid, options, file) {
    const cache = load(file)
    const key = `reviews:${String(appid || "")}`
    const value = cache.reviews?.[key]
    if (!value) return null
    const page = pageOptions(options)
    return {
      ok: true,
      offline: true,
      ...value,
      pagination: value.pagination || { limit: page.limit, offset: page.offset, has_more: false },
    }
  }

  async function listReviews(appid, options = {}) {
    const file = currentCachePath()
    const page = pageOptions(options)
    const result = await request("/community/v1/reviews", {
      query: { appid: String(appid || ""), limit: page.limit, offset: page.offset },
    })
    if (scopeChanged(file)) return accountChanged()
    if (!result.ok) return cachedReviews(appid, options, file) || { ...result, reviews: [], items: [] }
    const normalized = reviewList(result.data)
    const cache = load(file)
    cache.reviews = { ...(cache.reviews || {}), [`reviews:${String(appid || "")}`]: normalized }
    save(cache, file)
    return { ok: true, ...normalized }
  }

  function cacheReview(review, file) {
    if (!review?.appid) return
    const cache = load(file)
    const key = `reviews:${String(review.appid)}`
    const old = Array.isArray(cache.reviews?.[key]?.reviews) ? cache.reviews[key].reviews : []
    const reviews = [review, ...old.filter((item) => String(item?.id) !== String(review.id))]
    cache.reviews = {
      ...(cache.reviews || {}),
      [key]: { reviews, items: reviews, pagination: cache.reviews?.[key]?.pagination },
    }
    save(cache, file)
  }

  function patchCachedReview(review, file) {
    if (!review?.appid) return
    const cache = load(file)
    const key = `reviews:${String(review.appid)}`
    const value = cache.reviews?.[key]
    if (!value) return
    const reviews = (value.reviews || []).map((item) =>
      String(item?.id) === String(review.id) ? review : item,
    )
    cache.reviews[key] = { ...value, reviews, items: reviews }
    save(cache, file)
  }

  async function createReview(payload) {
    const file = currentCachePath()
    const result = await request("/community/v1/reviews", { method: "POST", body: payload || {} })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    const review = result.data?.review || result.data?.data || result.data
    cacheReview(review, file)
    return { ok: true, review, data: review }
  }

  async function updateReview(id, payload) {
    const file = currentCachePath()
    const result = await request(`/community/v1/reviews/${encodeURIComponent(String(id || ""))}`, {
      method: "PATCH",
      body: payload || {},
    })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    const review = result.data?.review || result.data?.data || result.data
    patchCachedReview(review, file)
    return { ok: true, review, data: review }
  }

  async function removeReview(id) {
    const file = currentCachePath()
    const result = await request(`/community/v1/reviews/${encodeURIComponent(String(id || ""))}`, {
      method: "DELETE",
    })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    const cache = load(file)
    for (const value of Object.values(cache.reviews || {})) {
      if (!Array.isArray(value?.reviews)) continue
      value.reviews = value.reviews.filter((item) => String(item?.id) !== String(id))
      value.items = value.reviews
    }
    save(cache, file)
    return { ok: true }
  }

  async function reportReview(id, payload) {
    const file = currentCachePath()
    const result = await request(
      `/community/v1/reviews/${encodeURIComponent(String(id || ""))}/report`,
      { method: "POST", body: payload || {} },
    )
    if (scopeChanged(file)) return accountChanged()
    return result.ok ? { ok: true, report: result.data?.report || result.data } : result
  }

  async function listCollections(options = {}) {
    const file = currentCachePath()
    const page = pageOptions(options)
    const result = await request("/community/v1/collections", {
      query: {
        limit: page.limit,
        offset: page.offset,
        mine: options.mine ? "true" : undefined,
        owner: options.owner || options.user_id,
        visibility: options.visibility,
      },
    })
    if (scopeChanged(file)) return accountChanged()
    const key = collectionCacheKey(options)
    if (!result.ok) {
      const cached = load(file).collections?.[key]
      return cached ? { ok: true, offline: true, ...cached } : { ...result, ...collectionList({}) }
    }
    const normalized = collectionList(result.data)
    const cache = load(file)
    cache.collections = { ...(cache.collections || {}), [key]: normalized }
    save(cache, file)
    return { ok: true, ...normalized }
  }

  async function getCollection(id) {
    const file = currentCachePath()
    const key = String(id || "")
    const result = await request(`/community/v1/collections/${encodeURIComponent(key)}`)
    if (scopeChanged(file)) return accountChanged()
    if (!result.ok) {
      const cached = load(file).collectionItems?.[key]
      return cached ? { ok: true, offline: true, collection: cached, data: cached } : result
    }
    const collection = result.data?.collection || result.data?.data || result.data
    const cache = load(file)
    cache.collectionItems = { ...(cache.collectionItems || {}), [key]: collection }
    save(cache, file)
    return { ok: true, collection, data: collection }
  }

  function invalidateCollections(id, file) {
    const cache = load(file)
    if (!cache.collectionItems || typeof cache.collectionItems !== "object") cache.collectionItems = {}
    if (id !== undefined) delete cache.collectionItems[String(id)]
    delete cache.collections
    save(cache, file)
  }

  async function createCollection(payload) {
    const file = currentCachePath()
    const result = await request("/community/v1/collections", { method: "POST", body: payload || {} })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    const collection = result.data?.collection || result.data?.data || result.data
    invalidateCollections(collection?.id, file)
    return { ok: true, collection, data: collection }
  }

  async function updateCollection(id, payload) {
    const file = currentCachePath()
    const key = String(id || "")
    const result = await request(`/community/v1/collections/${encodeURIComponent(key)}`, {
      method: "PATCH",
      body: payload || {},
    })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    const collection = result.data?.collection || result.data?.data || result.data
    invalidateCollections(key, file)
    return { ok: true, collection, data: collection }
  }

  async function removeCollection(id) {
    const file = currentCachePath()
    const key = String(id || "")
    const result = await request(`/community/v1/collections/${encodeURIComponent(key)}`, { method: "DELETE" })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    invalidateCollections(key, file)
    return { ok: true }
  }

  async function collectionItems(id, method, body, appid) {
    const file = currentCachePath()
    const key = String(id || "")
    const route = `/community/v1/collections/${encodeURIComponent(key)}/items`
    const suffix = method === "DELETE" && appid !== undefined ? `/${encodeURIComponent(String(appid))}` : ""
    const result = await request(`${route}${suffix}`, { method, body })
    if (!result.ok) return result
    if (scopeChanged(file)) return accountChanged()
    invalidateCollections(key, file)
    return {
      ok: true,
      ...(result.data && typeof result.data === "object" ? result.data : {}),
      collection: result.data?.collection || result.data?.data,
      data: result.data,
    }
  }

  return {
    request,
    listReviews,
    createReview,
    updateReview,
    removeReview,
    reportReview,
    listCollections,
    getCollection,
    createCollection,
    updateCollection,
    removeCollection,
    addCollectionItem: (id, appid) => collectionItems(id, "POST", { appid }, appid),
    replaceCollectionItems: (id, items) => collectionItems(id, "PUT", { items }, undefined),
    removeCollectionItem: (id, appid) => collectionItems(id, "DELETE", undefined, appid),
    reportCollection: async (id, payload) => {
      const file = currentCachePath()
      const result = await request(`/community/v1/collections/${encodeURIComponent(String(id || ""))}/report`, {
        method: "POST",
        body: payload || {},
      })
      if (scopeChanged(file)) return accountChanged()
      return result.ok ? { ok: true, report: result.data?.report || result.data } : result
    },
  }
}

module.exports = { createCommunityClient, ...createCommunityClient() }
