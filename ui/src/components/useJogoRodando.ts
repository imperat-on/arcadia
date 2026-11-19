"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { Game } from "./ps5-launcher/types"

// Acompanha o jogo em execução — qual é, e se o processo ainda vive.
//
// Quem sabe a verdade é o main: ele vigia o processo a cada 3s e avisa nas
// transições ("game:running"/"game:active"). O modo desktop já se ancorava
// nisso pelo card "jogando"; o console adivinhava pelo foco da janela, que
// erra sempre que o jogo não rouba o foco. Com o hook, a regra vive num lugar
// só.

/** Jogo demorando demais para abrir: desiste e destrava a interface. */
const ABERTURA_MS = 60000
/** Pediu para parar e o processo não morreu: solta o estado mesmo assim. */
const FECHANDO_MS = 10000
const JOGOS_VAZIOS: readonly Game[] = []

/** Um evento replayado pode chegar antes da biblioteca local. */
function jogoReplay(id: string): Game {
  return { id, title: id, launcher: "custom", launch_cmd: [] }
}

export function useJogoRodando(games: readonly Game[] = JOGOS_VAZIOS) {
  const [jogo, setJogo] = useState<Game | null>(null)
  // Lançado mas ainda não visto no vigia. A distinção importa: entre o
  // `launch` e o processo aparecer passam segundos, e um segundo toque em A
  // nesse intervalo não pode nem lançar de novo nem matar o jogo que está
  // subindo.
  const [confirmado, setConfirmado] = useState(false)
  const gamesRef = useRef<readonly Game[]>(games)
  gamesRef.current = games
  const jogoAtual = useRef<Game | null>(null)
  // O vigia manda transições. O false do wrapper Steam é retido no main; a
  // marca também permite distinguir uma sessão confirmada de uma saída rápida.
  const viuRodando = useRef(false)
  // Depois de cancelar/parar, um evento TRUE atrasado não deve reabrir o
  // estado local e cancelar o timeout que libera a trava.
  const encerrando = useRef(false)
  const generation = useRef(0)
  // O estado de lançamento carrega o token monotônico do main. Ele impede que
  // um idle atrasado de uma tentativa antiga limpe uma nova tentativa do mesmo
  // jogo, inclusive depois de um reload do renderer.
  const launchToken = useRef<number | null>(null)
  const latestLaunchToken = useRef(0)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)

  const resolverJogo = useCallback((id: string) => {
    return gamesRef.current.find((game) => String(game.id) === id) || jogoReplay(id)
  }, [])

  const limpar = useCallback(() => {
    generation.current += 1
    clearTimeout(timer.current)
    viuRodando.current = false
    encerrando.current = false
    launchToken.current = null
    jogoAtual.current = null
    setConfirmado(false)
    setJogo(null)
  }, [])

  const armarTimeoutDeAbertura = useCallback((minhaGeracao: number) => {
    clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      if (generation.current !== minhaGeracao || viuRodando.current) return
      // A UI deixa de exibir o pending depois do limite, mas permanece em
      // "encerrando" até receber idle/false; um TRUE atrasado não pode
      // ressuscitar a sessão nem cancelar esse desbloqueio.
      encerrando.current = true
      viuRodando.current = false
      jogoAtual.current = null
      setJogo(null)
      setConfirmado(false)
    }, ABERTURA_MS)
  }, [])

  /** Chame logo depois de `launch()`. */
  const iniciar = useCallback((g: Game) => {
    const minhaGeracao = ++generation.current
    clearTimeout(timer.current)
    encerrando.current = false
    viuRodando.current = false
    // O token real chega pelo evento game:launchState; um launch local novo
    // deve aceitar o próximo token e nunca o idle de uma geração encerrada.
    launchToken.current = null
    jogoAtual.current = g
    setConfirmado(false)
    setJogo(g)
    armarTimeoutDeAbertura(minhaGeracao)
  }, [armarTimeoutDeAbertura])

  // Parar e cancelar usam o mesmo IPC: o main fecha tanto um processo já
  // confirmado quanto o wrapper que ainda está subindo. Mantemos a sessão
  // local até o evento false (ou o timeout), para não liberar um segundo
  // lançamento enquanto o primeiro ainda pode criar um processo.
  const encerrar = useCallback(() => {
    const minhaGeracao = generation.current
    encerrando.current = true
    const pedido = window.launcherAPI?.closeGame?.()
    if (pedido) {
      void pedido.then(() => {
        if (generation.current === minhaGeracao) limpar()
      }).catch(() => {})
    }
    clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      if (generation.current === minhaGeracao) limpar()
    }, FECHANDO_MS)
  }, [limpar])

  const parar = encerrar
  const cancelar = encerrar

  const aoRodar = useCallback(
    (rodando: boolean) => {
      if (rodando) {
        if (encerrando.current) return
        viuRodando.current = true
        setConfirmado(true)
        clearTimeout(timer.current)
      } else {
        // Do NOT call limpar() here. The main process still holds
        // jogoEncerrando/jogoAtivo until finalizarSessao releases the token
        // and emits game:launchState("idle"). Clearing too early lets the
        // renderer show "Play" while the main rejects the launch.
        // The full clear comes from aoLancamento("idle") or encerrar's .then.
        viuRodando.current = false
        setConfirmado(false)
      }
    },
    [],
  )

  const aoAtivo = useCallback(
    (info: { rodando: boolean; gameId: string }) => {
      if (!info?.rodando) {
        aoRodar(false)
        return
      }
      if (encerrando.current) return
      // game:active carrega o id que game:running não tem. Isso também torna
      // possível reconstruir o estado depois de um reload do renderer.
      const id = String(info.gameId || "")
      const atual = jogoAtual.current
      // Um evento atrasado de outra sessão nunca pode substituir um launch
      // local ainda pendente/rodando. O main também serializa launches, mas o
      // renderer mantém esta barreira para proteger cliques que vazem durante
      // a troca de foco.
      if (atual && id && String(atual.id) !== id) return
      viuRodando.current = true
      setConfirmado(true)
      clearTimeout(timer.current)
      if (!id) return
      if (!atual || atual.title === id) {
        const resolvido = resolverJogo(id)
        jogoAtual.current = resolvido
        setJogo(resolvido)
      }
    },
    [aoRodar, resolverJogo],
  )

  const aoLancamento = useCallback(
    (info: { state?: string; gameId?: string; token?: number | null }) => {
      const state = String(info?.state || "")
      const id = String(info?.gameId || "")
      const token = Number.isInteger(info?.token) ? Number(info?.token) : null
      const atualToken = launchToken.current

      // Tokens are monotonic in the main process. Ignore callbacks from an
      // older Steam/pre-script chain, especially when the game id is reused.
      if (token !== null && token < latestLaunchToken.current) return
      if (state === "idle") {
        if (token !== null) {
          // A renderer-local launch may already be pending while the previous
          // token's idle event is still in flight. Do not clear that newer
          // local generation before its `starting` token arrives.
          if (atualToken !== null && token !== atualToken) return
          if (atualToken === null && token === latestLaunchToken.current && jogoAtual.current && confirmado) return
          latestLaunchToken.current = Math.max(latestLaunchToken.current, token)
        }
        limpar()
        return
      }
      if (!["starting", "running", "stopping"].includes(state)) return
      // Stop/timeout is terminal for this renderer generation. A delayed
      // running/starting notification must not cancel its release timer.
      if (encerrando.current && state !== "stopping") return

      const atual = jogoAtual.current
      if (atual && id && String(atual.id) !== id) return
      if (token !== null) {
        latestLaunchToken.current = token
        launchToken.current = token
      }
      const resolvido = id ? resolverJogo(id) : atual
      if (!resolvido) return
      if (!atual) {
        const minhaGeracao = ++generation.current
        clearTimeout(timer.current)
        encerrando.current = state === "stopping"
        viuRodando.current = false
        if (state === "stopping") {
          // The session was already cleared (Stop/Cancel ran or the game
          // closed before confirmation). A late "stopping" notification must
          // not resurrect the pending state — the game is closing, not
          // opening. Only keep the release barrier until the main confirms.
          jogoAtual.current = null
          timer.current = setTimeout(() => {
            if (generation.current === minhaGeracao) limpar()
          }, FECHANDO_MS)
        } else {
          jogoAtual.current = resolvido
          setJogo(resolvido)
          setConfirmado(false)
          armarTimeoutDeAbertura(minhaGeracao)
        }
      } else if (state === "stopping") {
        encerrando.current = true
        const minhaGeracao = generation.current
        clearTimeout(timer.current)
        timer.current = setTimeout(() => {
          if (generation.current === minhaGeracao) limpar()
        }, FECHANDO_MS)
      }
      // `running` here means that Arcadia spawned its wrapper. Confirmation
      // still comes only from game:running/game:active after a real process is
      // observed; this avoids counting a dead Steam URI helper as playtime.
    },
    [armarTimeoutDeAbertura, limpar, resolverJogo],
  )

  useEffect(() => {
    // Assina ANTES do replay: app:focusState no main reenvia game:running,
    // game:active e game:launchState para cobrir um renderer que montou depois
    // do launch.
    const api = window.launcherAPI
    const offRunning = api?.onGameRunning?.(aoRodar)
    const offActive = api?.onGameActive?.(aoAtivo)
    const offLaunch = api?.onGameLaunchState?.(aoLancamento)
    const replay = api?.getAppFocus?.()
    if (replay) void replay.catch(() => {})
    return () => {
      offRunning?.()
      offActive?.()
      offLaunch?.()
      clearTimeout(timer.current)
    }
  }, [aoAtivo, aoLancamento, aoRodar])

  // Se o replay chegou antes da biblioteca, troca o placeholder pelo objeto
  // completo assim que useLibraryState terminar de carregar.
  useEffect(() => {
    const atual = jogoAtual.current
    if (!atual) return
    const resolvido = gamesRef.current.find((game) => String(game.id) === String(atual.id))
    if (resolvido && resolvido !== atual) {
      jogoAtual.current = resolvido
      setJogo(resolvido)
    }
  }, [games])

  return {
    jogo,
    /** Processo confirmado pelo vigia — é quando o botão pode virar "Parar". */
    rodando: confirmado,
    /** Lançado, esperando o processo subir. */
    pendente: Boolean(jogo) && !confirmado,
    iniciar,
    parar,
    /** Cancela um launch pendente sem abrir espaço para outro processo. */
    cancelar,
    limpar,
  }
}
