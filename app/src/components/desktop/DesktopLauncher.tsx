"use client"

import { useCallback, useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import type { Profile, TorrentItem } from "../../global"
import { Sidebar, type DesktopView, type ConfigSub } from "./Sidebar"
import { WindowControls } from "./WindowControls"
import { LibraryView } from "./LibraryView"
import { DownloadsView } from "./DownloadsView"
import { SourcesView } from "./SourcesView"
import { aplicarA11y } from "./AccessibilityView"
import { SettingsView } from "./SettingsView"
import { WineSection } from "../ps5-launcher/WineManager"
import { PlayingBadge } from "./PlayingBadge"
import { StoreView } from "./StoreView"
import { HomeView } from "./HomeView"
import { PluginsView } from "./PluginsView"
import { StoreGamePage } from "./StoreGamePage"
import { GamePage } from "./GamePage"
import { GameSettingsDialog } from "./GameSettingsDialog"
import { LaunchModeDialog } from "./LaunchModeDialog"
import { AddGameDialog } from "./AddGameDialog"
import { avisarJogando } from "./PlayingBadge"
import { useI18n } from "../../i18n/I18nContext"
import { UpdateDialog, useAtualizacao } from "../UpdateDialog"
import { ProfilePage } from "../ps5-launcher/ProfilePage"
import { AchievementToasts } from "../ps5-launcher/AchievementToasts"
import { EditProfile } from "../ps5-launcher/EditProfile"

export function DesktopLauncher() {
  const { t } = useI18n()
  const [view, setView] = useState<DesktopView>("inicio")
  const [configSub, setConfigSub] = useState<ConfigSub>("gerais")
  const [games, setGames] = useState<Game[]>([])
  const [dmAtivos, setDmAtivos] = useState(0)
  const [torrAtivos, setTorrAtivos] = useState(0)
  const [baixado, setBaixado] = useState<{ appid: string; title: string } | null>(null)
  const [confirmBigPicture, setConfirmBigPicture] = useState(false)
  const [showProfile, setShowProfile] = useState(false)
  const [showEditProfile, setShowEditProfile] = useState(false)
  const [profile, setProfile] = useState<Profile>({})
  const [cfg, setCfg] = useState<{ tiles_color?: boolean; always_titles?: boolean; library_sidebar?: boolean }>({})
  const [librarySidebar, setLibrarySidebar] = useState(true)
  const [jogoPagina, setJogoPagina] = useState<Game | null>(null)
  const [jogoConfig, setJogoConfig] = useState<Game | null>(null)
  const [escolhendoLaunch, setEscolhendoLaunch] = useState<Game | null>(null)
  const [adicionando, setAdicionando] = useState(false)
  const atualizacao = useAtualizacao()

  const toggleLibrarySidebar = useCallback(() => {
    setLibrarySidebar((v) => {
      const novo = !v
      window.launcherAPI?.setConfig({ library_sidebar: novo })
      return novo
    })
  }, [])

  const jogar = useCallback((g: Game, mode?: "steam" | "exe") => {
    window.launcherAPI?.launch(g.launch_cmd, g.id, mode)
    avisarJogando(g)
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.disable_playtime_tracking !== true) window.launcherAPI?.setOverride(g.id, { last_played: Date.now() })
    })
  }, [])

  // Jogo Steam com executável configurado tem duas formas de iniciar: abre o
  // menu de escolha. Nos demais casos joga direto.
  const pedirJogar = useCallback((g: Game) => {
    if (g.launcher === "steam" && g.temExe) setEscolhendoLaunch(g)
    else jogar(g)
  }, [jogar])

  const instalar = useCallback((g: Game) => {
    if (g.launcher === "steam") {
      const appid = String(g.id).replace(/^steam:/, "")
      window.launcherAPI?.launch(["steam", `steam://install/${appid}`])
      return
    }
    // Epic/custom: a página do jogo cobre instalação; abrir a página basta.
    setJogoPagina(g)
  }, [])

  const carregar = useCallback(() => {
    window.launcherAPI?.getLibrary().then((g) => {
      if (Array.isArray(g)) setGames(g)
    })
    window.launcherAPI?.getConfig().then((c) => {
      setCfg(c || {})
      setProfile(c?.profile || {})
      if (typeof c?.library_sidebar === "boolean") setLibrarySidebar(c.library_sidebar)
    })
  }, [])

  const atualizarBiblioteca = useCallback(() => {
    window.launcherAPI?.refresh().then((g) => {
      if (Array.isArray(g)) setGames(g)
      else carregar()
    })
  }, [carregar])

  // Página aberta segura snapshot; após recarregar a lista, sincroniza pelo id
  // para o botão Jogar refletir installed atualizado (ex.: exePath salvo).
  useEffect(() => {
    setJogoPagina((p) => (p ? games.find((g) => g.id === p.id) || p : p))
  }, [games])

  useEffect(() => {
    carregar()
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.ui_scale) window.launcherAPI?.setZoom(c.ui_scale)
      aplicarA11y(c || {})
    })
    const conta = (items: { status?: string }[]) =>
      items.filter((i) => ["downloading", "queued", "paused"].includes(i.status || "")).length
    const contaTorr = (items: TorrentItem[]) =>
      items.filter((i) => !i.completo && !i.erro).length
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setDmAtivos(conta(q))
    })
    window.launcherAPI?.torrentList().then((r) => {
      if (Array.isArray(r?.downloads)) setTorrAtivos(contaTorr(r.downloads))
    })
    const offLib = window.launcherAPI?.onLibraryChanged(() => carregar())
    const offDm = window.launcherAPI?.onDmProgress((q) => setDmAtivos(conta(q)))
    const offTorr = window.launcherAPI?.onTorrentProgress((items) => setTorrAtivos(contaTorr(items)))
    const offDl = window.launcherAPI?.onStoreDownloaded((d) => setBaixado(d))
    return () => {
      offLib?.()
      offDm?.()
      offTorr?.()
      offDl?.()
    }
  }, [carregar])

  return (
    <div className="app-drag flex h-screen w-full select-none overflow-hidden bg-black text-white antialiased">
      <WindowControls />
      <Sidebar
        view={view}
        onView={(v) => { setShowProfile(false); setJogoPagina(null); setView(v) }}
        downloadsActive={dmAtivos + torrAtivos}
        onQuit={() => window.launcherAPI?.quit()}
        onBigPicture={() => setConfirmBigPicture(true)}
        configSub={configSub}
        onConfigSub={setConfigSub}
        profile={profile}
        onProfile={() => setShowProfile(true)}
        onRefresh={atualizarBiblioteca}
        games={games}
        librarySidebar={librarySidebar}
        onToggleLibrarySidebar={toggleLibrarySidebar}
        onOpenGame={(g) => { setShowProfile(false); setView("biblioteca"); setJogoPagina(g) }}
        onAddGame={() => setAdicionando(true)}
        activeGameId={jogoPagina?.id}
      />

      <main key={showProfile ? "profile" : view} className="view-in min-w-0 flex-1 overflow-hidden border-l border-white/[0.06]">
        {showProfile ? (
          <ProfilePage
            open
            embedded
            navActive={!showEditProfile}
            profile={profile}
            games={games}
            onClose={() => setShowProfile(false)}
            onEdit={() => setShowEditProfile(true)}
          />
        ) : (
          <>
            {jogoPagina && jogoPagina.launcher === "steam" && (
              <StoreGamePage
                embedded
                jogo={{
                  appid: String(jogoPagina.id).replace(/^steam:/, ""),
                  title: jogoPagina.title,
                  cover: jogoPagina.cover,
                  capa: jogoPagina.cover,
                  heroi: jogoPagina.hero,
                  manifest: true,
                }}
                game={jogoPagina}
                onClose={() => setJogoPagina(null)}
                onBaixar={() => instalar(jogoPagina)}
                onAdicionar={() => {}}
                onConfig={() => setJogoConfig(jogoPagina)}
                onRemover={() => {
                  window.launcherAPI?.storeRemoveFromLibrary(String(jogoPagina.id).replace(/^steam:/, "")).then(() => carregar())
                  setJogoPagina(null)
                }}
                onJogar={jogoPagina.installed !== false ? () => { const g = jogoPagina; setJogoPagina(null); pedirJogar(g) } : undefined}
                naBiblioteca
                ocupado={false}
              />
            )}
            {jogoPagina && jogoPagina.launcher !== "steam" && (
              <GamePage
                embedded
                game={jogoPagina}
                onClose={() => setJogoPagina(null)}
                onJogar={() => { const g = jogoPagina; setJogoPagina(null); pedirJogar(g) }}
                onInstalar={() => instalar(jogoPagina)}
                onImportar={() => window.launcherAPI?.gameImport(jogoPagina)}
                onConfig={() => setJogoConfig(jogoPagina)}
              />
            )}
            {!jogoPagina && view === "inicio" && <HomeView games={games} />}
            {!jogoPagina && view === "biblioteca" && <LibraryView games={games} tilesColor={cfg.tiles_color} alwaysTitles={cfg.always_titles} onRefresh={carregar} />}
            {!jogoPagina && view === "lojas" && <StoreView games={games} />}
            {!jogoPagina && view === "plugins" && <PluginsView />}
            {!jogoPagina && view === "downloads" && <DownloadsView />}
            {!jogoPagina && view === "fontes" && <SourcesView />}
            {!jogoPagina && view === "wine" && (
              <div className="h-full overflow-y-auto px-8 py-6">
                <WineSection />
              </div>
            )}
            {!jogoPagina && view === "config" && <SettingsView sub={configSub} onSaved={carregar} />}
          </>
        )}
      </main>

      <PlayingBadge />
      <AchievementToasts />
      {jogoConfig && <GameSettingsDialog game={jogoConfig} onClose={() => { setJogoConfig(null); carregar() }} />}
      {escolhendoLaunch && (
        <LaunchModeDialog
          game={escolhendoLaunch}
          onEscolher={(m) => { const g = escolhendoLaunch; setEscolhendoLaunch(null); jogar(g, m) }}
          onClose={() => setEscolhendoLaunch(null)}
        />
      )}
      {adicionando && <AddGameDialog onClose={() => setAdicionando(false)} onAdded={() => atualizarBiblioteca()} />}

      {atualizacao.info && (
        <UpdateDialog info={atualizacao.info} onDepois={atualizacao.dispensar} />
      )}

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
      <EditProfile
        open={showEditProfile}
        profile={profile}
        games={games}
        onClose={() => setShowEditProfile(false)}
        onChange={setProfile}
      />

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
