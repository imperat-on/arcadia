"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { Sidebar, type DesktopView, type ConfigSub } from "./Sidebar"
import { WindowControls } from "./WindowControls"
import { LibraryView } from "./LibraryView"
import { DownloadsView } from "./DownloadsView"
import { SourcesView } from "./SourcesView"
import { aplicarA11y } from "./AccessibilityView"
import { SettingsView } from "./SettingsView"
import { StoreView } from "./StoreView"
import { HomeView } from "./HomeView"
import { PluginsView } from "./PluginsView"
import { StoreGamePage } from "./StoreGamePage"
import { GamePage } from "./GamePage"
import { GameSettingsDialog } from "./GameSettingsDialog"
import { LaunchModeDialog } from "./LaunchModeDialog"
import { AddGameDialog } from "./AddGameDialog"
import { useI18n } from "../../i18n/I18nContext"
import { UpdateDialog, useAtualizacao } from "../UpdateDialog"
import { ProfilePage } from "../ps5-launcher/ProfilePage"
import { ProfileBridge } from "./ProfileBridge"
import { EditProfile } from "../ps5-launcher/EditProfile"
import { AchievementToast } from "./AchievementToast"
import { useAccount } from "../account/AccountContext"
import { FriendsProvider } from "../account/FriendsContext"
import { AuthDialog } from "./AuthDialog"
import { FriendsView } from "./FriendsView"
import { SyncStatusIndicator } from "./SyncStatusIndicator"
import { useLibraryState } from "../useLibraryState"
import { useDownloadBadges } from "../useDownloadBadges"
import { RetroStoreView, retroGameFromLibrary } from "./RetroStoreView"
import { useMode } from "../ModeContext"
import { useGameActions } from "../useGameActions"
import { useJogoRodando } from "../useJogoRodando"

// Roda DENTRO do <AccountProvider> (por isso consegue usar useAccount):
// na primeira vez sem sessão salva, manda o launcher abrir o login/sign-up.
// Só 1x por execução — se o usuário fechar no ✕, não reabre sozinho.
// Splash cobre até a sessão E o perfil online estarem prontos — senão o nome
// pisca: mostra o username puro antes do display_name chegar do servidor.
function AutoOpenLogin({
  onOpen,
  dispensado,
  onLogado,
}: {
  onOpen: () => void
  dispensado: boolean
  /** Chamado quando uma sessão aparece (login) — reseta o modo "pós sign-out". */
  onLogado?: () => void
}) {
  const { status, perfil } = useAccount()
  const abriu = useRef(false)
  useEffect(() => {
    if (status === "deslogado" && !abriu.current) {
      abriu.current = true
      onOpen()
    }
  }, [status, onOpen])
  useEffect(() => {
    if (status === "logado") onLogado?.()
  }, [status, onLogado])
  if (status === "carregando") return <div className="fixed inset-0 z-[95] bg-[#0d0d10]" />
  // Sessão pronta, mas perfil online ainda não carregou (display_name/avatar) —
  // segura o splash até resolver (evita o flash do nome)
  if (status === "logado" && !perfil) return <div className="fixed inset-0 z-[95] bg-[#0d0d10]" />
  if (status === "deslogado" && !dispensado && !abriu.current) {
    return <div className="fixed inset-0 z-[95] bg-[#0d0d10]" />
  }
  return null
}

