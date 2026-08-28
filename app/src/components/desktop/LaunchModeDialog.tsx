"use client"

import { useRef } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useGamepadNav } from "../ps5-launcher/useGamepadNav"
import type { Game } from "../ps5-launcher/types"

// Menu de escolha de como iniciar um jogo que tem as duas formas possíveis
// (modo padrão do Arcadia + executável configurado na aba Localizações).
export function LaunchModeDialog({
  game,
  onEscolher,
  onClose,
  active = true,
}: {
  game: Game
  onEscolher: (mode: "steam" | "exe") => void
  onClose: () => void
  /** Host focus gate; desktop callers keep the normal default. */
  active?: boolean
}) {
  const { t } = useI18n()
  const dialogRef = useRef<HTMLDivElement>(null)
  useGamepadNav(dialogRef, active, onClose)

  const Opcao = ({
    mode,
    titulo,
    desc,
  }: {
    mode: "steam" | "exe"
    titulo: string
    desc: string
  }) => (
    <button
      onClick={() => onEscolher(mode)}
      className="w-full rounded-xl border border-white/10 bg-white/[0.03] p-4 text-left transition-colors hover:border-[color:var(--accent)]/50 hover:bg-white/[0.06]"
    >
      <span className="block text-[14px] font-semibold text-white">{titulo}</span>
      <span className="mt-1 block text-[12px] leading-snug text-white/50">{desc}</span>
    </button>
  )

  return (
    <div
      ref={dialogRef}
      className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-[480px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-start justify-between">
          <h2 className="text-lg font-semibold text-white">{t("launchmode.titulo")}</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <p className="mb-5 text-[13px] text-white/70">
          {t("launchmode.subtitulo", { name: game.title })}
        </p>

        <div className="flex flex-col gap-3">
          <Opcao
            mode="steam"
            titulo={t("launchmode.steam_titulo")}
            desc={t("launchmode.steam_desc")}
          />
          <Opcao mode="exe" titulo={t("launchmode.exe_titulo")} desc={t("launchmode.exe_desc")} />
        </div>
      </div>
    </div>
  )
}
