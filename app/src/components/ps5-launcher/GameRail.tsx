"use client"

import { useEffect, useLayoutEffect, useRef } from "react"
import { useI18n } from "../../i18n/I18nContext"
import type { Game } from "./types"
import { LauncherIcon } from "./HeroSection"
import { isRovingKey, nextRovingIndex } from "./rovingTab.mjs"
import { signalRailEdge } from "./railEdgeMotion"

interface GameRailProps {
  games: Game[]
  selectedIndex: number
  cardScale?: number
  /** Arte 1:1 por id de jogo (Xbox/Worker, resolvida no main); ausente = capa normal. */
  artes?: Record<string, string>
  onSelect: (index: number) => void
  onLaunch: (game: Game) => void
}

const FALLBACK_GRADIENTS: Record<string, string> = {
  steam: "linear-gradient(145deg,#142231,#071018)",
  heroic: "linear-gradient(145deg,#202038,#0b0b14)",
  lutris: "linear-gradient(145deg,#17283a,#091018)",
  psn: "linear-gradient(145deg,#0a2550,#041027)",
}

function coverFor(game: Game, artes?: Record<string, string>) {
  // Arte 1:1 de verdade (Xbox/Worker): é ela que faz a moldura quadrada encaixar
  // sem recorte. Sem ela, cai na capa normal (o CSS recorta o centro).
  const quadrada = artes ? artes[game.id] : undefined
  if (quadrada) return quadrada
  const appid = game.launcher === "steam" ? String(game.id).replace(/^steam:/, "") : ""
  if (game.cover?.includes("/header.jpg") && appid) {
    return `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`
  }
  return game.cover
}

