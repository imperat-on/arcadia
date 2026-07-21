"use client"

import { useEffect, useState } from "react"
import type { Game } from "./types"
import { GameCard } from "./GameCard"
import { useI18n } from "../../i18n/I18nContext"

interface GameLibraryProps {
  games: Game[]
  selectedIndex: number
  cardScale?: number
  onSelect: (index: number) => void
  onLaunch: (game: Game) => void
}

// Dimensões base do trilho (multiplicadas pela escala das capas).
const BASE_CARD_W = 158
const BASE_GAP = 20
const LEFT_PAD = 40 // onde o card selecionado "ancora" a partir da esquerda

export function GameLibrary({
  games,
  selectedIndex,
  cardScale = 1,
  onSelect,
  onLaunch,
}: GameLibraryProps) {
  const { t } = useI18n()
  const [mounted, setMounted] = useState(false)
  useEffect(() => {
    const t = setTimeout(() => setMounted(true), 60)
    return () => clearTimeout(t)
  }, [])

  const CARD_W = BASE_CARD_W * cardScale
  const GAP = BASE_GAP * cardScale
  const railH = Math.round(CARD_W * 1.5 * 1.18) // altura p/ o card focado crescer

  // Trilho desliza para manter o card focado ancorado à esquerda.
  const translateX = LEFT_PAD - selectedIndex * (CARD_W + GAP)

  return (
    <div className="relative px-10 pb-4 select-none">
      {/* Cabeçalho */}
      <div className="flex items-center mb-4">
        <h2 className="text-sm font-semibold text-[#7a8aaa] tracking-[0.15em] uppercase">
          {t("library.titulo")}
          <span className="ml-2 text-xs opacity-60">({games.length})</span>
        </h2>
      </div>

      {/* Área do trilho: overflow escondido (nada sai da tela) */}
      <div className="relative" style={{ height: railH }}>
        <div className="absolute inset-0 overflow-hidden">
          <div
            className="flex items-end"
            style={{
              height: "100%",
              gap: GAP,
              transform: `translateX(${translateX}px)`,
              transition: mounted
                ? "transform 0.42s cubic-bezier(0.22, 1, 0.36, 1)"
                : "none",
              willChange: "transform",
            }}
          >
            {games.map((game, i) => (
              <GameCard
                key={game.id}
                game={game}
                focused={selectedIndex === i}
                width={CARD_W}
                onFocus={() => onSelect(i)}
                onLaunch={() => onLaunch(game)}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}
