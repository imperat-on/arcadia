"use client"

// Sidebar do modo desktop (estilo Heroic, adaptada ao tema do Arcadia).
import { useMemo, useState } from "react"
import type { Profile } from "../../global"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

export type DesktopView = "inicio" | "biblioteca" | "lojas" | "plugins" | "downloads" | "wine" | "config" | "acessibilidade"
export type ConfigSub = "gerais" | "integracoes" | "metadados"

const ITENS: { id: DesktopView; label: string; icon: React.ReactNode; labelKey: string }[] = [
  { id: "inicio", label: "Início", labelKey: "sidebar.inicio", icon: <IconHome /> },
  { id: "biblioteca", label: "Biblioteca", labelKey: "sidebar.biblioteca", icon: <IconGrid /> },
  { id: "lojas", label: "Loja", labelKey: "sidebar.lojas", icon: <IconStore /> },
  { id: "plugins", label: "Componentes", labelKey: "sidebar.plugins", icon: <IconPlugin /> },
  { id: "downloads", label: "Downloads", labelKey: "sidebar.downloads", icon: <IconDownload /> },
  { id: "wine", label: "Wine Manager", labelKey: "sidebar.wine", icon: <IconWine /> },
  { id: "config", label: "Configurações", labelKey: "settings.title", icon: <IconGear /> },
  { id: "acessibilidade", label: "Acessibilidade", labelKey: "sidebar.acessibilidade", icon: <IconA11y /> },
]

const CONFIG_SUBS: { id: ConfigSub; label: string; labelKey: string }[] = [
  { id: "gerais", label: "Config. Gerais", labelKey: "settings.general" },
  { id: "integracoes", label: "Integrações", labelKey: "settings.integracoes" },
  { id: "metadados", label: "Metadados", labelKey: "settings.metadados.titulo" },
]

