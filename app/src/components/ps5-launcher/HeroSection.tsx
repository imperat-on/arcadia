"use client"

import { useEffect, useState } from "react"
import type { Game } from "./types"
import { useI18n } from "../../i18n/I18nContext"
import { userLocale } from "../../i18n/locale"

interface HeroSectionProps {
  game: Game | null
  trailerUrl?: string | null
  rodando?: boolean
  abrindo?: boolean
  onLaunch: () => void
  onMore: () => void
  onToggleFavorite?: () => void
}

type AchievementProgress = { unlocked: number; total: number } | null

function formatPlaytime(minutes?: number) {
  if (!minutes) return "—"
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  return hours ? `${hours}h ${rest ? `${rest}m` : ""}`.trim() : `${rest}m`
}

function formatLastPlayed(timestamp?: number) {
  if (!timestamp) return "Nunca"
  return new Date(timestamp).toLocaleDateString(userLocale(), { day: "2-digit", month: "2-digit", year: "numeric" })
}

function InfoIcon({ path }: { path: string }) {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="h-4 w-4 shrink-0" aria-hidden="true"><path d={path} strokeLinecap="round" strokeLinejoin="round" /></svg>
}

export function HeroSection({ game, trailerUrl, rodando, abrindo, onLaunch, onMore, onToggleFavorite }: HeroSectionProps) {
  const { t } = useI18n()
  const [achievements, setAchievements] = useState<AchievementProgress>(null)

  useEffect(() => {
    let live = true
    setAchievements(null)
    if (!game || !window.launcherAPI) return

    if (game.retro && game.systemId && window.launcherAPI.retroachievementsGameProgress) {
      window.launcherAPI.retroachievementsGameProgress(game.title, game.systemId).then((result) => {
        if (!live || !result?.ok || !result.game) return
        setAchievements({ unlocked: result.game.numAwardedToUser || 0, total: result.game.numAchievements || 0 })
      }).catch(() => {})
    } else if (game.launcher === "steam" && window.launcherAPI.achievementsGet) {
      const appid = String(game.id).replace(/^steam:/, "")
      window.launcherAPI.achievementsGet(appid).then((items) => {
        if (!live || !items?.length) return
        setAchievements({ unlocked: items.filter((item) => item.achieved).length, total: items.length })
      }).catch(() => {})
    }
    return () => { live = false }
  }, [game?.id])

  if (!game) return <div className="retro-hero-section flex-1" />

  const progress = achievements?.total ? Math.round((achievements.unlocked / achievements.total) * 100) : null
  const platform = game.platform || game.systemId || game.launcher
  const players = game.players || (game.categories?.some((item) => /multi|coop/i.test(item)) ? "Multiplayer" : "Single Player")
  const score = game.rating ? Math.max(0, Math.min(5, game.rating)) : game.metacritic ? game.metacritic / 20 : 0

  return (
    <section key={game.id} data-theme-slot="home.hero" className="retro-hero-section anim-rise relative min-h-0 flex-1 border-t px-6 py-4">
      <div className="retro-game-dashboard grid h-full min-h-0 grid-cols-[1.08fr_.95fr_.82fr] gap-5">
        <article className="retro-game-copy flex min-w-0 flex-col justify-center pr-3">
          <span className="mb-3 text-[9px] font-black uppercase tracking-[0.12em] text-[var(--retro-violet)]">{game.developer || game.publisher || game.launcher}</span>
          {game.logo ? <img src={game.logo} alt={game.title} className="mb-3 max-h-14 max-w-[80%] object-contain object-left" /> : (
            <h1 className="game-name mb-2 line-clamp-2 font-display text-[clamp(2rem,3.4vw,4.1rem)] font-black uppercase leading-[.9] text-white">{game.title}</h1>
          )}
          <div className="mb-3 flex items-center gap-4">
            <strong className="font-display text-lg text-[var(--retro-phosphor)]">{game.year || "—"}</strong>
            {score > 0 && <span className="retro-stars text-sm tracking-[.12em]" aria-label={`Nota ${score.toFixed(1)} de 5`}>{Array.from({ length: 5 }).map((_, index) => <i key={index} data-filled={index + .5 < score}>★</i>)}</span>}
          </div>
          <p className="mb-4 line-clamp-3 max-w-[620px] text-[11px] leading-[1.7] text-white/60">{game.description || "Informações deste jogo serão exibidas aqui assim que os metadados estiverem disponíveis."}</p>
          <div className="flex flex-wrap gap-2">
            <button data-theme-action="launch" onClick={onLaunch} className={`retro-play-button inline-flex min-w-[116px] items-center justify-center gap-2 border px-5 py-3 text-[10px] font-black uppercase tracking-[0.12em] ${rodando ? "is-running" : ""}`}>
              {rodando ? <span className="h-3 w-3 bg-current" /> : <span className="retro-play-triangle" />}
              {rodando ? t("hero.parar") : abrindo ? t("hero.abrindo") : game.installed === false ? t("hero.instalar") : t("hero.jogar")}
            </button>
            <button data-theme-action="details" onClick={onMore} className="retro-outline-button border px-5 py-3 text-[9px] font-black uppercase tracking-[0.1em]">{t("gameoverview.detalhes")}</button>
            <button data-theme-action="favorite" onClick={onToggleFavorite} aria-pressed={Boolean(game.favorite)} className="retro-outline-button border px-4 py-3 text-[9px] font-black uppercase tracking-[0.08em]"><span className="mr-2 text-[var(--retro-violet)]">♥</span>{game.favorite ? "Favorito" : "Favoritar"}</button>
          </div>
        </article>

        <div className="retro-crt-frame self-center">
          <div className="retro-crt-screen">
            {trailerUrl ? (
              <video
                key={trailerUrl}
                className="retro-crt-video h-full w-full object-cover"
                src={trailerUrl}
                poster={game.hero || game.cover}
                autoPlay
                loop
                playsInline
                ref={(element) => {
                  if (element) element.volume = 0.4
                }}
              />
            ) : game.hero || game.cover ? (
              <img src={game.hero || game.cover} alt="" className="h-full w-full object-cover" draggable={false} />
            ) : (
              <div className="grid h-full place-items-center text-white/20"><LauncherIcon launcher={game.launcher} size={54} /></div>
            )}
            <div className="retro-crt-screen-lines" />
          </div>
          <div className="retro-crt-controls"><span /><span /><span /></div>
        </div>

        <aside className="flex min-h-0 flex-col justify-center gap-3">
          <section className="retro-info-card border p-4">
            <h2 className="mb-3 text-[10px] font-black uppercase tracking-[0.1em] text-[var(--retro-violet)]">Informações</h2>
            <dl className="space-y-2 text-[9px]">
              <InfoRow icon="M4 5h16v11H4zM8 20h8M12 16v4" label="Plataforma" value={String(platform)} />
              <InfoRow icon="M12 3v18M3 12h18" label="Gênero" value={game.genre || "—"} />
              <InfoRow icon="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8" label="Modo de jogo" value={players} />
              <InfoRow icon="M12 8v4l3 2M21 12a9 9 0 1 1-9-9" label="Última sessão" value={formatLastPlayed(game.last_played)} />
              <InfoRow icon="M12 2v4M12 18v4M4.9 4.9l2.8 2.8M16.3 16.3l2.8 2.8M2 12h4M18 12h4" label="Tempo de jogo" value={formatPlaytime(game.playtime_minutes)} />
            </dl>
          </section>

          <section className="retro-achievement-card border p-4">
            <h2 className="mb-3 text-[10px] font-black uppercase tracking-[0.1em] text-[var(--retro-violet)]">Conquistas</h2>
            <div className="flex items-center gap-3">
              <span className="text-xl text-[var(--retro-phosphor)]">♜</span>
              <strong className="min-w-[46px] text-xs text-white">{achievements ? `${achievements.unlocked}/${achievements.total}` : "—"}</strong>
              <div className="retro-progress-track h-2 flex-1 overflow-hidden"><span style={{ width: `${progress || 0}%` }} /></div>
            </div>
            <span className="mt-2 block pl-9 text-[9px] font-black text-[var(--retro-phosphor)]">{progress == null ? "Sem dados" : `${progress}%`}</span>
          </section>
        </aside>
      </div>
    </section>
  )
}

