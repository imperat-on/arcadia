"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { RetroGame, RetroSource } from "../../global"
import { useI18n } from "../../i18n/I18nContext"

/** Número de itens por página usado pelo catálogo Retro. */
export const RETRO_PAGE_SIZE = 24

interface RetroStoreViewProps {
  /** Permite que a tela seja usada em um container já rolável (ex.: Loja). */
  className?: string
}

/**
 * Catálogo de jogos clássicos.
 *
 * O catálogo não tenta falar com a rede no renderer: toda a comunicação passa
 * por retroList/retroGame, expostos pelo preload. Isso deixa a tela utilizável
 * também quando a integração Retro ainda não está disponível (nesse caso ela
 * mostra uma mensagem de erro em vez de lançar uma exceção).
 */
export function RetroStoreView({ className = "" }: RetroStoreViewProps) {
  const { t, locale } = useI18n()
  const [query, setQuery] = useState("")
  const [appliedQuery, setAppliedQuery] = useState("")
  const [games, setGames] = useState<RetroGame[]>([])
  const [sources, setSources] = useState<RetroSource[]>([])
  const [offset, setOffset] = useState(0)
  const [total, setTotal] = useState<number | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState("")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<RetroGame | null>(null)
  const [detailSources, setDetailSources] = useState<RetroSource[]>([])
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailError, setDetailError] = useState("")
  const listGeneration = useRef(0)
  const detailGeneration = useRef(0)

  const loadList = useCallback(
    async (nextQuery: string, nextOffset: number) => {
      const generation = ++listGeneration.current
      setLoading(true)
      setError("")

      const bridge = window.launcherAPI
      if (!bridge?.retroList) {
        if (generation !== listGeneration.current) return
        setGames([])
        setSources([])
        setTotal(0)
        setHasMore(false)
        setLoading(false)
        setError(t("store.retro_unavailable"))
        return
      }

      const payload: { query?: string; offset: number; limit: number } = {
        offset: nextOffset,
        limit: RETRO_PAGE_SIZE,
      }
      if (nextQuery) payload.query = nextQuery

      try {
        const response = await bridge.retroList(payload)
        if (generation !== listGeneration.current) return
        const items = Array.isArray(response?.games) ? response.games : []
        const responseTotal =
          typeof response?.total === "number" && Number.isFinite(response.total)
            ? Math.max(0, response.total)
            : null
        setGames(items)
        setSources(Array.isArray(response?.sources) ? response.sources : [])
        setTotal(responseTotal)
        setHasMore(
          typeof response?.hasMore === "boolean"
            ? response.hasMore
            : responseTotal !== null
              ? nextOffset + items.length < responseTotal
              : items.length >= RETRO_PAGE_SIZE,
        )
        if (!response?.ok) setError(response?.error || t("store.retro_load_failed"))
      } catch (cause) {
        if (generation !== listGeneration.current) return
        setGames([])
        setSources([])
        setTotal(0)
        setHasMore(false)
        setError(cause instanceof Error ? cause.message : t("store.retro_load_failed"))
      } finally {
        if (generation === listGeneration.current) setLoading(false)
      }
    },
    [t],
  )

  useEffect(() => {
    void loadList(appliedQuery, offset)
  }, [appliedQuery, offset, loadList])

  const openGame = useCallback(
    async (game: RetroGame) => {
      const generation = ++detailGeneration.current
      setSelectedId(game.id)
      setDetail(null)
      setDetailSources([])
      setDetailError("")
      setDetailLoading(true)

      const bridge = window.launcherAPI
      if (!bridge?.retroGame) {
        if (generation !== detailGeneration.current) return
        setDetailLoading(false)
        setDetailError(t("store.retro_unavailable"))
        return
      }

      try {
        const response = await bridge.retroGame(game.id)
        if (generation !== detailGeneration.current) return
        if (response?.ok && response.game) {
          setDetail(response.game)
          setDetailSources(Array.isArray(response.sources) ? response.sources : [])
        } else {
          setDetail(game)
          setDetailError(response?.error || t("store.retro_detail_failed"))
        }
      } catch (cause) {
        if (generation !== detailGeneration.current) return
        // A card already has enough metadata to remain useful when a detail
        // request fails (for example while a source is temporarily offline).
        setDetail(game)
        setDetailError(cause instanceof Error ? cause.message : t("store.retro_detail_failed"))
      } finally {
        if (generation === detailGeneration.current) setDetailLoading(false)
      }
    },
    [t],
  )

  const closeGame = useCallback(() => {
    detailGeneration.current++
    setSelectedId(null)
    setDetail(null)
    setDetailSources([])
    setDetailError("")
    setDetailLoading(false)
  }, [])

  const submitSearch = useCallback(
    (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault()
      const nextQuery = query.trim()
      setOffset(0)
      setAppliedQuery(nextQuery)
    },
    [query],
  )

  const clearSearch = useCallback(() => {
    setQuery("")
    setAppliedQuery("")
    setOffset(0)
  }, [])

  const page = Math.floor(offset / RETRO_PAGE_SIZE) + 1
  const totalPages = total === null ? null : Math.max(1, Math.ceil(total / RETRO_PAGE_SIZE))
  const visibleSources = useMemo(() => {
    const byId = new Map<string, RetroSource>()
    for (const source of [...sources, ...detailSources]) {
      if (source?.id) byId.set(source.id, source)
    }
    return [...byId.values()]
  }, [detailSources, sources])

  if (selectedId) {
    return (
      <section
        data-testid="retro-game-detail"
        aria-label={t("store.retro_detail")}
        className={`min-h-full ${className}`}
      >
        <button
          type="button"
          onClick={closeGame}
          className="mb-5 inline-flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2 text-[12px] text-white/70 transition-colors hover:border-white/25 hover:text-white"
        >
          <span aria-hidden="true">←</span>
          {t("store.retro_back")}
        </button>

        {detailLoading && (
          <div className="rounded-xl border border-white/[0.08] bg-white/[0.02] p-5 text-[13px] text-white/55">
            {t("store.retro_loading_detail")}
          </div>
        )}

        {detailError && (
          <p
            role="alert"
            className="mb-4 rounded-lg border border-amber-200/15 bg-amber-200/[0.04] px-3 py-2 text-[12px] text-amber-100/75"
          >
            {detailError}
          </p>
        )}

        {detail && (
          <RetroDetail
            game={detail}
            sources={visibleSources}
            locale={locale}
            onOpenUri={(uri) => void window.launcherAPI?.openExternal?.(uri)}
            t={t}
          />
        )}
      </section>
    )
  }

  return (
    <section
      data-testid="retro-store-view"
      aria-label={t("store.retro_title")}
      className={`min-h-full ${className}`}
    >
      <div className="mb-5 flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="ui-title mb-1">{t("store.retro_title")}</h1>
          <p className="ui-subtitle">{t("store.retro_description")}</p>
        </div>
        {total !== null && !loading && (
          <span className="text-[12px] text-white/40">
            {t("store.retro_count", { count: total })}
          </span>
        )}
      </div>

      <form onSubmit={submitSearch} className="mb-5 flex max-w-[860px] gap-2" role="search">
        <div className="relative flex-1">
          <input
            data-testid="retro-search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("store.retro_search_placeholder")}
            aria-label={t("store.retro_search_placeholder")}
            spellCheck={false}
            className="ui-input w-full py-2.5 pl-3.5 pr-9 text-[13px] placeholder:text-white/25"
          />
          {query && (
            <button
              type="button"
              onClick={clearSearch}
              title={t("common.cancelar")}
              aria-label={t("common.cancelar")}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-white/35 transition-colors hover:text-white"
            >
              <span aria-hidden="true">×</span>
            </button>
          )}
        </div>
        <button
          type="submit"
          disabled={loading}
          className="ui-btn-primary rounded-lg px-4 py-2.5 text-[12px] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {loading ? t("store.buscando") : t("store.buscar")}
        </button>
      </form>

      {appliedQuery && !loading && !error && (
        <p className="mb-3 text-[12px] text-white/50">
          {t("store.retro_results_for", { query: appliedQuery })}
        </p>
      )}

      {error && (
        <div
          role="alert"
          className="mb-5 rounded-xl border border-white/10 bg-white/[0.03] px-4 py-3 text-[13px] text-white/65"
        >
          {error}
          <button
            type="button"
            onClick={() => void loadList(appliedQuery, offset)}
            className="ml-3 rounded-md border border-white/15 px-2 py-1 text-[11px] text-white/75 hover:border-white/30 hover:text-white"
          >
            {t("store.retro_try_again")}
          </button>
        </div>
      )}

      {loading ? (
        <RetroSkeleton />
      ) : games.length ? (
        <>
          <div className="grid-stagger grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3 pb-5">
            {games.map((game) => (
              <RetroCard key={game.id} game={game} onOpen={() => void openGame(game)} t={t} />
            ))}
          </div>

          {(offset > 0 || hasMore) && (
            <nav
              aria-label={t("store.retro_pagination")}
              className="flex items-center justify-center gap-2 pb-7 pt-2"
            >
              <button
                type="button"
                onClick={() => setOffset(Math.max(0, offset - RETRO_PAGE_SIZE))}
                disabled={offset <= 0 || loading}
                className="min-w-9 rounded-lg border border-white/10 px-3 py-2 text-[13px] text-white/70 transition-colors hover:border-white/25 hover:text-white disabled:opacity-30"
              >
                ‹
              </button>
              <span className="px-2 text-[12px] text-white/45">
                {totalPages
                  ? t("store.retro_page_of", { page, total: totalPages })
                  : t("store.retro_page", { page })}
              </span>
              <button
                type="button"
                onClick={() => setOffset(offset + RETRO_PAGE_SIZE)}
                disabled={!hasMore || loading}
                className="min-w-9 rounded-lg border border-white/10 px-3 py-2 text-[13px] text-white/70 transition-colors hover:border-white/25 hover:text-white disabled:opacity-30"
              >
                ›
              </button>
            </nav>
          )}
        </>
      ) : (
        <div className="rounded-xl border border-white/[0.08] bg-white/[0.02] px-5 py-10 text-center text-[13px] text-white/50">
          {appliedQuery ? t("store.nada_encontrado") : t("store.retro_empty")}
        </div>
      )}
    </section>
  )
}

