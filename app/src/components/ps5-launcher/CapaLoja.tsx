"use client"

import { useState } from "react"
import { ImgCascata } from "./ImgCascata"
import { urlsPaisagem, urlsRetrato } from "./capaJogo"
import type { JogoLinha } from "./types"

/**
 * A capa de um ladrilho da loja, em quatro degraus.
 *
 * 1. arte retrato oficial da Steam (`library_600x900`) — o caso normal
 * 2. capa da SteamGridDB, pedida SÓ quando o degrau 1 falha. É arte da
 *    comunidade em 600x900 de verdade, então o ladrilho fica idêntico aos
 *    vizinhos, sem tarja
 * 3. arte horizontal encaixada na moldura: uma cópia borrada preenche o fundo e
 *    o header aparece inteiro por cima. Um header 460x215 em `object-cover`
 *    numa moldura 3:4 mostraria só uma tira do meio
 * 4. o título, quando não existe imagem nenhuma
 *
 * A consulta à SteamGridDB é disparada pelo `onError` da imagem oficial, e não
 * de véspera: assim ela acontece só para os poucos jogos sem arte vertical, e
 * não uma vez por ladrilho da tela.
 */
export function CapaLoja({ jogo }: { jogo: JogoLinha }) {
  // undefined = ainda não perguntamos · "" = perguntamos e não existe
  const [alternativa, setAlternativa] = useState<string | undefined>(undefined)
  const [semOficial, setSemOficial] = useState(false)

  const buscarAlternativa = () => {
    setSemOficial(true)
    if (alternativa !== undefined) return
    window.launcherAPI?.storeCover?.(jogo.appid).then((r) => setAlternativa(r?.url || ""))
  }

  if (!semOficial) {
    return <ImgCascata fontes={urlsRetrato(jogo)} onEsgotar={buscarAlternativa} />
  }

  if (alternativa) {
    return <ImgCascata fontes={[alternativa]} fallback={<Encaixada jogo={jogo} />} />
  }

  return <Encaixada jogo={jogo} />
}

// Arte horizontal dentro da moldura vertical, sem corte.
function Encaixada({ jogo }: { jogo: JogoLinha }) {
  const paisagem = urlsPaisagem(jogo)
  if (!paisagem.length) return <TituloSo jogo={jogo} />

  return (
    <div className="relative h-full w-full overflow-hidden">
      {/* Fundo: a mesma arte ampliada e borrada, para a moldura ficar com as
          cores do jogo em vez de um bloco cinza. A saturação compensa o que o
          desfoque tira. */}
      <div className="absolute inset-0 scale-150 blur-2xl brightness-[0.7] saturate-150">
        <ImgCascata fontes={paisagem} />
      </div>
      <div className="absolute inset-0 flex items-center">
        <ImgCascata fontes={paisagem} className="!object-contain" fallback={<TituloSo jogo={jogo} />} />
      </div>
    </div>
  )
}

function TituloSo({ jogo }: { jogo: JogoLinha }) {
  return (
    <div className="flex h-full w-full items-center justify-center bg-[var(--loja-sup-2)] px-3 text-center">
      <span className="line-clamp-4 text-[13px] leading-tight text-[var(--loja-apagado)]">{jogo.title}</span>
    </div>
  )
}