// Espelha o perfil ONLINE no perfil local em memória — identidade única.
// Logado: name=display_name||username, avatar=avatar_url, summary/country/city/showcase.
// Deslogado: restaura o perfil local original (guardado no 1º merge DA SESSÃO).
// IMPORTANTE: o original é capturado ANTES do merge e resetado ao deslogar,
// senão a SEGUNDA sessão captura o perfil já mergeado da primeira (vazamento).
export function DesktopLauncher() {
  const { t } = useI18n()
  const { perfil } = useAccount()
  const { setMode } = useMode()
  const [view, setView] = useState<DesktopView>("inicio")
  const [configSub, setConfigSub] = useState<ConfigSub>("gerais")
  const {
    games,
    setGames,
    profile,
    setProfile,
    config: cfg,
    libraryLoaded,
    reloadLibrary,
  } = useLibraryState()
  const downloadsActive = useDownloadBadges({ includeTorrents: true })
  const [baixado, setBaixado] = useState<{ appid: string; title: string } | null>(null)
  const [confirmBigPicture, setConfirmBigPicture] = useState(false)
  const [showEditProfile, setShowEditProfile] = useState(false)
  const [librarySidebar, setLibrarySidebar] = useState(true)
  const [jogoPagina, setJogoPagina] = useState<Game | null>(null)
  const [retroPaginaJogo, setRetroPaginaJogo] = useState<Game | null>(null)
  const [jogoConfig, setJogoConfig] = useState<Game | null>(null)
  const [contaAberta, setContaAberta] = useState(false)
  // Pós sign-out: a tela de login fica OBRIGATÓRIA (X não fecha) até logar —
  // evita o estado "meia-conta" (tela antiga sem dados de ninguém).
  const [aposLogout, setAposLogout] = useState(false)
  const [contaDispensada, setContaDispensada] = useState(false)
  const [escolhendoLaunch, setEscolhendoLaunch] = useState<Game | null>(null)
  const [adicionando, setAdicionando] = useState(false)
  const jogoAtivo = useJogoRodando()
  const gameRunning = jogoAtivo.rodando || jogoAtivo.pendente
  const [appFocused, setAppFocused] = useState(() => document.hasFocus())
  const appFocusedRef = useRef(appFocused)
  const gameRunningRef = useRef(gameRunning)
  appFocusedRef.current = appFocused
  gameRunningRef.current = gameRunning
  const gameActions = useGameActions({
    setGames,
    onChooseLaunch: setEscolhendoLaunch,
    onLaunchWarning: (_game, warnings) => console.warn("arcadia:", warnings.join("; ")),
    canLaunch: () => appFocusedRef.current && !gameRunningRef.current,
  })
  useEffect(() => {
    const aoFoco = (focused: boolean) => {
      appFocusedRef.current = focused
      setAppFocused(focused)
    }
    const off = window.launcherAPI?.onAppFocus?.(aoFoco)
    const atual = window.launcherAPI?.getAppFocus?.()
    if (atual) void atual.then(aoFoco).catch(() => {})
    return () => off?.()
  }, [])
  const launchDesktopGame = useCallback(
    (game: Game, mode?: "steam" | "exe") => {
      if (!appFocusedRef.current || gameRunning) {
        return Promise.resolve({ ok: false, error: "O launcher está ocupado com outro jogo." })
      }
      return gameActions.launch(game, mode)
    },
    [gameActions, gameRunning],
  )
  const launchDesktopCommand = useCallback(
    (command: string[], gameId?: string, mode?: "steam" | "exe") => {
      if (!appFocusedRef.current || gameRunning) {
        return Promise.resolve({ ok: false, error: "O launcher está ocupado com outro jogo." })
      }
      return gameActions.launchCommand(command, gameId, mode)
    },
    [gameActions, gameRunning],
  )
  const atualizacao = useAtualizacao()
  const retroPaginaSeed = useMemo(
    () => (retroPaginaJogo ? retroGameFromLibrary(retroPaginaJogo) : undefined),
    [retroPaginaJogo],
  )

  const toggleLibrarySidebar = useCallback(() => {
    setLibrarySidebar((v) => {
      const novo = !v
      window.launcherAPI?.setConfig({ library_sidebar: novo })
      return novo
    })
  }, [])

  const instalar = useCallback((g: Game) => {
    if (g.launcher === "steam") {
      const appid = String(g.id).replace(/^steam:/, "")
      void launchDesktopCommand(["steam", `steam://install/${appid}`])
      return
    }
    // Epic/custom: a página do jogo cobre instalação; abrir a página basta.
    setJogoPagina(g)
  }, [launchDesktopCommand])

  // Página aberta segura snapshot; após recarregar a lista, sincroniza pelo id
  // para o botão Jogar refletir installed atualizado (ex.: exePath salvo).
  useEffect(() => {
    setJogoPagina((p) => (p ? games.find((g) => g.id === p.id) || p : p))
  }, [games])

  useEffect(() => {
    if (!libraryLoaded) return
    const requested = Number(cfg.ui_scale)
    const promoteDefault = cfg.desktop_font_scale_v3 !== true && (!Number.isFinite(requested) || requested === 1)
    const safeScale = Math.min(1.1, Math.max(.7, promoteDefault ? 1.1 : (Number.isFinite(requested) ? requested : 1.1)))
    if (cfg.ui_scale !== safeScale || cfg.desktop_font_scale_v3 !== true) {
      window.launcherAPI?.setConfig({ ui_scale: safeScale, desktop_font_scale_v3: true })
    }
    window.launcherAPI?.setZoom(safeScale, "desktop")
    aplicarA11y(cfg)
  }, [cfg, libraryLoaded])

  useEffect(() => {
    const offDl = window.launcherAPI?.onStoreDownloaded((d) => setBaixado(d))
    return () => offDl?.()
  }, [])

  useEffect(() => {
    if (typeof cfg.library_sidebar === "boolean") setLibrarySidebar(cfg.library_sidebar)
  }, [cfg.library_sidebar])



  return (
    <>
      <AutoOpenLogin
        onOpen={() => setContaAberta(true)}
        dispensado={contaDispensada}
        onLogado={() => setAposLogout(false)}
      />
      <ProfileBridge perfilLocal={profile} setPerfilLocal={setProfile} />
      <div
        className="arcadia-desktop-retro app-drag flex h-screen w-full select-none overflow-hidden bg-black text-white antialiased"
        onClickCapture={(event) => {
          if (!appFocusedRef.current && gameRunningRef.current) {
            event.preventDefault()
            event.stopPropagation()
          }
        }}
        onKeyDownCapture={(event) => {
          if (!appFocusedRef.current && gameRunningRef.current) {
            event.preventDefault()
            event.stopPropagation()
          }
        }}
      >
      <WindowControls />
      <Sidebar
        view={view}
        onView={(v) => {
          setJogoPagina(null)
          setRetroPaginaJogo(null)
          setView(v)
        }}
        downloadsActive={downloadsActive}
        onQuit={() => window.launcherAPI?.quit()}
        onBigPicture={() => setConfirmBigPicture(true)}
        configSub={configSub}
        onConfigSub={setConfigSub}
        profile={profile}
        onProfile={() => setView("perfil")}
        onLogout={() => {
          setContaAberta(true)
          setAposLogout(true)
        }}
        onRefresh={() => { void gameActions.refresh() }}
        games={games}
        librarySidebar={librarySidebar}
        onToggleLibrarySidebar={toggleLibrarySidebar}
        onOpenGame={(g) => {
          setView("biblioteca")
          // Jogos Retro abrem a loja Retro (mesma tela que na biblioteca)
          if (g.launcher === "retro" || g.retro === true || String(g.id).startsWith("retro:")) {
            setRetroPaginaJogo(g)
            setJogoPagina(null)
          } else {
            setJogoPagina(g)
            setRetroPaginaJogo(null)
          }
        }}
        onAddGame={() => setAdicionando(true)}
        activeGameId={jogoPagina?.id}
      />

      <main
        key={view}
        className="desktop-retro-main view-in flex min-w-0 flex-1 flex-col overflow-hidden border-l border-white/[0.06]"
      >
        <DesktopHeader />
        <div className="min-h-0 flex-1 overflow-hidden">
        {jogoPagina && String(jogoPagina.id).startsWith("steam:") && (
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
                  window.launcherAPI
                    ?.storeRemoveFromLibrary(String(jogoPagina.id).replace(/^steam:/, ""))
                    .then(() => gameActions.refresh())
                  setJogoPagina(null)
                }}
                onJogar={
                  jogoPagina.installed !== false
                    ? () => {
                        // Fica na página do jogo ao lançar (não volta pra Library).
                        void launchDesktopGame(jogoPagina)
                      }
                    : undefined
                }
                naBiblioteca
                ocupado={false}
              />
            )}
            {jogoPagina && !String(jogoPagina.id).startsWith("steam:") && !(jogoPagina.launcher === "retro" || jogoPagina.retro === true || String(jogoPagina.id).startsWith("retro:")) && (
              <GamePage
                embedded
                game={jogoPagina}
                onClose={() => setJogoPagina(null)}
                onJogar={() => {
                  // Fica na página do jogo ao lançar (não volta pra Library).
                  void launchDesktopGame(jogoPagina)
                }}
                onInstalar={() => instalar(jogoPagina)}
                onImportar={() => window.launcherAPI?.gameImport(jogoPagina)}
                onConfig={() => setJogoConfig(jogoPagina)}
              />
            )}
            {retroPaginaJogo && view === "biblioteca" && !jogoPagina && (
              <div className="h-full overflow-hidden">
                <RetroStoreView
                  initialGameId={retroPaginaJogo.id}
                  initialGame={retroPaginaSeed}
                  onExit={() => setRetroPaginaJogo(null)}
                  onOpenDownloads={() => setView("downloads")}
                  onLaunchGame={(game) => { void launchDesktopGame(game) }}
                />
              </div>
            )}
            {!jogoPagina && !retroPaginaJogo && view === "inicio" && (
              <HomeView games={games} />
            )}
            {!jogoPagina && !retroPaginaJogo && view === "biblioteca" && (
              <LibraryView
                games={games}
                tilesColor={cfg.tiles_color}
                alwaysTitles={cfg.always_titles}
                actions={gameActions}
                onRetroOpen={(game) => setRetroPaginaJogo(game)}
              />
            )}
            {!jogoPagina && view === "lojas" && (
              <StoreView
                games={games}
                ativo={appFocused && !gameRunning}
                appFocused={appFocused}
                gameRunning={gameRunning}
                runningGameId={jogoAtivo.jogo?.id}
                onOpenDownloads={() => setView("downloads")}
                onLaunchGame={(game) => { void launchDesktopGame(game) }}
              />
            )}
            {!jogoPagina && view === "plugins" && <PluginsView />}
            {!jogoPagina && view === "downloads" && <DownloadsView />}
            {!jogoPagina && view === "fontes" && <SourcesView onOpenDownloads={() => setView("downloads")} />}
            {!jogoPagina && view === "amigos" && <FriendsView games={games} />}
            {!jogoPagina && view === "perfil" && (
              <ProfilePage
                open
                embedded
                navActive={!showEditProfile}
                profile={perfil ? { ...profile, name: perfil.display_name || perfil.username || profile.name, avatar: perfil.avatar_url ?? "", background: perfil.background_url ?? "", banner: perfil.banner_url ?? "" } : profile}
                games={games}
                onClose={() => setView("inicio")}
                onEdit={() => setShowEditProfile(true)}
                onJogoClick={(g) => {
                  // Mesma tela de quando clica no jogo na Biblioteca.
                  setView("biblioteca")
                  if (g.launcher === "retro" || g.retro === true || String(g.id).startsWith("retro:")) {
                    setRetroPaginaJogo(g)
                    setJogoPagina(null)
                  } else {
                    setJogoPagina(g)
                    setRetroPaginaJogo(null)
                  }
                }}
              />
            )}
            {!jogoPagina && view === "config" && (
              <SettingsView sub={configSub} onSaved={reloadLibrary} />
            )}
        </div>
      </main>

      {jogoConfig && (
        <GameSettingsDialog
          game={jogoConfig}
          onClose={() => {
            setJogoConfig(null)
            reloadLibrary()
          }}
        />
      )}
      {escolhendoLaunch && (
        <LaunchModeDialog
          game={escolhendoLaunch}
          onEscolher={(m) => {
            const g = escolhendoLaunch
            setEscolhendoLaunch(null)
            void launchDesktopGame(g, m)
          }}
          onClose={() => setEscolhendoLaunch(null)}
        />
      )}
      {adicionando && (
        <AddGameDialog
          onClose={() => setAdicionando(false)}
          onAdded={() => { void gameActions.refresh() }}
        />
      )}

      {atualizacao.info && (
        <UpdateDialog info={atualizacao.info} onDepois={atualizacao.dispensar} />
      )}

      {baixado && (
        <div className="fixed inset-0 z-[75] flex items-center justify-center bg-black/60 backdrop-blur-sm">
          <div className="w-[420px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl">
            <h3 className="mb-2 text-lg font-semibold text-white">
              {t("desktop.store.download_concluido")}
            </h3>
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
                <svg
                  width="13"
                  height="13"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.4"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M23 4v6h-6" />
                  <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
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
        <div
          className="fixed inset-0 z-[75] flex items-center justify-center bg-black/60 backdrop-blur-sm"
          onClick={() => setConfirmBigPicture(false)}
        >
          <div
            className="w-[400px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-6 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="mb-2 text-lg font-semibold text-white">
              {t("desktop.entrar_big_picture")}
            </h3>
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
                onClick={() => void setMode("console")}
                className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                {t("desktop.entrar")}
              </button>
            </div>
          </div>
        </div>
      )}
      <AchievementToast />
      <AuthDialog
        open={contaAberta}
        semFechar={aposLogout}
        onClose={() => {
          // Pós sign-out: X não dispensa (login obrigatório até escolher conta)
          if (aposLogout) return
          setContaAberta(false)
          setContaDispensada(true)
        }}
      />
      <SyncStatusIndicator />
      </div>
    </>
  )
}

function DesktopHeader() {
  return <header className="desktop-retro-topbar relative z-30 h-8 shrink-0 border-b" style={{ WebkitAppRegion: "drag" } as React.CSSProperties} />
}