function RetroCard({
  game,
  onOpen,
  t,
}: {
  game: RetroGame
  onOpen: () => void
  t: (key: string, vars?: Record<string, string | number>) => string
}) {
  return (
    <article
      data-testid={`retro-game-card-${game.id}`}
      className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02] transition-colors hover:border-white/20"
    >
      <button
        type="button"
        onClick={onOpen}
        className="block aspect-[460/215] w-full cursor-pointer bg-black text-left"
        title={game.title}
      >
        <RetroArtwork game={game} title={game.title} />
      </button>
      <div className="p-3">
        <button
          type="button"
          onClick={onOpen}
          className="mb-2 block w-full truncate text-left text-[13px] font-medium text-white hover:text-[color:var(--accent)]"
          title={game.title}
        >
          {game.title}
        </button>
        <div className="flex items-center justify-between gap-2 text-[11px] text-white/45">
          <span className="truncate" title={game.platform || undefined}>
            {game.platform || t("store.retro_platform_unknown")}
          </span>
          <span className="shrink-0 truncate" title={game.sourceTitle || game.sourceId}>
            {game.sourceTitle || game.sourceId}
          </span>
        </div>
      </div>
    </article>
  )
}

function RetroDetail({
  game,
  sources,
  locale,
  onOpenUri,
  t,
}: {
  game: RetroGame
  sources: RetroSource[]
  locale: string
  onOpenUri: (uri: string) => void
  t: (key: string, vars?: Record<string, string | number>) => string
}) {
  const uris = Array.isArray(game.uris)
    ? game.uris.filter((uri): uri is string => Boolean(uri))
    : []
  const source = sources.find((item) => item.id === game.sourceId)
  const parsedDate = game.uploadDate ? new Date(game.uploadDate) : null
  const date =
    game.uploadDate && parsedDate && !Number.isNaN(parsedDate.getTime())
      ? new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(parsedDate)
      : game.uploadDate || ""

  return (
    <article
      className="overflow-hidden rounded-2xl border border-white/[0.1] bg-white/[0.025]"
      data-testid="retro-detail-card"
    >
      <div className="grid gap-0 md:grid-cols-[minmax(240px,0.75fr)_minmax(0,1.25fr)]">
        <div className="aspect-[460/215] min-h-[190px] bg-black md:aspect-auto">
          <RetroArtwork game={game} title={game.title} />
        </div>
        <div className="p-5 md:p-7">
          <h1 className="mb-2 text-xl font-semibold text-white">{game.title}</h1>
          <div className="mb-5 flex flex-wrap gap-2 text-[11px] text-white/55">
            {game.platform && <Badge>{game.platform}</Badge>}
            {(game.sourceTitle || game.sourceId) && (
              <Badge>{game.sourceTitle || game.sourceId}</Badge>
            )}
          </div>
          {game.description && (
            <p className="mb-6 whitespace-pre-wrap text-[13px] leading-relaxed text-white/65">
              {game.description}
            </p>
          )}

          <dl className="grid grid-cols-2 gap-x-5 gap-y-3 border-t border-white/[0.08] pt-4 text-[12px]">
            {game.fileSize && (
              <div>
                <dt className="text-white/35">{t("store.retro_size")}</dt>
                <dd className="mt-0.5 text-white/75">{game.fileSize}</dd>
              </div>
            )}
            {date && (
              <div>
                <dt className="text-white/35">{t("store.retro_date")}</dt>
                <dd className="mt-0.5 text-white/75">{date}</dd>
              </div>
            )}
          </dl>
        </div>
      </div>

      {(source || uris.length > 0) && (
        <div className="border-t border-white/[0.08] p-5 md:p-7">
          <h2 className="mb-3 text-sm font-medium text-white/80">{t("store.retro_sources")}</h2>
          {source?.description && (
            <p className="mb-3 text-[12px] text-white/50">{source.description}</p>
          )}
          {uris.length > 0 ? (
            <div className="flex flex-wrap gap-2">
              {uris.map((uri, index) => (
                <button
                  key={`${uri}-${index}`}
                  type="button"
                  onClick={() => onOpenUri(uri)}
                  className="max-w-full truncate rounded-lg border border-white/10 px-3 py-2 text-left text-[11px] text-white/65 transition-colors hover:border-white/25 hover:text-white"
                  title={uri}
                >
                  {t("store.retro_open_source", { count: index + 1 })}
                </button>
              ))}
            </div>
          ) : (
            <p className="text-[12px] text-white/45">{t("store.retro_no_uris")}</p>
          )}
        </div>
      )}
    </article>
  )
}

