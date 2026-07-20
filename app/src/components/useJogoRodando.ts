"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { Game } from "./ps5-launcher/types"

// Acompanha o jogo em execução — qual é, e se o processo ainda vive.
//
// Quem sabe a verdade é o main: ele vigia o processo a cada 3s e avisa nas
// transições ("game:running"). O modo desktop já se ancorava nisso pelo card
// "jogando"; o console adivinhava pelo foco da janela, que erra sempre que o
// jogo não rouba o foco. Com o hook, a regra vive num lugar só.

/** Jogo demorando demais para abrir: desiste e destrava a interface. */
const ABERTURA_MS = 60000
/** Pediu para parar e o processo não morreu: solta o estado mesmo assim. */
const FECHANDO_MS = 10000

export function useJogoRodando() {
  const [jogo, setJogo] = useState<Game | null>(null)
  // Lançado mas ainda não visto no vigia. A distinção importa: entre o
  // `launch` e o processo aparecer passam segundos, e um segundo toque em A
  // nesse intervalo não pode nem lançar de novo nem matar o jogo que está
  // subindo.
  const [confirmado, setConfirmado] = useState(false)
  // O vigia manda transições. Sem esta marca, um "false" chegando antes de o
  // jogo subir apagaria o jogo recém-lançado.
  const viuRodando = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)

  const limpar = useCallback(() => {
    clearTimeout(timer.current)
    viuRodando.current = false
    setConfirmado(false)
    setJogo(null)
  }, [])

  /** Chame logo depois de `launch()`. */
  const iniciar = useCallback((g: Game) => {
    clearTimeout(timer.current)
    viuRodando.current = false
    setConfirmado(false)
    setJogo(g)
    timer.current = setTimeout(() => {
      if (!viuRodando.current) {
        setJogo(null)
        setConfirmado(false)
      }
    }, ABERTURA_MS)
  }, [])

  const parar = useCallback(() => {
    window.launcherAPI?.closeGame()
    clearTimeout(timer.current)
    timer.current = setTimeout(limpar, FECHANDO_MS)
  }, [limpar])

  useEffect(() => {
    const off = window.launcherAPI?.onGameRunning((rodando) => {
      if (rodando) {
        viuRodando.current = true
        setConfirmado(true)
        clearTimeout(timer.current)
      } else if (viuRodando.current) {
        limpar()
      }
    })
    return () => {
      off?.()
      clearTimeout(timer.current)
    }
  }, [limpar])

  return {
    jogo,
    /** Processo confirmado pelo vigia — é quando o botão pode virar "Parar". */
    rodando: confirmado,
    /** Lançado, esperando o processo subir. */
    pendente: Boolean(jogo) && !confirmado,
    iniciar,
    parar,
    limpar,
  }
}
