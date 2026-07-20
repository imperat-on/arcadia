"use client"

import { ImgCascata } from "./ImgCascata"
import { urlsPaisagem, urlsRetrato } from "./capaJogo"
import type { JogoLinha } from "./types"

/**
 * A capa de um ladrilho da loja, em três degraus.
 *
 * 1. arte retrato oficial (`library_600x900`) — o caso normal
 * 2. arte horizontal encaixada na moldura vertical: uma cópia borrada preenche
 *    o fundo e o header aparece inteiro por cima. Sem isso, um header 460x215
 *    em `object-cover` numa moldura 3:4 mostra só uma tira do meio da imagem —
 *    era esse o "sem capa" que via na tela, já que a maioria dos lançamentos
 *    recentes não tem arte vertical publicada
 * 3. o título, quando o jogo não tem imagem nenhuma
 */
export function CapaLoja({ jogo }: { jogo: JogoLinha }) {
  const paisagem = urlsPaisagem(jogo)

  return (
    <ImgCascata
      fontes={urlsRetrato(jogo)}
      fallback={
        paisagem.length ? (
          <div className="relative h-full w-full overflow-hidden">
            {/* Fundo: a mesma arte ampliada e borrada, só para preencher a
                moldura com as cores do jogo em vez de um bloco cinza. */}
            <div className="absolute inset-0 scale-125 blur-xl brightness-[0.55]">
              <ImgCascata fontes={paisagem} />
            </div>
            <div className="absolute inset-0 flex items-center">
              <ImgCascata fontes={paisagem} className="!object-contain" fallback={<TituloSo jogo={jogo} />} />
            </div>
          </div>
        ) : (
          <TituloSo jogo={jogo} />
        )
      }
    />
  )
}

function TituloSo({ jogo }: { jogo: JogoLinha }) {
  return (
    <div className="flex h-full w-full items-center justify-center bg-[var(--loja-sup-2)] px-3 text-center">
      <span className="line-clamp-4 text-[13px] leading-tight text-[var(--loja-apagado)]">{jogo.title}</span>
    </div>
  )
}
