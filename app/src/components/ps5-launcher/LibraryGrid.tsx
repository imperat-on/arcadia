"use client"

import { useEffect, useRef } from "react"
import type { Game } from "./types"
import { GameCard } from "./GameCard"

interface LibraryGridProps {
  games: Game[]
  selectedIndex: number
  columns: number
  onSelect: (index: number) => void
  onLaunch: (game: Game) => void
  emptyMessage?: string
  scrollRef?: React.RefObject<HTMLDivElement | null>
}

export function LibraryGrid({
  games,
  selectedIndex,
  columns,
  onSelect,
  onLaunch,
  emptyMessage,
  scrollRef,
}: LibraryGridProps) {
  const selRef = useRef<HTMLDivElement>(null)

  // Rola o card selecionado para dentro da vista. "auto" (instantâneo) — com
  // "smooth" o scroll acumulava e a grade pulava tudo de uma vez ao navegar rápido.
  useEffect(() => {
    selRef.current?.scrollIntoView({ block: "nearest", behavior: "auto" })
  }, [selectedIndex])

  if (games.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <p className="text-lg text-[#8a93a6]">
          {emptyMessage || "Nada por aqui ainda."}
        </p>
      </div>
    )
  }

  return (
    // pt-20: folga para o card em foco crescer para cima sem ser cortado pelo scroll
    <div ref={scrollRef} className="flex-1 overflow-y-auto px-10 pt-20 pb-10">
      <div
        className="grid gap-5"
        style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
      >
        {games.map((game, i) => {
          const focused = i === selectedIndex
          return (
            <div key={game.id} ref={focused ? selRef : undefined} className="scroll-mt-24 scroll-mb-8">
              <GameCard
                game={game}
                focused={focused}
                width="100%"
                onFocus={() => onSelect(i)}
                onLaunch={() => onLaunch(game)}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}
