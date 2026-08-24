"use client"

import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"

/**
 * The community bridge is intentionally typed locally.  The main process may
 * be updated independently from the renderer and older installations do not
 * expose these methods yet; keeping the structural type here lets the panel
 * render an explicit offline state instead of making the whole page fail.
 */
type PageOptions = { limit?: number; offset?: number; mine?: boolean }

type CommunityReview = {
  id: string | number
  appid?: string
  title?: string | null
  text?: string | null
  rating?: number | null
  positive?: boolean | number | null
  hours?: number | null
  created_at?: string | null
  updated_at?: string | null
  user_id?: string | null
  username?: string | null
  display_name?: string | null
  author?: {
    username?: string | null
    display_name?: string | null
    avatar_url?: string | null
  } | null
}

type CollectionItem = {
  appid: string
  title?: string | null
  note?: string | null
}

type CommunityCollection = {
  id: string
  title: string
  description?: string | null
  visibility?: "public" | "unlisted" | "private" | string | null
  created_at?: string | null
  updated_at?: string | null
  user_id?: string | null
  username?: string | null
  display_name?: string | null
  item_count?: number | null
  items?: CollectionItem[]
}

type Pagination = {
  limit?: number
  offset?: number
  has_more?: boolean
  hasMore?: boolean
}

type CommunityResult<T> = {
  ok?: boolean
  offline?: boolean
  error?: string
  message?: string
  reviews?: CommunityReview[]
  collections?: CommunityCollection[]
  items?: T[]
  pagination?: Pagination
  review?: CommunityReview
  collection?: CommunityCollection
}

type CommunityApi = {
  communityReviews?: (appid: string, options?: PageOptions) => Promise<CommunityResult<CommunityReview>>
  communityReviewCreate?: (payload: Record<string, unknown>) => Promise<CommunityResult<CommunityReview>>
  communityReviewUpdate?: (
    id: string | number,
    payload: Record<string, unknown>,
  ) => Promise<CommunityResult<CommunityReview>>
  communityReviewRemove?: (id: string | number) => Promise<CommunityResult<CommunityReview>>
  communityReviewReport?: (id: string | number, payload: Record<string, unknown>) => Promise<CommunityResult<unknown>>
  communityCollections?: (options?: PageOptions) => Promise<CommunityResult<CommunityCollection>>
  communityCollectionCreate?: (payload: Record<string, unknown>) => Promise<CommunityResult<CommunityCollection>>
  communityCollectionUpdate?: (
    id: string,
    payload: Record<string, unknown>,
  ) => Promise<CommunityResult<CommunityCollection>>
  communityCollectionRemove?: (id: string) => Promise<CommunityResult<CommunityCollection>>
  communityCollectionReport?: (id: string, payload: Record<string, unknown>) => Promise<CommunityResult<unknown>>
}

type CommunityWindow = { launcherAPI?: CommunityApi }

type View = "reviews" | "collections"
type LoadStatus = "idle" | "loading" | "ready" | "error" | "offline"

type ReviewDraft = {
  title: string
  text: string
  rating: string
  hours: string
}

type CollectionDraft = {
  title: string
  description: string
  visibility: "public" | "unlisted" | "private"
}

const PAGE_SIZE = 8
const EMPTY_REVIEW: ReviewDraft = { title: "", text: "", rating: "5", hours: "0" }
const EMPTY_COLLECTION: CollectionDraft = { title: "", description: "", visibility: "public" }

function apiFromWindow(): CommunityApi | undefined {
  if (typeof window === "undefined") return undefined
  return (window as unknown as CommunityWindow).launcherAPI
}

function resultItems<T>(result: CommunityResult<T> | null | undefined, key: "reviews" | "collections") {
  const value = result?.[key]
  if (Array.isArray(value)) return value as T[]
  return Array.isArray(result?.items) ? result.items : []
}

function hasMore(result: CommunityResult<unknown> | null | undefined) {
  return Boolean(result?.pagination?.has_more ?? result?.pagination?.hasMore)
}

