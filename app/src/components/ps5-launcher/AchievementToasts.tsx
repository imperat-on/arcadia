"use client"

import { useEffect, useState } from "react"

interface UnlockPayload {
  appid: string
  title: string
  desc: string
  icon: string
  percent: number
  unlock: number
}

interface Toast extends UnlockPayload {
  key: number
  saindo?: boolean
}

const DURACAO = 6000

// Toasts de conquista estilo PS5: deslizam do topo no canto direito,
// ficam ~6s e saem com fade. Fica por cima de tudo (z-[70]).
export function AchievementToasts() {
  const [toasts, setToasts] = useState<Toast[]>([])

  useEffect(() => {
    const api = window.launcherAPI
    if (!api?.onAchievementUnlocked) return
    return api.onAchievementUnlocked((data) => {
      const key = Date.now() + Math.random()
      setToasts((t) => [...t.slice(-2), { ...data, key }]) // máx. 3 na tela
      setTimeout(() => {
        setToasts((t) => t.map((x) => (x.key === key ? { ...x, saindo: true } : x)))
      }, DURACAO - 400)
      setTimeout(() => {
        setToasts((t) => t.filter((x) => x.key !== key))
      }, DURACAO)
    })
  }, [])

  return (
    <div className="pointer-events-none fixed right-6 top-6 z-[70] flex w-[380px] flex-col gap-3">
      {toasts.map((t) => {
        const pct = typeof t.percent === "number" ? t.percent : parseFloat(String(t.percent)) || 0
        const rara = pct > 0 && pct <= 10
        return (
          <div
            key={t.key}
            className={`flex items-center gap-4 rounded-2xl border border-white/12 bg-black/85 px-5 py-4 shadow-2xl shadow-black/60 backdrop-blur-2xl ${t.saindo ? "toast-out" : "toast-in"}`}
          >
            <img
              src={t.icon}
              alt=""
              className="h-12 w-12 shrink-0 rounded-lg ring-1 ring-white/20"
              draggable={false}
            />
            <div className="min-w-0 flex-1">
              <span className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-[0.2em]" style={{ color: "var(--accent)" }}>
                <Trofeu className="h-3.5 w-3.5" />
                Conquista desbloqueada
              </span>
              <h4 className="mt-0.5 truncate text-sm font-medium text-white">{t.title}</h4>
              {t.desc && <p className="truncate text-xs font-light text-white/60">{t.desc}</p>}
              {pct > 0 && (
                <span className={`mt-0.5 block text-[10px] ${rara ? "text-[#ffd23f]" : "text-white/35"}`}>
                  {pct.toFixed(1).replace(".", ",")}% dos jogadores{rara ? " — rara" : ""}
                </span>
              )}
            </div>
          </div>
        )
      })}
    </div>
  )
}

function Trofeu({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M6 9H4.5a2.5 2.5 0 0 1 0-5H6" />
      <path d="M18 9h1.5a2.5 2.5 0 0 0 0-5H18" />
      <path d="M4 22h16" />
      <path d="M10 14.66V17c0 .55-.47.98-.97 1.21C7.85 18.75 7 20.24 7 22" />
      <path d="M14 14.66V17c0 .55.47.98.97 1.21C16.15 18.75 17 20.24 17 22" />
      <path d="M18 2H6v7a6 6 0 0 0 12 0V2Z" />
    </svg>
  )
}
