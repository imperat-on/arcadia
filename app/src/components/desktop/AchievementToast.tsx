"use client"

// Toast de conquista desbloqueada dentro do launcher (sem som — o som fica
// na janela nativa do electron/notify.js): canto inferior direito, fila com
// no máximo 3, auto-hide ~5s. Independente do painel — o watcher do main
// process entrega o payload mesmo com a loja fechada.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

type PayloadConquista = {
  appid: string
  key: string
  title: string
  desc?: string
  icon?: string
  percent?: number
  unlock?: number
}

type ItemToast = PayloadConquista & { _id: number }

export function AchievementToast() {
  const { t } = useI18n()
  const [filas, setFilas] = useState<ItemToast[]>([])

  useEffect(() => {
    const off = window.launcherAPI?.onAchievementUnlocked((payload) => {
      setFilas((prev) => [{ ...payload, _id: Date.now() + Math.random() }, ...prev].slice(0, 3))
    })
    return off
  }, [])

  return (
    <>
      <div className="pointer-events-none fixed bottom-6 right-6 z-50 flex flex-col gap-2">
        {filas.map((f) => (
          <ToastItem
            key={f._id}
            item={f}
            heading={t("conquistas.desbloquear_ok")}
            onFechar={() => setFilas((prev) => prev.filter((x) => x._id !== f._id))}
          />
        ))}
      </div>
      <style>{`
        @keyframes ach-in { from { opacity:0; transform:translateY(12px);} to {opacity:1; transform:translateY(0);} }
        @keyframes ach-out { from {opacity:1; transform:translateY(0);} to {opacity:0; transform:translateY(-8px);} }
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
      className="pointer-events-auto relative flex items-center gap-3 rounded-xl border border-white/10 bg-[#1c1c22]/95 px-3 py-2.5 shadow-2xl shadow-black/60 backdrop-blur-md"
      style={{
        minWidth: "280px",
        maxWidth: "360px",
        animation: saindo ? "ach-out 200ms ease-out forwards" : "ach-in 300ms ease-out",
      }}
    >
      <button
        onClick={onFechar}
        className="absolute right-2 top-2 text-white/40 transition-colors hover:text-white"
        aria-label="Fechar"
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
        <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-[#f5a623]">
          {heading}
        </div>
        <div className="truncate text-[13px] font-semibold text-white">{item.title}</div>
        <div className="truncate text-[12px] text-white/55">{item.desc || " "}</div>
      </div>
    </div>
  )
}