function Badge({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-1">
      {children}
    </span>
  )
}

function RetroArtwork({ game, title }: { game: RetroGame; title: string }) {
  const [index, setIndex] = useState(0)
  const urls = useMemo(
    () =>
      [game.cover, game.capa, game.fallbackCover].filter((value): value is string =>
        Boolean(value),
      ),
    [game.cover, game.capa, game.fallbackCover],
  )
  useEffect(() => setIndex(0), [game.id, urls.join("|")])

  if (!urls[index]) {
    return (
      <div className="flex h-full w-full flex-col items-center justify-center gap-2 bg-[#121216] px-3 text-center text-white/35">
        <svg
          width="26"
          height="26"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          aria-hidden="true"
        >
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <path d="m21 15-5-5L5 21" />
        </svg>
        <span className="text-[11px] leading-tight">{title}</span>
      </div>
    )
  }

  return (
    <img
      src={urls[index]}
      alt=""
      loading="lazy"
      decoding="async"
      className="h-full w-full object-cover"
      draggable={false}
      onError={() => setIndex((value) => value + 1)}
    />
  )
}

function RetroSkeleton() {
  return (
    <div
      className="grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3 pb-6"
      aria-hidden="true"
    >
      {Array.from({ length: 8 }, (_, index) => (
        <div
          key={index}
          className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]"
        >
          <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
          <div className="p-3">
            <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
            <div className="h-3 animate-pulse rounded bg-white/[0.04]" />
          </div>
        </div>
      ))}
    </div>
  )
}

export default RetroStoreView
