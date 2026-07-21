"use client"

import { useEffect, useRef } from "react"
import type { Profile } from "../../global"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"

interface UserMenuProps {
  open: boolean
  onClose: () => void
  onOpenSettings: () => void
  onOpenProfile: () => void
  onRefresh: () => void
  showHidden: boolean
  onToggleShowHidden: () => void
  profile?: Profile
}

export function UserMenu({
  open,
  onClose,
  onOpenSettings,
  onOpenProfile,
  onRefresh,
  showHidden,
  onToggleShowHidden,
  profile,
}: UserMenuProps) {
  const ref = useRef<HTMLDivElement>(null)
  const { t } = useI18n()
  useGamepadNav(ref, open, onClose)

  // Fecha ao clicar fora ou apertar Esc.
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("mousedown", onDown)
    window.addEventListener("keydown", onKey)
    return () => {
      window.removeEventListener("mousedown", onDown)
      window.removeEventListener("keydown", onKey)
    }
  }, [open, onClose])

  if (!open) return null

  const item = (
    label: string,
    onClick: () => void,
    opts?: { danger?: boolean; hint?: string },
  ) => (
    <button
      onClick={() => {
        onClose()
        onClick()
      }}
      className="w-full flex items-center justify-between gap-6 px-4 py-2.5 text-sm text-left transition-colors"
      style={{ color: opts?.danger ? "#ff6b6b" : "#e8eaf0" }}
      onMouseEnter={(e) =>
        (e.currentTarget.style.background = "rgba(255,255,255,0.07)")
      }
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span>{label}</span>
      {opts?.hint && (
        <span className="text-xs text-[#7a8aaa]">{opts.hint}</span>
      )}
    </button>
  )

  const divider = (
    <div style={{ height: 1, background: "rgba(255,255,255,0.07)" }} />
  )

  return (
    <div
      ref={ref}
      className="gp-scope absolute right-0 top-12 w-64 rounded-xl overflow-hidden z-50"
      style={{
        background: "rgba(22,29,48,0.98)",
        border: "1px solid rgba(255,255,255,0.1)",
        boxShadow: "0 16px 48px rgba(0,0,0,0.55)",
        backdropFilter: "blur(16px)",
      }}
    >
      {/* Cabeçalho da conta (clique abre o perfil) */}
      <button
        onClick={() => {
          onClose()
          onOpenProfile()
        }}
        className="w-full px-4 py-3 flex items-center gap-3 text-left transition-colors hover:brightness-125"
        style={{ background: "rgba(0,114,206,0.12)" }}
      >
        <div
          className="w-9 h-9 rounded-full flex items-center justify-center text-xs font-bold text-white overflow-hidden"
          style={{ background: "linear-gradient(135deg, #0072ce, #003791)" }}
        >
          {profile?.avatar ? (
            <img src={profile.avatar} alt="" className="w-full h-full object-cover" />
          ) : (
            (profile?.name?.[0] || "J").toUpperCase()
          )}
        </div>
        <div className="leading-tight">
          <div className="text-sm font-semibold text-white">
            {profile?.name || t("profile.jogador")}
          </div>
          <div className="text-xs text-[#00a8ff]">
            {profile?.owner !== false ? t("profile.dono") : t("profile.online")}
          </div>
        </div>
      </button>
      {divider}

      {item(t("profile.meu_perfil"), onOpenProfile)}
      {item(t("profile.atualizar_biblioteca"), onRefresh)}
      {item(t("profile.mostrar_ocultos"), onToggleShowHidden, {
        hint: showHidden ? t("common.ligado") : t("common.desligado"),
      })}
      {item(t("settings.title"), onOpenSettings)}
      {divider}
      {item(t("profile.sair"), () => window.launcherAPI?.quit(), { danger: true })}
    </div>
  )
}