function InfoRow({ icon, label, value }: { icon: string; label: string; value: string }) {
  return <div className="grid grid-cols-[18px_98px_1fr] items-center gap-1.5"><span className="text-[var(--retro-violet)]"><InfoIcon path={icon} /></span><dt className="font-black uppercase tracking-[.06em] text-white/45">{label}</dt><dd className="truncate text-right text-white/75">{value}</dd></div>
}

export function LauncherIcon({ launcher, size = 14 }: { launcher: string; size?: number }) {
  if (launcher === "steam") return <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M12 0A12 12 0 0 0 .02 11.04l6.43 2.66a3.4 3.4 0 0 1 2.1-.59l2.86-4.14v-.06a4.53 4.53 0 1 1 4.42 4.53l-4.07 2.91v.16a3.39 3.39 0 0 1-6.72.67L.44 15.27A12 12 0 1 0 12 0Zm-4.46 18.21-1.47-.61a2.55 2.55 0 1 0 3.27-3.46 2.53 2.53 0 0 0-1.88-.03l1.53.63a1.88 1.88 0 1 1-1.45 3.47Zm8.4-6.29a3.02 3.02 0 1 1 0-6.03 3.02 3.02 0 0 1 0 6.03Zm0-.75a2.27 2.27 0 1 0 0-4.53 2.27 2.27 0 0 0 0 4.53Z"/></svg>
  if (launcher === "heroic") return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 2 2 7l10 5 10-5-10-5ZM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M20 4H4a2 2 0 0 0-2 2v12c0 1.1.9 2 2 2h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2Zm-8 3a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7Zm7 13H5v-.23c0-.62.28-1.2.76-1.58C7.47 17.18 9.64 17 12 17s4.53.18 6.24 1.19c.48.38.76.97.76 1.58V20Z"/></svg>
}