export function GameRail({ games, selectedIndex, cardScale = 1.6, artes, onSelect, onLaunch }: GameRailProps) {
  const { t } = useI18n()
  const railRef = useRef<HTMLDivElement>(null)
  const selectedRef = useRef<HTMLButtonElement>(null)
  const frameRef = useRef<HTMLSpanElement>(null)
  const previousCover = useRef<{ id: string; x: number; y: number } | null>(null)
  const cardWidth = Math.round(142 * Math.min(1.6, Math.max(.85, cardScale)))
  const selectedId = games[selectedIndex]?.id

  useLayoutEffect(() => {
    const rail = railRef.current
    const frame = frameRef.current
    const cover = selectedRef.current?.querySelector(".retro-library-cover")
    if (!rail || !frame || !cover || !selectedId) {
      previousCover.current = null
      return
    }
    const bounds = cover.getBoundingClientRect()
    const viewport = rail.getBoundingClientRect()
    const next = { id: selectedId, x: bounds.left - viewport.left + rail.scrollLeft, y: bounds.top - viewport.top + rail.scrollTop }
    const previous = previousCover.current
    previousCover.current = next
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)")
    if (!previous || previous.id === next.id || preference.matches || !bounds.width || !bounds.height) return

    Object.assign(frame.style, {
      left: `${next.x}px`, top: `${next.y}px`, width: `${bounds.width}px`, height: `${bounds.height}px`,
      borderRadius: getComputedStyle(cover).borderRadius,
    })
    const motion = frame.animate([
      { transform: `translate(${previous.x - next.x}px, ${previous.y - next.y}px)`, opacity: 1 },
      { transform: "translate(0, 0)", opacity: 1, offset: .8 },
      { transform: "translate(0, 0)", opacity: 0 },
    ], { duration: 200, easing: "cubic-bezier(.2,.75,.25,1)" })
    const cancel = () => motion.cancel()
    preference.addEventListener("change", cancel)
    return () => { preference.removeEventListener("change", cancel); cancel() }
  }, [selectedId, selectedIndex, cardWidth])

  useEffect(() => {
    const rail = railRef.current
    const selected = selectedRef.current
    if (rail && selected) {
      // scrollIntoView também rolava o documento inteiro quando o trilho era
      // o primeiro item. Em tela cheia isso empurrava a sidebar para fora do
      // viewport. Alterar apenas scrollLeft mantém o shell imóvel.
      const viewport = rail.getBoundingClientRect()
      const card = selected.getBoundingClientRect()
      const target = rail.scrollLeft + card.left + card.width / 2 - viewport.left - viewport.width / 2
      rail.scrollTo({
        left: Math.max(0, Math.min(target, rail.scrollWidth - rail.clientWidth)),
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth",
      })
    }
    if (railRef.current?.contains(document.activeElement)) selectedRef.current?.focus({ preventScroll: true })
  }, [selectedIndex, selectedId, cardWidth])

  return (
    <section className="retro-featured relative shrink-0 border-b px-5 pb-3 pt-3">
      <div className="retro-featured-label mb-2 px-1 text-[10px] font-black uppercase tracking-[0.12em]">{t("gameoverview.em_destaque")}</div>
      <div ref={railRef} className="retro-game-rail relative flex select-none items-start gap-3 overflow-x-auto px-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden" role="group" aria-label={t("topbar.jogos")}>
        <span ref={frameRef} aria-hidden="true" className="pointer-events-none absolute z-20 border-2 border-[color:var(--accent)] opacity-0" />
        {games.map((game, index) => {
          const focused = index === selectedIndex
          const cover = coverFor(game, artes)
          return (
            <button
              key={game.id}
              ref={focused ? selectedRef : undefined}
              type="button"
              tabIndex={focused ? 0 : -1}
              data-roving-item="true"
              data-roving-index={index}
              data-active={focused}
              onFocus={() => index !== selectedIndex && onSelect(index)}
              onClick={() => focused ? onLaunch(game) : onSelect(index)}
              onKeyDown={(event) => {
                if (!isRovingKey(event.key)) return
                event.preventDefault()
                event.stopPropagation()
                const next = nextRovingIndex(index, games.length, event.key)
                if (next === null) return
                if (next === index && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
                  signalRailEdge(event.currentTarget, event.key === "ArrowLeft" ? -1 : 1)
                  return
                }
                onSelect(next)
                requestAnimationFrame(() => railRef.current?.querySelector<HTMLButtonElement>(`[data-roving-index="${next}"]`)?.focus({ preventScroll: true }))
              }}
              className="retro-library-card shrink-0 text-left outline-none"
              style={{ width: cardWidth }}
              aria-label={game.title}
              aria-current={focused ? "true" : undefined}
            >
              <div className="retro-library-cover relative overflow-hidden" style={{ height: Math.round(cardWidth * 1.42), background: FALLBACK_GRADIENTS[game.launcher] || "#09100f" }}>
                {cover ? <img src={cover} onError={(event) => { event.currentTarget.style.visibility = "hidden" }} alt={game.title} className="ps5-cover-art" loading="lazy" draggable={false} /> : (
                  <div className="flex h-full flex-col items-center justify-center gap-3 p-3 text-center text-white/50">
                    <LauncherIcon launcher={game.launcher} size={28} />
                    <span className="line-clamp-3 text-[11px] font-bold uppercase">{game.title}</span>
                  </div>
                )}
              </div>
              <strong className="mt-2 block line-clamp-2 px-1 text-[9px] font-bold uppercase leading-tight tracking-[0.025em] text-white/80">{game.title}</strong>
              <span className="mt-1 block px-1 text-[9px] font-black text-[var(--accent)]">{game.year || "—"}</span>
            </button>
          )
        })}
      </div>
      <div className="retro-page-dots mt-2 flex justify-center gap-2" aria-hidden="true">
        {Array.from({ length: Math.min(4, Math.max(1, Math.ceil(games.length / 6))) }).map((_, index) => {
          const current = Math.min(3, Math.floor(selectedIndex / Math.max(1, Math.ceil(games.length / 4))))
          return <span key={index} data-active={index === current} />
        })}
      </div>
    </section>
  )
}
