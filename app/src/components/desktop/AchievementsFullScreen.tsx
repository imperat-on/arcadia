"use client"

import type { ReactNode } from "react"

export function AchievementsFullScreen({
  done,
  total,
  progress,
  onClose,
  children,
}: {
  done: number
  total: number
  progress: number
  onClose: () => void
  children: ReactNode
}) {
  return (
    <div className="fixed inset-0 z-[80] flex flex-col bg-[#030405] text-white" role="dialog" aria-modal="true" aria-label="Todas as conquistas">
      <header className="flex h-[58px] shrink-0 items-center gap-3 border-b border-white/[.1] bg-[#050608] px-5">
        <button type="button" autoFocus onClick={onClose} className="detail-icon-btn" title="Voltar" aria-label="Voltar">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
        </button>
        <div className="min-w-0 flex-1">
          <h2 className="text-[15px] font-semibold">Conquistas</h2>
          <p className="text-[10px] text-white/45">{done}/{total} desbloqueadas</p>
        </div>
        <button type="button" onClick={onClose} className="detail-achievements-all">Fechar</button>
      </header>
      <div data-gamepad-scroll className="min-h-0 flex-1 overflow-y-auto px-5 py-5">
        <div className="mx-auto max-w-[1500px]">
          <div className="mb-5 h-1 overflow-hidden rounded bg-white/10">
            <span className="block h-full bg-[var(--desktop-green)]" style={{ width: `${progress}%` }} />
          </div>
          {children}
        </div>
      </div>
    </div>
  )
}
