"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import type { Profile, TorrentItem } from "../../global"
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
  const [view, setView] = useState<DesktopView>("inicio")
  const [configSub, setConfigSub] = useState<ConfigSub>("gerais")
  const [games, setGames] = useState<Game[]>([])
  const [dmAtivos, setDmAtivos] = useState(0)
  const [torrAtivos, setTorrAtivos] = useState(0)
  const [baixado, setBaixado] = useState<{ appid: string; title: string } | null>(null)
  const [confirmBigPicture, setConfirmBigPicture] = useState(false)
  const [showEditProfile, setShowEditProfile] = useState(false)
  const [profile, setProfile] = useState<Profile>({})
  const [cfg, setCfg] = useState<{
    tiles_color?: boolean
    always_titles?: boolean
    library_sidebar?: boolean
  }>({})
  const [librarySidebar, setLibrarySidebar] = useState(true)
  const [jogoPagina, setJogoPagina] = useState<Game | null>(null)
  const [jogoConfig, setJogoConfig] = useState<Game | null>(null)
  const [contaAberta, setContaAberta] = useState(false)
  // Pós sign-out: a tela de login fica OBRIGATÓRIA (X não fecha) até logar —
  // evita o estado "meia-conta" (tela antiga sem dados de ninguém).
  const [aposLogout, setAposLogout] = useState(false)
  const [contaDispensada, setContaDispensada] = useState(false)
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
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.disable_playtime_tracking !== true)
        window.launcherAPI?.setOverride(g.id, { last_played: Date.now() })
    })
  }, [])

  // Jogo Steam com executável configurado tem duas formas de iniciar: abre o
  // menu de escolha. Nos demais casos joga direto.
  const pedirJogar = useCallback(
    (g: Game) => {
      if (g.launcher === "steam" && g.temExe) setEscolhendoLaunch(g)
      else jogar(g)
    },
    [jogar],
  )

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
      if (c?.ui_scale) window.launcherAPI?.setZoom(c.ui_scale, "desktop")
      aplicarA11y(c || {})
    })
    const conta = (items: { status?: string }[]) =>
      items.filter((i) => ["downloading", "queued", "paused"].includes(i.status || "")).length
    const contaTorr = (items: TorrentItem[]) => items.filter((i) => !i.completo && !i.erro).length
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setDmAtivos(conta(q))
    })
    window.launcherAPI?.torrentList().then((r) => {
      if (Array.isArray(r?.downloads)) setTorrAtivos(contaTorr(r.downloads))
    })
    const offLib = window.launcherAPI?.onLibraryChanged(() => carregar())
    const offDm = window.launcherAPI?.onDmProgress((q) => setDmAtivos(conta(q)))
    const offTorr = window.launcherAPI?.onTorrentProgress((items) =>
      setTorrAtivos(contaTorr(items)),
    )
    const offDl = window.launcherAPI?.onStoreDownloaded((d) => setBaixado(d))
    return () => {
      offLib?.()
      offDm?.()
      offTorr?.()
      offDl?.()
    }
  }, [carregar])

  return (
    <>
      <AutoOpenLogin
        onOpen={() => setContaAberta(true)}
        dispensado={contaDispensada}
        onLogado={() => setAposLogout(false)}
      />
      <ProfileBridge perfilLocal={profile} setPerfilLocal={setProfile} />
      <div className="app-drag flex h-screen w-full select-none overflow-hidden bg-black text-white antialiased">
      <WindowControls />
      <Sidebar
        view={view}
        onView={(v) => {
          setJogoPagina(null)
          setView(v)
        }}
        downloadsActive={dmAtivos + torrAtivos}
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
        onRefresh={atualizarBiblioteca}
        games={games}
        librarySidebar={librarySidebar}
        onToggleLibrarySidebar={toggleLibrarySidebar}
        onOpenGame={(g) => {
          setView("biblioteca")
          setJogoPagina(g)
        }}
        onAddGame={() => setAdicionando(true)}
        activeGameId={jogoPagina?.id}
      />

      <main
        key={view}
        className="view-in min-w-0 flex-1 overflow-hidden border-l border-white/[0.06]"
      >
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
                    .then(() => carregar())
                  setJogoPagina(null)
                }}
                onJogar={
                  jogoPagina.installed !== false
                    ? () => {
                        // Fica na página do jogo ao lançar (não volta pra Library).
                        pedirJogar(jogoPagina)
                      }
                    : undefined
                }
                naBiblioteca
                ocupado={false}
              />
            )}
            {jogoPagina && !String(jogoPagina.id).startsWith("steam:") && (
              <GamePage
                embedded
                game={jogoPagina}
                onClose={() => setJogoPagina(null)}
                onJogar={() => {
                  // Fica na página do jogo ao lançar (não volta pra Library).
                  pedirJogar(jogoPagina)
                }}
                onInstalar={() => instalar(jogoPagina)}
                onImportar={() => window.launcherAPI?.gameImport(jogoPagina)}
                onConfig={() => setJogoConfig(jogoPagina)}
              />
            )}
            {!jogoPagina && view === "inicio" && <HomeView games={games} />}
            {!jogoPagina && view === "biblioteca" && (
              <LibraryView
                games={games}
                tilesColor={cfg.tiles_color}
                alwaysTitles={cfg.always_titles}
                onRefresh={carregar}
              />
            )}
            {!jogoPagina && view === "lojas" && <StoreView games={games} />}
            {!jogoPagina && view === "plugins" && <PluginsView />}
            {!jogoPagina && view === "downloads" && <DownloadsView />}
            {!jogoPagina && view === "fontes" && <SourcesView />}
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
                  setJogoPagina(g)
                }}
              />
            )}
            {!jogoPagina && view === "config" && (
              <SettingsView sub={configSub} onSaved={carregar} />
            )}
      </main>

      {jogoConfig && (
        <GameSettingsDialog
          game={jogoConfig}
          onClose={() => {
            setJogoConfig(null)
            carregar()
          }}
        />
      )}
      {escolhendoLaunch && (
        <LaunchModeDialog
          game={escolhendoLaunch}
          onEscolher={(m) => {
            const g = escolhendoLaunch
            setEscolhendoLaunch(null)
            jogar(g, m)
          }}
          onClose={() => setEscolhendoLaunch(null)}
        />
      )}
      {adicionando && (
        <AddGameDialog
          onClose={() => setAdicionando(false)}
          onAdded={() => atualizarBiblioteca()}
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
