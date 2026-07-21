"use client"

import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

export function GameDetailsDialog({ game, onClose }: { game: Game; onClose: () => void }) {
  const { t } = useI18n()
  const horas = game.playtime_minutes != null ? Math.floor(game.playtime_minutes / 60) : null
  const campos: [string, string][] = [
    [t("gameoverview.detalhes.fonte"), game.launcher],
    ...(game.genre ? [[t("gameoverview.detalhes.genero"), String(game.genre)] as [string, string]] : []),
    ...(game.year ? [[t("gameoverview.detalhes.lancamento"), String(game.year)] as [string, string]] : []),
    ...(game.developer ? [[t("gameoverview.detalhes.desenvolvedora"), game.developer] as [string, string]] : []),
    ...(game.publisher ? [[t("gameoverview.detalhes.publicadora"), game.publisher] as [string, string]] : []),
    ...(horas != null ? [[t("gameoverview.detalhes.tempo_jogo"), t("gameoverview.tempo.horas_minutos", { h: String(horas), m: String(game.playtime_minutes! % 60) })]] as [string, string][] : []),
    ...(game.achievements_total ? [[t("gameoverview.detalhes.conquistas"), `${game.achievements_done || 0}/${game.achievements_total}`] as [string, string]] : []),
    ...(game.players ? [[t("gameoverview.detalhes.jogadores"), game.players] as [string, string]] : []),
  ]

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={onClose}>
      <div
        className="flex max-h-[85vh] w-[520px] max-w-[92vw] flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-[#0d0d10] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Hero */}
        <div className="relative h-[180px] w-full shrink-0 overflow-hidden bg-black">
          {game.hero || game.cover ? (
            <img src={game.hero || game.cover} alt="" className="h-full w-full object-cover" draggable={false} />
          ) : null}
          <div className="absolute inset-0 bg-gradient-to-b from-transparent via-transparent to-[#0d0d10]" />
          <button onClick={onClose} className="absolute right-3 top-3 rounded-md bg-black/50 p-1.5 text-white/70 transition-colors hover:bg-black/70 hover:text-white">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
          <h2 className="absolute bottom-3 left-5 right-5 truncate text-xl font-light text-white drop-shadow">{game.title}</h2>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {/* Campos */}
          <div className="mb-4 grid grid-cols-2 gap-x-6 gap-y-2.5">
            {campos.map(([k, v]) => (
              <div key={k}>
                <div className="text-[11px] uppercase tracking-wider text-white/35">{k}</div>
                <div className="truncate text-[13px] capitalize text-white/85">{v}</div>
              </div>
            ))}
          </div>
          {game.description ? (
            <p className="text-[13px] leading-relaxed text-white/60">{game.description}</p>
          ) : (
            <p className="text-[13px] text-white/30">{t("gameoverview.sem_descricao")}</p>
          )}
          <p className="mt-4 font-mono text-[11px] text-white/25">ID: {game.id}</p>
        </div>
      </div>
    </div>
  )
}
