"use client"

import { forwardRef, useEffect, useMemo, useRef, useState } from "react"
import type { NewsItem } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { userLocale } from "../../i18n/locale"
import type { Game } from "./types"

type OverviewInfo = {
  short_description?: string
  about?: string
  publishers?: string[]
  developers?: string[]
  release_date?: string
  languages?: string
  header?: string
  background?: string
  screenshots?: { thumb: string; full: string }[]
  movies?: { id: number; name: string; thumb: string; mp4?: string; webm?: string }[]
}

type Achievement = {
  title: string
  desc?: string
  icon?: string
  icongray?: string
  achieved?: boolean
}

type MediaItem = {
  src: string
  full: string
  label: string
  trailer?: boolean
}

interface GameOverviewProps {
  game: Game
  news: NewsItem[]
  appFocused?: boolean
  rodando?: boolean
  abrindo?: boolean
  closing?: boolean
  onClose: () => void
  onLaunch: (game: Game) => void
  onOpenNews: (url: string) => void
}

type OverviewGame = Game & {
  screenshots?: string[]
  titleScreens?: string[]
}

const unique = (values: (string | undefined | null)[]) => [
  ...new Set(values.filter((value): value is string => Boolean(value))),
]

function cleanText(value?: string): string {
  return (value || "")
    .replace(/<br\s*\/?>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"')
    .replace(/&#039;/g, "'")
    .replace(/\s+/g, " ")
    .trim()
}

function formatPlaytime(minutes?: number): string {
  if (!minutes) return "—"
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  if (!hours) return `${rest} min`
  return `${hours}h${rest ? ` ${rest}m` : ""}`
}

function formatLastPlayed(timestamp?: number): string {
  if (!timestamp) return "Nunca"
  return new Date(timestamp).toLocaleDateString(userLocale(), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  })
}

function timeSince(date: string): string {
  const stamp = new Date(date).getTime()
  if (!Number.isFinite(stamp)) return ""
  const minutes = Math.max(0, Math.floor((Date.now() - stamp) / 60000))
  if (minutes < 60) return `${Math.max(1, minutes)} min atrás`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h atrás`
  return `${Math.floor(hours / 24)}d atrás`
}

export const GameOverview = forwardRef<HTMLDivElement, GameOverviewProps>(function GameOverview(
  { game, news, appFocused = true, rodando, abrindo, closing, onClose, onLaunch, onOpenNews },
  ref,
) {
  const { t } = useI18n()
  const mediaGame = game as OverviewGame
  const [meta, setMeta] = useState<OverviewInfo | null>(null)
  const [trailer, setTrailer] = useState<string | null>(null)
  const [media, setMedia] = useState("hero")
  const [achievements, setAchievements] = useState<Achievement[] | null>(null)
  const [retroProgress, setRetroProgress] = useState<{ unlocked: number; total: number } | null>(null)
  const backRef = useRef<HTMLButtonElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => backRef.current?.focus({ preventScroll: true }))
    return () => window.cancelAnimationFrame(frame)
  }, [game.id])

  useEffect(() => {
    let live = true
    setMeta(null)
    const api = window.launcherAPI
    if (!api) return () => { live = false }
    api.gameSysinfo(game).then((result) => {
      if (live) setMeta(result?.info || null)
    }).catch(() => {})
    return () => { live = false }
  }, [game.id])

  useEffect(() => {
    let live = true
    setTrailer(null)
    const api = window.launcherAPI
    if (!api) return () => { live = false }
    api.trailerPath(game.id).then((result) => {
      if (live) setTrailer(result?.path || null)
    }).catch(() => {})
    return () => { live = false }
  }, [game.id])

  useEffect(() => {
    if (trailer || !meta?.movies?.length) return
    const movie = meta.movies[0]
    const source = movie.mp4 || movie.webm || null
    if (source) setTrailer(source)
  }, [meta, trailer])

  useEffect(() => {
    let live = true
    setAchievements(null)
    setRetroProgress(null)
    const api = window.launcherAPI
    if (!api) return () => { live = false }

    if (game.retro && game.systemId && api.retroachievementsGameProgress) {
      api.retroachievementsGameProgress(game.title, game.systemId).then((result) => {
        if (live && result?.game) {
          setRetroProgress({
            unlocked: result.game.numAwardedToUser || 0,
            total: result.game.numAchievements || 0,
          })
        }
      }).catch(() => {})
      return () => { live = false }
    }

    if (game.launcher !== "steam") {
      setAchievements([])
      return () => { live = false }
    }

    const appid = String(game.id).replace(/^steam:/, "")
    api.achievementsGet(appid).then((items) => {
      if (live) setAchievements(Array.isArray(items) ? items : [])
    }).catch(() => {
      if (live) setAchievements([])
    })
    return () => { live = false }
  }, [game.id, game.launcher, game.retro, game.systemId, game.title])

  useEffect(() => {
    setMedia("hero")
  }, [game.id])

  useEffect(() => {
    const video = videoRef.current
    if (!video || media !== "trailer" || !trailer || !appFocused) return
    video.muted = true
    void video.play().catch(() => {})
  }, [media, trailer, appFocused])

  const backdrop = game.hero || meta?.background || meta?.header || game.cover || ""
  const cover = game.cover || meta?.header || backdrop
  const description = cleanText(game.description || meta?.short_description || meta?.about) || t("gameoverview.sem_descricao")
  const developer = game.developer || meta?.developers?.[0] || game.publisher || game.launcher
  const publisher = game.publisher || meta?.publishers?.[0] || "—"
  const platform = game.platform || game.systemId || game.launcher
  const release = game.year || meta?.release_date || "—"
  const tags = unique([game.genre, ...(game.categories || [])])
    .flatMap((value) => value.split(/[,/]/).map((item) => item.trim()))
    .filter(Boolean)
    .slice(0, 4)

  const screenshots = useMemo(() => unique([
    ...(mediaGame.screenshots || []),
    ...(mediaGame.titleScreens || []),
    ...(meta?.screenshots || []).flatMap((shot) => [shot.full, shot.thumb]),
  ]).filter((image) => image !== backdrop).slice(0, 5), [mediaGame.screenshots, mediaGame.titleScreens, meta?.screenshots, backdrop])

  const mediaItems = useMemo<MediaItem[]>(() => [
    ...(trailer ? [{
      src: meta?.movies?.[0]?.thumb || backdrop || cover,
      full: "trailer",
      label: t("gameoverview.trailer"),
      trailer: true,
    }] : []),
    ...screenshots.map((image, index) => ({
      src: image,
      full: image,
      label: `Imagem ${index + 1}`,
    })),
  ].filter((item): item is MediaItem => Boolean(item.src)), [trailer, meta?.movies, backdrop, cover, screenshots, t])

  const relatedNews = useMemo(() => {
    const words = game.title.toLocaleLowerCase().split(/\s+/).filter((word) => word.length > 3)
    const related = news.filter((item) => words.some((word) =>
      `${item.title} ${item.summary}`.toLocaleLowerCase().includes(word),
    ))
    return (related.length ? related : news).slice(0, 2)
  }, [game.title, news])

  const unlocked = retroProgress?.unlocked ?? achievements?.filter((item) => item.achieved).length ?? 0
  const total = retroProgress?.total ?? achievements?.length ?? 0
  const progress = total ? Math.round((unlocked / total) * 100) : 0
  const preview = media !== "trailer" && media !== "hero" ? media : backdrop || cover
  const showingTrailer = media === "trailer" && Boolean(trailer) && appFocused

  return (
    <div
      ref={ref}
      data-theme-slot="overview.root"
      className={`arcadia-overview gp-scope fixed inset-0 z-[70] overflow-hidden text-white ${closing ? "is-closing" : ""}`}
      role="dialog"
      aria-modal="true"
      aria-label={`${t("gameoverview.detalhes")}: ${game.title}`}
    >
      <div className="arcadia-overview__backdrop" style={{ backgroundImage: backdrop ? `url(${backdrop})` : undefined }} />
      <div className="arcadia-overview__backdrop-blur" style={{ backgroundImage: backdrop ? `url(${backdrop})` : undefined }} />
      <div className="arcadia-overview__wash" />
      <div className="arcadia-overview__grid" />
      <div className="arcadia-overview__scanlines" />
      <div className="arcadia-overview__sweep" />

      <header className="arcadia-overview__top">
        <button ref={backRef} type="button" data-theme-action="back" onClick={onClose} className="arcadia-overview__back-button">
          <span className="arcadia-overview__key">B</span>
          <span>{t("gameoverview.controle.voltar")}</span>
        </button>
        <div className="arcadia-overview__brand" aria-hidden="true">
          <strong>ARCADIA</strong>
          <span>GAME HUB / {String(game.launcher).toUpperCase()}</span>
        </div>
        <span className="arcadia-overview__signal"><i /> ONLINE // {String(platform).toUpperCase()}</span>
      </header>

      <main className="arcadia-overview__layout">
        <section className="arcadia-overview__hero">
          <div className="arcadia-overview__cover-column">
            <div className="arcadia-overview__cover-halo" />
            <div className="arcadia-overview__cover-card">
              {cover ? <img src={cover} alt="" draggable={false} /> : <span>{game.title}</span>}
              <div className="arcadia-overview__cover-shade" />
              <span className="arcadia-overview__cover-label">{game.installed === false ? "NÃO INSTALADO" : "NA BIBLIOTECA"}</span>
            </div>
            <span className="arcadia-overview__cover-index">ARC // {String(game.id).replace(/^steam:/, "").slice(0, 12)}</span>
          </div>

          <div className="arcadia-overview__identity">
            <span className="arcadia-overview__eyebrow">{developer} <b>//</b> JOGO SELECIONADO</span>
            {game.logo ? <img src={game.logo} alt={game.title} className="arcadia-overview__logo" /> : <h1>{game.title}</h1>}
            <div className="arcadia-overview__tags">
              {tags.map((tag) => <span key={tag}>{tag}</span>)}
              <span>{release}</span>
            </div>
            <p className="arcadia-overview__description">{description}</p>
            <div className="arcadia-overview__actions">
              <button type="button" onClick={() => onLaunch(game)} className="arcadia-overview__action arcadia-overview__action--primary">
                <span className="arcadia-overview__action-icon">{rodando ? "■" : "▶"}</span>
                {rodando ? t("hero.parar") : abrindo ? t("hero.abrindo") : game.installed === false ? t("hero.instalar") : t("gameoverview.jogar_agora")}
              </button>
              {trailer && <button type="button" onClick={() => setMedia("trailer")} className="arcadia-overview__action">
                <span className="arcadia-overview__action-icon">▷</span>
                {t("gameoverview.trailer")}
              </button>}
            </div>
            <div className="arcadia-overview__controls"><span><b>A</b> selecionar</span><span><b>B</b> voltar</span><span><b>R1</b> próxima aba</span></div>
          </div>
        </section>

        <aside className="arcadia-overview__side">
          <section className="arcadia-overview__panel arcadia-overview__progress-panel">
            <div className="arcadia-overview__panel-heading"><span>PROGRESSO // JORNADA</span><strong>{progress}%</strong></div>
            <div className="arcadia-overview__progress-line"><span style={{ width: `${progress}%` }} /></div>
            <div className="arcadia-overview__progress-copy"><span>{total ? `${unlocked}/${total}` : "—"} CONQUISTAS</span><span>{game.playtime_minutes ? formatPlaytime(game.playtime_minutes) : "NOVA SESSÃO"}</span></div>
          </section>

          <section className="arcadia-overview__preview">
            <div className="arcadia-overview__preview-media">
              {showingTrailer && trailer ? <video ref={videoRef} key={trailer} src={trailer} poster={backdrop || cover} autoPlay loop muted playsInline /> : preview ? <img key={preview} src={preview} alt="" draggable={false} /> : <span className="arcadia-overview__preview-empty">SEM ARTE DISPONÍVEL</span>}
              <div className="arcadia-overview__preview-shade" />
              <span className="arcadia-overview__preview-status">{showingTrailer ? "TRAILER // PLAYING" : "SIGNAL // READY"}</span>
              <span className="arcadia-overview__preview-code">{String(game.id).slice(0, 8).toUpperCase()}</span>
            </div>
          </section>

          <section className="arcadia-overview__panel arcadia-overview__data-panel">
            <div className="arcadia-overview__panel-heading"><span>DADOS DO JOGO</span><span className="arcadia-overview__panel-dot" /></div>
            <DataRow label="Plataforma" value={String(platform)} />
            <DataRow label="Última sessão" value={formatLastPlayed(game.last_played)} />
            <DataRow label="Desenvolvedora" value={developer} />
            <DataRow label="Distribuidora" value={publisher} />
          </section>
        </aside>

        <section className="arcadia-overview__activity">
          <div className="arcadia-overview__activity-heading"><span>ATIVIDADES</span><small>SELECIONE UMA ATIVIDADE</small></div>
          <div className="arcadia-overview__activity-rail">
            <button type="button" onClick={() => onLaunch(game)} className="arcadia-overview__activity-card arcadia-overview__activity-card--launch">
              <span className="arcadia-overview__activity-mark">▶</span>
              <span><b>{rodando ? "JOGO EM EXECUÇÃO" : "CONTINUAR JOGANDO"}</b><small>{game.playtime_minutes ? formatPlaytime(game.playtime_minutes) : "Começar uma nova sessão"}</small></span>
              <em>A</em>
            </button>
            {mediaItems.map((item) => <button type="button" key={`${item.full}-${item.label}`} onClick={() => setMedia(item.full)} className={`arcadia-overview__activity-card arcadia-overview__activity-card--media ${media === item.full ? "is-active" : ""}`}>
              <span className="arcadia-overview__activity-thumb"><img src={item.src} alt="" draggable={false} />{item.trailer && <i>▶</i>}</span>
              <span><b>{item.label}</b><small>{item.trailer ? "Assistir agora" : "Abrir captura"}</small></span>
            </button>)}
            {relatedNews.map((item) => <button type="button" key={item.id} onClick={() => onOpenNews(item.url)} className="arcadia-overview__activity-card arcadia-overview__activity-card--news">
              <span className="arcadia-overview__activity-mark">✦</span>
              <span><b>{item.title}</b><small>{item.source} {timeSince(item.date) && `// ${timeSince(item.date)}`}</small></span>
            </button>)}
            {!mediaItems.length && !relatedNews.length && <div className="arcadia-overview__empty-activity">Nenhuma atividade adicional disponível.</div>}
          </div>
        </section>
      </main>
    </div>
  )
})

function DataRow({ label, value }: { label: string; value: string }) {
  return <div className="arcadia-overview__data-row"><span>{label}</span><strong>{value}</strong></div>
}
