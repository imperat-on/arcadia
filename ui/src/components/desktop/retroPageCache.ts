import { getRetroCover } from "./retroArtwork"

type RetroList = Window["launcherAPI"]["retroList"]
type Params = NonNullable<Parameters<RetroList>[0]>
type Page = Awaited<ReturnType<RetroList>>

const PAGE_TTL_MS = 60_000
const MAX_STALE_MS = 10 * 60_000
const MAX_PAGES = 24
const pages = new Map<string, { page: Page; at: number }>()
const pendingPages = new Map<string, Promise<Page>>()

function pageKey(params: Params, account: string) {
  return JSON.stringify([account, params.query || "", params.system || "", params.systems || [], params.variants || "retail", params.mode || "essentials", params.offset || 0, params.limit || 24])
}

export function getCachedRetroPage(params: Params, account: string, allowStale = false): Page | undefined {
  const key = pageKey(params, account)
  const cached = pages.get(key)
  if (!cached) return undefined
  const age = Date.now() - cached.at
  if (age >= MAX_STALE_MS) {
    pages.delete(key)
    return undefined
  }
  if (!allowStale && age >= PAGE_TTL_MS) return undefined
  return cached.page
}

export function loadRetroPage(params: Params, account: string): Promise<Page> {
  const cached = getCachedRetroPage(params, account)
  if (cached) {
    preloadRetroCovers(cached.games || [])
    return Promise.resolve(cached)
  }
  const key = pageKey(params, account)
  const pending = pendingPages.get(key)
  if (pending) return pending
  const request = Promise.resolve().then(() => window.launcherAPI.retroList(params)).then(page => {
    if (page?.ok && Array.isArray(page.games)) {
      pages.delete(key)
      pages.set(key, { page, at: Date.now() })
      while (pages.size > MAX_PAGES) pages.delete(pages.keys().next().value)
      preloadRetroCovers(page.games)
    }
    return page
  }).finally(() => {
    if (pendingPages.get(key) === request) pendingPages.delete(key)
  })
  pendingPages.set(key, request)
  return request
}

export function prefetchRetroPage(params: Params, account: string) {
  void loadRetroPage(params, account).catch(() => {})
}

// Image usa o cache HTTP persistente do WebView sem exigir CORS dos CDNs.
// A fila e limitada: nada de baixar o catalogo inteiro ou reter bitmaps gigantes.
const MAX_WARM_COVERS = 64
const MAX_QUEUED_COVERS = 16
const COVER_TTL_MS = 5 * 60_000
const warmCovers = new Map<string, number>()
const pendingCovers = new Set<string>()
const coverQueue: Array<() => void> = []
let activeCovers = 0

function warmCover(url: string) {
  if (typeof Image === "undefined" || pendingCovers.has(url)) return
  const loadedAt = warmCovers.get(url)
  if (loadedAt !== undefined && Date.now() - loadedAt < COVER_TTL_MS) return
  if (pendingCovers.size >= MAX_QUEUED_COVERS) return
  pendingCovers.add(url)
  const start = () => {
    activeCovers++
    let image: HTMLImageElement | undefined
    let timer: ReturnType<typeof setTimeout>
    let done = false
    const finish = (loaded: boolean) => {
      if (done) return
      done = true
      clearTimeout(timer)
      if (image) {
        image.onload = null
        image.onerror = null
      }
      pendingCovers.delete(url)
      if (loaded) {
        warmCovers.delete(url)
        warmCovers.set(url, Date.now())
        while (warmCovers.size > MAX_WARM_COVERS) warmCovers.delete(warmCovers.keys().next().value)
      }
      activeCovers--
      coverQueue.shift()?.()
    }
    try {
      image = new Image()
      image.decoding = "async"
      image.fetchPriority = "low"
      timer = setTimeout(() => { image!.src = ""; finish(false) }, 8000)
      image.onload = () => finish(true)
      image.onerror = () => finish(false)
      image.src = url
    } catch {
      finish(false)
    }
  }
  if (activeCovers < 4) start()
  else coverQueue.push(start)
}

export function preloadRetroCovers(games: Page["games"]) {
  for (const game of (games || []).slice(0, 8)) {
    const url = getRetroCover(game)
    if (url) warmCover(url)
  }
}