export function Sidebar({
  view,
  onView,
  downloadsActive,
  onQuit,
  onBigPicture,
  configSub,
  onConfigSub,
  profile,
  onProfile,
  onRefresh,
  games,
  librarySidebar,
  onToggleLibrarySidebar,
  onOpenGame,
  onAddGame,
  activeGameId,
}: {
  view: DesktopView
  onView: (v: DesktopView) => void
  downloadsActive: number
  onQuit: () => void
  onBigPicture: () => void
  configSub: ConfigSub
  onConfigSub: (s: ConfigSub) => void
  profile: Profile
  onProfile: () => void
  onRefresh: () => void
  games: Game[]
  librarySidebar: boolean
  onToggleLibrarySidebar: () => void
  onOpenGame: (g: Game) => void
  onAddGame: () => void
  activeGameId?: string
}) {
  const { t } = useI18n()
  const [profileMenu, setProfileMenu] = useState(false)
  const [buscaJogos, setBuscaJogos] = useState("")
  const nome = profile.name || t("profile.jogador")

  const jogos = useMemo(() => {
    const l = games.filter((g) => !g.hidden)
    const q = buscaJogos.trim().toLowerCase()
    const f = q ? l.filter((g) => g.title.toLowerCase().includes(q)) : l
    return [...f].sort((a, b) => Number(b.favorite || false) - Number(a.favorite || false) || a.title.localeCompare(b.title))
  }, [games, buscaJogos])
  return (
    <aside className="flex h-full w-64 shrink-0 flex-col" style={{ background: "var(--sidebar-bg)" }}>
      <div className="px-6 pb-5 pt-6">
        <button onClick={() => setProfileMenu((v) => !v)} className="group flex w-full items-center gap-3 rounded-xl p-1 text-left transition-colors hover:bg-white/[0.04]">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-full border border-white/10 bg-white/[0.06] text-sm font-semibold text-white/80">
            {profile.avatar ? <img src={profile.avatar} alt="" className="h-full w-full object-cover" draggable={false} /> : nome[0]?.toUpperCase()}
          </div>
          <div className="min-w-0">
            <div className="arcadia-blink text-[10px] font-light uppercase tracking-[0.28em] text-white/55" aria-label="Arcadia">
              {"Arcadia".split("").map((ch, i) => <span key={i} style={{ animationDelay: `${i * 0.14}s` }}>{ch}</span>)}
            </div>
            <div className="truncate text-[13px] font-medium text-white/80 group-hover:text-white">{nome}</div>
          </div>
        </button>
        {profileMenu && (
          <div className="mt-2 overflow-hidden rounded-xl border border-white/10 bg-[#16161c]/95 shadow-2xl shadow-black/50 backdrop-blur">
            <ProfileMenuItem label={t("profile.meu_perfil")} onClick={() => { setProfileMenu(false); onProfile() }} />
            <ProfileMenuItem label={t("profile.atualizar_biblioteca")} onClick={() => { setProfileMenu(false); onRefresh() }} />
            <ProfileMenuItem label={t("settings.title")} onClick={() => { setProfileMenu(false); onConfigSub("gerais"); onView("config") }} />
            <div className="h-px bg-white/10" />
            <ProfileMenuItem danger label={t("profile.sair")} onClick={() => { setProfileMenu(false); onQuit() }} />
          </div>
        )}
      </div>

      {/* Itens */}
      <nav className="flex flex-col gap-1 px-3">
        {ITENS.map((it) => {
          const active = view === it.id
          return (
            <div key={it.id}>
              <button
                onClick={() => onView(it.id)}
                className={`relative flex w-full items-center gap-3 rounded-lg px-4 py-2.5 text-left text-[15px] transition-colors ${
                  active ? "bg-white/[0.07] text-white" : "text-white/50 hover:bg-white/[0.04] hover:text-white/85"
                }`}
              >
                {active && (
                  <span className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-full" style={{ background: "var(--accent)" }} />
                )}
                {it.icon}
                {t(it.labelKey)}
                {it.id === "downloads" && downloadsActive > 0 && (
                  <span
                    className="ml-auto flex h-5 min-w-5 items-center justify-center rounded-full px-1.5 text-[11px] font-bold text-black"
                    style={{ background: "var(--accent)" }}
                  >
                    {downloadsActive}
                  </span>
                )}
              </button>

              {/* Sub-itens de Configurações: aparecem logo abaixo do item pai */}
              {it.id === "config" && view === "config" && (
                <div className="ml-9 mt-1 flex flex-col gap-0.5">
                  {CONFIG_SUBS.map((s) => {
                    const ativo = configSub === s.id
                    return (
                      <button
                        key={s.id}
                        onClick={() => {
                          onConfigSub(s.id)
                          onView("config")
                        }}
                        className={`flex items-center gap-2.5 rounded-lg px-3 py-1.5 text-left text-[14px] transition-colors ${
                          ativo ? "bg-white/[0.06] text-white" : "text-white/45 hover:bg-white/[0.03] hover:text-white/85"
                        }`}
                      >
                        <span
                          className="h-1 w-1 rounded-full"
                          style={{ background: ativo ? "var(--accent)" : "rgba(255,255,255,0.25)" }}
                        />
                        {t(s.labelKey)}
                      </button>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </nav>

      {/* Lista de jogos estilo Hydra (permanente em todas as abas) */}
      {(
        <div className="mt-3 flex min-h-0 flex-1 flex-col border-t border-white/[0.06] pt-2">
          <div className="flex items-center gap-1 px-4 pb-1">
            <button
              onClick={onToggleLibrarySidebar}
              className="flex flex-1 items-center gap-1.5 text-left text-[11px] font-semibold uppercase tracking-wider text-white/50 transition-colors hover:text-white/80"
              title={librarySidebar ? t("sidebar.ocultar_jogos") : t("sidebar.mostrar_jogos")}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"
                className={`transition-transform ${librarySidebar ? "" : "-rotate-90"}`}>
                <polyline points="6 9 12 15 18 9" />
              </svg>
              {t("sidebar.jogos")}
              <span className="ml-0.5 rounded bg-white/10 px-1.5 py-0.5 text-[10px] font-bold text-white/60">{jogos.length}</span>
            </button>
            <button onClick={onAddGame} title={t("sidebar.adicionar_jogo")} className="rounded p-1 text-white/45 transition-colors hover:bg-white/[0.06] hover:text-white">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
            </button>
            <button onClick={onRefresh} title={t("sidebar.atualizar")} className="rounded p-1 text-white/45 transition-colors hover:bg-white/[0.06] hover:text-white">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M23 4v6h-6" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" /></svg>
            </button>
          </div>

          {librarySidebar && (
            <>
              <div className="px-3 pb-2 pt-1">
                <input
                  value={buscaJogos}
                  onChange={(e) => setBuscaJogos(e.target.value)}
                  placeholder={t("library.buscar")}
                  spellCheck={false}
                  className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-1.5 text-[12px] text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
                />
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
                {jogos.map((g) => (
                  <button
                    key={g.id}
                    onClick={() => onOpenGame(g)}
                    className={`relative flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors ${
                      activeGameId === g.id ? "bg-white/[0.07]" : "hover:bg-white/[0.06]"
                    }`}
                    title={g.title}
                  >
                    {activeGameId === g.id && (
                      <span className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full" style={{ background: "var(--accent)" }} />
                    )}
                    <GameIcon game={g} />
                    <span className={`truncate text-[13px] ${activeGameId === g.id ? "text-white" : "text-white/75"}`}>{g.title}</span>
                  </button>
                ))}
                {jogos.length === 0 && (
                  <p className="px-2 py-4 text-center text-[12px] text-white/30">{t("library.vazio")}</p>
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* Rodapé */}
      <div className="mt-auto px-6 py-4">
        <button
          onClick={onBigPicture}
          className="mb-3 flex w-full items-center gap-2.5 rounded-xl border border-white/10 px-3 py-2.5 text-left text-[13px] font-semibold text-white/70 transition-colors hover:border-[color:var(--accent)] hover:text-white"
          title={t("sidebar.modo_tela_cheia")}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
            <rect x="2" y="4" width="20" height="13" rx="2" /><path d="M8 21h8m-4-4v4" />
          </svg>
          {t("sidebar.bigpicture")}
        </button>
        <div className="flex items-center justify-between text-xs text-white/35">
          <span>{t("app.name")} · {t("sidebar.modo_desktop")}</span>
          <button onClick={onQuit} className="flex items-center gap-1.5 text-white/50 transition-colors hover:text-white" title={t("sidebar.sair")}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
              <line x1="12" x2="12" y1="2" y2="12" />
            </svg>
            {t("sidebar.sair")}
          </button>
        </div>
      </div>
    </aside>
  )
}

function GameIcon({ game }: { game: Game }) {
  // Prefere ícone quadrado (Steam); cai na capa como miniatura; depois letra.
  const fontes = [game.icon, game.cover].filter(Boolean) as string[]
  const [fase, setFase] = useState(0)
  const src = fontes[fase]
  if (src) {
    return <img src={src} alt="" onError={() => setFase((f) => f + 1)} draggable={false} className="h-7 w-7 shrink-0 rounded-md object-cover" />
  }
  return (
    <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[12px] font-bold text-white/70" style={{ background: "color-mix(in srgb, var(--accent) 22%, transparent)" }}>
      {game.title[0]?.toUpperCase() || "?"}
    </span>
  )
}

function ProfileMenuItem({ label, onClick, danger }: { label: string; onClick: () => void; danger?: boolean }) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center px-4 py-2.5 text-left text-[13px] transition-colors hover:bg-white/[0.07] ${danger ? "text-[#ff6b81]" : "text-white/80"}`}
    >
      {label}
    </button>
  )
}

const s = { width: 17, height: 17, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round", strokeLinejoin: "round" } as const

function IconGrid() {
  return (
    <svg {...s}>
      <rect width="7" height="7" x="3" y="3" rx="1" /><rect width="7" height="7" x="14" y="3" rx="1" />
      <rect width="7" height="7" x="14" y="14" rx="1" /><rect width="7" height="7" x="3" y="14" rx="1" />
    </svg>
  )
}
function IconStore() {
  return (
    <svg {...s}>
      <path d="m2 7 4.41-4.41A2 2 0 0 1 7.83 2h8.34a2 2 0 0 1 1.42.59L22 7" />
      <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
      <path d="M15 22v-4a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2v4" />
      <path d="M2 7h20" />
    </svg>
  )
}
function IconDownload() {
  return (
    <svg {...s}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" x2="12" y1="15" y2="3" />
    </svg>
  )
}
function IconWine() {
  return (
    <svg {...s}>
      <path d="M8 22h8" /><path d="M12 15v7" /><path d="M5 3h14l-1.5 7.5a5.5 5.5 0 0 1-11 0L5 3z" />
    </svg>
  )
}
function IconGear() {
  return (
    <svg {...s}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  )
}

function IconA11y() {
  return (
    <svg {...s}>
      <circle cx="12" cy="4.5" r="2" />
      <path d="M4 8.5c2.7.6 5.3 1 8 1s5.3-.4 8-1" />
      <path d="M12 9.5v5l-3 7" />
      <path d="M12 14.5l3 7" />
    </svg>
  )
}

function IconHome() {
  return (
    <svg {...s}>
      <path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
      <polyline points="9 22 9 12 15 12 15 22" />
    </svg>
  )
}

function IconPlugin() {
  return (
    <svg {...s}>
      <path d="M12 2v4" />
      <path d="m16.2 7.8 2.9-2.9" />
      <path d="M18 12h4" />
      <path d="m16.2 16.2 2.9 2.9" />
      <path d="M12 18v4" />
      <path d="m4.9 19.1 2.9-2.9" />
      <path d="M2 12h4" />
      <path d="m4.9 4.9 2.9 2.9" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  )
}

