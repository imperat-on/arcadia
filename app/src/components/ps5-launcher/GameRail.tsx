"use client"

import { useEffect, useRef } from "react"
import type { Game } from "./types"
import { LauncherIcon } from "./HeroSection"
import { isRovingKey, nextRovingIndex } from "./rovingTab.cjs"

interface GameRailProps {
  games: Game[]
  selectedIndex: number
  cardScale?: number
  onSelect: (index: number) => void
  onLaunch: (game: Game) => void
}

const FALLBACK_GRADIENTS: Record<string, string> = {
  steam: "linear-gradient(145deg,#142231,#071018)",
  heroic: "linear-gradient(145deg,#202038,#0b0b14)",
  lutris: "linear-gradient(145deg,#17283a,#091018)",
  psn: "linear-gradient(145deg,#0a2550,#041027)",
}

function coverFor(game: Game) {
  const appid = game.launcher === "steam" ? String(game.id).replace(/^steam:/, "") : ""
  if (game.cover?.includes("/header.jpg") && appid) {
    return `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`
  }
  return game.cover
}

export function GameRail({ games, selectedIndex, cardScale = 1.6, onSelect, onLaunch }: GameRailProps) {
  const railRef = useRef<HTMLDivElement>(null)
  const selectedRef = useRef<HTMLButtonElement>(null)
  const lastMove = useRef(0)
  const cardWidth = Math.round(142 * Math.min(1.6, Math.max(.85, cardScale)))

  useEffect(() => {
    const now = performance.now()
    const fast = now - lastMove.current < 320
    lastMove.current = now
    const rail = railRef.current
    const selected = selectedRef.current
    if (rail && selected) {
      // scrollIntoView também rolava o documento inteiro quando o trilho era
      // o primeiro item. Em tela cheia isso empurrava a sidebar para fora do
      // viewport. Alterar apenas scrollLeft mantém o shell imóvel.
      const target = selected.offsetLeft - (rail.clientWidth - selected.offsetWidth) / 2
      rail.scrollTo({ left: Math.max(0, target), behavior: fast ? "auto" : "smooth" })
    }
    if (railRef.current?.contains(document.activeElement)) selectedRef.current?.focus({ preventScroll: true })
  }, [selectedIndex])

  const move = (delta: number) => onSelect(Math.max(0, Math.min(games.length - 1, selectedIndex + delta)))

  return (
    <section className="retro-featured relative shrink-0 border-b px-5 pb-3 pt-3">
      <div className="retro-featured-label mb-2 px-1 text-[10px] font-black uppercase tracking-[0.12em]">Em destaque</div>
      <button type="button" onClick={() => move(-1)} disabled={selectedIndex === 0} className="retro-rail-arrow retro-rail-arrow-left absolute left-1 top-1/2 z-20 grid h-12 w-7 place-items-center text-2xl disabled:opacity-15" aria-label="Jogo anterior">‹</button>
      <div ref={railRef} className="retro-game-rail flex select-none items-start gap-3 overflow-x-auto px-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden" role="group" aria-label="Biblioteca de jogos">
        {games.map((game, index) => {
          const focused = index === selectedIndex
          const cover = coverFor(game)
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
                onSelect(next)
                requestAnimationFrame(() => railRef.current?.querySelector<HTMLButtonElement>(`[data-roving-index="${next}"]`)?.focus({ preventScroll: true }))
              }}
              className="retro-library-card shrink-0 text-left outline-none"
              style={{ width: cardWidth }}
              aria-label={`${game.title} — selecionar`}
            >
              <div className="retro-library-cover relative overflow-hidden" style={{ height: Math.round(cardWidth * 1.42), background: FALLBACK_GRADIENTS[game.launcher] || "#09100f" }}>
                {cover ? <img src={cover} alt={game.title} className="ps5-cover-art" loading="lazy" draggable={false} /> : (
                  <div className="flex h-full flex-col items-center justify-center gap-3 p-3 text-center text-white/50">
                    <LauncherIcon launcher={game.launcher} size={28} />
                    <span className="line-clamp-3 text-[11px] font-bold uppercase">{game.title}</span>
                  </div>
                )}
              </div>
              <strong className="mt-2 block truncate px-1 text-[9px] font-bold uppercase tracking-[0.025em] text-white/80">{game.title}</strong>
              <span className="mt-1 block px-1 text-[9px] font-black text-[var(--retro-phosphor)]">{game.year || "—"}</span>
            </button>
          )
        })}
      </div>
      <button type="button" onClick={() => move(1)} disabled={selectedIndex >= games.length - 1} className="retro-rail-arrow retro-rail-arrow-right absolute right-1 top-1/2 z-20 grid h-12 w-7 place-items-center text-2xl disabled:opacity-15" aria-label="Próximo jogo">›</button>
      <div className="retro-page-dots mt-2 flex justify-center gap-2" aria-hidden="true">
        {Array.from({ length: Math.min(4, Math.max(1, Math.ceil(games.length / 6))) }).map((_, index) => {
          const current = Math.min(3, Math.floor(selectedIndex / Math.max(1, Math.ceil(games.length / 4))))
          return <span key={index} data-active={index === current} />
        })}
      </div>
    </section>
  )
}
