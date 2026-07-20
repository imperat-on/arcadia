"use client"

import { useState } from "react"

export interface JogoLinha {
  appid: string
  title: string
  cover?: string
  manifest?: boolean
  fontes?: string[]
}

export interface FichaJogo {
  generos?: string[]
  preco?: string
  metacritic?: number
  lancamento?: string
  fundo?: string
}

// Capa retrato. A Steam serve library_600x900 para quase tudo; o que não tem
// cai no header horizontal e, por último, no título — demos e lançamentos
// recentes costumam não ter a arte vertical ainda.
function Capa({ appid, title }: { appid: string; title: string }) {
  const [fase, setFase] = useState(0)
  const fontes = [
    `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`,
    `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/header.jpg`,
  ]
  if (fase >= fontes.length) {
    return (
      <div className="flex h-full w-full items-center justify-center bg-[#111114] px-3 text-center">
        <span className="line-clamp-4 text-[13px] leading-tight text-white/45">{title}</span>
      </div>
    )
  }
  return (
    <img
      src={fontes[fase]}
      alt=""
      loading="lazy"
      draggable={false}
      className="h-full w-full object-cover"
      onError={() => setFase((f) => f + 1)}
    />
  )
}

const CAPA_W = 168
const RATIO = 1.5
const GAP = 18
// O painel cobre ~2,4 capas. Mais que isso escondia meia tela de jogos; menos
// não sobrava espaço para o texto respirar.
const PAINEL_W = Math.round(CAPA_W * 2.4 + GAP * 2)

interface StoreGridProps {
  jogos: JogoLinha[]
  carregando?: boolean
  /** Jogo em foco e a ficha dele — o painel expandido lê daqui. */
  focado: JogoLinha | null
  ficha: FichaJogo | null
  trailer: { url: string; poster: string } | null
  /** Pausa o trailer quando a janela perde o foco. */
  ativo: boolean
  onFocar: (jogo: JogoLinha) => void
  onAbrir: (jogo: JogoLinha) => void
}

export function StoreGrid({
  jogos,
  carregando,
  focado,
  ficha,
  trailer,
  ativo,
  onFocar,
  onAbrir,
}: StoreGridProps) {
  // Colunas por linha, para saber se o ladrilho está encostado na borda
  // direita — o painel precisa crescer para o outro lado nesse caso.
  const [colunas, setColunas] = useState(6)

  const medir = (el: HTMLDivElement | null) => {
    if (!el) return
    const cabe = Math.max(1, Math.floor((el.clientWidth + GAP) / (CAPA_W + GAP)))
    if (cabe !== colunas) setColunas(cabe)
  }

  if (carregando) {
    return (
      <div
        className="grid px-12 pb-12"
        style={{ gridTemplateColumns: `repeat(auto-fill, ${CAPA_W}px)`, gap: GAP }}
      >
        {Array.from({ length: 18 }).map((_, i) => (
          <div
            key={`sk${i}`}
            className="animate-pulse rounded-xl bg-white/[0.05]"
            style={{ width: CAPA_W, height: CAPA_W * RATIO }}
          />
        ))}
      </div>
    )
  }

  if (!jogos.length) {
    return <p className="px-12 py-16 text-white/35">Nada por aqui.</p>
  }

  return (
    <div
      ref={medir}
      className="grid px-12 pb-16"
      style={{ gridTemplateColumns: `repeat(auto-fill, ${CAPA_W}px)`, gap: GAP }}
    >
      {jogos.map((j, i) => {
        const emFoco = focado?.appid === j.appid
        // Encostado na direita: o painel ancora à direita e cresce para a
        // esquerda. Sem isto ele seria recortado pela borda — a mesma classe de
        // erro que cortou a capa selecionada no trilho de jogos.
        const naBorda = colunas > 2 && i % colunas >= colunas - 2

        return (
          <button
            key={j.appid}
            onClick={() => onAbrir(j)}
            onFocus={() => onFocar(j)}
            data-appid={j.appid}
            aria-label={j.title}
            className="loja-item relative outline-none"
            style={{ width: CAPA_W, height: CAPA_W * RATIO, zIndex: emFoco ? 20 : 1 }}
          >
            {/* A capa some sob o painel quando expande, e volta assim que o
                foco sai — é isso que dá a leitura de "a capa virou o painel". */}
            <div
              className="loja-capa absolute inset-0 overflow-hidden rounded-xl bg-[#111114]"
              style={{ opacity: emFoco ? 0 : undefined }}
            >
              <Capa appid={j.appid} title={j.title} />
              {j.manifest === false && (
                <span className="absolute inset-x-0 bottom-0 bg-black/75 py-1 text-center text-[11px] font-medium text-white/55">
                  Sem manifesto
                </span>
              )}
            </div>

            {emFoco && (
              <div
                className="loja-painel absolute top-0 overflow-hidden rounded-xl text-left"
                style={{
                  width: PAINEL_W,
                  height: CAPA_W * RATIO, // mesma altura: nenhuma linha se desloca
                  ...(naBorda ? { right: 0 } : { left: 0 }),
                }}
              >
                {/* Arte de fundo: o painel é horizontal, então a arte
                    widescreen entra sem o corte que o formato retrato impunha. */}
                <img
                  src={ficha?.fundo || `https://cdn.akamai.steamstatic.com/steam/apps/${j.appid}/header.jpg`}
                  alt=""
                  className="absolute inset-0 h-full w-full object-cover"
                />
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
                <div className="absolute inset-0 bg-gradient-to-t from-black via-black/75 to-black/20" />

                <div className="absolute inset-x-0 bottom-0 p-5">
                  <h3 className="text-xl font-light leading-tight">{j.title}</h3>
                  <div className="mt-1.5 flex min-h-[18px] flex-wrap items-center gap-x-2 text-[12px] text-white/50">
                    {ficha?.generos?.length ? <span>{ficha.generos.slice(0, 3).join(" · ")}</span> : null}
                    {ficha?.lancamento && <span className="text-white/30">{ficha.lancamento}</span>}
                  </div>
                  <div className="mt-2 flex min-h-[22px] items-center gap-2.5 text-[13px]">
                    {ficha?.preco && <span className="text-white/90">{ficha.preco}</span>}
                    {ficha?.metacritic ? (
                      <span
                        className="rounded px-1.5 py-0.5 text-[11px] font-semibold text-black"
                        style={{ background: "var(--loja-cor)" }}
                      >
                        {ficha.metacritic}
                      </span>
                    ) : null}
                  </div>
                  <p className="mt-1 min-h-[15px] text-[11px] text-white/35">
                    {j.fontes?.length
                      ? `Disponível em ${j.fontes.join(", ")}`
                      : j.manifest === false
                        ? "Sem manifesto"
                        : ""}
                  </p>
                  {/* Sem botões aqui: as ações são os atalhos do controle, e um
                      botão no meio da grade viraria armadilha de foco. */}
                  <p className="mt-2.5 text-[11px] text-white/30">
                    A abrir · X baixar · Y adicionar
                  </p>
                </div>
              </div>
            )}
          </button>
        )
      })}
    </div>
  )
}