function resultMessage(result: CommunityResult<unknown> | null | undefined, fallback: string) {
  return result?.message || result?.error || fallback
}

function isOffline(result: CommunityResult<unknown> | null | undefined) {
  return Boolean(result?.offline) || /offline|network|fetch|rede|internet|timeout|indisponível|indisponivel/i.test(
    String(result?.error || result?.message || ""),
  )
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error || "Erro inesperado")
}

function formatDate(value?: string | null) {
  if (!value) return ""
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleDateString()
}

function authorName(review: CommunityReview) {
  return (
    review.display_name ||
    review.author?.display_name ||
    review.username ||
    review.author?.username ||
    "Membro da comunidade"
  )
}

function collectionOwner(collection: CommunityCollection) {
  return collection.display_name || collection.username || "Membro da comunidade"
}

function statusLabel(status: LoadStatus) {
  if (status === "loading") return "Carregando comunidade…"
  if (status === "offline") return "Comunidade offline."
  if (status === "error") return "Não foi possível carregar a comunidade."
  return ""
}

export interface CommunityPanelProps {
  appid: string
  title: string
}

/** Reviews e coleções públicas do jogo, com cache/offline e paginação local. */
export function CommunityPanel({ appid, title }: CommunityPanelProps) {
  const [view, setView] = useState<View>("reviews")
  const [reviews, setReviews] = useState<CommunityReview[]>([])
  const [collections, setCollections] = useState<CommunityCollection[]>([])
  const [reviewOffset, setReviewOffset] = useState(0)
  const [collectionOffset, setCollectionOffset] = useState(0)
  const [reviewsMore, setReviewsMore] = useState(false)
  const [collectionsMore, setCollectionsMore] = useState(false)
  const [reviewStatus, setReviewStatus] = useState<LoadStatus>("idle")
  const [collectionStatus, setCollectionStatus] = useState<LoadStatus>("idle")
  const [reviewError, setReviewError] = useState("")
  const [collectionError, setCollectionError] = useState("")
  const [mineOnly, setMineOnly] = useState(false)
  const [reviewDraft, setReviewDraft] = useState<ReviewDraft>(EMPTY_REVIEW)
  const [collectionDraft, setCollectionDraft] = useState<CollectionDraft>(EMPTY_COLLECTION)
  const [editingReview, setEditingReview] = useState<CommunityReview | null>(null)
  const [editingCollection, setEditingCollection] = useState<CommunityCollection | null>(null)
  const [reviewFormOpen, setReviewFormOpen] = useState(false)
  const [collectionFormOpen, setCollectionFormOpen] = useState(false)
  const [mutationBusy, setMutationBusy] = useState(false)
  const [mutationError, setMutationError] = useState("")
  const reviewRequest = useRef(0)
  const collectionRequest = useRef(0)

  const loadReviews = useCallback(async (offset = 0) => {
    const request = ++reviewRequest.current
    const api = apiFromWindow()
    setReviewOffset(offset)
    setReviewError("")
    setReviewStatus("loading")
    if (!api?.communityReviews) {
      setReviews([])
      setReviewsMore(false)
      setReviewStatus("offline")
      setReviewError("Este launcher ainda não expõe a API da comunidade.")
      return
    }
    try {
      const result = await api.communityReviews(appid, { limit: PAGE_SIZE, offset })
      if (request !== reviewRequest.current) return
      const next = resultItems<CommunityReview>(result, "reviews")
      setReviews(next)
      setReviewsMore(hasMore(result))
      setReviewStatus(result?.ok === false ? (isOffline(result) ? "offline" : "error") : result?.offline ? "offline" : "ready")
      if (result?.ok === false) setReviewError(resultMessage(result, "Não foi possível carregar as avaliações."))
    } catch (error) {
      if (request !== reviewRequest.current) return
      setReviews([])
      setReviewsMore(false)
      setReviewStatus("offline")
      setReviewError(errorMessage(error))
    }
  }, [appid])

  const loadCollections = useCallback(async (offset = 0, onlyMine = mineOnly) => {
    const request = ++collectionRequest.current
    const api = apiFromWindow()
    setCollectionOffset(offset)
    setCollectionError("")
    setCollectionStatus("loading")
    if (!api?.communityCollections) {
      setCollections([])
      setCollectionsMore(false)
      setCollectionStatus("offline")
      setCollectionError("Este launcher ainda não expõe a API da comunidade.")
      return
    }
    try {
      const result = await api.communityCollections({ limit: PAGE_SIZE, offset, mine: onlyMine })
      if (request !== collectionRequest.current) return
      const next = resultItems<CommunityCollection>(result, "collections")
      setCollections(next)
      setCollectionsMore(hasMore(result))
      setCollectionStatus(result?.ok === false ? (isOffline(result) ? "offline" : "error") : result?.offline ? "offline" : "ready")
      if (result?.ok === false) setCollectionError(resultMessage(result, "Não foi possível carregar as coleções."))
    } catch (error) {
      if (request !== collectionRequest.current) return
      setCollections([])
      setCollectionsMore(false)
      setCollectionStatus("offline")
      setCollectionError(errorMessage(error))
    }
  }, [mineOnly])

  useEffect(() => {
    setReviewOffset(0)
    setCollectionOffset(0)
    void loadReviews(0)
    void loadCollections(0, mineOnly)
  }, [appid, loadReviews, loadCollections, mineOnly])

  const openReviewCreate = () => {
    setEditingReview(null)
    setReviewDraft(EMPTY_REVIEW)
    setMutationError("")
    setReviewFormOpen(true)
  }

  const openReviewEdit = (review: CommunityReview) => {
    setEditingReview(review)
    setReviewDraft({
      title: review.title || "",
      text: review.text || "",
      rating: String(review.rating || (review.positive === false || review.positive === 0 ? 1 : 5)),
      hours: String(review.hours || 0),
    })
    setMutationError("")
    setReviewFormOpen(true)
  }

  const saveReview = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const text = reviewDraft.text.trim()
    if (!text) {
      setMutationError("Escreva o texto da avaliação.")
      return
    }
    const api = apiFromWindow()
    if (!api) {
      setMutationError("Comunidade offline: API indisponível.")
      return
    }
    setMutationBusy(true)
    setMutationError("")
    const payload = {
      appid,
      title: reviewDraft.title.trim() || undefined,
      text,
      rating: Math.min(5, Math.max(1, Number(reviewDraft.rating) || 5)),
      positive: Number(reviewDraft.rating) >= 3,
      hours: Math.max(0, Number(reviewDraft.hours) || 0),
    }
    try {
      const result = editingReview
        ? await api.communityReviewUpdate?.(editingReview.id, payload)
        : await api.communityReviewCreate?.(payload)
      if (!result || result.ok === false) {
        setMutationError(resultMessage(result, "Não foi possível salvar a avaliação."))
        return
      }
      setReviewFormOpen(false)
      setEditingReview(null)
      await loadReviews(reviewOffset)
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const removeReview = async (review: CommunityReview) => {
    if (!apiFromWindow()?.communityReviewRemove || !window.confirm("Remover esta avaliação?")) return
    setMutationBusy(true)
    setMutationError("")
    try {
      const result = await apiFromWindow()!.communityReviewRemove!(review.id)
      if (!result || result.ok === false) {
        setMutationError(resultMessage(result, "Não foi possível remover a avaliação."))
        return
      }
      await loadReviews(reviewOffset)
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const reportReview = async (review: CommunityReview) => {
    const api = apiFromWindow()
    if (!api?.communityReviewReport) return
    const reason = window.prompt("Por que esta avaliação deve ser analisada?", "conteúdo inadequado")?.trim()
    if (!reason) return
    setMutationBusy(true)
    setMutationError("")
    try {
      const result = await api.communityReviewReport(review.id, { reason })
      if (!result || result.ok === false) setMutationError(resultMessage(result, "Não foi possível enviar a denúncia."))
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const openCollectionCreate = () => {
    setEditingCollection(null)
    setCollectionDraft(EMPTY_COLLECTION)
    setMutationError("")
    setCollectionFormOpen(true)
  }

  const openCollectionEdit = (collection: CommunityCollection) => {
    setEditingCollection(collection)
    setCollectionDraft({
      title: collection.title || "",
      description: collection.description || "",
      visibility: collection.visibility === "private" || collection.visibility === "unlisted" ? collection.visibility : "public",
    })
    setMutationError("")
    setCollectionFormOpen(true)
  }

  const saveCollection = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const collectionTitle = collectionDraft.title.trim()
    if (!collectionTitle) {
      setMutationError("Informe um título para a coleção.")
      return
    }
    const api = apiFromWindow()
    if (!api) {
      setMutationError("Comunidade offline: API indisponível.")
      return
    }
    setMutationBusy(true)
    setMutationError("")
    const payload: Record<string, unknown> = {
      title: collectionTitle,
      description: collectionDraft.description.trim() || null,
      visibility: collectionDraft.visibility,
    }
    if (!editingCollection) payload.items = [{ appid, title }]
    try {
      const result = editingCollection
        ? await api.communityCollectionUpdate?.(editingCollection.id, payload)
        : await api.communityCollectionCreate?.(payload)
      if (!result || result.ok === false) {
        setMutationError(resultMessage(result, "Não foi possível salvar a coleção."))
        return
      }
      setCollectionFormOpen(false)
      setEditingCollection(null)
      await loadCollections(collectionOffset, mineOnly)
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const removeCollection = async (collection: CommunityCollection) => {
    if (!apiFromWindow()?.communityCollectionRemove || !window.confirm("Remover esta coleção?")) return
    setMutationBusy(true)
    setMutationError("")
    try {
      const result = await apiFromWindow()!.communityCollectionRemove!(collection.id)
      if (!result || result.ok === false) {
        setMutationError(resultMessage(result, "Não foi possível remover a coleção."))
        return
      }
      await loadCollections(collectionOffset, mineOnly)
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const reportCollection = async (collection: CommunityCollection) => {
    const api = apiFromWindow()
    if (!api?.communityCollectionReport) return
    const reason = window.prompt("Por que esta coleção deve ser analisada?", "conteúdo inadequado")?.trim()
    if (!reason) return
    setMutationBusy(true)
    setMutationError("")
    try {
      const result = await api.communityCollectionReport(collection.id, { reason })
      if (!result || result.ok === false) setMutationError(resultMessage(result, "Não foi possível enviar a denúncia."))
    } catch (error) {
      setMutationError(errorMessage(error))
    } finally {
      setMutationBusy(false)
    }
  }

  const status = view === "reviews" ? reviewStatus : collectionStatus
  const currentError = view === "reviews" ? reviewError : collectionError

  return (
    <section className="rounded-2xl border border-white/[0.08] bg-white/[0.025] p-4 text-white/85" aria-label="Comunidade">
      <header className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-[0.16em] text-white/80">Comunidade</h2>
          <p className="mt-1 text-xs text-white/45">Avaliações e coleções para {title || `o jogo ${appid}`}</p>
        </div>
        <div className="flex rounded-lg border border-white/10 bg-black/20 p-0.5" role="tablist" aria-label="Conteúdo da comunidade">
          <button
            type="button"
            role="tab"
            aria-selected={view === "reviews"}
            onClick={() => setView("reviews")}
            className={`rounded-md px-3 py-1.5 text-xs transition-colors ${view === "reviews" ? "bg-white/15 text-white" : "text-white/50 hover:text-white"}`}
          >
            Avaliações
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={view === "collections"}
            onClick={() => setView("collections")}
            className={`rounded-md px-3 py-1.5 text-xs transition-colors ${view === "collections" ? "bg-white/15 text-white" : "text-white/50 hover:text-white"}`}
          >
            Coleções
          </button>
        </div>
      </header>

      {status === "loading" && <p className="mb-3 text-xs text-white/45">{statusLabel(status)}</p>}
      {status === "offline" && (
        <p className="mb-3 rounded-lg border border-amber-400/20 bg-amber-400/[0.07] px-3 py-2 text-xs text-amber-100/80" role="status">
          {statusLabel(status)} {currentError || "Os dados podem estar indisponíveis até a conexão voltar."}
        </p>
      )}
      {status === "error" && (
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-red-400/20 bg-red-400/[0.07] px-3 py-2 text-xs text-red-100/85" role="alert">
          <span>{currentError || statusLabel(status)}</span>
          <button
            type="button"
            className="rounded border border-white/15 px-2 py-1 text-white/75 hover:bg-white/10"
            onClick={() => (view === "reviews" ? void loadReviews(reviewOffset) : void loadCollections(collectionOffset, mineOnly))}
          >
            Tentar novamente
          </button>
        </div>
      )}
      {mutationError && (
        <div className="mb-3 rounded-lg border border-red-400/20 bg-red-400/[0.07] px-3 py-2 text-xs text-red-100/85" role="alert">
          {mutationError}
        </div>
      )}

      {view === "reviews" ? (
        <div role="tabpanel" aria-label="Avaliações da comunidade">
          <div className="mb-3 flex items-center justify-between gap-2">
            <span className="text-xs text-white/45">{reviews.length ? `${reviews.length} nesta página` : "Ainda não há avaliações."}</span>
            <button type="button" onClick={openReviewCreate} className="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-black transition-opacity hover:opacity-85">
              Escrever avaliação
            </button>
          </div>
          {reviewFormOpen && (
            <ReviewForm
              draft={reviewDraft}
              setDraft={setReviewDraft}
              editing={Boolean(editingReview)}
              busy={mutationBusy}
              onSubmit={saveReview}
              onCancel={() => setReviewFormOpen(false)}
            />
          )}
          <div className="flex flex-col gap-2">
            {reviews.map((review) => (
              <article key={String(review.id)} className="rounded-xl border border-white/[0.07] bg-black/20 p-3">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="flex items-center gap-2 text-xs">
                      <strong className="text-white/80">{authorName(review)}</strong>
                      <span className="text-amber-300" aria-label={`${review.rating || (review.positive ? 5 : 1)} de 5 estrelas`}>
                        {"★".repeat(Math.max(1, Math.min(5, Number(review.rating) || (review.positive ? 5 : 1))))}
                      </span>
                    </div>
                    {review.title && <h3 className="mt-1 text-sm font-medium text-white/90">{review.title}</h3>}
                  </div>
                  <span className="text-[11px] text-white/35">{formatDate(review.created_at)}</span>
                </div>
                <p className="mt-2 whitespace-pre-wrap text-xs leading-relaxed text-white/65">{review.text || "(sem texto)"}</p>
                <div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-[11px] text-white/35">
                  <span>{review.hours ? `${review.hours}h jogadas` : ""}</span>
                  <span className="flex gap-2">
                    <button type="button" onClick={() => openReviewEdit(review)} className="hover:text-white/80">Editar</button>
                    <button type="button" disabled={mutationBusy} onClick={() => void removeReview(review)} className="hover:text-red-200 disabled:opacity-40">Remover</button>
                    <button type="button" disabled={mutationBusy} onClick={() => void reportReview(review)} className="hover:text-amber-200 disabled:opacity-40">Denunciar</button>
                  </span>
                </div>
              </article>
            ))}
          </div>
          {reviews.length === 0 && reviewStatus === "ready" && <p className="py-4 text-center text-xs text-white/35">Seja o primeiro a avaliar este jogo.</p>}
          <Pager offset={reviewOffset} hasMore={reviewsMore} busy={reviewStatus === "loading"} onPrevious={() => void loadReviews(Math.max(0, reviewOffset - PAGE_SIZE))} onNext={() => void loadReviews(reviewOffset + PAGE_SIZE)} />
        </div>
      ) : (
        <div role="tabpanel" aria-label="Coleções da comunidade">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <label className="flex items-center gap-2 text-xs text-white/50">
              <input type="checkbox" checked={mineOnly} onChange={(event) => setMineOnly(event.target.checked)} />
              Minhas coleções
            </label>
            <button type="button" onClick={openCollectionCreate} className="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-black transition-opacity hover:opacity-85">
              Criar coleção
            </button>
          </div>
          {collectionFormOpen && (
            <CollectionForm
              appid={appid}
              gameTitle={title}
              draft={collectionDraft}
              setDraft={setCollectionDraft}
              editing={Boolean(editingCollection)}
              busy={mutationBusy}
              onSubmit={saveCollection}
              onCancel={() => setCollectionFormOpen(false)}
            />
          )}
          <div className="flex flex-col gap-2">
            {collections.map((collection) => (
              <article key={collection.id} className="rounded-xl border border-white/[0.07] bg-black/20 p-3">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <h3 className="text-sm font-medium text-white/90">{collection.title}</h3>
                    <p className="mt-1 text-[11px] text-white/40">por {collectionOwner(collection)} · {collection.item_count || collection.items?.length || 0} jogos</p>
                  </div>
                  <span className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase text-white/40">{collection.visibility || "public"}</span>
                </div>
                {collection.description && <p className="mt-2 text-xs text-white/60">{collection.description}</p>}
                <div className="mt-2 flex justify-end gap-2 text-[11px] text-white/35">
                  <button type="button" onClick={() => openCollectionEdit(collection)} className="hover:text-white/80">Editar</button>
                  <button type="button" disabled={mutationBusy} onClick={() => void removeCollection(collection)} className="hover:text-red-200 disabled:opacity-40">Remover</button>
                  <button type="button" disabled={mutationBusy} onClick={() => void reportCollection(collection)} className="hover:text-amber-200 disabled:opacity-40">Denunciar</button>
                </div>
              </article>
            ))}
          </div>
          {collections.length === 0 && collectionStatus === "ready" && <p className="py-4 text-center text-xs text-white/35">Nenhuma coleção encontrada.</p>}
          <Pager offset={collectionOffset} hasMore={collectionsMore} busy={collectionStatus === "loading"} onPrevious={() => void loadCollections(Math.max(0, collectionOffset - PAGE_SIZE), mineOnly)} onNext={() => void loadCollections(collectionOffset + PAGE_SIZE, mineOnly)} />
        </div>
      )}
    </section>
  )
}

function ReviewForm({
  draft,
  setDraft,
  editing,
  busy,
  onSubmit,
  onCancel,
}: {
  draft: ReviewDraft
  setDraft: (draft: ReviewDraft) => void
  editing: boolean
  busy: boolean
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onCancel: () => void
}) {
  return (
    <form onSubmit={onSubmit} className="mb-3 rounded-xl border border-white/10 bg-black/25 p-3">
      <div className="grid gap-2 sm:grid-cols-[1fr_100px_100px]">
        <label className="text-xs text-white/55">Título (opcional)<input value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} maxLength={120} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs text-white outline-none focus:border-[color:var(--accent)]" /></label>
        <label className="text-xs text-white/55">Nota<select value={draft.rating} onChange={(event) => setDraft({ ...draft, rating: event.target.value })} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs text-white outline-none"><option value="5">5 / 5</option><option value="4">4 / 5</option><option value="3">3 / 5</option><option value="2">2 / 5</option><option value="1">1 / 5</option></select></label>
        <label className="text-xs text-white/55">Horas<input type="number" min="0" step="0.1" value={draft.hours} onChange={(event) => setDraft({ ...draft, hours: event.target.value })} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs text-white outline-none focus:border-[color:var(--accent)]" /></label>
      </div>
      <label className="mt-2 block text-xs text-white/55">Avaliação<textarea required value={draft.text} onChange={(event) => setDraft({ ...draft, text: event.target.value })} maxLength={4000} rows={4} className="mt-1 w-full resize-y rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs leading-relaxed text-white outline-none focus:border-[color:var(--accent)]" /></label>
      <div className="mt-2 flex justify-end gap-2"><button type="button" onClick={onCancel} className="rounded-lg border border-white/10 px-3 py-1.5 text-xs text-white/60 hover:bg-white/10">Cancelar</button><button type="submit" disabled={busy} className="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-black disabled:opacity-50">{busy ? "Salvando…" : editing ? "Salvar alterações" : "Publicar"}</button></div>
    </form>
  )
}

function CollectionForm({
  appid,
  gameTitle,
  draft,
  setDraft,
  editing,
  busy,
  onSubmit,
  onCancel,
}: {
  appid: string
  gameTitle: string
  draft: CollectionDraft
  setDraft: (draft: CollectionDraft) => void
  editing: boolean
  busy: boolean
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onCancel: () => void
}) {
  return (
    <form onSubmit={onSubmit} className="mb-3 rounded-xl border border-white/10 bg-black/25 p-3">
      <div className="grid gap-2 sm:grid-cols-[1fr_120px]">
        <label className="text-xs text-white/55">Título<input required value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} maxLength={120} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs text-white outline-none focus:border-[color:var(--accent)]" /></label>
        <label className="text-xs text-white/55">Visibilidade<select value={draft.visibility} onChange={(event) => setDraft({ ...draft, visibility: event.target.value as CollectionDraft["visibility"] })} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs text-white outline-none"><option value="public">Pública</option><option value="unlisted">Não listada</option><option value="private">Privada</option></select></label>
      </div>
      <label className="mt-2 block text-xs text-white/55">Descrição (opcional)<textarea value={draft.description} onChange={(event) => setDraft({ ...draft, description: event.target.value })} maxLength={2000} rows={3} className="mt-1 w-full resize-y rounded-lg border border-white/10 bg-black/30 px-2.5 py-2 text-xs leading-relaxed text-white outline-none focus:border-[color:var(--accent)]" /></label>
      {!editing && <p className="mt-2 text-[11px] text-white/35">A coleção nova começa com {gameTitle || `o jogo ${appid}`}.</p>}
      <div className="mt-2 flex justify-end gap-2"><button type="button" onClick={onCancel} className="rounded-lg border border-white/10 px-3 py-1.5 text-xs text-white/60 hover:bg-white/10">Cancelar</button><button type="submit" disabled={busy} className="rounded-lg bg-[color:var(--accent)] px-3 py-1.5 text-xs font-semibold text-black disabled:opacity-50">{busy ? "Salvando…" : editing ? "Salvar alterações" : "Criar"}</button></div>
    </form>
  )
}

function Pager({
  offset,
  hasMore: more,
  busy,
  onPrevious,
  onNext,
}: {
  offset: number
  hasMore: boolean
  busy: boolean
  onPrevious: () => void
  onNext: () => void
}) {
  if (!offset && !more) return null
  return (
    <nav className="mt-3 flex items-center justify-between border-t border-white/[0.07] pt-3" aria-label="Paginação">
      <button type="button" disabled={!offset || busy} onClick={onPrevious} className="rounded-lg border border-white/10 px-2.5 py-1.5 text-xs text-white/55 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-30">Anterior</button>
      <span className="text-[11px] text-white/35">Página {Math.floor(offset / PAGE_SIZE) + 1}</span>
      <button type="button" disabled={!more || busy} onClick={onNext} className="rounded-lg border border-white/10 px-2.5 py-1.5 text-xs text-white/55 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-30">Próxima</button>
    </nav>
  )
}

export default CommunityPanel
