"use client"

import { useRef } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { StoreTile } from "./StoreTile"
import { rolagemSuave } from "./rolagem"
import type { JogoLinha } from "./types"

interface StoreRailProps {
  titulo: string
  jogos: JogoLinha[]
  carregando?: boolean
  /** Abre a categoria inteira em grade. */
  onVerTudo?: () => void
  naBiblioteca?: (j: JogoLinha) => boolean
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
  onAdicionar?: (j: JogoLinha) => void
}

// Um trilho horizontal: cabeçalho com título, "Ver tudo" e setas, e um
// scroller de ladrilhos abaixo.
//
// As setas são para o mouse e ficam FORA da navegação por controle
// (tabIndex -1): como alvos focáveis, virariam armadilha entre um trilho e o
// seguinte, já que ocupam a mesma faixa vertical dos cards.
export function StoreRail({
  titulo,
  jogos,
  carregando,
  onVerTudo,
  naBiblioteca,
  onFocar,
  onAbrir,
  onAdicionar,
}: StoreRailProps) {
  const { t } = useI18n()
  const scroller = useRef<HTMLDivElement | null>(null)

  const rolar = (dir: 1 | -1) => {
    const el = scroller.current
    if (!el) return
    el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: rolagemSuave() })
  }

  return (
    <section aria-label={titulo}>
      <div className="mb-3 flex items-end justify-between gap-4 px-12">
        <h2 className="text-[22px] font-bold tracking-tight text-white">{titulo}</h2>
        <div className="flex items-center gap-2">
          {onVerTudo && (
            <button
              onClick={onVerTudo}
              tabIndex={-1}
              className="text-[13px] font-medium text-[var(--loja-apagado)] outline-none transition-colors hover:text-white"
            >
{t("rail.ver_tudo")}
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
                style={{ width: 200, aspectRatio: "3 / 4" }}
              />
            ))
          : jogos.map((j, i) => (
              <StoreTile
                key={j.appid}
                jogo={j}
                indice={i}
                naBiblioteca={naBiblioteca?.(j)}
                onFocar={onFocar}
                onAbrir={onAbrir}
                onAdicionar={onAdicionar}
              />
            ))}
      </div>
    </section>
  )
}

function Seta({ dir, onClick }: { dir: 1 | -1; onClick: () => void }) {
  const { t } = useI18n()
  return (
    <button
      onClick={onClick}
      tabIndex={-1}
      aria-label={dir === 1 ? t("rail.avancar") : t("rail.voltar")}
      className="grid h-9 w-9 place-items-center rounded-full bg-[var(--loja-sup-2)] text-white outline-none transition-colors hover:bg-[var(--loja-sup-3)]"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
        <path d={dir === 1 ? "m9 5 7 7-7 7" : "m15 5-7 7 7 7"} />
      </svg>
    </button>
  )
}
