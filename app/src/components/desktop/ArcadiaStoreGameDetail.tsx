"use client"

import { useEffect, useMemo, useRef, useState } from "react"
import Hls from "hls.js"
import type { NewsItem } from "../../global"
import type { Game } from "../ps5-launcher/types"
import { useFriends } from "../account/FriendsContext"
import { stripHtml } from "./GameDetailPanels"
import { RetroAchievementsGamePanel } from "./RetroAchievementsGamePanel"
import { AchievementsFullScreen } from "./AchievementsFullScreen"

type Info = {
  short_description?: string
  about?: string
  publishers?: string[]
  developers?: string[]
  release_date?: string
  languages?: string
  header?: string
  background?: string
  screenshots?: { thumb: string; full: string }[]
  movies?: { id: number; name: string; thumb: string; mp4: string; webm: string; hls?: string }[]
}

type Movie = NonNullable<Info["movies"]>[number]

function iniciarVideo(video: HTMLVideoElement) {
  // O trailer e sempre silencioso no inicio para respeitar a politica de
  // autoplay do Chromium. O usuario ainda pode ligar o som nos controles.
  video.defaultMuted = true
  video.muted = true
  void video.play().catch(() => {})
}

type Achievement = {
  title: string
  desc?: string
  icon?: string
  icongray?: string
  achieved?: boolean
}

export type ArcadiaStoreGameDetailAction = {
  label: string
  onClick: () => void
  disabled?: boolean
  kind?: "primary" | "outline" | "danger"
  icon?: "play" | "plus" | "download" | "settings" | "trash" | "stop"
}

export type ArcadiaRetroGameDetail = {
  systemId?: string
  platform?: string
  description?: string
  genres?: string[]
  releaseYear?: number | null
  developers?: string[]
  publishers?: string[]
  offerCount?: number
  availableCount?: number
  fileSize?: string
  sourceCount?: number
  links?: { label: string; onClick: () => void }[]
}

