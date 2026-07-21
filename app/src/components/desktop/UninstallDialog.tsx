"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import type { Game } from "../ps5-launcher/types"

// Diálogo de desinstalação (estilo Heroic) para jogos não-Steam:
// opção de remover o prefixo e/ou apagar configurações+logs.
export function UninstallDialog({
  game,
  onConfirm,
  onClose,
}: {
  game: Game
  onConfirm: (opts: { removePrefix: boolean; removeSettings: boolean }) => void
  onClose: () => void
}) {
  const { t } = useI18n()
  const [prefix, setPrefix] = useState("")
  const [removePrefix, setRemovePrefix] = useState(false)
  const [removeSettings, setRemoveSettings] = useState(false)

  useEffect(() => {
    window.launcherAPI?.gameSettingsGet(game.id).then((r) => {
      setPrefix(r?.settings?.prefixPath || r?.defaultPrefix || "")
    })
  }, [game.id])

  const Check = ({ checked, onChange, label, obs }: { checked: boolean; onChange: (v: boolean) => void; label: React.ReactNode; obs: string }) => (
    <label className="mb-4 flex cursor-pointer items-start gap-3">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-0.5 h-[18px] w-[18px] shrink-0 cursor-pointer appearance-none rounded-[4px] border border-white/30 checked:border-transparent"
        style={
          checked
            ? {
                background: "var(--accent)",
                backgroundImage: "url(\"data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='3.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='20 6 9 17 4 12'/></svg>\")",
                backgroundSize: "12px",
                backgroundPosition: "center",
                backgroundRepeat: "no-repeat",
              }
            : undefined
        }
      />
      <span>
        <span className="block text-[13px] text-white/85">{label}</span>
        <span className="mt-0.5 block text-[12px] leading-snug text-white/40">{obs}</span>
      </span>
    </label>
  )

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-[540px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-start justify-between">
          <h2 className="text-lg font-semibold text-white">{t("uninstall.titulo")}</h2>
          <button onClick={onClose} className="rounded-md p-1 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <p className="mb-5 text-[14px] text-white/85">
          {t("uninstall.confirmar", { name: game.title })}
        </p>

        <Check
          checked={removePrefix}
          onChange={setRemovePrefix}
          label={<>{t("uninstall.remover_prefixo")}: <span className="text-white/55">{prefix || "…"}</span></>}
          obs={t("uninstall.obs_remover")}
        />
        <Check
          checked={removeSettings}
          onChange={setRemoveSettings}
          label={t("uninstall.apagar_config")}
          obs={t("uninstall.obs_apagar")}
        />

        <div className="mt-2 flex justify-end gap-2.5">
          <button
            onClick={() => onConfirm({ removePrefix, removeSettings })}
            className="rounded-lg border px-6 py-2 text-[12px] font-semibold tracking-wider transition-colors hover:bg-white/[0.06]"
            style={{ borderColor: "color-mix(in srgb, var(--accent) 60%, transparent)", color: "var(--accent)" }}
          >
            {t("uninstall.sim")}
          </button>
          <button
            onClick={onClose}
            className="rounded-lg border border-white/15 px-6 py-2 text-[12px] font-semibold tracking-wider text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
          >
            {t("uninstall.nao")}
          </button>
        </div>
      </div>
    </div>
  )
}
