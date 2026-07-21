"use client"

import { useI18n } from "../../i18n/I18nContext"
import { StoreHero } from "./StoreHero"
import { StoreRail } from "./StoreRail"
import type { FichaJogo, JogoLinha } from "./types"

export interface SecaoVitrine {
  id: string
  rotulo: string
  jogos: JogoLinha[]
}

interface StoreShowcaseProps {
  secoes: SecaoVitrine[]
  carregando: boolean
  /** Destaques do herói e o índice atual (o estado vive no pai, que busca a ficha). */
  destaques: JogoLinha[]
  heroiIdx: number
  onHeroiIdx: (i: number) => void
  fichaHeroi: FichaJogo | null
  trailerHeroi: { url: string } | null
  ativo: boolean
  ocupado: boolean
  naBiblioteca: (j: JogoLinha) => boolean
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
  onBaixar: (j: JogoLinha) => void
  onAdicionar: (j: JogoLinha) => void
  onVerCategoria: (id: string) => void
}

// A vitrine: herói no topo, um trilho por categoria e uma faixa de promoções
// quebrando a coluna no meio.
export function StoreShowcase({
  secoes,
  carregando,
  destaques,
  heroiIdx,
  onHeroiIdx,
  fichaHeroi,
  trailerHeroi,
  ativo,
  ocupado,
  naBiblioteca,
  onFocar,
  onAbrir,
  onBaixar,
  onAdicionar,
  onVerCategoria,
}: StoreShowcaseProps) {
  const { t } = useI18n()
  const heroi = destaques[heroiIdx]

  return (
    <div className="flex flex-col gap-12 pb-16">
      {destaques.length > 0 && (
        <StoreHero
          jogos={destaques}
          ficha={fichaHeroi}
          trailer={trailerHeroi}
          ativo={ativo}
          naBiblioteca={Boolean(heroi && naBiblioteca(heroi))}
          ocupado={ocupado}
          indice={heroiIdx}
          onIndice={onHeroiIdx}
          onAbrir={onAbrir}
          onBaixar={onBaixar}
          onAdicionar={onAdicionar}
        />
      )}

      {secoes.map((s, i) => (
        <div key={s.id} className="flex flex-col gap-12">
          <StoreRail
            titulo={s.rotulo}
            jogos={s.jogos}
            carregando={carregando}
            naBiblioteca={naBiblioteca}
            onVerTudo={() => onVerCategoria(s.id)}
            onFocar={onFocar}
            onAbrir={onAbrir}
            onAdicionar={onAdicionar}
          />

          {/* Depois do segundo trilho: é onde a coluna de capas começa a
              cansar, e a faixa dá o respiro sem empurrar tudo para baixo. */}
          {i === 1 && (
            <div className="px-12">
              <button onClick={() => onVerCategoria("specials")} className="loja-faixa block w-full">
                <p className="text-[11px] font-bold uppercase tracking-[0.2em] text-black/60">{t("store.promocoes")}</p>
                <h3 className="mt-2 max-w-xl text-3xl font-bold leading-tight text-black">
                  {t("store.promocoes_desc")}
                </h3>
                <span className="mt-5 inline-block rounded-full bg-black/85 px-6 py-2.5 text-[13px] font-semibold text-white">
                  {t("store.ver_promocoes")}
                </span>
                {/* Formas decorativas: o mesmo brilho difuso do mock, mas
                    herdando a cor do jogo em foco. */}
                <span className="pointer-events-none absolute -right-16 -top-16 h-72 w-72 rounded-full bg-white/15 blur-3xl" />
                <span className="pointer-events-none absolute -bottom-20 right-24 h-56 w-56 rounded-full bg-white/10 blur-3xl" />
              </button>
            </div>
          )}
        </div>
      ))}
    </div>
  )
}
