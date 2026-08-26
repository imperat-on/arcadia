"use client"

import { useEffect, useState } from "react"
import { UserMenu } from "./UserMenu"
import type { Profile } from "../../global"
import { userLocale } from "../../i18n/locale"
import { useI18n } from "../../i18n/I18nContext"

export type LibraryFilter = "all" | "favorites" | "collections"

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
  libraryFilter?: LibraryFilter
  onLibraryFilter?: (filter: LibraryFilter) => void
  search?: string
  onSearch?: (value: string) => void
}

const Icon = ({ children, className = "" }: { children: React.ReactNode; className?: string }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
    {children}
  </svg>
)

const Gamepad = ({ className = "" }: { className?: string }) => <Icon className={className}><line x1="6" x2="10" y1="11" y2="11"/><line x1="8" x2="8" y1="9" y2="13"/><line x1="15" x2="15.01" y1="12" y2="12"/><line x1="18" x2="18.01" y1="10" y2="10"/><path d="M17.3 5H6.7a4 4 0 0 0-4 3.6C2.6 9.4 2 14.5 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.4-1.4A2 2 0 0 1 9.8 16h4.4a2 2 0 0 1 1.4.6L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.5-.6-6.6-.7-7.4A4 4 0 0 0 17.3 5Z"/></Icon>
const News = ({ className = "" }: { className?: string }) => <Icon className={className}><path d="M4 22h16a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H8a2 2 0 0 0-2 2v16a2 2 0 0 1-4 0V9"/><path d="M10 6h8v4h-8zM10 14h8M10 18h5"/></Icon>
const Store = ({ className = "" }: { className?: string }) => <Icon className={className}><path d="M3 9h18l-1.5 10.5a2 2 0 0 1-2 1.5h-11a2 2 0 0 1-2-1.5zM3 9l2-5h14l2 5M8.5 13a3.5 3.5 0 0 0 7 0"/></Icon>
const Gear = ({ className = "" }: { className?: string }) => <Icon className={className}><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></Icon>

export const TABS = ["topbar.noticias", "topbar.jogos", "topbar.loja"]

export function TopBar({
  profile, activeTab, onTab, onRefresh, onOpenSettings, onOpenProfile,
  menuOpen, onToggleMenu, onCloseMenu, showHidden, onToggleShowHidden,
  downloadsActive = 0, onOpenDownloads, libraryFilter = "all",
  onLibraryFilter, search = "", onSearch,
}: TopBarProps) {
  const { t } = useI18n()
  const initial = (profile?.name?.[0] || t("topbar.fallback_inicial")).toUpperCase()
  const [now, setNow] = useState(new Date())

  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  const navigation = [
    { tab: 1, label: t("topbar.jogos"), Icon: Gamepad },
    { tab: 0, label: t("topbar.noticias"), Icon: News },
    { tab: 2, label: t("topbar.loja"), Icon: Store },
  ]
  const sectionTitle = activeTab === 0 ? t("topbar.noticias") : activeTab === 2 ? t("topbar.loja") : ""

  return (
    <>
      <aside data-theme-slot="home.navigation" className="retro-nav-shell anim-nav fixed inset-y-0 left-0 z-30 flex w-[124px] flex-col border-r">
        <div className="retro-nav-brand flex h-[74px] flex-col justify-center border-b px-5">
          <strong className="leading-none">ARCADIA</strong>
        </div>

        <nav className="flex flex-col px-2 py-3" aria-label="Main navigation">
          {navigation.map(({ tab, label, Icon: ItemIcon }) => (
            <button key={tab} onClick={() => onTab(tab)} data-active={activeTab === tab} className="retro-nav-button relative flex h-[78px] flex-col items-center justify-center gap-1 border text-[10px] font-bold uppercase tracking-[0.08em]">
              <ItemIcon className="h-7 w-7" />
              <span>{label}</span>
            </button>
          ))}
          <button onClick={onOpenSettings} className="retro-nav-button relative flex h-[78px] flex-col items-center justify-center gap-1 border text-[10px] font-bold uppercase tracking-[0.08em]">
            <Gear className="h-7 w-7" />
            <span>{t("topbar.configuracoes")}</span>
          </button>
        </nav>

        <div className="mt-auto px-2 pb-4">
          <div className="retro-clock mb-3 border-y px-2 py-3 text-center">
            <strong className="block font-display text-lg tabular-nums">{now.toLocaleTimeString(userLocale(), { hour: "2-digit", minute: "2-digit" })}</strong>
            <span className="mt-1 block text-[8px] tabular-nums tracking-[0.08em]">{now.toLocaleDateString(userLocale())}</span>
          </div>
          <button onClick={onOpenDownloads} className="retro-nav-tool relative mx-auto grid h-10 w-10 place-items-center border" title={t("topbar.downloads")}>
            <Icon className="h-5 w-5"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></Icon>
            {downloadsActive > 0 && <span className="absolute -right-1 -top-1 min-w-4 bg-[var(--retro-phosphor)] px-1 text-[8px] font-black text-black">{downloadsActive}</span>}
          </button>
        </div>
      </aside>

      <div data-theme-slot="home.topbar" className="retro-top-strip fixed left-[124px] right-0 top-0 z-30 flex h-[54px] items-center border-b px-6">
        {activeTab === 1 ? (
          <nav className="retro-library-tabs mx-auto flex h-full items-center gap-10" aria-label="Library filters">
            {([ ["all", "Biblioteca"], ["favorites", "Favoritos"], ["collections", "Coleções"] ] as [LibraryFilter, string][]).map(([filter, label]) => (
              <button key={filter} onClick={() => onLibraryFilter?.(filter)} data-active={libraryFilter === filter} className="relative flex h-full items-center gap-2 text-[10px] font-bold uppercase tracking-[0.12em]">
                {filter === "all" && <Gamepad className="h-4 w-4" />}
                {label}
              </button>
            ))}
          </nav>
        ) : <strong className="text-[11px] uppercase tracking-[0.18em] text-white/70">{sectionTitle}</strong>}

        {activeTab === 1 && (
          <label className="retro-search-box absolute right-[86px] flex h-8 w-[230px] items-center gap-2 border px-3">
            <Icon className="h-4 w-4"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></Icon>
            <input value={search} onChange={(event) => onSearch?.(event.target.value)} placeholder="Buscar jogos..." className="min-w-0 flex-1 border-0 bg-transparent p-0 text-[10px] text-white outline-none" />
          </label>
        )}

        <div className="absolute right-5 top-1/2 -translate-y-1/2">
          <button onClick={onToggleMenu} className="retro-profile-button grid h-9 w-9 place-items-center overflow-hidden border text-[10px] font-black">
            {profile?.avatar ? <img src={profile.avatar} alt="" className="h-full w-full object-cover" /> : initial}
          </button>
          <UserMenu open={menuOpen} onClose={onCloseMenu} onOpenSettings={onOpenSettings} onOpenProfile={onOpenProfile} onRefresh={onRefresh} showHidden={showHidden} onToggleShowHidden={onToggleShowHidden} profile={profile} />
        </div>
      </div>
    </>
  )
}
