"use client"

import { useState, useEffect } from "react"
import { UserMenu } from "./UserMenu"
import type { Profile } from "../../global"

interface TopBarProps {
  profile?: Profile
  activeTab: number
  onTab: (i: number) => void
  onRefresh: () => void
  onOpenSettings: () => void
  onOpenProfile: () => void
  menuOpen: boolean
  onToggleMenu: () => void
  onCloseMenu: () => void
  showHidden: boolean
  onToggleShowHidden: () => void
  downloadsActive?: number
  onOpenDownloads?: () => void
}

// Ícones das abas (traço 1.8, estilo lucide — combina com o resto do app).
const IconeBase = ({ className = "", children }: { className?: string; children: React.ReactNode }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
    {children}
  </svg>
)

const IconeNoticias = ({ className = "" }: { className?: string }) => (
  <IconeBase className={className}>
    <path d="M4 22h16a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H8a2 2 0 0 0-2 2v16a2 2 0 0 1-4 0V9" />
    <path d="M18 14h-8" />
    <path d="M15 18h-5" />
    <path d="M10 6h8v4h-8V6Z" />
  </IconeBase>
)

const IconeJogos = ({ className = "" }: { className?: string }) => (
  <IconeBase className={className}>
    <line x1="6" x2="10" y1="11" y2="11" />
    <line x1="8" x2="8" y1="9" y2="13" />
    <line x1="15" x2="15.01" y1="12" y2="12" />
    <line x1="18" x2="18.01" y1="10" y2="10" />
    <path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z" />
  </IconeBase>
)

const IconeBiblioteca = ({ className = "" }: { className?: string }) => (
  <IconeBase className={className}>
    <rect width="7" height="7" x="3" y="3" rx="1" />
    <rect width="7" height="7" x="14" y="3" rx="1" />
    <rect width="7" height="7" x="14" y="14" rx="1" />
    <rect width="7" height="7" x="3" y="14" rx="1" />
  </IconeBase>
)

export const TABS = ["Notícias", "Jogos", "Biblioteca"]
const TAB_ICONES = [IconeNoticias, IconeJogos, IconeBiblioteca]

export function TopBar({
  profile,
  activeTab,
  onTab,
  onRefresh,
  onOpenSettings,
  onOpenProfile,
  menuOpen,
  onToggleMenu,
  onCloseMenu,
  showHidden,
  onToggleShowHidden,
  downloadsActive = 0,
  onOpenDownloads,
}: TopBarProps) {
  const initial = (profile?.name?.[0] || "J").toUpperCase()
  const [time, setTime] = useState("")
  const active = activeTab

  useEffect(() => {
    const update = () =>
      setTime(
        new Date().toLocaleTimeString("pt-BR", {
          hour: "2-digit",
          minute: "2-digit",
        }),
      )
    update()
    const interval = setInterval(update, 1000)
    return () => clearInterval(interval)
  }, [])

  return (
    <header className="anim-nav flex items-center justify-between px-10 pt-6 pb-4">
      {/* Abas em ícones: ativa em branco com ponto accent embaixo */}
      <nav className="flex items-center gap-14">
        {TABS.map((tab, i) => {
          const isActive = i === active
          const Icone = TAB_ICONES[i]
          return (
            <button
              key={tab}
              onClick={() => onTab(i)}
              title={tab}
              aria-label={tab}
              className="relative flex flex-col items-center pb-1 transition-colors duration-200"
              style={{
                color: isActive ? "#ffffff" : "rgba(255,255,255,0.40)",
                filter: "drop-shadow(0 2px 14px rgba(0,0,0,0.55))",
              }}
            >
              <Icone className="h-7 w-7" />
              <span
                className="absolute -bottom-1 h-1 w-1 rounded-full transition-opacity duration-200"
                style={{ background: "var(--accent)", opacity: isActive ? 1 : 0 }}
              />
            </button>
          )
        })}
      </nav>

      {/* Direita: downloads + engrenagem + perfil + relógio */}
      <div className="flex items-center gap-6">
        <button
          onClick={onOpenDownloads}
          title="Downloads"
          aria-label="Downloads"
          className="relative text-white/75 transition-colors hover:text-white"
          style={{ filter: "drop-shadow(0 2px 10px rgba(0,0,0,0.6))" }}
        >
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" />
            <line x1="12" x2="12" y1="15" y2="3" />
          </svg>
          {downloadsActive > 0 && (
            <span
              className="absolute -right-2 -top-1.5 flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-[10px] font-bold text-black"
              style={{ background: "var(--accent)" }}
            >
              {downloadsActive}
            </span>
          )}
        </button>

        <button
          onClick={onOpenSettings}
          title="Configurações"
          aria-label="Configurações"
          className="text-white/75 hover:text-white transition-colors"
          style={{ filter: "drop-shadow(0 2px 10px rgba(0,0,0,0.6))" }}
        >
          <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19.14 12.94a7.6 7.6 0 000-1.88l2.03-1.58a.48.48 0 00.12-.61l-1.92-3.32a.48.48 0 00-.58-.22l-2.39.96a7.03 7.03 0 00-1.62-.94l-.36-2.54a.47.47 0 00-.47-.4h-3.84a.47.47 0 00-.47.4l-.36 2.54c-.59.24-1.13.56-1.62.94l-2.39-.96a.48.48 0 00-.58.22L2.77 8.87a.48.48 0 00.12.61l2.03 1.58a7.6 7.6 0 000 1.88l-2.03 1.58a.48.48 0 00-.12.61l1.92 3.32c.12.21.37.29.58.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.04.23.24.4.47.4h3.84c.23 0 .43-.17.47-.4l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.46 0 .58-.22l1.92-3.32a.48.48 0 00-.12-.61l-2.03-1.58zM12 15.6A3.6 3.6 0 1112 8.4a3.6 3.6 0 010 7.2z" />
          </svg>
        </button>

        <div className="relative">
          <button
            onClick={onToggleMenu}
            title="Perfil"
            className="w-9 h-9 rounded-full flex items-center justify-center text-xs font-bold text-white overflow-hidden transition-transform hover:scale-105"
            style={{ background: "linear-gradient(135deg, #0072ce, #003791)" }}
          >
            {profile?.avatar ? (
              <img src={profile.avatar} alt="" className="w-full h-full object-cover" />
            ) : (
              initial
            )}
          </button>
          <UserMenu
            open={menuOpen}
            onClose={onCloseMenu}
            onOpenSettings={onOpenSettings}
            onOpenProfile={onOpenProfile}
            onRefresh={onRefresh}
            showHidden={showHidden}
            onToggleShowHidden={onToggleShowHidden}
            profile={profile}
          />
        </div>

        <span
          className="text-white text-[22px] font-medium tabular-nums"
          style={{ textShadow: "0 2px 14px rgba(0,0,0,0.55)" }}
        >
          {time}
        </span>
      </div>
    </header>
  )
}
