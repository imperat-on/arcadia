"use client"

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { RetroGame, RetroOfferSummary, RetroSource } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import type { JogoLoja, OpcaoTorrent } from "../useStoreActions"
import { MetodoDownloadDialog } from "./MetodoDownloadDialog"
import { getRetroCover, loadRetroCovers } from "./retroArtwork"
import { StoreGamePage, type RetroStoreDetail } from "./StoreGamePage"
import type { Game } from "../ps5-launcher/types"

/** Número de itens por página usado pelo catálogo Retro. */
export const RETRO_PAGE_SIZE = 24

const RETRO_SYSTEMS = [
  ["sony-playstation", "PlayStation"], ["sony-playstation-2", "PlayStation 2"],
  ["sony-playstation-3", "PlayStation 3"], ["sony-psp", "PSP"],
  ["nintendo-nes", "Nintendo NES"], ["nintendo-snes", "Super Nintendo"],
  ["nintendo-64", "Nintendo 64"], ["nintendo-gamecube", "Nintendo GameCube"],
  ["nintendo-wii", "Nintendo Wii"], ["nintendo-ds", "Nintendo DS"],
  ["nintendo-dsi", "Nintendo DSi"], ["nintendo-game-boy", "Game Boy"],
  ["nintendo-game-boy-color", "Game Boy Color"], ["nintendo-game-boy-advance", "Game Boy Advance"],
] as const

interface RetroStoreViewProps {
  /** Permite que a tela seja usada em um container já rolável (ex.: Loja). */
  className?: string
  /** Integra o botão Voltar do gamepad sem expor detalhes ao container. */
  backRequest?: number
  onDetailChange?: (open: boolean) => void
  onDownloadDialogChange?: (open: boolean) => void
  onOpenDownloads?: () => void
  /** Lançamento da biblioteca compartilhado entre desktop e console. */
  onLaunchGame?: (game: Game) => void
  initialGameId?: string
  initialGame?: RetroGame
  onExit?: () => void
}

type RetroDownloadChoice = {
  game: RetroGame
  jogo: JogoLoja
  opcoes: OpcaoTorrent[]
}

export function retroGameFromLibrary(game: Game): RetroGame {
  const rawGenres = game.genres
  const genres = Array.isArray(rawGenres)
    ? rawGenres.filter((genre): genre is string => typeof genre === "string")
    : undefined
  const rawReleaseYear = game.releaseYear ?? game.year
  const parsedReleaseYear =
    typeof rawReleaseYear === "number"
      ? rawReleaseYear
      : typeof rawReleaseYear === "string" && /^\d{4}$/.test(rawReleaseYear)
        ? Number(rawReleaseYear)
        : undefined

  return {
    id: game.id,
    title: game.title,
    cover: game.cover,
    hero: game.hero,
    platform: game.platform,
    description: game.description,
    genres,
    releaseYear: parsedReleaseYear,
    systemId: game.systemId,
  }
}

function makeRetroDownloadChoice(game: RetroGame): RetroDownloadChoice {
  const uris = Array.isArray(game.uris)
    ? game.uris.filter((uri): uri is string => Boolean(uri))
    : []
  const source = game.sourceTitle || game.sourceId
  const title = game.originalTitle || game.title
  return {
    game,
    jogo: {
      appid: game.id,
      title: game.title,
      cover: getRetroCover(game) || undefined,
      capa: game.capa,
    },
    opcoes: uris.map((uri, index) => ({
      ref: `${game.id}:${index}`,
      magnet: uri,
      fonte: source,
      tituloFonte: uris.length > 1 ? `${title} · ${index + 1}` : title,
      fileSize: game.fileSize || "",
      http: /^https?:\/\//i.test(uri),
    })),
  }
}

/**
 * Catálogo de jogos clássicos.
 *
 * O catálogo não tenta falar com a rede no renderer: toda a comunicação passa
 * por retroList/retroGame, expostos pelo preload. Isso deixa a tela utilizável
 * também quando a integração Retro ainda não está disponível (nesse caso ela
 * mostra uma mensagem de erro em vez de lançar uma exceção).
 */
