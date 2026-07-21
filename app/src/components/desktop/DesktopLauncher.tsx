"use client"

import { useCallback, useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { Sidebar, type DesktopView, type ConfigSub } from "./Sidebar"
import { LibraryView } from "./LibraryView"
import { DownloadsView } from "./DownloadsView"
import { AccessibilityView, aplicarA11y } from "./AccessibilityView"
import { SettingsView } from "./SettingsView"
import { WineSection } from "../ps5-launcher/WineManager"
import { PlayingBadge } from "./PlayingBadge"
import { StoreView } from "./StoreView"
import { useI18n } from "../../i18n/I18nContext"

export function DesktopLauncher() {
  const { t } = useI18n()
  const [view, setView] = useState<DesktopView>("biblioteca")
  const [configSub, setConfigSub] = useState<ConfigSub>("gerais")
  const [games, setGames] = useState<Game[]>([])
  const [dmAtivos, setDmAtivos] = useState(0)
  const [baixado, setBaixado] = useState<{ appid: string; title: string } | null>(null)
  const [confirmBigPicture, setConfirmBigPicture] = useState(false)
  const [cfg, setCfg] = useState<{ tiles_color?: boolean; always_titles?: boolean }>({})

  const carregar = useCallback(() => {
    window.launcherAPI?.getLibrary().then((g) => {
      if (Array.isArray(g)) setGames(g)
    })
    window.launcherAPI?.getConfig().then((c) => setCfg(c || {}))
  }, [])

  useEffect(() => {
    carregar()
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.ui_scale) window.launcherAPI?.setZoom(c.ui_scale)
      aplicarA11y(c || {})
    })
    const conta = (items: { status?: string }[]) =>
      items.filter((i) => ["downloading", "queued", "paused"].includes(i.status || "")).length
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setDmAtivos(conta(q))
    })
    const offLib = window.launcherAPI?.onLibraryChanged(() => carregar())
    const offDm = window.launcherAPI?.onDmProgress((q) => setDmAtivos(conta(q)))
    const offDl = window.launcherAPI?.onStoreDownloaded((d) => setBaixado(d))
    return () => {
      offLib?.()
      offDm?.()
      offDl?.()
    }
  }, [carregar])

  return (
    <div className="flex h-screen w-full select-none overflow-hidden bg-black text-white antialiased">
      <Sidebar
        view={view}
        onView={setView}
        downloadsActive={dmAtivos}
        onQuit={() => window.launcherAPI?.quit()}
        onBigPicture={() => setConfirmBigPicture(true)}
        configSub={configSub}
        onConfigSub={setConfigSub}
      />

      <main key={view} className="view-in min-w-0 flex-1 overflow-hidden border-l border-white/[0.06]">
        {view === "biblioteca" && <LibraryView games={games} tilesColor={cfg.tiles_color} alwaysTitles={cfg.always_titles} onRefresh={carregar} />}
        {view === "lojas" && <StoreView games={games} />}
        {view === "downloads" && <DownloadsView />}
        {view === "wine" && (
          <div className="h-full overflow-y-auto px-8 py-6">
            <WineSection />
          </div>
        )}
        {view === "acessibilidade" && <AccessibilityView />}
        {view === "config" && <SettingsView sub={configSub} onSaved={carregar} />}
      </main>

      <PlayingBadge />

      {baixado && (
        <div className="fixed inset-0 z-[75] flex items-center justify-center bg-black/60 backdrop-blur-sm">
          <div className="w-[420px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl">
            <h3 className="mb-2 text-lg font-semibold text-white">{t("desktop.store.download_concluido")}</h3>
            <p className="mb-5 text-[13px] leading-relaxed text-white/60">
              <span className="font-medium text-white/90">"{baixado.title}"</span>
              {t("desktop.store.instalado")}
              {t("desktop.store.reinicie_steam")}
            </p>
            <div className="flex justify-end gap-2.5">
              <button
                onClick={() => setBaixado(null)}
                className="rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
              >
                {t("desktop.mais_tarde")}
              </button>
              <button
                onClick={async () => {
                  setBaixado(null)
                  await window.launcherAPI?.slssteamLaunch()
                }}
                className="flex items-center gap-2 rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M23 4v6h-6" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                </svg>
                {t("desktop.restart_steam")}
              </button>
            </div>
          </div>
        </div>
      )}
      {confirmBigPicture && (
        <div className="fixed inset-0 z-[75] flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setConfirmBigPicture(false)}>
          <div className="w-[400px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <h3 className="mb-2 text-lg font-semibold text-white">{t("desktop.entrar_big_picture")}</h3>
            <p className="mb-5 text-[13px] leading-relaxed text-white/60">
              {t("desktop.big_picture_desc")}
            </p>
            <div className="flex justify-end gap-2.5">
              <button
                onClick={() => setConfirmBigPicture(false)}
                className="rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
              >
                {t("common.cancelar")}
              </button>
              <button
                onClick={() => window.launcherAPI?.enterConsole()}
                className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                {t("desktop.entrar")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