export function ArcadiaStoreGameDetail({
  appid,
  title,
  hero,
  header,
  info,
  game,
  busy,
  actions,
  onClose,
  retro,
  statusMessage,
}: {
  appid: string
  title: string
  hero: string
  header: string
  info: Info | null
  game?: Game
  busy: boolean
  actions: ArcadiaStoreGameDetailAction[]
  onClose: () => void
  retro?: ArcadiaRetroGameDetail
  statusMessage?: string
}) {
  const { data: friendsData } = useFriends()
  const isRetro = Boolean(retro)
  const [achievements, setAchievements] = useState<Achievement[] | null>(null)
  const [news, setNews] = useState<NewsItem[]>([])
  const [media, setMedia] = useState(hero)
  const [selectedMovie, setSelectedMovie] = useState<Movie | null>(null)
  const videoRef = useRef<HTMLVideoElement>(null)

  useEffect(() => {
    setMedia(hero)
    setSelectedMovie(null)
  }, [hero, appid])
  useEffect(() => {
    if (isRetro) {
      setAchievements([])
      return
    }
    let live = true
    window.launcherAPI?.achievementsGet(appid).then((items) => live && setAchievements(items || []))
    return () => { live = false }
  }, [appid, isRetro])
  useEffect(() => {
    if (isRetro) {
      setNews([])
      return
    }
    let live = true
    window.launcherAPI?.getGameNews(appid).then((items) => {
      if (live) setNews(Array.isArray(items) ? items.slice(0, 2) : [])
    })
    return () => { live = false }
  }, [appid, isRetro])

  const tags = useMemo(() => {
    const source = isRetro
      ? [retro?.platform, ...(retro?.genres || [])]
      : [game?.genre, ...(game?.categories || [])]
    const values = source.filter(Boolean).flatMap((value) => String(value).split(/[,/]/))
    return [...new Set(values.map((value) => value.trim()).filter(Boolean))].slice(0, 4)
  }, [game, isRetro, retro])
  const thumbs = useMemo(() => [
    ...(info?.movies || []).map((item) => ({ src: item.thumb, full: item.mp4 || item.webm || item.hls || "", movie: item, label: item.name })),
    ...(info?.screenshots || []).map((item) => ({ src: item.thumb, full: item.full, movie: null, label: title })),
  ].slice(0, 7), [info?.movies, info?.screenshots, title])
  const directMovieUrl = selectedMovie?.mp4 || selectedMovie?.webm || ""

  useEffect(() => {
    const video = videoRef.current
    if (!video || !selectedMovie) return

    let live = true
    const tentarIniciar = () => {
      if (live) iniciarVideo(video)
    }
    video.addEventListener("loadedmetadata", tentarIniciar)
    video.addEventListener("canplay", tentarIniciar)

    const limpar = () => {
      live = false
      video.removeEventListener("loadedmetadata", tentarIniciar)
      video.removeEventListener("canplay", tentarIniciar)
    }

    if (directMovieUrl) {
      tentarIniciar()
      return limpar
    }
    if (!selectedMovie.hls) return limpar
    if (!Hls.isSupported()) {
      video.src = selectedMovie.hls
      video.load()
      tentarIniciar()
      return limpar
    }
    const hls = new Hls()
    hls.on(Hls.Events.MANIFEST_PARSED, tentarIniciar)
    hls.loadSource(selectedMovie.hls)
    hls.attachMedia(video)
    return () => {
      limpar()
      hls.destroy()
    }
  }, [selectedMovie?.id, selectedMovie?.hls, directMovieUrl])

  const selectMedia = (item: (typeof thumbs)[number]) => {
    setMedia(item.full)
    setSelectedMovie(item.movie)
  }
  const done = achievements?.filter((item) => item.achieved).length || 0
  const total = achievements?.length || 0
  const progress = total ? Math.round(done / total * 100) : 0
  const friends = friendsData?.friends.slice(0, 5) || []
  const primary = actions.find((action) => action.kind === "primary")
  const secondary = actions.filter((action) => action !== primary)
  const description = stripHtml(retro?.description || info?.short_description || info?.about || game?.description || "")
  const developer = retro?.developers?.[0] || info?.developers?.[0] || game?.developer || (isRetro ? "Arcadia Retro" : "Arcadia Store")
  const platform = retro?.platform || game?.platform || (game?.launcher === "steam" ? "PC (Steam)" : "PC")
  const release = retro?.releaseYear ? String(retro.releaseYear) : info?.release_date || String(game?.year || "—")
  const links = isRetro
    ? [
        ...(retro?.links || []),
        { label: "RetroAchievements", onClick: () => window.launcherAPI?.openExternal("https://retroachievements.org/") },
        { label: "Configurar conquistas", onClick: () => window.launcherAPI?.openExternal("https://retroachievements.org/controlpanel.php") },
      ].slice(0, 4)
    : [
        { label: "Página na Loja", onClick: () => window.launcherAPI?.openExternal(`https://store.steampowered.com/app/${encodeURIComponent(appid)}/`) },
        { label: "Central da Comunidade", onClick: () => window.launcherAPI?.openExternal(`https://steamcommunity.com/app/${encodeURIComponent(appid)}`) },
        { label: "Guias", onClick: () => window.launcherAPI?.openExternal(`https://steamcommunity.com/app/${encodeURIComponent(appid)}/guides/`) },
        { label: "Suporte", onClick: () => window.launcherAPI?.openExternal("https://help.steampowered.com/") },
      ]

  return <div data-gamepad-cursor-surface className="arcadia-game-detail flex h-full min-h-0 flex-col overflow-hidden bg-[#030405] text-white">
    <header className="flex h-[50px] shrink-0 items-center gap-3 border-b border-white/[.09] bg-[#050608] px-4">
      <button ref={(node) => node?.focus()} onClick={onClose} className="detail-icon-btn" title="Voltar" aria-label="Voltar"><Icon name="back" /></button>
      <h1 className="min-w-0 flex-1 truncate text-[15px] font-semibold">{title}</h1>
      {statusMessage && <span role="status" className="max-w-[32%] truncate text-[10px] text-white/50">{statusMessage}</span>}
      <div className="flex items-center gap-2">
        {secondary.slice(0, 1).map((action) => <ActionButton key={action.label} action={action} />)}
        {primary && <ActionButton action={primary} />}
        {secondary.slice(1).map((action) => <ActionButton key={action.label} action={action} compact />)}
      </div>
    </header>

    <div data-gamepad-scroll className="min-h-0 flex-1 overflow-y-auto">
      <div className="detail-layout mx-auto grid max-w-[1500px] grid-cols-[minmax(0,1fr)_292px] gap-4 px-4 py-3">
        <main className="min-w-0 space-y-3">
          <section className="detail-hero relative h-[330px] overflow-hidden rounded-[7px] border border-white/[.1] bg-black">
            {selectedMovie ? <>
              <video ref={videoRef} key={selectedMovie.id} src={directMovieUrl || undefined} poster={selectedMovie.thumb} controls autoPlay muted playsInline preload="auto" className="absolute inset-0 h-full w-full bg-black object-contain" />
              <button onClick={() => { setSelectedMovie(null); setMedia(hero) }} className="detail-trailer-close absolute right-3 top-3 z-10 grid h-8 w-8 place-items-center rounded-full border border-white/25 bg-black/70 text-lg text-white/80 hover:text-white" title="Fechar trailer" aria-label="Fechar trailer">×</button>
            </> : isRetro ? (
              <img src={media} alt="" className="absolute inset-0 h-full w-full bg-black object-contain" onError={(event) => { event.currentTarget.src = header }} />
            ) : <>
              <img src={media} alt="" className="absolute inset-0 h-full w-full object-cover" onError={(event) => { event.currentTarget.src = header }} />
              <div className="pointer-events-none absolute inset-0 bg-[linear-gradient(90deg,rgba(3,4,5,.96)_0%,rgba(3,4,5,.82)_24%,rgba(3,4,5,.18)_58%,rgba(3,4,5,.12)_100%),linear-gradient(0deg,rgba(3,4,5,.82),transparent_42%)]" />
              <div className="absolute inset-y-0 left-0 flex w-[45%] min-w-[360px] flex-col justify-center px-9 py-7">
              <span className="mb-2 text-[10px] font-bold uppercase tracking-[.15em] text-[var(--desktop-violet)]">{developer}</span>
              <h2 className="game-name mb-3 max-w-[520px] text-[clamp(34px,4vw,58px)] font-black uppercase leading-[.92] tracking-[-.045em] text-white">{title}</h2>
              {tags.length > 0 && <div className="mb-3 flex flex-wrap gap-1.5">{tags.map((tag) => <span key={tag} className="rounded-[3px] border border-[var(--desktop-violet)]/70 bg-[var(--desktop-violet)]/10 px-2 py-1 text-[8px] font-bold uppercase tracking-[.08em] text-[var(--desktop-violet)]">{tag}</span>)}</div>}
              {description && <p className="line-clamp-3 max-w-[420px] text-[11px] leading-[1.6] text-white/66">{description}</p>}
              <div className="mt-4 flex items-center gap-2">
                {thumbs[0]?.movie && <button onClick={() => selectMedia(thumbs[0])} className="detail-hero-outline"><Icon name="play" /> Ver trailer</button>}
                <button className="detail-icon-btn" aria-label="Mais opções">•••</button>
              </div>
              </div>
            </>}
          </section>

          {thumbs.length > 0 && <section className="relative flex gap-2 overflow-x-auto px-5 pb-1">
            {thumbs.map((item, index) => <button key={`${item.src}-${index}`} onClick={() => selectMedia(item)} className={`detail-thumb relative aspect-video w-[184px] shrink-0 overflow-hidden rounded-[5px] border ${item.movie ? selectedMovie?.id === item.movie.id : !selectedMovie && media === item.full ? "border-[var(--desktop-green)]" : "border-white/[.12]"}`} title={item.label}>
              <img src={item.src} alt="" className="h-full w-full object-cover" />
              {!!item.movie && <span className="absolute inset-0 grid place-items-center bg-black/25"><span className="grid h-9 w-9 place-items-center rounded-full border border-white/50 bg-black/50"><Icon name="play" /></span></span>}
            </button>)}
          </section>}

          {retro ? (
            <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(260px,1fr)] gap-3">
              {retro.systemId ? (
                <RetroAchievementsGamePanel title={title} systemId={retro.systemId} compact />
              ) : (
                <DetailPanel className="h-[286px] overflow-hidden" title="Conquistas">
                  <p className="text-[11px] text-white/35">Sistema não identificado.</p>
                </DetailPanel>
              )}
              <DetailPanel className="h-[286px] overflow-hidden" title="Descrição">
                {description ? (
                  <p className="max-h-[230px] overflow-y-auto whitespace-pre-wrap pr-1 text-[11px] leading-[1.65] text-white/60">{description}</p>
                ) : (
                  <p className="text-[11px] text-white/35">Sem descrição disponível.</p>
                )}
              </DetailPanel>
            </div>
          ) : (
            <div className="grid grid-cols-[minmax(0,1.8fr)_minmax(260px,1fr)] gap-3">
              <CompactAchievements items={achievements} done={done} total={total} progress={progress} />
              <NewsPanel items={news} fallbackImage={header} />
            </div>
          )}
        </main>

        <aside className="space-y-3">
          <DetailPanel title="Progresso" right={<strong className="text-[var(--desktop-green)]">{progress}%</strong>}>
            <div className="mb-4 h-1 overflow-hidden rounded bg-white/10"><span className="block h-full bg-[linear-gradient(90deg,var(--desktop-violet),var(--desktop-green))]" style={{ width: `${progress}%` }} /></div>
            <dl className="grid grid-cols-[96px_1fr] gap-x-3 gap-y-3 text-[10px]">
              <Meta label="Tempo de jogo" value={formatPlaytime(game?.playtime_minutes)} />
              <Meta label="Última sessão" value={formatLastPlayed(game?.last_played)} />
              <Meta label="Plataforma" value={platform} />
              <Meta label="Lançamento" value={release} />
              <Meta label="Desenvolvedora" value={developer} />
              <Meta label="Distribuidora" value={retro?.publishers?.[0] || info?.publishers?.[0] || game?.publisher || "—"} />
              <Meta label="Modo de jogo" value={game?.players || "—"} />
              <Meta label="Idioma" value={languageSummary(info?.languages)} />
            </dl>
          </DetailPanel>

          <DetailPanel title="Amigos que jogam" right={<span className="text-[10px] text-[var(--desktop-violet)]">Ver todos</span>}>
            {friends.length ? <div className="space-y-2.5">{friends.map((friend, index) => <div key={friend.id} className="flex items-center gap-2.5">
              {friend.avatar_url ? <img src={friend.avatar_url} alt="" className="h-8 w-8 rounded-full border border-white/10 object-cover" /> : <span className="grid h-8 w-8 place-items-center rounded-full bg-[var(--desktop-violet)]/25 text-[11px] font-bold">{(friend.display_name || friend.username)[0]?.toUpperCase()}</span>}
              <div className="min-w-0"><div className="truncate text-[11px] font-medium text-white/85">{friend.display_name || friend.username}</div><div className={index < 2 ? "text-[9px] text-[var(--desktop-green)]" : "text-[9px] text-white/35"}>{index < 2 ? "Online" : "Amigo no Arcadia"}</div></div>
            </div>)}</div> : <p className="text-[10px] leading-relaxed text-white/35">Nenhum amigo disponível.</p>}
          </DetailPanel>

          {links.length > 0 && <DetailPanel title="Links rápidos">
            {links.map((link) => <QuickLink key={link.label} label={link.label} onClick={link.onClick} />)}
          </DetailPanel>}
        </aside>
      </div>
    </div>
  </div>
}

