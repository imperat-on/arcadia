"use client"

import { ImgCascata } from "./ImgCascata"
import { urlsCapa } from "./capaJogo"
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
  /** Jogo em foco — o herói é reativo e mostra sempre este. */
  focado: JogoLinha | null
  ficha: FichaJogo | null
  trailer: { url: string; poster: string } | null
  ativo: boolean
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
  onVerCategoria: (id: string) => void
}

// A vitrine: um herói widescreen no topo e um trilho por categoria abaixo.
//
// O herói é REATIVO — mostra o jogo em foco, e não um destaque fixo. Assim ele
// reaproveita a ficha e o trailer que já buscamos para o foco, sem uma segunda
// requisição, e a tela inteira responde à navegação em vez de ficar parada.
export function StoreShowcase({
  secoes,
  carregando,
  focado,
  ficha,
  trailer,
  ativo,
  onFocar,
  onAbrir,
  onVerCategoria,
}: StoreShowcaseProps) {
  // Antes do primeiro foco, o herói adianta o primeiro jogo da primeira seção.
  const heroi = focado || secoes.find((s) => s.jogos.length)?.jogos[0] || null

  return (
    <div className="flex flex-col gap-9 pb-10">
      {heroi && (
        <div className="loja-heroi relative" key={heroi.appid}>
          <ImgCascata fontes={[ficha?.fundo || "", ...urlsCapa(heroi, "paisagem")].filter(Boolean)} loading="eager" />
          {trailer && ativo && (
            <video
              key={trailer.url}
              src={trailer.url}
              poster={trailer.poster}
              autoPlay
              loop
              muted
              playsInline
              className="absolute inset-0 h-full w-full object-cover"
            />
          )}
          {/* Degradês: um lateral para o texto ter contraste, outro no rodapé
              para o herói se dissolver no primeiro trilho em vez de cortar. */}
          <div className="absolute inset-0 bg-gradient-to-r from-black via-black/55 to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-32 bg-gradient-to-t from-black to-transparent" />

          <div className="absolute inset-0 flex flex-col justify-end px-12 pb-10">
            <h1 className="max-w-[55%] text-4xl font-semibold leading-tight tracking-tight">{heroi.title}</h1>
            <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px] text-white/60">
              {ficha?.generos?.length ? <span>{ficha.generos.slice(0, 3).join(" · ")}</span> : null}
              {ficha?.lancamento ? <span className="text-white/40">{ficha.lancamento}</span> : null}
              {ficha?.preco ? (
                <span className="text-[17px] font-semibold" style={{ color: "var(--loja-cor)" }}>
                  {ficha.preco}
                </span>
              ) : null}
            </div>
          </div>
        </div>
      )}

      {secoes.map((s) => (
        <StoreRail
          key={s.id}
          titulo={s.rotulo}
          jogos={s.jogos}
          carregando={carregando}
          onVerTudo={() => onVerCategoria(s.id)}
          onFocar={onFocar}
          onAbrir={onAbrir}
        />
      ))}
    </div>
  )
}
