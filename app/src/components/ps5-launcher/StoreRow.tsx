"use client"

import { useRef, useState } from "react"

export interface JogoLinha {
  appid: string
  title: string
  cover?: string
  manifest?: boolean
  fontes?: string[]
}

// Capa retrato da loja. A Steam serve library_600x900 para quase tudo; o que
// não tem cai no header horizontal e, por último, no título em texto — demos e
// lançamentos recentes costumam não ter a arte vertical ainda.
function CapaLoja({ appid, title }: { appid: string; title: string }) {
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

interface StoreRowProps {
  titulo: string
  jogos: JogoLinha[]
  carregando?: boolean
  onAbrir: (jogo: JogoLinha) => void
  /** Chamado quando uma capa ganha foco — alimenta o herói do topo. */
  onFocar?: (jogo: JogoLinha) => void
}

const CAPA_W = 176
const RATIO = 1.5
const PAD = 12

export function StoreRow({ titulo, jogos, carregando, onAbrir, onFocar }: StoreRowProps) {
  // Posição do item em foco e progresso da rolagem: numa linha de 24 capas sem
  // barra visível, não havia nenhuma noção de onde se está nem de quanto falta.
  const [pos, setPos] = useState(0)
  const [progresso, setProgresso] = useState(0)
  const trilho = useRef<HTMLDivElement>(null)

  if (!carregando && !jogos.length) return null

  const aoRolar = () => {
    const el = trilho.current
    if (!el) return
    const max = el.scrollWidth - el.clientWidth
    setProgresso(max > 0 ? el.scrollLeft / max : 0)
  }

  return (
    <section className="mb-8">
      <div className="mb-3 flex items-baseline gap-3 px-12">
        <h2 className="text-[15px] font-medium text-white/70">{titulo}</h2>
        {pos > 0 && (
          <span className="text-[12px] tabular-nums text-white/30">
            {pos} / {jogos.length}
          </span>
        )}
        <div className="ml-auto h-px w-40 overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full transition-[width] duration-200"
            style={{ width: `${Math.max(6, progresso * 100)}%`, background: "var(--loja-cor)" }}
          />
        </div>
      </div>
      <div
        ref={trilho}
        onScroll={aoRolar}
        className="loja-linha flex items-start overflow-x-auto px-12 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        // O slot já tem a altura da capa em tamanho cheio mais o respiro do
        // realce. Sem isso o `overflow-x` recortaria a sombra e o anel de foco,
        // que foi exatamente o defeito que apareceu no trilho de jogos.
        style={{ minHeight: CAPA_W * RATIO + PAD * 2 + 24, paddingBottom: 24 }}
      >
        {carregando
          ? Array.from({ length: 6 }).map((_, i) => (
              <div key={`sk${i}`} style={{ width: CAPA_W + PAD * 2, padding: PAD }}>
                <div
                  className="animate-pulse rounded-xl bg-white/[0.05]"
                  style={{ width: CAPA_W, height: CAPA_W * RATIO, transform: "scale(0.82)", transformOrigin: "center top" }}
                />
              </div>
            ))
          : jogos.map((j, i) => (
              <button
                key={j.appid}
                onClick={() => onAbrir(j)}
                onFocus={() => {
                  setPos(i + 1)
                  onFocar?.(j)
                }}
                data-appid={j.appid}
                className="loja-item relative shrink-0 scroll-mx-12 outline-none"
                style={{ width: CAPA_W + PAD * 2, padding: PAD }}
                aria-label={j.title}
              >
                <div
                  className="loja-capa relative overflow-hidden rounded-xl bg-[#111114]"
                  style={{ width: CAPA_W, height: CAPA_W * RATIO }}
                >
                  <CapaLoja appid={j.appid} title={j.title} />
                  {/* Marca só o indisponível: a maioria tem manifesto, então um
                      selo em cada capa disponível seria ruído. */}
                  {j.manifest === false && (
                    <span className="absolute bottom-0 inset-x-0 bg-black/75 py-1 text-center text-[11px] font-medium text-white/55">
                      Sem manifesto
                    </span>
                  )}
                </div>
              </button>
            ))}
      </div>
    </section>
  )
}