function CompactAchievements({ items, done, total, progress }: { items: Achievement[] | null; done: number; total: number; progress: number }) {
  const [allOpen, setAllOpen] = useState(false)
  useEffect(() => {
    if (!allOpen) return
    const closeWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setAllOpen(false)
    }
    window.addEventListener("keydown", closeWithEscape)
    return () => window.removeEventListener("keydown", closeWithEscape)
  }, [allOpen])

  return <>
    <DetailPanel className="h-[286px] overflow-hidden" title="Conquistas" right={<div className="flex items-center gap-3">
      <span className="text-[9px] text-white/45">{items ? `${done}/${total} desbloqueadas` : "Carregando…"}</span>
      {items && items.length > 6 && <button type="button" onClick={() => setAllOpen(true)} className="detail-achievements-all">Ver todas</button>}
    </div>}>
      <div className="mb-3 h-1 overflow-hidden rounded bg-white/10"><span className="block h-full bg-[var(--desktop-green)]" style={{ width: `${progress}%` }} /></div>
      {items?.length ? <div className="detail-achievement-grid grid gap-2 overflow-x-hidden pb-1 pr-1">{items.slice(0, 6).map((item, index) => <article key={`${item.title}-${index}`} className={`detail-achievement-card flex h-[210px] min-w-0 flex-col overflow-hidden rounded-[6px] border px-2 py-2.5 text-center ${item.achieved ? "border-[var(--desktop-green)]/45 bg-[var(--desktop-green)]/[.035]" : "border-white/[.08] bg-white/[.015]"}`}>
        <div className="detail-achievement-icon relative mx-auto mb-3 aspect-square w-16 shrink-0 overflow-hidden rounded-[5px] bg-white/5">{item.icon || item.icongray ? <img src={item.achieved ? item.icon : item.icongray || item.icon} alt="" className={`h-full w-full object-cover ${item.achieved ? "" : "opacity-55 sepia"}`} /> : null}</div>
        <h4 className={`line-clamp-2 text-[9px] font-semibold leading-[1.35] ${item.achieved ? "text-white/90" : "text-white/65"}`}>{item.title}</h4>
        <p className="mt-2 line-clamp-3 text-[7px] leading-[1.45] text-white/35">{item.desc || (item.achieved ? "Desbloqueada" : "Bloqueada")}</p>
      </article>)}</div> : <p className="py-8 text-center text-[10px] text-white/30">{items ? "Sem conquistas" : "Carregando conquistas…"}</p>}
    </DetailPanel>
    {allOpen && items?.length && <AchievementsFullScreen done={done} total={total} progress={progress} onClose={() => setAllOpen(false)}>
      <div className="detail-achievement-full-grid grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
        {items.map((item, index) => <article key={`${item.title}-${index}`} className={`detail-achievement-card flex min-h-[230px] min-w-0 flex-col overflow-hidden rounded-[6px] border px-3 py-3 text-center ${item.achieved ? "border-[var(--desktop-green)]/45 bg-[var(--desktop-green)]/[.035]" : "border-white/[.08] bg-white/[.015]"}`}>
          <div className="detail-achievement-icon relative mx-auto mb-4 aspect-square w-20 shrink-0 overflow-hidden rounded-[5px] bg-white/5">{item.icon || item.icongray ? <img src={item.achieved ? item.icon : item.icongray || item.icon} alt="" className={`h-full w-full object-cover ${item.achieved ? "" : "opacity-55 sepia"}`} /> : null}</div>
          <h4 className={`line-clamp-2 text-[11px] font-semibold leading-[1.35] ${item.achieved ? "text-white/90" : "text-white/65"}`}>{item.title}</h4>
          <p className="mt-2 line-clamp-4 text-[9px] leading-[1.45] text-white/35">{item.desc || (item.achieved ? "Desbloqueada" : "Bloqueada")}</p>
        </article>)}
      </div>
    </AchievementsFullScreen>}
  </>
}

