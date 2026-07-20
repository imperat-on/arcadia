"use client"

import { useRef } from "react"
import { ImgCascata } from "./ImgCascata"
import { rolagemSuave } from "./rolagem"
import { urlsCapa } from "./capaJogo"
import type { JogoLinha } from "./types"

interface StoreTileProps {
  jogo: JogoLinha
  /** Atraso da animação de entrada, em ordem de aparição. */
  indice?: number
  /** Largura do ladrilho. A altura vem da proporção 3:4 mais o bloco de meta. */
  largura?: number
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
}

// Capa retrato com um bloco de meta abaixo — título numa linha e preço na
// outra, com o valor cheio riscado quando há desconto. O selo de desconto fica
// sobre a capa, no canto superior esquerdo.
//
// Ao receber foco o ladrilho se centraliza no scroller do trilho: o
// useGamepadNav escolhe o vizinho pela geometria e alcança cards fora da área
// visível, então sem isto o foco sumiria da tela.
export function StoreTile({ jogo, indice = 0, largura = 176, onFocar, onAbrir }: StoreTileProps) {
  const ref = useRef<HTMLButtonElement | null>(null)

  return (
    <button
      ref={ref}
      onClick={() => onAbrir(jogo)}
      onFocus={() => {
        onFocar(jogo)
        ref.current?.scrollIntoView({ block: "nearest", inline: "center", behavior: rolagemSuave() })
      }}
      data-appid={jogo.appid}
      aria-label={jogo.title}
      className="loja-tile group shrink-0 text-left outline-none"
      style={{ width: largura, animationDelay: `${Math.min(indice * 20, 300)}ms` }}
    >
      <div className="loja-tile__capa relative overflow-hidden rounded-lg bg-[#111114]" style={{ aspectRatio: "3 / 4" }}>
        <ImgCascata
          fontes={urlsCapa(jogo, "retrato")}
          fallback={
            <div className="flex h-full w-full items-center justify-center bg-[#111114] px-3 text-center">
              <span className="line-clamp-4 text-[13px] leading-tight text-white/45">{jogo.title}</span>
            </div>
          }
        />

        {jogo.desconto ? (
          <span
            className="absolute left-2 top-2 rounded px-1.5 py-0.5 text-[11px] font-bold text-black"
            style={{ background: "var(--loja-cor)" }}
          >
            -{jogo.desconto}%
          </span>
        ) : null}

        {jogo.manifest === false && (
          <span className="absolute inset-x-0 bottom-0 bg-black/75 py-1 text-center text-[11px] font-medium text-white/55">
            Sem manifesto
          </span>
        )}
      </div>

      <div className="mt-2.5">
        <h3 className="truncate text-[13px] font-medium text-white/90">{jogo.title}</h3>
        {jogo.preco ? (
          <div className="mt-0.5 flex items-baseline gap-2">
            <span className="text-[13px] font-semibold text-white">{jogo.preco}</span>
            {jogo.precoOriginal ? (
              <span className="text-[11px] text-white/40 line-through">{jogo.precoOriginal}</span>
            ) : null}
          </div>
        ) : (
          // Espaço reservado: sem ele, as capas de um trilho misto (com e sem
          // preço) ficariam desalinhadas na vertical.
          <div className="mt-0.5 h-[18px]" />
        )}
      </div>
    </button>
  )
}
