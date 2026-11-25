"use client"

// O processo principal escolhe a superfície. Só eventos "launcher" entram na fila.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import type { TrophyUnlock } from "../../global"

type ItemToast = TrophyUnlock & { _id: string }

export function AchievementToast() {
  const { t } = useI18n()
  const [filas, setFilas] = useState<ItemToast[]>([])

  useEffect(() => {
    const off = window.launcherAPI?.onAchievementUnlocked((payload) => {
      if (payload.notice !== "launcher") return
      setFilas((prev) => [{ ...payload, _id: `${payload.appid}:${payload.apiname || payload.key}:${payload.unlock}` }, ...prev].slice(0, 3))
      if (payload.sound) void new Audio(payload.platinum ? "./sounds/platinum.wav" : "./sounds/unlock.wav").play().catch(() => {})
    })
    return off
  }, [])

  return (
    <>
      <div role="status" className="pointer-events-none fixed bottom-6 right-6 z-[95] flex flex-col gap-2">
        {filas.map((f) => (
          <ToastItem
            key={f._id}
            item={f}
            heading={t(f.platinum ? "trofeus.platina" : f.rare ? "trofeus.raro" : "conquistas.desbloquear_ok")}
            onFechar={() => setFilas((prev) => prev.filter((x) => x._id !== f._id))}
          />
        ))}
      </div>
      <style>{`
        @keyframes ach-in { from { opacity:0; transform:translateY(12px);} to {opacity:1; transform:translateY(0);} }
        @keyframes ach-out { from {opacity:1; transform:translateY(0);} to {opacity:0; transform:translateY(-8px);} }
        @keyframes ach-platinum { from {opacity:0; transform:translateY(12px) scale(.95);} to {opacity:1; transform:none;} }
        @media (prefers-reduced-motion: no-preference) {
          [data-trophy-motion="in"] { animation: ach-in 300ms ease-out; }
          [data-trophy-motion="platinum"] { animation: ach-platinum 420ms ease-out; }
          [data-trophy-motion="out"] { animation: ach-out 200ms ease-out forwards; }
        }
      `}</style>
    </>
  )
}

function ToastItem({
  item,
  heading,
  onFechar,
}: {
  item: ItemToast
  heading: string
  onFechar: () => void
}) {
  const [saindo, setSaindo] = useState(false)
  const { t } = useI18n()

  // Auto-hide ~5s: marca saindo (anima 200ms), depois remove da fila.
  useEffect(() => {
    const t = setTimeout(() => setSaindo(true), 5000)
    return () => clearTimeout(t)
  }, [])

  useEffect(() => {
    if (!saindo) return
    const t = setTimeout(onFechar, 200)
    return () => clearTimeout(t)
  }, [saindo, onFechar])

  return (
    <div
      className="arcadia-achievement-toast pointer-events-auto relative flex items-center gap-3 rounded-xl border border-white/10 bg-[color:var(--surface-3)]/95 px-3 py-2.5 shadow-2xl shadow-black/60 backdrop-blur-md"
      data-trophy-motion={item.reducedMotion ? undefined : saindo ? "out" : item.platinum ? "platinum" : "in"}
      style={{
        minWidth: "280px",
        maxWidth: "360px",
      }}
    >
      <button
        onClick={onFechar}
        className="absolute right-2 top-2 text-white/40 transition-colors hover:text-white"
        aria-label={t("common.fechar")}
      >
        <svg
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
      {item.icon ? (
        <img src={item.icon} alt="" className="h-12 w-12 shrink-0 rounded-md object-cover" />
      ) : (
        <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md bg-white/10 text-2xl">
          🏆
        </div>
      )}
      <div className="flex min-w-0 flex-col">
        <div className={`text-[10px] font-semibold uppercase tracking-[0.18em] ${item.rare ? "text-[var(--state-info-soft)]" : "text-[var(--state-warn)]"}`}>
          {heading}
        </div>
        <div className="truncate text-[13px] font-semibold text-white">{item.title}</div>
        <div className="truncate text-[12px] text-white/65">{item.gameTitle || item.desc}</div>
      </div>
    </div>
  )
}