function NewsPanel({ items, fallbackImage }: { items: NewsItem[]; fallbackImage: string }) {
  return <DetailPanel className="h-[286px] overflow-hidden" title="Novidades">
    {items.length ? <div className="space-y-2">{items.map((item) => <button key={item.id} onClick={() => window.launcherAPI?.openExternal(item.url)} className="detail-news-item flex w-full gap-3 rounded-[4px] border-b border-white/[.06] p-2.5 text-left last:border-0">
      <img src={item.image || fallbackImage} alt="" className="h-[82px] w-[120px] shrink-0 rounded-[4px] object-cover" />
      <span className="min-w-0"><time className="block text-[8px] text-white/35">{new Date(item.date).toLocaleDateString("pt-BR")}</time><strong className="detail-news-title mt-1 block text-[10px] leading-[1.3] text-white/85">{item.title}</strong><span className="detail-news-summary mt-1 block text-[8px] leading-[1.4] text-white/40">{item.summary}</span></span>
    </button>)}</div> : <p className="py-8 text-center text-[10px] text-white/30">Sem novidades relacionadas.</p>}
  </DetailPanel>
}

function DetailPanel({ title, right, children, className = "" }: { title: string; right?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return <section className={`rounded-[7px] border border-white/[.1] bg-[#080a0d] p-3.5 shadow-[0_8px_20px_rgba(0,0,0,.22)] ${className}`}>
    <header className="mb-3 flex items-center justify-between gap-2"><h3 className="text-[10px] font-bold uppercase tracking-[.09em] text-white/78">{title}</h3>{right}</header>{children}
  </section>
}

function Meta({ label, value }: { label: string; value: string }) { return <><dt className="uppercase text-[8px] tracking-[.06em] text-white/32">{label}</dt><dd className="min-w-0 truncate text-white/70" title={value}>{value}</dd></> }
function QuickLink({ label, onClick }: { label: string; onClick: () => void }) { return <button onClick={onClick} className="flex w-full items-center gap-2 border-b border-white/[.045] py-1.5 text-left text-[10px] text-white/58 last:border-0 hover:text-[var(--desktop-green)]"><Icon name="link" />{label}</button> }

function ActionButton({ action, compact, heroButton }: { action: ArcadiaStoreGameDetailAction; compact?: boolean; heroButton?: boolean }) {
  const kind = action.kind || "outline"
  const className = heroButton ? "detail-hero-primary" : compact ? "detail-icon-btn" : kind === "primary" ? "detail-action-primary" : kind === "danger" ? "detail-action-danger" : "detail-action-outline"
  return <button onClick={action.onClick} disabled={action.disabled} className={className} title={action.label}><Icon name={action.icon || "plus"} />{!compact && action.label}</button>
}

function Icon({ name }: { name: string }) {
  if (name === "back") return <svg viewBox="0 0 24 24"><path d="m15 18-6-6 6-6" /></svg>
  if (name === "play") return <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
  if (name === "download") return <svg viewBox="0 0 24 24"><path d="M12 3v12m-5-5 5 5 5-5M5 21h14" /></svg>
  if (name === "settings") return <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.4 1A8 8 0 0 0 15 6l-.3-2.6h-4L10.4 6a8 8 0 0 0-1.5.9l-2.4-1-2 3.4 2 1.5a7 7 0 0 0 0 2.2l-2 1.5 2 3.4 2.4-1a8 8 0 0 0 1.5.9l.3 2.6h4l.3-2.6a8 8 0 0 0 1.5-.9l2.4 1 2-3.4-2-1.5a7 7 0 0 0 .1-1Z" /></svg>
  if (name === "trash" || name === "stop") return <svg viewBox="0 0 24 24"><path d="M6 7h12M9 7V4h6v3m2 0-1 14H8L7 7" /></svg>
  if (name === "link") return <svg viewBox="0 0 24 24"><path d="M10 13a5 5 0 0 0 7 0l2-2a5 5 0 0 0-7-7l-1 1M14 11a5 5 0 0 0-7 0l-2 2a5 5 0 0 0 7 7l1-1" /></svg>
  return <svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
}

function formatPlaytime(minutes: unknown) { const n = Number(minutes); if (!Number.isFinite(n) || n <= 0) return "—"; return `${Math.floor(n / 60)}h ${Math.round(n % 60)}m` }
function formatLastPlayed(value: unknown) { const n = Number(value); if (!Number.isFinite(n) || n <= 0) return "—"; const date = new Date(n); return date.toDateString() === new Date().toDateString() ? "Hoje" : date.toLocaleDateString("pt-BR") }
function languageSummary(value?: string) { const text = stripHtml(value || "").replace(/\*/g, ""); if (!text) return "—"; const parts = text.split(/,\s*/).filter(Boolean); return parts.length > 2 ? `${parts.slice(0, 2).join(", ")}, +${parts.length - 2}` : parts.join(", ") }
