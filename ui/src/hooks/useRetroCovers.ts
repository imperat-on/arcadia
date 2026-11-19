"use client"

import { convertFileSrc } from "@tauri-apps/api/core"
import { useEffect, useState } from "react"

import type { RetroGame } from "../global"

/**
 * Capas locais do catálogo Retro, buscadas em lote.
 *
 * ## Por que existe
 *
 * As capas do catálogo apontam para o original em hosts externos: medido nesta
 * máquina, uma página de 24 jogos baixava **29,4 MB** (1,25 MB de média, 494 KB
 * no maior PNG), e como o GitHub responde `max-age=300`, os mesmos 29 MB
 * voltavam a cada cinco minutos.
 *
 * O backend baixa cada capa uma vez, reduz para 480 px e guarda em disco. Este
 * hook recebe o caminho do arquivo e o converte com `convertFileSrc` para uma
 * URL do protocolo de asset do Tauri.
 *
 * ## Por que URL e não data URI
 *
 * Data URI não tem endereço, então o WebKit não consegue cachear o bitmap: cada
 * visita recriava os ~25 MB de imagem decodificada da página. Como URL, o
 * WebKit cacheia e descarta junto com o componente.
 *
 * ## Silêncio por design
 *
 * Falha devolve mapa vazio e o card usa a URL remota do catálogo, que é o
 * comportamento sem cache. O cache é otimização, nunca requisito.
 */
export function useRetroCovers(games: RetroGame[]) {
  const [capas, setCapas] = useState<Record<string, string>>({})

  // A lista de ids é a chave real: `games` muda de identidade a cada página.
  const chave = games.map((game) => game.id).join("|")

  useEffect(() => {
    const urls = games
      .map((game) => {
        const url = game.cover || game.capa || game.fallbackCover
        return typeof url === "string" && url.startsWith("http")
          ? { id: game.id, url }
          : null
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
        const convertidas: Record<string, string> = {}
        for (const [id, caminho] of Object.entries(resposta.covers)) {
          // Só converte o que é caminho absoluto; se vier outra coisa, ignora
          // e o card cai na URL do servidor.
          if (typeof caminho === "string" && caminho.startsWith("/")) {
            try {
              convertidas[id] = convertFileSrc(caminho)
            } catch {
              // `convertFileSrc` indisponível: segue sem cache.
            }
          }
        }
        // Mescla: páginas diferentes compartilham capas.
        setCapas((anteriores) => ({ ...anteriores, ...convertidas }))
      })
      .catch(() => {})

    return () => {
      vivo = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chave])

  return capas
}
