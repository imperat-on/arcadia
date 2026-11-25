// TypeScript · React renderer · Arcadia Desktop and Big Picture
import { useEffect, useRef, useState } from "react"
import type { Game } from "./ps5-launcher/types"
import type { TrophyUnlock } from "../global"
import { useAccountOptional } from "./account/AccountContext"
import { addSessionTrophy } from "./sessionTrophies"

export type GameSessionSummary = { title: string; seconds: number; trophies: TrophyUnlock[] }

export function useGameReturnMotion(running: boolean, busy: boolean, focused: boolean, game: Game | null, reviewing = false) {
  const surface = useRef<HTMLDivElement>(null)
  const played = useRef(false)
  const account = useAccountOptional()?.session?.user?.id
  const session = useRef<{ game: Game; account?: string; started: number; startedEpoch: number; ended?: number; seconds?: number; unlocks: Map<string, TrophyUnlock> } | null>(null)
  const [result, setResult] = useState<{ account?: string; data: GameSessionSummary } | null>(null)
  const summary = result?.account === account ? result?.data ?? null : null

  useEffect(() => window.launcherAPI?.onAchievementUnlocked?.(trophy => {
    const active = session.current
    const now = Math.floor(Date.now() / 1000)
    // The Steam bin may flush on exit, after the renderer has already returned.
    if (!active || active.account !== account || (active.ended !== undefined && now - active.ended > 20)) return
    if (addSessionTrophy(active.unlocks, active.game.id, active.startedEpoch, active.ended ?? now, trophy) && active.seconds !== undefined) {
      setResult({ account, data: { title: active.game.title, seconds: active.seconds, trophies: [...active.unlocks.values()] } })
    }
  }), [account])

  useEffect(() => {
    if (running) {
      played.current = true
      setResult(null)
      if (game && (session.current?.game.id !== game.id || session.current?.ended !== undefined)) {
        session.current = { game, account, started: performance.now(), startedEpoch: Math.floor(Date.now() / 1000), unlocks: new Map() }
      }
    } else if (!busy && session.current && session.current.ended === undefined) {
      const active = session.current
      active.ended = Math.floor(Date.now() / 1000)
      active.seconds = Math.floor((performance.now() - active.started) / 1000)
      if (active.account === account) setResult({ account, data: { title: active.game.title, seconds: active.seconds, trophies: [...active.unlocks.values()] } })
    }
    if (busy || !focused || !played.current) return
    played.current = false

    const preference = window.matchMedia("(prefers-reduced-motion: reduce)")
    if (preference.matches || !surface.current) return
    const motion = surface.current.animate([{ opacity: .84 }, { opacity: 1 }], {
      duration: 240,
      easing: "ease-out",
    })
    const cancel = () => motion.cancel()
    preference.addEventListener("change", cancel)
    return () => {
      preference.removeEventListener("change", cancel)
      cancel()
    }
  }, [running, busy, focused, game, account])

  useEffect(() => {
    if (!summary || !focused || busy || reviewing) return
    const timer = window.setTimeout(() => setResult(null), 6500)
    return () => window.clearTimeout(timer)
  }, [summary, focused, busy, reviewing])

  return { surface, summary: (focused || reviewing) && !busy ? summary : null }
}
