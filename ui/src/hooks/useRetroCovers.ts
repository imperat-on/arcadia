"use client"

import { useEffect, useState } from "react"

import type { RetroGame } from "../global"

/**
 * Capas locais do catálogo Retro, buscadas em lote.
 *
 * ## Por que existe
 *
 * As capas do catálogo apontam para o tamanho original: medido nesta máquina,
 * uma página de 24 jogos baixava 29,4 MB de imagem (1,25 MB de média, 494 KB na
 * maior). Para um card de 210 px isso é desperdício de banda e de decodificação
 * — e como o `raw.githubusercontent.com` responde `max-age=300`, os mesmos 29 MB
 * voltavam a cada cinco minutos.
 *
 * O backend (`retro:covers`) baixa cada capa uma vez, reduz para 480 px de
 * largura e guarda em disco. Este hook recebe os data URIs já prontos.
 *
 * ## Por que em lote
 *
 * Uma chamada por página em vez de uma por card: 24 chamadas IPC concorrentes
 * com latência de rede cada uma ocupariam a fila e fariam a grade aparecer aos
 * pedaços. Em lote, a primeira visita mostra o placeholder e troca tudo de uma
 * vez quando a resposta chega; as seguintes são instantâneas (só disco).
 *
 * ## Silêncio por design
 *
 * Falha devolve mapa vazio e o card cai na URL remota do catálogo — o
 * comportamento anterior. Cache é otimização, nunca requisito.
 */
export function useRetroCovers(games: RetroGame[]) {
  const [capas, setCapas] = useState<Record<string, string>>({})

  useEffect(() => {
    const urls = games
      .map((game) => {
        const url = game.cover || game.capa || game.fallbackCover
        return typeof url === "string" && url.startsWith("http") ? { id: game.id, url } : null
      })
      .filter((item): item is { id: string; url: string } => Boolean(item))

    if (!urls.length) {
      setCapas({})
      return
    }

    let vivo = true
    void window.launcherAPI
      ?.retroCovers?.({ urls })
      .then((resposta) => {
        if (!vivo || !resposta?.ok || !resposta.covers) return
        // Mescla: páginas diferentes compartilham capas, e descartar o que já
        // veio faria o card piscar de volta para o remoto.
        setCapas((anteriores) => ({ ...anteriores, ...resposta.covers }))
      })
      .catch(() => {})

    return () => {
      vivo = false
    }
    // `games` muda de identidade a cada página; a chave real é a lista de ids.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [games.map((game) => game.id).join("|")])

  return capas
}
