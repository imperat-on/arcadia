"use client"

import { useEffect, useRef } from "react"
import type { Game } from "./types"
import { LauncherIcon } from "./HeroSection"

interface GameRailProps {
  games: Game[]
  selectedIndex: number
  /** Escala das capas vinda das configurações. */
  cardScale?: number
  onSelect: (index: number) => void
  onLaunch: (game: Game) => void
}

const BASE_TILE_W = 100 // capa comum (retrato 2:3)
const BASE_TILE_SEL_W = 152 // capa selecionada
const RATIO = 1.5 // altura = largura * 1.5
const PANEL_PAD = 10 // respiro do painel atrás da selecionada

const FALLBACK_GRADIENTS: Record<string, string> = {
  steam: "linear-gradient(160deg, #1b2838 0%, #0d1a26 60%, #1b2838 100%)",
  heroic: "linear-gradient(160deg, #1c1f2e 0%, #0f1119 60%, #1e1028 100%)",
  lutris: "linear-gradient(160deg, #1a1a2e 0%, #16213e 60%, #0f3460 100%)",
  psn: "linear-gradient(160deg, #0a1a3a 0%, #04122b 60%, #002a6b 100%)",
}

export function GameRail({
  games,
  selectedIndex,
  cardScale = 1,
  onSelect,
  onLaunch,
}: GameRailProps) {
  const selRef = useRef<HTMLButtonElement>(null)
  const lastMove = useRef(0)

  const TILE_W = BASE_TILE_W * cardScale
  const TILE_SEL_W = BASE_TILE_SEL_W * cardScale
  const ROW_H = TILE_SEL_W * RATIO + PANEL_PAD * 2

  // Mantém a capa selecionada à vista. Rolagem rápida usa scroll INSTANTÂNEO
  // (o smooth do navegador não cancela o anterior e acumula, "pulando tudo de
  // uma vez"); passo único fica suave.
  useEffect(() => {
    const now = performance.now()
    const fast = now - lastMove.current < 320 // inclui o hold do gamepad (~260ms)
    lastMove.current = now
    selRef.current?.scrollIntoView({
      block: "nearest",
      inline: "nearest",
      behavior: fast ? "auto" : "smooth",
    })
  }, [selectedIndex])

  return (
    // Capas alinhadas pelo topo, como na referência. Scrollbar escondida: navega-se por seleção.
    <div
      className="rail-anim flex items-start gap-3 px-10 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden select-none"
      style={{ height: ROW_H }}
    >
      {games.map((game, i) => {
        const focused = i === selectedIndex
        return (
          <button
            key={game.id}
            ref={focused ? selRef : undefined}
            onClick={() => {
              if (focused) onLaunch(game)
              else onSelect(i)
            }}
            className="relative flex-shrink-0 rounded-2xl outline-none scroll-mx-10"
            style={{
              // O SLOT tem largura fixa. Antes animávamos width/height, que são
              // propriedades de layout: o navegador refazia o layout do trilho
              // inteiro a cada frame e todos os vizinhos escorregavam junto —
              // era daí que vinha o tremido. Agora só a capa cresce, por
              // transform, e ninguém mais sai do lugar.
              width: TILE_W + PANEL_PAD * 2,
              height: TILE_SEL_W * RATIO + PANEL_PAD * 2,
              padding: PANEL_PAD,
              zIndex: focused ? 10 : 1,
            }}
            aria-label={`${game.title} — selecionar`}
          >
            <div
              className="w-full rounded-xl overflow-hidden"
              style={{
                height: TILE_W * RATIO,
                background:
                  FALLBACK_GRADIENTS[game.launcher] ??
                  "linear-gradient(160deg, #0d0d0f 0%, #000000 100%)",
                // Cresce a partir do topo (as capas são alinhadas pelo topo),
                // sobe um pouco e ganha um anel na cor de destaque. Escala e
                // deslocamento são compostos na GPU: sem reflow, sem tremer.
                transform: focused
                  ? `translateY(-6px) scale(${TILE_SEL_W / TILE_W})`
                  : "translateY(0) scale(1)",
                transformOrigin: "center top",
                boxShadow: focused
                  ? "0 18px 44px rgba(0,0,0,0.7), 0 0 0 2px var(--accent)"
                  : "0 2px 12px rgba(0,0,0,0.4)",
                // Oculto (visível só com "Mostrar ocultos"): apagado e sem cor.
                opacity: game.hidden ? 0.4 : focused ? 1 : 0.7,
                filter: game.hidden ? "grayscale(1)" : focused ? "none" : "brightness(0.75)",
                transition:
                  "transform var(--dur-2) var(--ease), box-shadow var(--dur-2) var(--ease), opacity var(--dur-2) var(--ease), filter var(--dur-2) var(--ease)",
                willChange: "transform",
              }}
            >
              {game.cover ? (
                <img
                  src={game.cover}
                  alt=""
                  className="w-full h-full object-cover"
                  loading="lazy"
                />
              ) : (
                <div className="w-full h-full flex flex-col items-center justify-center gap-2 p-2">
                  <LauncherIcon launcher={game.launcher} size={20} />
                  <span className="text-white/80 text-[10px] font-medium text-center leading-tight line-clamp-3">
                    {game.title}
                  </span>
                </div>
              )}
            </div>
          </button>
        )
      })}
    </div>
  )
}
