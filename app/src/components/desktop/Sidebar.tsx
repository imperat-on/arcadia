"use client"

// Sidebar do modo desktop (estilo Heroic, adaptada ao tema do Arcadia).
import { useI18n } from "../../i18n/I18nContext"

export type DesktopView = "biblioteca" | "lojas" | "downloads" | "wine" | "config" | "acessibilidade"
export type ConfigSub = "gerais" | "integracoes" | "metadados"

const ITENS: { id: DesktopView; label: string; icon: React.ReactNode; labelKey: string }[] = [
  { id: "biblioteca", label: "Biblioteca", labelKey: "sidebar.biblioteca", icon: <IconGrid /> },
  { id: "lojas", label: "Lojas", labelKey: "sidebar.lojas", icon: <IconStore /> },
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
}: {
  view: DesktopView
  onView: (v: DesktopView) => void
  downloadsActive: number
  onQuit: () => void
  onBigPicture: () => void
  configSub: ConfigSub
  onConfigSub: (s: ConfigSub) => void
}) {
  const { t } = useI18n()
  return (
    <aside className="flex h-full w-64 shrink-0 flex-col" style={{ background: "var(--sidebar-bg)" }}>
      {/* Logo */}
      <div className="flex items-center gap-3 px-6 pb-6 pt-7">
        <img src="./logo.svg" alt="Arcadia" className="h-10 w-10" draggable={false} />
        <span className="text-lg font-light uppercase tracking-[0.35em] text-white/90">Arcadia</span>
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
