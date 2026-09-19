"use client"

import { forwardRef, useEffect, useMemo, useRef, useState } from "react"
import type { NewsItem } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { userLocale } from "../../i18n/locale"
import type { Game } from "./types"
import { horasCombinadas, useSteamHoras } from "../steamHoras"
import { useFriends } from "../account/FriendsContext"

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
  visible?: boolean
  rodando?: boolean
  abrindo?: boolean
  closing?: boolean
  onClose: () => void
  onLaunch: (game: Game) => void
  onOpenNews: (url: string) => void
  onMore?: () => void
  onOpenFriends?: () => void
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
  { game, news, appFocused = true, visible = true, rodando, abrindo, closing, onClose, onLaunch, onOpenNews, onMore, onOpenFriends },
  ref,
) {
  const { t } = useI18n()
  const { data: friendsData } = useFriends()
  const steamHoras = useSteamHoras()
  const minutes = horasCombinadas(steamHoras, [game.id, game.appid], game.playtime_minutes)
  const mediaGame = game as OverviewGame
  const [meta, setMeta] = useState<OverviewInfo | null>(null)
  const [trailer, setTrailer] = useState<string | null>(null)
  const [media, setMedia] = useState("hero")
  const [achievements, setAchievements] = useState<Achievement[] | null>(null)
  const [retroProgress, setRetroProgress] = useState<{ unlocked: number; total: number } | null>(
    null,
  )
  // Consultas IPC e leitura de trailer são adiadas até o painel terminar de
  // subir. Dispará-las no primeiro frame roubava tempo da animação.
  const [ready, setReady] = useState(false)
  const backRef = useRef<HTMLButtonElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)

  useEffect(() => {
    if (!visible) return
    const frame = window.requestAnimationFrame(() =>
      backRef.current?.focus({ preventScroll: true }),
    )
    return () => window.cancelAnimationFrame(frame)
  }, [game.id, visible])

  useEffect(() => {
    setReady(false)
    if (!visible) return
    const timer = window.setTimeout(() => setReady(true), 150)
    return () => window.clearTimeout(timer)
  }, [game.id, visible])

  useEffect(() => {
    let live = true
    setMeta(null)
    if (!ready) return () => { live = false }
    const api = window.launcherAPI
    if (!api)
      return () => {
        live = false
      }
    api
      .gameSysinfo(game)
      .then((result) => {
        if (live) setMeta(result?.info || null)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [game.id, ready])

  useEffect(() => {
    let live = true
    setTrailer(null)
    if (!ready) return () => { live = false }
    const api = window.launcherAPI
    if (!api)
      return () => {
        live = false
      }
    api
      .trailerPath(game.id)
      .then((result) => {
        if (live) setTrailer(result?.path || null)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [game.id, ready])

  useEffect(() => {
    if (trailer || !meta?.movies?.length) return
    const movie = meta.movies[0]
    const source = movie.mp4 || movie.webm || null
    if (source) setTrailer(source)
  }, [meta, trailer])

  useEffect(() => {
    let live = true
    let request = 0
    setAchievements(null)
    setRetroProgress(null)
    if (!ready) return () => { live = false }
    const api = window.launcherAPI
    if (!api) { setAchievements([]); return }
    const appid = String(game.appid || game.id).replace(/^steam:/, "")
    const refresh = async () => {
      const current = ++request
      try {
        if (game.retro && game.systemId && api.retroachievementsGameProgress) {
          const result = await api.retroachievementsGameProgress(game.title, game.systemId)
          if (!live || current !== request) return
          setAchievements((result?.achievements || []).map(item => ({ title: item.title, desc: item.description, icon: item.badgeUrl, icongray: item.badgeLockedUrl, achieved: item.unlocked })))
          setRetroProgress(result?.game ? { unlocked: result.game.numAwardedToUser || 0, total: result.game.numAchievements || 0 } : null)
        } else if (game.launcher === "steam") {
          const items = await api.achievementsGet(appid)
          if (live && current === request) setAchievements(Array.isArray(items) ? items : [])
        } else if (live) setAchievements([])
      } catch {
        // A transient refresh failure must not erase the last known progress.
        if (live && current === request) setAchievements(items => items ?? [])
      }
    }
    void refresh()
    // The same IPC as the desktop panel; re-read the authoritative list to
    // preserve schema aliases instead of matching duplicate display titles.
    const offUnlock = api.onAchievementUnlocked?.(payload => {
      if (!game.retro && payload.appid === appid) void refresh()
    })
    const offLibrary = api.onLibraryChanged?.(() => { void refresh() })
    const offSync = api.onSyncState?.(() => { void refresh() })
    // RA has no unlock event in the launcher. Returning from the emulator
    // queries the service again, as does reopening the hub.
    const offFocus = api.onAppFocus?.(focused => { if (focused) void refresh() })
    const onFocus = () => { void refresh() }
    if (!api.onAppFocus) window.addEventListener("focus", onFocus)
    return () => {
      live = false
      offUnlock?.()
      offLibrary?.()
      offSync?.()
      offFocus?.()
      window.removeEventListener("focus", onFocus)
    }
  }, [game.id, game.appid, game.launcher, game.retro, game.systemId, game.title, ready])

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
  const description =
    cleanText(game.description || meta?.short_description || meta?.about) ||
    t("gameoverview.sem_descricao")
  const developer = game.developer || meta?.developers?.[0] || game.publisher || game.launcher
  const publisher = game.publisher || meta?.publishers?.[0] || "—"
  const platform = game.platform || game.systemId || game.launcher
  const release = game.year || meta?.release_date || "—"
  const tags = unique([game.genre, ...(game.categories || [])])
    .flatMap((value) => value.split(/[,/]/).map((item) => item.trim()))
    .filter(Boolean)
    .slice(0, 4)

  const screenshots = useMemo(() => {
    const local = unique([
      ...(mediaGame.screenshots || []),
      ...(mediaGame.titleScreens || []),
    ]).filter((image) => image !== backdrop)
    const localFulls = new Set(local)
    const remote = (meta?.screenshots || [])
      .map((shot) => ({ src: shot.thumb || shot.full, full: shot.full || shot.thumb }))
      .filter((shot) => shot.src && shot.full && shot.src !== backdrop && !localFulls.has(shot.full))
    return [...local.map((image) => ({ src: image, full: image })), ...remote].slice(0, 5)
  }, [mediaGame.screenshots, mediaGame.titleScreens, meta?.screenshots, backdrop])

  const mediaItems = useMemo<MediaItem[]>(
    () =>
      [
        ...(trailer
          ? [
              {
                src: meta?.movies?.[0]?.thumb || backdrop || cover,
                full: "trailer",
                label: t("gameoverview.trailer"),
                trailer: true,
              },
            ]
          : []),
        ...screenshots.map((image, index) => ({
          src: image.src,
          full: image.full,
          label: `Imagem ${index + 1}`,
        })),
      ].filter((item): item is MediaItem => Boolean(item.src)),
    [trailer, meta?.movies, backdrop, cover, screenshots, t],
  )

  const relatedNews = useMemo(() => {
    const words = game.title
      .toLocaleLowerCase()
      .split(/\s+/)
      .filter((word) => word.length > 3)
    const related = news.filter((item) =>
      words.some((word) => `${item.title} ${item.summary}`.toLocaleLowerCase().includes(word)),
    )
    return (related.length ? related : news).slice(0, 2)
  }, [game.title, news])

  const unlocked =
    retroProgress?.unlocked ?? achievements?.filter((item) => item.achieved).length ?? 0
  const total = retroProgress?.total ?? achievements?.length ?? 0
  const progress = total ? Math.round((unlocked / total) * 100) : 0
  const preview = media !== "trailer" && media !== "hero" ? media : backdrop || cover
  const showingTrailer = media === "trailer" && Boolean(trailer) && appFocused
  const overviewTime = new Date().toLocaleTimeString(userLocale(), { hour: "2-digit", minute: "2-digit" })

  return (
    <div
      ref={ref}
      className={`arcadia-overview gp-scope fixed inset-0 z-[70] overflow-hidden text-white ${closing ? "is-closing" : ""} ${!visible ? "is-hidden" : ""}`}
      role="dialog"
      aria-modal="true"
      aria-label={`${t("gameoverview.detalhes")}: ${game.title}`}
      onKeyDown={(event) => {
        if (event.key !== "Tab") return
        const targets = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not([disabled]),video[controls],[tabindex="0"]')).filter(el => el.getBoundingClientRect().width > 0)
        const first = targets[0]
        const last = targets[targets.length - 1]
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
      }}
    >
      <div className="ps5-overview-bg" style={{ backgroundImage: backdrop ? `url(${backdrop})` : undefined }} />
      <div className="ps5-overview-wash" />
      <header className="ps5-overview-top">
        <button ref={backRef} type="button" onClick={onClose} className="ps5-overview-back">{t("gameoverview.controle.voltar")}</button>
        <div className="ps5-overview-game"><div className="ps5-overview-icon">{cover && <img src={cover} alt="" />}</div><span>{game.title}</span></div>
        <time className="ps5-overview-time-top">{overviewTime}</time>
      </header>
      <main key={game.id} className="ps5-overview-main" data-gamepad-scroll>
        <section className="ps5-overview-activity">
          <span className="ps5-overview-badge">{tags[0] || String(platform)}</span>
          {game.logo ? <img src={game.logo} alt={game.title} className="ps5-overview-logo" /> : <h1>{game.title}</h1>}
          <p>{minutes ? t("gameoverview.continuar", { tempo: formatPlaytime(minutes) }) : description}</p>
          <div className="ps5-overview-actions">
            <button
              type="button"
              onClick={() => onLaunch(game)}
              data-game-action={rodando ? "stop" : abrindo ? "cancel" : undefined}
              aria-label={
                rodando
                  ? t("hero.parar")
                  : abrindo
                    ? t("common.cancelar")
                    : game.installed === false
                      ? t("hero.instalar")
                      : t("gameoverview.jogar_agora")
              }
              className={`ps5-overview-play ${abrindo ? "is-opening" : ""}`}
            >
              {rodando ? "■" : abrindo ? "×" : "▶"}{" "}
              {rodando
                ? t("hero.parar")
                : abrindo
                  ? t("common.cancelar")
                  : game.installed === false
                    ? t("hero.instalar")
                    : t("gameoverview.jogar_agora")}
            </button>
            <button type="button" onClick={onMore} className="ps5-overview-more" aria-label={t("console.options")}>•••</button>
            {trailer && <button type="button" onClick={() => setMedia("trailer")} className="ps5-overview-link">{t("gameoverview.trailer")}</button>}
          </div>
        </section>
        <aside className="ps5-overview-product">
          {cover ? <img src={cover} alt="" /> : <span>{game.title}</span>}
          <strong>{game.installed === false ? t("hero.instalar") : t("store.na_biblioteca")}</strong>
          <small>{release} · {developer}</small>
        </aside>
        <div className="ps5-overview-stats">
          <section className="ps5-overview-progress"><span>◔ {t("gameoverview.progresso")}</span><b>{progress}%</b><i><em style={{ width: `${progress}%` }} /></i><small>{total ? t("gameoverview.desbloqueadas", { done: unlocked, total }) : t("conquistas.vazio")}</small></section>
          <section className="ps5-overview-time">◷ {minutes ? t("gameoverview.jogado", { tempo: formatPlaytime(minutes) }) : t("gameoverview.nao_jogado")}</section>
        </div>
        <section className="console-achievements">
          <h2>{t("conquistas.titulo")} <small>{total ? `${unlocked} / ${total}` : ""}</small></h2>
          {achievements === null ? <p role="status">{t("common.carregando")}</p> : achievements.length ? <div className="console-achievement-grid">{achievements.map((item, index) => <article key={`${item.title}-${index}`} tabIndex={0} data-unlocked={Boolean(item.achieved)}>
            {(item.icon || item.icongray) && <img src={item.achieved ? item.icon : item.icongray || item.icon} alt="" loading="lazy" />}
            <div><h3>{item.title}</h3><p>{item.desc}</p><small>{t(item.achieved ? "console.unlocked" : "console.locked")}</small></div>
          </article>)}</div> : <p>{t("conquistas.vazio")}</p>}
        </section>
        {mediaItems.length > 0 && <section className="console-media"><h2>{t("gameoverview.trailer")}</h2><div>{mediaItems.map(item => <button type="button" key={item.full} onClick={() => setMedia(item.full)} aria-label={item.label}><img src={item.src} alt=""/><span>{item.label}</span></button>)}</div>{media !== "hero" && (showingTrailer && trailer ? <video ref={videoRef} src={trailer} controls autoPlay muted playsInline /> : <img className="console-media-preview" src={preview} alt={game.title} />)}</section>}
        <section className="console-related"><h2>{t("gameoverview.detalhes")}</h2><p>{description}</p><p>{[developer, publisher, platform, release, ...tags].filter(Boolean).join(" · ")}</p></section>
        <section className="console-related"><h2>{t("console.friends")}</h2><button type="button" onClick={onOpenFriends}>{friendsData?.friends?.length ? friendsData.friends.map(friend => friend.display_name || friend.username).join(" · ") : t("gameoverview.nenhum_amigo")}</button></section>
        {relatedNews.length > 0 && <section className="console-related"><h2>{t("topbar.noticias")}</h2>{relatedNews.map(item => <button key={item.url} type="button" onClick={() => onOpenNews(item.url)}>{item.title}</button>)}</section>}

      </main>
    </div>
  )
})

function DataRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="arcadia-overview__data-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}
