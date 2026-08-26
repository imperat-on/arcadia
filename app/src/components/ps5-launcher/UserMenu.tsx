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

  const item = (label: string, onClick: () => void, opts?: { danger?: boolean; hint?: string }) => (
    <button
      onClick={() => {
        onClose()
        onClick()
      }}
      className="retro-user-menu-item flex w-full items-center justify-between gap-6 px-4 py-2.5 text-left"
      data-danger={opts?.danger || undefined}
    >
      <span>{label}</span>
      {opts?.hint && <span className="retro-user-menu-hint text-xs">{opts.hint}</span>}
    </button>
  )

  const divider = <div className="retro-user-menu-divider" />

  return (
    <div
      ref={ref}
      className="retro-user-menu gp-scope absolute right-0 top-12 z-50 w-64 overflow-hidden"
    >
      {/* Cabeçalho da conta (clique abre o perfil) */}
      <button
        onClick={() => {
          onClose()
          onOpenProfile()
        }}
        className="retro-user-menu-header flex w-full items-center gap-3 px-4 py-3 text-left"
      >
        <div
          className="retro-user-menu-avatar flex h-9 w-9 items-center justify-center overflow-hidden text-xs font-bold text-white"
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
