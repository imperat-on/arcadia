"use client"

import { useRef } from "react"
import { StoreTile } from "./StoreTile"
import { rolagemSuave } from "./rolagem"
import type { JogoLinha } from "./types"

interface StoreRailProps {
  titulo: string
  jogos: JogoLinha[]
  carregando?: boolean
  /** Abre a categoria inteira em grade. */
  onVerTudo?: () => void
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
}

// Um trilho horizontal: cabeçalho com título, "Ver tudo" e setas, e um
// scroller de ladrilhos abaixo.
//
// As setas são para o mouse e ficam FORA da navegação por controle
// (tabIndex -1): como alvos focáveis, virariam armadilha entre um trilho e o
// seguinte, já que ocupam a mesma faixa vertical dos cards.
export function StoreRail({ titulo, jogos, carregando, onVerTudo, onFocar, onAbrir }: StoreRailProps) {
  const scroller = useRef<HTMLDivElement | null>(null)

  const rolar = (dir: 1 | -1) => {
    const el = scroller.current
    if (!el) return
    el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: rolagemSuave() })
  }

  return (
    <section aria-label={titulo}>
      <div className="mb-3 flex items-end justify-between gap-4 px-12">
        <h2 className="text-[19px] font-semibold tracking-tight text-white">{titulo}</h2>
        <div className="flex items-center gap-2">
          {onVerTudo && (
            <button
              onClick={onVerTudo}
              tabIndex={-1}
              className="text-[12px] text-white/45 outline-none transition-colors hover:text-white"
            >
              Ver tudo
            </button>
          )}
          <Seta dir={-1} onClick={() => rolar(-1)} />
          <Seta dir={1} onClick={() => rolar(1)} />
        </div>
      </div>

      <div
        ref={scroller}
        className="flex gap-4 overflow-x-auto px-12 pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {carregando && !jogos.length
          ? Array.from({ length: 8 }).map((_, i) => (
              <div
                key={`sk${i}`}
                className="shrink-0 animate-pulse rounded-lg bg-white/[0.04]"
                style={{ width: 176, aspectRatio: "3 / 4" }}
              />
            ))
          : jogos.map((j, i) => (
              <StoreTile key={j.appid} jogo={j} indice={i} onFocar={onFocar} onAbrir={onAbrir} />
            ))}
      </div>
    </section>
  )
}

function Seta({ dir, onClick }: { dir: 1 | -1; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      tabIndex={-1}
      aria-label={dir === 1 ? "Avançar" : "Voltar"}
      className="grid h-8 w-8 place-items-center rounded-full border border-white/10 text-white/60 outline-none transition-colors hover:border-white/25 hover:text-white"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
        <path d={dir === 1 ? "m9 5 7 7-7 7" : "m15 5-7 7 7 7"} />
      </svg>
    </button>
  )
}
