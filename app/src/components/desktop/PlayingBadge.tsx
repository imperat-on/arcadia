"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "../ps5-launcher/types"

// Card "jogando <jogo>" no canto inferior direito — aparece quando um jogo
// é lançado e fica visível ENQUANTO o processo do jogo existir (o main
// process vigia via pgrep e avisa nas transições "game:running"). O botão X
// fecha o jogo (game:close). Comunicação entre componentes via CustomEvent.

const EVENTO = "arcadia:jogando"
const ABERTURA_MS = 60000 // jogo demorando demais pra aparecer -> desiste
const FECHANDO_MS = 10000 // clicou no X e o processo não morreu -> esconde

/** Dispara o card "jogando". Chame logo após launcherAPI.launch(). */
export function avisarJogando(game: Game) {
  window.dispatchEvent(new CustomEvent(EVENTO, { detail: game }))
}

export function PlayingBadge() {
  const [game, setGame] = useState<Game | null>(null)
  const [visivel, setVisivel] = useState(false)
  const viuRodando = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)

  useEffect(() => {
    const handler = (e: Event) => {
      const g = (e as CustomEvent<Game>).detail
      if (!g) return
      setGame(g)
      setVisivel(true)
      viuRodando.current = false
      clearTimeout(timer.current)
      // Se o jogo nem chegar a abrir, esconde depois de um tempo.
      timer.current = setTimeout(() => {
        if (!viuRodando.current) setVisivel(false)
      }, ABERTURA_MS)
    }
    window.addEventListener(EVENTO, handler)

    // Vigia do processo (main): true = jogo abriu, false = fechou.
    const off = window.launcherAPI?.onGameRunning((rodando) => {
      if (rodando) {
        viuRodando.current = true
        clearTimeout(timer.current)
      } else if (viuRodando.current) {
        setVisivel(false)
      }
    })

    return () => {
      window.removeEventListener(EVENTO, handler)
      off?.()
      clearTimeout(timer.current)
    }
  }, [])

  if (!visivel || !game) return null

  const fecharJogo = () => {
    window.launcherAPI?.closeGame()
    clearTimeout(timer.current)
    // Se o processo não morrer (falha no pkill), esconde mesmo assim.
    timer.current = setTimeout(() => setVisivel(false), FECHANDO_MS)
  }

  return (
    <div
      className="fixed bottom-5 right-5 z-[70] flex w-[320px] items-center gap-3 overflow-hidden rounded-2xl border border-white/15 bg-[#0d1017]/95 p-3 shadow-2xl shadow-black/60 backdrop-blur-md"
      style={{ animation: "jogando-in 0.3s cubic-bezier(0.2, 0.9, 0.3, 1.2)" }}
    >
      {/* Capa (2:3, sempre inteira) */}
      <div className="relative h-[72px] w-[48px] shrink-0 overflow-hidden rounded-lg bg-black">
        {game.cover || game.hero ? (
          <img
            src={game.cover || game.hero}
            alt=""
            className="h-full w-full object-cover"
            draggable={false}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-[9px] text-white/25">sem capa</div>
        )}
      </div>

      {/* Texto */}
      <div className="min-w-0 flex-1">
        <span className="mb-0.5 flex items-center gap-1.5 text-[12px] font-bold italic leading-none text-[#3ddc6e]">
          <span className="relative flex h-1.5 w-1.5">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-[#3ddc6e] opacity-60" />
            <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-[#3ddc6e]" />
          </span>
          jogando
        </span>
        <span className="block truncate text-[14px] font-medium text-white/90" title={game.title}>
          {game.title}
        </span>
        <span className="mt-0.5 block text-[11px] capitalize text-white/35">{game.launcher}</span>
      </div>

      {/* Fechar o jogo */}
      <button
        onClick={fecharJogo}
        title="Fechar o jogo"
        className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#e8703a] text-white transition-transform hover:scale-110"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
          <line x1="5" y1="5" x2="19" y2="19" />
          <line x1="19" y1="5" x2="5" y2="19" />
        </svg>
      </button>

      <style>{`
        @keyframes jogando-in {
          from { opacity: 0; transform: translateY(14px) scale(0.96); }
          to { opacity: 1; transform: translateY(0) scale(1); }
        }
      `}</style>
    </div>
  )
}