export function RetroStoreView({
  className = "",
  backRequest = 0,
  onDetailChange,
  onDownloadDialogChange,
  onOpenDownloads,
  onLaunchGame,
  initialGameId,
  initialGame,
  onExit,
}: RetroStoreViewProps) {
  const { t } = useI18n()
  const [query, setQuery] = useState("")
  const [appliedQuery, setAppliedQuery] = useState("")
  const [system, setSystem] = useState("")
  const [games, setGames] = useState<RetroGame[]>([])
  const [sources, setSources] = useState<RetroSource[]>([])
  const [offset, setOffset] = useState(0)
  const [total, setTotal] = useState<number | null>(null)
  const [totalOffers, setTotalOffers] = useState<number | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState("")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<RetroGame | null>(null)
  const [detailSources, setDetailSources] = useState<RetroSource[]>([])
  const [detailOffers, setDetailOffers] = useState<RetroOfferSummary[]>([])
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailError, setDetailError] = useState("")
  const [downloadUri, setDownloadUri] = useState("")
  const [downloadMessage, setDownloadMessage] = useState("")
  const [downloadChoice, setDownloadChoice] = useState<RetroDownloadChoice | null>(null)
  const listGeneration = useRef(0)
  const detailGeneration = useRef(0)

  const loadList = useCallback(
    async (nextQuery: string, nextOffset: number, nextSystem: string) => {
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

      const payload: { query?: string; system?: string; variants?: "all"; mode?: "essentials" | "all"; offset: number; limit: number } = {
        offset: nextOffset,
        limit: RETRO_PAGE_SIZE,
      }
      if (nextQuery) payload.query = nextQuery
      if (nextSystem) payload.system = nextSystem
      // A vitrine Retro abre no catálogo completo; a caixa de variantes só
      // controla releases beta/demo/patches, não o modo editorial da lista.
      payload.mode = "all"

      try {
        const response = await bridge.retroList(payload)
        if (generation !== listGeneration.current) return
        const items = Array.isArray(response?.games) ? response.games : []
        const rawTotal = response?.total ?? response?.totalGames
        const responseTotal =
          typeof rawTotal === "number" && Number.isFinite(rawTotal)
            ? Math.max(0, rawTotal)
            : null
        setGames(items)
        setSources(Array.isArray(response?.sources) ? response.sources : [])
        setTotal(responseTotal)
        setTotalOffers(
          typeof response?.totalOffers === "number" && Number.isFinite(response.totalOffers)
            ? Math.max(0, response.totalOffers)
            : null,
        )
        setHasMore(
          typeof response?.hasMore === "boolean"
            ? response.hasMore
            : responseTotal !== null
              ? nextOffset + items.length < responseTotal
              : items.length >= RETRO_PAGE_SIZE,
        )
        if (!response?.ok) {
          setHasMore(false)
          if (nextOffset === 0) {
            setGames([])
            setSources([])
          }
          setError(response?.error || t("store.retro_load_failed"))
        }
      } catch (cause) {
        if (generation !== listGeneration.current) return
        if (nextOffset === 0) {
          setGames([])
          setSources([])
          setTotal(0)
          setTotalOffers(null)
        }
        setHasMore(false)
        setError(cause instanceof Error ? cause.message : t("store.retro_load_failed"))
      } finally {
        if (generation === listGeneration.current) setLoading(false)
      }
    },
    [t],
  )

  // Aberto direto num jogo (Library): a grade nunca aparece nessa rota, então
  // buscar retro:list só desperdiça a query mais cara do catálogo. Se essa rota
  // passar a voltar para a grade sem desmontar, a guarda deverá considerar selectedId.
  useEffect(() => {
    if (initialGameId) return
    void loadList(appliedQuery, offset, system)
  }, [appliedQuery, offset, system, loadList, initialGameId])

  const openGame = useCallback(
    async (game: RetroGame) => {
      const generation = ++detailGeneration.current
      setSelectedId(game.id)
      // Semeia com o que já sabemos (card da grade ou jogo da biblioteca): a página
      // pinta na hora e o retro:game só substitui/completa os campos.
      setDetail(game.title && game.title !== game.id ? game : null)
      setDetailSources([])
      setDetailOffers([])
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
          setDetailOffers(Array.isArray(response.offers) ? response.offers : [])
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

  useEffect(() => {
    if (!initialGameId) return
    void openGame(initialGame || { id: initialGameId, title: initialGameId })
  }, [initialGameId, initialGame, openGame])

  // Every URI is represented in the shared Hydra download dialog. The single
  // download action opens the source step with all alternatives, so Retro has
  // the same source and folder choices as the regular Store flow.
  const openDownloadDialog = useCallback(async (game: RetroGame) => {
    const legacyChoice = makeRetroDownloadChoice(game)
    if (legacyChoice.opcoes.length) {
      setDownloadMessage("")
      setDownloadChoice(legacyChoice)
      return
    }

    const getOffer = window.launcherAPI?.retroOffer
    if (!getOffer || !detailOffers.length) return
    setDownloadMessage(t("store.retro_downloading"))
    try {
      const resolved = await Promise.all(detailOffers.map((offer) => getOffer(offer.id)))
      const opcoes: OpcaoTorrent[] = []
      for (const response of resolved) {
        const offer = response?.ok ? response.offer : undefined
        if (!offer) continue
        for (const [index, uri] of (offer.uris || []).entries()) {
          opcoes.push({
            ref: `${offer.id}:${index}`,
            magnet: uri,
            fonte: offer.sourceTitle || offer.sourceId,
            tituloFonte: offer.originalTitle || game.title,
            fileSize: offer.fileSize || "",
            http: /^https?:\/\//i.test(uri),
          })
        }
      }
      if (!opcoes.length) {
        setDownloadMessage(t("store.retro_no_uris"))
        return
      }
      setDownloadMessage("")
      setDownloadChoice({
        game,
        jogo: {
          appid: game.id,
          title: game.title,
          cover: getRetroCover(game) || undefined,
          capa: game.capa,
        },
        opcoes,
      })
    } catch (cause) {
      setDownloadMessage(cause instanceof Error ? cause.message : t("store.retro_download_failed"))
    }
  }, [detailOffers, t])

  const startDownload = useCallback(
    async (game: RetroGame, uri: string, savePath: string) => {
      setDownloadChoice(null)
      setDownloadUri(uri)
      setDownloadMessage("")
      try {
        const start = window.launcherAPI?.torrentStart
        if (!start) {
          setDownloadMessage(t("store.retro_unavailable"))
          return
        }
        const response = await start({
          gameId: game.id,
          url: uri,
          savePath,
          title: game.title,
          cover: getRetroCover(game) || undefined,
        })
        setDownloadMessage(
          response?.ok
            ? t("store.retro_download_started")
            : response?.error || t("store.retro_download_failed"),
        )
        if (response?.ok || response?.queued) onOpenDownloads?.()
      } catch (cause) {
        setDownloadMessage(
          cause instanceof Error ? cause.message : t("store.retro_download_failed"),
        )
      } finally {
        setDownloadUri("")
      }
    },
    [onOpenDownloads, t],
  )

  const closeGame = useCallback(() => {
    detailGeneration.current++
    setSelectedId(null)
    setDetail(null)
    setDetailSources([])
    setDetailOffers([])
    setDetailError("")
    setDetailLoading(false)
    setDownloadUri("")
    setDownloadMessage("")
    setDownloadChoice(null)
    if (initialGameId) onExit?.()
  }, [initialGameId, onExit])

  useEffect(() => {
    onDetailChange?.(Boolean(selectedId))
    return () => onDetailChange?.(false)
  }, [onDetailChange, selectedId])

  useEffect(() => {
    onDownloadDialogChange?.(Boolean(downloadChoice))
    return () => onDownloadDialogChange?.(false)
  }, [downloadChoice, onDownloadDialogChange])

  const previousBackRequest = useRef(backRequest)
  useEffect(() => {
    if (backRequest === previousBackRequest.current) return
    previousBackRequest.current = backRequest
    if (downloadChoice) {
      setDownloadChoice(null)
      return
    }
    if (selectedId) closeGame()
  }, [backRequest, closeGame, downloadChoice, selectedId])

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

  const visibleSources = useMemo(() => {
    const byId = new Map<string, RetroSource>()
    for (const source of [...sources, ...detailSources]) {
      if (source?.id) byId.set(source.id, source)
    }
    return [...byId.values()]
  }, [detailSources, sources])
  const page = Math.floor(offset / RETRO_PAGE_SIZE) + 1
  const totalPages = total === null ? null : Math.max(1, Math.ceil(total / RETRO_PAGE_SIZE))

  if (selectedId) {
    return (
      <section
        data-testid="retro-game-detail"
        aria-label={t("store.retro_detail")}
        className={`h-full min-h-0 ${className}`}
      >
        {detailLoading && <RetroDetailSkeleton />}

        {detailError && (
          <p
            role="alert"
            className="mb-4 rounded-lg border border-amber-200/15 bg-amber-200/[0.04] px-3 py-2 text-[12px] text-amber-100/75"
          >
            {detailError}
          </p>
        )}

        {detail && !detailLoading && (
          <RetroDetail
            game={detail}
            offers={detailOffers}
            sources={visibleSources}
            downloadUri={downloadUri}
            downloadMessage={downloadMessage}
            onDownloadUri={() => void openDownloadDialog(detail)}
            onClose={closeGame}
            onRemoved={initialGameId ? closeGame : undefined}
            onLaunchGame={onLaunchGame}
            t={t}
          />
        )}

        {downloadChoice && (
          <MetodoDownloadDialog
            jogo={downloadChoice.jogo}
            opcoes={downloadChoice.opcoes}
            // Retro games only have Hydra URIs. The shared dialog therefore
            // starts at the source step and never offers a Depot action.
            onDepot={() => setDownloadChoice(null)}
            onTorrent={(uri, savePath) => void startDownload(downloadChoice.game, uri, savePath)}
            onClose={() => setDownloadChoice(null)}
            depotDisponivel={false}
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
            {totalOffers !== null
              ? t("store.retro_totals", { games: total, downloads: totalOffers })
              : t("store.retro_count", { count: total })}
          </span>
        )}
      </div>

      <form onSubmit={submitSearch} className="desktop-fluid-search mb-5 flex max-w-[1040px] gap-2" role="search">
        <select
          value={system}
          onChange={(event) => { setSystem(event.target.value); setOffset(0) }}
          aria-label={t("store.retro_platform_filter")}
          className="ui-input min-w-[190px] px-3 py-2.5 text-[12px] text-white/75 [color-scheme:dark]"
          style={{ colorScheme: "dark" }}
        >
          <option value="" className="bg-[#151515] text-white">{t("store.retro_all_platforms")}</option>
          {RETRO_SYSTEMS.map(([id, label]) => <option key={id} value={id} className="bg-[#151515] text-white">{label}</option>)}
        </select>
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
            onClick={() => void loadList(appliedQuery, offset, system)}
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
          <div className="retro-catalog-grid grid-stagger grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3 pb-5">
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
                className="min-w-9 rounded-lg border border-white/10 px-3 py-2 text-[13px] text-white/70 transition-colors hover:border-white/25 hover:text-white"
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

const RetroCard = memo(function RetroCard({
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
      data-testid={`retro-game-card-container-${game.id}`}
      className="retro-catalog-card overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02] transition-colors hover:border-white/20"
    >
      <button
        data-testid={`retro-game-card-${game.id}`}
        type="button"
        onClick={onOpen}
        className="block aspect-[2/3] w-full cursor-pointer bg-black text-left"
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
            {game.offerCount
              ? t("store.retro_offer_count", { count: game.offerCount })
              : game.sourceTitle || game.sourceId}
          </span>
        </div>
      </div>
    </article>
  )
}, (previous, next) => previous.game === next.game && previous.t === next.t)

function RetroDetail({
  game,
  offers,
  sources,
  downloadUri,
  downloadMessage,
  onDownloadUri,
  onClose,
  onRemoved,
  onLaunchGame,
  t,
}: {
  game: RetroGame
  offers: RetroOfferSummary[]
  sources: RetroSource[]
  downloadUri: string
  downloadMessage: string
  onDownloadUri: (uri: string) => void
  onClose: () => void
  onRemoved?: () => void
  onLaunchGame?: (game: Game) => void
  t: (key: string, vars?: Record<string, string | number>) => string
}) {
  const [inLibrary, setInLibrary] = useState(false)
  const [libraryMessage, setLibraryMessage] = useState("")
  const [confirmandoRemover, setConfirmandoRemover] = useState(false)
  const [removendo, setRemovendo] = useState(false)
  const [hasEmulator, setHasEmulator] = useState(false)
  const uris = Array.isArray(game.uris)
    ? game.uris.filter((uri): uri is string => Boolean(uri))
    : []
  const availableCount = uris.length || offers.reduce((sum, offer) => sum + offer.uriCount, 0)
  const cover = getRetroCover(game)
  const media = [...new Set([game.hero, cover, ...(game.screenshots || []), ...(game.titleScreens || [])].filter((value): value is string => Boolean(value)))]
  const hero = media[0] || cover || "/placeholder.jpg"

  useEffect(() => {
    let ativo = true
    setLibraryMessage("")
    const verificar = () => {
      window.launcherAPI?.getLibrary?.().then((games) => {
        if (!ativo) return
        setInLibrary(Array.isArray(games) && games.some((item) => item.id === game.id))
      }).catch(() => {})
    }
    verificar()
    const off = window.launcherAPI?.onLibraryChanged?.(() => verificar())
    return () => { ativo = false; off?.() }
  }, [game.id])

  useEffect(() => {
    let ativo = true
    setHasEmulator(false)
    window.launcherAPI?.gameSettingsGet?.(game.id).then((response) => {
      const settings = response?.settings
      if (ativo && settings?.emulatorId && settings?.romPath) setHasEmulator(true)
    }).catch(() => {})
    return () => { ativo = false }
  }, [game.id])

  const adicionar = async () => {
    const result = await window.launcherAPI?.retroLibraryAdd?.({
      id: game.id,
      title: game.title,
      systemId: game.systemId,
      platform: game.platform,
      cover,
      hero,
      description: game.description,
      genres: game.genres,
      releaseYear: game.releaseYear,
    })
    if (result?.ok) {
      setInLibrary(true)
      setLibraryMessage("")
    } else {
      setLibraryMessage(result?.error || "Could not add to Library")
    }
  }

  const remover = async () => {
    setRemovendo(true)
    setLibraryMessage("")
    try {
      const response = await window.launcherAPI?.retroLibraryRemove?.(game.id)
      if (response?.ok) {
        setConfirmandoRemover(false)
        setInLibrary(false)
        onRemoved?.()
      } else {
        setLibraryMessage(response?.error || t("store.retro_remover_erro"))
      }
    } catch {
      setLibraryMessage(t("store.retro_remover_erro"))
    } finally {
      setRemovendo(false)
    }
  }

  const retroGame: Game = {
    id: game.id,
    title: game.title,
    launcher: "retro",
    launch_cmd: [],
    cover,
    hero,
    description: game.description,
    platform: game.platform,
    systemId: game.systemId,
    genre: game.genres?.join(", "),
    year: game.releaseYear ?? undefined,
    developer: game.developer?.[0],
    publisher: game.publisher?.[0],
    installed: inLibrary,
    retro: true,
  }

  const jogar = () => onLaunchGame?.(retroGame)

  const links = [
    ...sources
      .filter((source) => source?.url)
      .map((source) => ({ label: source.title || source.id, onClick: () => window.launcherAPI?.openExternal(source.url) })),
    ...(game.systemId
      ? [
          { label: "RetroAchievements", onClick: () => window.launcherAPI?.openExternal("https://retroachievements.org/") },
          { label: "Configurar conquistas", onClick: () => window.launcherAPI?.openExternal("https://retroachievements.org/controlpanel.php") },
        ]
      : []),
  ].slice(0, 4)

  return (
    <>
      <StoreGamePage
        embedded
        jogo={{ appid: game.id, title: game.title, cover, capa: cover, heroi: hero, manifest: true }}
        game={retroGame}
        onClose={onClose}
        onBaixar={() => onDownloadUri("")}
        onAdicionar={() => void adicionar()}
        onRemover={() => setConfirmandoRemover(true)}
        onJogar={hasEmulator ? jogar : undefined}
        statusMessage={libraryMessage || downloadMessage}
        naBiblioteca={inLibrary}
        ocupado={Boolean(downloadUri || removendo)}
        slssteamAtivo={availableCount > 0}
        retro={{
          systemId: game.systemId,
          platform: game.platform,
          description: game.description,
          genres: game.genres,
          releaseYear: game.releaseYear,
          developers: game.developer,
          publishers: game.publisher,
          offerCount: game.offerCount,
          availableCount,
          fileSize: game.fileSize,
          sourceCount: sources.length,
          links,

          screenshots: media.slice(1),
        } satisfies RetroStoreDetail}
      />

      {confirmandoRemover && (
        <div
          className="fixed inset-0 z-[75] flex items-center justify-center bg-black/70 backdrop-blur-sm"
          onClick={() => { if (!removendo) setConfirmandoRemover(false) }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="retro-remove-title"
            className="w-[460px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            <h2 id="retro-remove-title" className="mb-2 text-lg font-semibold text-white">
              {t("store.retro_remover_titulo")}
            </h2>
            <p className="mb-5 text-[13px] leading-relaxed text-white/60">
              {t("store.retro_remover_desc", { title: game.title })}
            </p>
            <div className="flex justify-end gap-2.5">
              <button
                type="button"
                onClick={() => setConfirmandoRemover(false)}
                disabled={removendo}
                className="rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
              >
                {t("common.cancelar")}
              </button>
              <button
                type="button"
                onClick={() => void remover()}
                disabled={removendo}
                className="rounded-lg border border-[#ff6b81]/45 px-5 py-2.5 text-[12px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
              >
                {t("common.remover")}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  )
}

function RetroArtwork({ game, title }: { game: RetroGame; title: string }) {
  const [index, setIndex] = useState(0)
  const immediateCover = getRetroCover(game)
  const [resolvedCovers, setResolvedCovers] = useState<string[]>(immediateCover ? [immediateCover] : [])
  const urls = resolvedCovers
  useEffect(() => {
    let alive = true
    void loadRetroCovers(game).then((covers) => {
      if (!alive) return
      setResolvedCovers((previous) =>
        previous.length === covers.length && previous.every((cover, index) => cover === covers[index])
          ? previous
          : covers,
      )
    })
    return () => {
      alive = false
    }
  }, [game.id, game.title, game.platform, game.cover, game.capa, game.fallbackCover])
  useEffect(() => setIndex(0), [game.id, urls.join("|")])

  if (!urls[index]) {
    const platform = String(game.systemId || game.platform || "Retro")
      .replace(/^sony-/, "")
      .replace(/^nintendo-/, "")
      .replace(/-/g, " ")
    const initials = platform
      .split(/\s+/)
      .filter(Boolean)
      .map((part) => part[0])
      .join("")
      .slice(0, 4)
      .toUpperCase()
    return (
      <div className="relative flex h-full w-full flex-col items-center justify-center overflow-hidden bg-[radial-gradient(circle_at_50%_20%,color-mix(in_srgb,var(--accent)_32%,#17171d),#0d0d11_68%)] px-5 text-center">
        <span className="absolute left-4 top-4 text-[10px] font-semibold uppercase tracking-[0.2em] text-white/40">
          Arcadia Retro
        </span>
        <span className="mb-5 flex h-20 w-20 items-center justify-center rounded-2xl border border-white/15 bg-black/25 text-2xl font-bold tracking-wider text-white/75 shadow-2xl">
          {initials || "R"}
        </span>
        <span className="line-clamp-3 text-[14px] font-semibold leading-snug text-white/85">{title}</span>
        <span className="mt-3 text-[10px] uppercase tracking-[0.16em] text-white/35">{platform}</span>
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
  return <RetroCardSkeleton count={8} />
}

function RetroCardSkeleton({ count }: { count: number }) {
  return (
    <div className="retro-catalog-grid grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3 pb-5" aria-hidden="true">
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className="retro-catalog-card overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
          <div className="aspect-[2/3] w-full animate-pulse bg-white/[0.05]" />
          <div className="p-3">
            <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
            <div className="h-3 animate-pulse rounded bg-white/[0.04]" />
          </div>
        </div>
      ))}
    </div>
  )
}

function RetroDetailSkeleton() {
  return (
    <div className="store-game-skeleton flex h-full min-h-0 flex-col overflow-hidden bg-[#0b0b0e] text-white">
      <div className="h-[50px] shrink-0 border-b border-white/[.08] bg-[#09090c]" />
      <div className="min-h-0 flex-1 overflow-hidden">
        <div className="store-game-hero h-[46vh] min-h-[280px] animate-pulse bg-white/[.04]" />
        <div className="mx-auto grid max-w-[1400px] grid-cols-1 gap-6 px-6 py-6 lg:grid-cols-[1fr_360px]">
          <div className="space-y-5">
            <div className="h-64 animate-pulse rounded-xl bg-white/[.04]" />
            <div className="h-40 animate-pulse rounded-xl bg-white/[.04]" />
          </div>
          <div className="space-y-5">
            <div className="h-40 animate-pulse rounded-xl bg-white/[.04]" />
            <div className="h-64 animate-pulse rounded-xl bg-white/[.04]" />
          </div>
        </div>
      </div>
    </div>
  )
}

export default RetroStoreView
