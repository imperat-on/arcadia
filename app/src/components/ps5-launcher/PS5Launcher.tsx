"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { Game } from "./types"
import { HeroSection } from "./HeroSection"
import { HeroBackground } from "./HeroBackground"
import { GameRail } from "./GameRail"
import { NewsView } from "./NewsView"
import { BootScreen } from "./BootScreen"
import ProfileSelect from "./ProfileSelect"
import { DownloadManager } from "./DownloadManager"
import { GameOverview } from "./GameOverview"
import { useGamepadNav } from "./useGamepadNav"
import { useAccountOptional, OWNER_USERNAME } from "../account/AccountContext"
import { GameContextMenu } from "./GameContextMenu"
import { TrailerPicker } from "./TrailerPicker"
import { EditMetadata } from "./EditMetadata"
import { TopBar, TABS, type LibraryFilter } from "./TopBar"
import { StoreView } from "../desktop/StoreView"
import { ConsoleDestinoDialog, type DestinoOpcao } from "./ConsoleDestinoDialog"
import { useStoreActions } from "../useStoreActions"
import { useJogoRodando } from "../useJogoRodando"
import { useLibraryState } from "../useLibraryState"
import { fmtMiB } from "../tamanho"
import { ProfilePage } from "./ProfilePage"
import { ProfileBridge } from "../desktop/ProfileBridge"
import { EditProfile } from "./EditProfile"
import type { NewsItem } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { UpdateDialog, useAtualizacao } from "../UpdateDialog"
import { LaunchModeDialog } from "../desktop/LaunchModeDialog"
import { AchievementToast } from "../desktop/AchievementToast"
import { useGameActions } from "../useGameActions"

const MOCK_GAMES: Game[] = [
  {
    id: "1",
    title: "Neon Horizon",
    launcher: "steam",
    launch_cmd: ["steam", "steam://rungameid/1001"],
    cover: "/cover1.png",
    hero: "/hero-bg.png",
  },
  {
    id: "2",
    title: "Wasteland Chronicles",
    launcher: "heroic",
    launch_cmd: ["heroic", "--launch", "1002"],
    cover: "/cover2.png",
    hero: "/hero-bg.png",
  },
  {
    id: "3",
    title: "Dragon's Throne",
    launcher: "lutris",
    launch_cmd: ["lutris", "lutris:rungameid/1003"],
    cover: "/cover3.png",
  },
  {
    id: "4",
    title: "Abyssal Depths",
    launcher: "steam",
    launch_cmd: ["steam", "steam://rungameid/1004"],
    cover: "/cover4.png",
  },
  {
    id: "5",
    title: "Blade of Edo",
    launcher: "heroic",
    launch_cmd: ["heroic", "--launch", "1005"],
    cover: "/cover5.png",
  },
  {
    id: "6",
    title: "Void Protocol",
    launcher: "lutris",
    launch_cmd: ["lutris", "lutris:rungameid/1006"],
  },
  {
    id: "7",
    title: "Iron Galaxy",
    launcher: "steam",
    launch_cmd: ["steam", "steam://rungameid/1007"],
  },
  {
    id: "8",
    title: "Shadow Realm",
    launcher: "heroic",
    launch_cmd: ["heroic", "--launch", "1008"],
  },
]

const TAB_COUNT = TABS.length

// Um jogo fullscreen pode deixar o BrowserWindow sem um elemento ativo quando
// termina (principalmente depois de um launch por teclado). Não basta marcar o
// estado como focado: o próximo Enter precisa voltar a um controle real, e não
// a um botão escondido do overview que está fechando.
function podeReceberFoco(el: HTMLElement | null): el is HTMLElement {
  if (!el || !el.isConnected || el === document.body || el === document.documentElement) return false
  if (el.hasAttribute("disabled")) return false
  const nativeOrEditable = el.matches(
    'button:not([disabled]), a[href], area[href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [contenteditable]:not([contenteditable="false"]), [tabindex]',
  )
  // tabIndex normalizes every negative tabindex (not only -1); a visible div
  // or an input[type=hidden] must not win the fallback just because it exists.
  if (!nativeOrEditable || el.tabIndex < 0) return false
  const style = window.getComputedStyle(el)
  if (style.display === "none" || style.visibility === "hidden") return false
  for (let node: HTMLElement | null = el; node; node = node.parentElement) {
    if (node.hasAttribute("inert") || node.getAttribute("aria-hidden") === "true") return false
    if (node.classList.contains("is-hidden") || node.classList.contains("is-closing")) return false
  }
  return true
}

function focoDeRetorno(preferido: HTMLElement | null): void {
  const atual = document.activeElement instanceof HTMLElement ? document.activeElement : null
  if (podeReceberFoco(atual)) return
  const candidatos: HTMLElement[] = []
  if (preferido) candidatos.push(preferido)
  const overview = document.querySelector<HTMLElement>(
    '.arcadia-overview:not(.is-hidden) button:not([disabled]), [role="dialog"]:not(.is-hidden) button:not([disabled])',
  )
  if (overview) candidatos.push(overview)
  const selecionado = document.querySelector<HTMLElement>(
    '[data-roving-item="true"][data-active="true"]',
  )
  if (selecionado) candidatos.push(selecionado)
  const primeiro = document.querySelector<HTMLElement>(
    'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled])',
  )
  if (primeiro) candidatos.push(primeiro)
  const alvo = candidatos.find(podeReceberFoco)
  alvo?.focus({ preventScroll: true })
}

interface LaunchToast {
  title: string
  visible: boolean
}

export function PS5Launcher() {
  // Fora do Electron (dev no navegador) cai no mock; dentro, carrega o real.
  const { games, setGames, profile, setProfile, config, libraryLoaded } = useLibraryState(
    typeof window !== "undefined" && window.launcherAPI ? [] : MOCK_GAMES,
  )
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [toast, setToast] = useState<LaunchToast>({ title: "", visible: false })
  const [showEditProfile, setShowEditProfile] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const [showProfile, setShowProfile] = useState(false)
  const [cardScale, setCardScale] = useState(1.6)
  // Abas: 0 Notícias · 1 Jogos (trilho)
  const [activeTab, setActiveTab] = useState(1) // abre em Jogos (Notícias é a aba 0)
  const [libraryFilter, setLibraryFilter] = useState<LibraryFilter>("all")
  const [librarySearch, setLibrarySearch] = useState("")
  // Ids já iniciados (persistido): alimenta o selo "Nunca jogado".
  const [recent, setRecent] = useState<string[]>([])
  const { t } = useI18n()
  const conta = useAccountOptional()

  // Menu de contexto do jogo (Start) e visibilidade dos ocultos.
  const [ctxGame, setCtxGame] = useState<Game | null>(null)
  const [trailerPickGame, setTrailerPickGame] = useState<Game | null>(null)
  const [editGame, setEditGame] = useState<Game | null>(null)
  const [showHidden, setShowHidden] = useState(false)

  // Jogo em execução, segundo o vigia de processo do main — e não mais um
  // palpite pelo foco da janela. Guarda QUAL jogo é, para o botão do herói
  // virar "Parar" só naquele.
  const jogoAtivo = useJogoRodando(games)
  const atualizacao = useAtualizacao()
  // Para pausar trailer de fundo e vídeo da loja, "abrindo" já conta como
  // jogo em cena.
  const gameRunning = jogoAtivo.rodando || jogoAtivo.pendente
  const gameRunningRef = useRef(false)
  gameRunningRef.current = gameRunning
  // Evita dois IPCs quando Enter/Space fica pressionado durante a transição
  // para a janela fullscreen (o retorno do primeiro IPC é assíncrono).
  const launchPendingRef = useRef(false)
  const jogoAtivoRef = useRef(jogoAtivo)
  jogoAtivoRef.current = jogoAtivo

  // Trailer no fundo (estilo PS5): toca ao focar o jogo por um instante.
  const [trailerUrl, setTrailerUrl] = useState<string | null>(null)
  const trailerAutoRef = useRef(true)

  // Aba de notícias (RSS PT-BR).
  const [news, setNews] = useState<NewsItem[]>([])
  const [newsLoading, setNewsLoading] = useState(false)
  const newsRef = useRef<HTMLDivElement>(null)
  const [overviewOpen, setOverviewOpen] = useState(false)
  const [overviewMounted, setOverviewMounted] = useState(false)
  const [overviewClosing, setOverviewClosing] = useState(false)
  const overviewReopenRef = useRef(false)
  const overviewRef = useRef<HTMLDivElement>(null)
  const overviewCloseTimer = useRef<number | null>(null)
  const openOverview = useCallback((force = false) => {
    // O hub só existe na aba Jogos. A Loja e Notícias têm suas próprias telas
    // e nunca devem receber o Game Overview por cima ao pressionar ↓.
    if (!force && activeTab !== 1) return
    // Um novo "descer" durante o retorno não pode se perder: guarda a intenção
    // e abre assim que a animação inversa terminar.
    if (overviewClosing) {
      overviewReopenRef.current = true
      return
    }
    if (overviewCloseTimer.current) {
      window.clearTimeout(overviewCloseTimer.current)
      overviewCloseTimer.current = null
    }
    setOverviewClosing(false)
    setOverviewMounted(true)
    setOverviewOpen(true)
  }, [activeTab, overviewClosing])
  const closeOverview = useCallback(() => {
    if (overviewCloseTimer.current) window.clearTimeout(overviewCloseTimer.current)
    setOverviewClosing(true)
    overviewCloseTimer.current = window.setTimeout(() => {
      setOverviewOpen(false)
      setOverviewClosing(false)
      overviewCloseTimer.current = null
      if (overviewReopenRef.current) {
        overviewReopenRef.current = false
        window.requestAnimationFrame(() => setOverviewOpen(true))
      }
    }, 1000)
  }, [])
  useEffect(() => {
    return () => {
      if (overviewCloseTimer.current) window.clearTimeout(overviewCloseTimer.current)
    }
  }, [])

  // Boot + portão de perfil (declarados ANTES do modalOpenRef, que os lê).
  const [boot, setBoot] = useState(true)
  const [bootSaindo, setBootSaindo] = useState(false)
  // O portão de perfil JÁ NASCE montado por baixo do vídeo de boot (z-75 < z-80).
  // Assim, quando o vídeo sai em fade, o perfil já está lá — sem flash da home.
  const [perfilGate, setPerfilGate] = useState(true)
  const [perfilSaindo, setPerfilSaindo] = useState(false)
  const [posLogin, setPosLogin] = useState(false) // dispara a coreografia da home
  const perfilRef = useRef<HTMLDivElement>(null)
  // Confirmação do perfil: a tela sai em fade rápido e a home entra com a
  // coreografia em 3 fases (máscara → navegação/herói → cascata das capas).
  const confirmarPerfil = () => {
    if (perfilSaindo) return
    setPerfilSaindo(true)
    setPosLogin(true)
    setTimeout(() => setPerfilGate(false), 360)
    // A classe .pos-login precisa SAIR quando a coreografia acaba. Ficando
    // para sempre, qualquer remontagem do trilho (trocar de aba e voltar)
    // reexecutava a cascata de entrada do login — uma abertura de sessão
    // acontecendo no meio da navegação. 1,6s cobre a fase mais longa (0,55s
    // de atraso + 0,7s de fade).
    setTimeout(() => setPosLogin(false), 1600)
  }

  const [showDownloads, setShowDownloads] = useState(false)
  // Jogo Epic aguardando escolha do destino de instalação.
  const [instalarGame, setInstalarGame] = useState<Game | null>(null)
  const [destinosEpic, setDestinosEpic] = useState<DestinoOpcao[]>([])
  // Jogo Steam sem manifesto em nenhum provedor: o único caminho que resta é
  // a própria Steam, e a pessoa decide se quer.
  const [semManifesto, setSemManifesto] = useState<{
    jogo: { appid: string; title: string }
    motivo: string
  } | null>(null)
  const [escolhendoLaunch, setEscolhendoLaunch] = useState<Game | null>(null)

  const {
    launch: launchGame,
    launchCommand,
    saveMetadata,
    toggleFavorite,
    toggleHidden,
    refresh: refreshLibrary,
  } = useGameActions({
    setGames,
    onChooseLaunch: setEscolhendoLaunch,
    onLaunchWarning: (_game, warnings) => {
      setToast({ title: warnings.join("\n"), visible: true })
      setTimeout(() => setToast((current) => ({ ...current, visible: false })), 7000)
    },
    onLaunchError: (_game, error) => {
      setToast({ title: error, visible: true })
      setTimeout(() => setToast((current) => ({ ...current, visible: false })), 7000)
    },
  })

  // Instalação de jogo Steam pelo NOSSO downloader (manifesto + DepotDownloader),
  // o mesmo caminho do botão Baixar da loja — e não pelo cliente da Steam.
  const acoesLoja = useStoreActions(games, {
    onSemManifesto: (jogo, motivo) => setSemManifesto({ jogo, motivo }),
  })
  // `_activate` é um useCallback sem dependências (o trilho o chama a cada
  // tecla); a ref evita capturar uma versão velha do hook.
  const baixarSteamRef = useRef<(g: Game) => void>(() => {})
  baixarSteamRef.current = (g: Game) =>
    acoesLoja.baixar({ appid: String(g.id).replace(/^steam:/, ""), title: g.title })

  // Destinos do Epic: a pasta padrão e uma pasta Arcadia na raiz de cada
  // biblioteca Steam — os discos que a pessoa já reservou para jogos, sem se
  // misturar com o steamapps da Steam.
  useEffect(() => {
    if (!instalarGame) return
    let cancelado = false
    ;(async () => {
      const api = window.launcherAPI
      const cfg = await api?.getConfig()
      const home = window.launcherPaths?.home || "~"
      const bases = [
        {
          caminho: cfg?.default_install_path || `${home}/Games/Arcadia`,
          rotulo: t("ps5.destino.pasta_padrao"),
        },
        ...((await api?.storeLibraries()) || []).map((l) => ({
          caminho: `${l.steamDir}/Arcadia`,
          rotulo: t("ps5.destino.disco_steam"),
        })),
      ].filter((d, i, arr) => arr.findIndex((o) => o.caminho === d.caminho) === i)
      const comEspaco = await Promise.all(
        bases.map(async (d) => {
          const r = await api?.diskSpace(d.caminho)
          return r?.ok ? { ...d, livre: r.free } : d
        }),
      )
      if (!cancelado) setDestinosEpic(comEspaco)
    })()
    return () => {
      cancelado = true
      setDestinosEpic([])
    }
  }, [instalarGame])
  const dmRef = useRef<HTMLDivElement>(null)

  // modalOpenRef: algum overlay/modal aberto → bloqueia TUDO (inclusive trocar aba).
  const modalOpenRef = useRef(false)
  modalOpenRef.current =
    boot ||
    perfilGate ||
    showDownloads ||
    showEditProfile ||
    menuOpen ||
    showProfile ||
    overviewOpen ||
    Boolean(ctxGame) ||
    Boolean(editGame) ||
    Boolean(trailerPickGame) ||
    Boolean(instalarGame) ||
    Boolean(semManifesto) ||
    Boolean(escolhendoLaunch) ||
    Boolean(acoesLoja.escolhendo)

  // uiBlockedRef: pausa a navegação de JOGOS (D-pad/A). Vale também na aba de
  // Notícias, que tem foco próprio — mas o L1/R1 (trocar aba) segue funcionando.
  const uiBlockedRef = useRef(false)
  // A Loja tem navegação própria por foco, igual às Notícias: sem isto o
  // direcional moveria a seleção do trilho de jogos por trás da loja.
  // Notícias (0) e Loja (2) têm navegação/foco próprios. O handler global
  // da biblioteca não pode abrir o Game Overview por cima dessas telas.
  uiBlockedRef.current = modalOpenRef.current || activeTab === 0 || activeTab === 2

  // Ambas as abas mostram a biblioteca inteira; muda só a forma de exibir.
  // Jogos ocultos só aparecem com "Mostrar ocultos" ligado (menu do Select).
  const GRID_COLUMNS = 7
  const viewGames = useMemo(() => {
    const query = librarySearch.trim().toLocaleLowerCase()
    return games.filter((game) => {
      if (!showHidden && game.hidden) return false
      if (libraryFilter === "favorites" && !game.favorite) return false
      if (libraryFilter === "collections" && !game.categories?.length) return false
      if (query && !game.title.toLocaleLowerCase().includes(query)) return false
      return true
    })
  }, [games, showHidden, libraryFilter, librarySearch])
  // Abas: 0 Notícias · 1 Jogos (trilho); a Loja fica fora do Big Picture.
  const newsMode = activeTab === 0
  const storeMode = activeTab === 2
  const columns = GRID_COLUMNS

  // A Loja antiga pode ter ficado selecionada durante hot reload; no Big
  // Picture atual existem somente Notícias e Jogos.
  useEffect(() => {
    if (activeTab > 1) setActiveTab(1)
  }, [activeTab])

  const selectedGame = viewGames[selectedIndex] ?? viewGames[0] ?? null

  // O loop do gamepad não re-registra a cada troca de seleção; lê por aqui.
  const selectedGameRef = useRef<Game | null>(null)
  selectedGameRef.current = selectedGame
  // Espelha o filtro de ocultos para o recarregamento da biblioteca calcular o
  // novo índice sobre a MESMA lista que a tela mostra.
  const showHiddenRef = useRef(showHidden)
  showHiddenRef.current = showHidden

  // Carrega a biblioteca real (library.json) via o estado compartilhado.
  const bootVideoFim = useRef(false)
  const bootLibOk = useRef(false)
  const tentarSairBoot = () => {
    if (!bootVideoFim.current || !bootLibOk.current) return
    setBootSaindo(true)
    setTimeout(() => setBoot(false), 900)
  }
  useEffect(() => {
    if (!libraryLoaded) return
    bootLibOk.current = libraryLoaded
    tentarSairBoot()
    const firstNewDefaults = config.big_picture_scale_defaults_v3 !== true
    const consoleScale = firstNewDefaults ? 1.3 : (config.console_ui_scale ?? 1.3)
    const coverScale = firstNewDefaults ? 1.6 : (config.card_scale ?? 1.6)
    if (firstNewDefaults) {
      window.launcherAPI?.setConfig({
        console_ui_scale: consoleScale,
        card_scale: coverScale,
        big_picture_scale_defaults_v2: true,
        big_picture_scale_defaults_v3: true,
      })
    }
    window.launcherAPI?.setZoom(consoleScale, "console")
    trailerAutoRef.current = config.trailer_auto !== false
    applyUiPrefs({ ...config, card_scale: coverScale })
    try {
      const r = JSON.parse(localStorage.getItem("gs_recent") || "[]")
      if (Array.isArray(r)) setRecent(r)
    } catch {
      /* ignore */
    }
  }, [config, libraryLoaded])

  // Aplica preferências visuais (escala das capas + cor de destaque).
  function applyUiPrefs(c: { card_scale?: number; accent?: string }) {
    setCardScale(c?.card_scale ?? 1.6)
    document.documentElement.style.setProperty("--accent", c?.accent || "#00a8ff")
  }

  // Trailer no fundo: ao focar um jogo por ~1,5s, toca o trailer. Se não estiver
  // baixado ainda, busca no YouTube em segundo plano e toca quando pronto (desde
  // que você ainda esteja no mesmo jogo). Trocar de jogo corta o trailer na hora.
  const selId = selectedGame?.id
  useEffect(() => {
    setTrailerUrl(null)
    const g = selectedGame
    const api = window.launcherAPI
    if (!g || !api || !trailerAutoRef.current) return
    if (
      showEditProfile ||
      menuOpen ||
      showProfile ||
      gameRunning ||
      ctxGame ||
      trailerPickGame
    )
      return
    if (boot || perfilGate) return // boot/seleção de perfil: nada de trailer
    if (newsMode || storeMode) return // essas abas têm visual próprio
    let cancelled = false
    const t = setTimeout(async () => {
      const { path } = await api.trailerPath(g.id)
      if (cancelled) return
      if (path) {
        setTrailerUrl(path)
        return
      }
      const r = await api.trailerDownload(g.id, g.title)
      if (!cancelled && r.ok && r.path && selectedGameRef.current?.id === g.id) {
        setTrailerUrl(r.path)
      }
    }, 1500)
    return () => {
      cancelled = true
      clearTimeout(t)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    selId,
    showEditProfile,
    menuOpen,
    showProfile,
    gameRunning,
    ctxGame,
    trailerPickGame,
    newsMode,
    storeMode,
    boot,
    perfilGate,
  ])

  // Notícias: busca alinhada ao RELÓGIO (marcos de 5 min — :00,:05,:10…).
  // O slot também gira o destaque da aba (rotação a cada 5 min).
  const SLOT_5 = 5 * 60 * 1000
  const [newsSlot, setNewsSlot] = useState(() => Math.floor(Date.now() / SLOT_5))

  // Foco real da janela (eventos blur/focus do Electron — no gamescope o
  // document.hasFocus() mente). Trava gamepad e silencia trailer.
  const [appFocused, setAppFocused] = useState(() => document.hasFocus())
  const appFocusedRef = useRef(appFocused)
  const ultimoFocoRef = useRef<HTMLElement | null>(null)
  appFocusedRef.current = appFocused
  useEffect(() => {
    const lembrarFoco = (event: FocusEvent) => {
      if (event.target instanceof HTMLElement && podeReceberFoco(event.target)) {
        ultimoFocoRef.current = event.target
      }
    }
    const aoFocar = () => {
      appFocusedRef.current = true
      setAppFocused(true)
      // O compositor pode entregar o evento focus antes de o conteúdo voltar a
      // estar visível. Um frame deixa show()/restore() terminar sem atropelar
      // um clique que o usuário acabou de fazer.
      window.requestAnimationFrame(() => focoDeRetorno(ultimoFocoRef.current))
    }
    const aoDesfocar = () => {
      appFocusedRef.current = false
      setAppFocused(false)
    }
    const aoFocoDoMain = (f: boolean) => {
      appFocusedRef.current = Boolean(f)
      setAppFocused(Boolean(f))
      if (f) window.requestAnimationFrame(() => focoDeRetorno(ultimoFocoRef.current))
    }
    window.addEventListener("focusin", lembrarFoco)
    window.addEventListener("focus", aoFocar)
    window.addEventListener("blur", aoDesfocar)
    const off = window.launcherAPI?.onAppFocus(aoFocoDoMain)
    const offLaunchError = window.launcherAPI?.onLaunchError?.(() => {
      launchPendingRef.current = false
      jogoAtivoRef.current.limpar()
    })
    // Consulta o estado atual além de ouvir eventos: um launch pode ter
    // acontecido antes deste componente montar, especialmente após reload do
    // renderer dentro do gamescope.
    const estadoAtual = window.launcherAPI?.getAppFocus?.()
    if (estadoAtual) void estadoAtual.then(aoFocoDoMain).catch(() => {})
    return () => {
      window.removeEventListener("focusin", lembrarFoco)
      window.removeEventListener("focus", aoFocar)
      window.removeEventListener("blur", aoDesfocar)
      off?.()
      offLaunchError?.()
    }
  }, [])
  useEffect(() => {
    if (!newsMode && !overviewOpen) return
    const api = window.launcherAPI
    if (!api) return
    let timer = 0
    const buscar = () => {
      if (!news.length) setNewsLoading(true)
      api
        .getNews()
        .then((n) => {
          if (Array.isArray(n) && n.length) setNews(n)
        })
        .finally(() => setNewsLoading(false))
    }
    const agendar = () => {
      const agora = Date.now()
      const espera = SLOT_5 - (agora % SLOT_5) + 250 // pequena margem pós-marco
      timer = window.setTimeout(() => {
        setNewsSlot(Math.floor(Date.now() / SLOT_5))
        buscar()
        agendar()
      }, espera)
    }
    buscar() // carga inicial / ao voltar para a aba
    agendar()
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newsMode, overviewOpen])

  // Navegação por controle na aba de notícias (D-pad move o foco, A abre, B nada).
  // Notícias: navegação SÓ por scroll (analógico direito). Sem foco espacial —
  // o anel azul de foco no card destaque poluía a tela.
  useGamepadNav(newsRef, newsMode && appFocused && !gameRunning, undefined, true)
  // A loja agora é a StoreView nativa (React puro): busca, cards e página do
  // jogo são todos DOM comum, com foco padrão dos <button>. O useGamepadNav
  // move o foco espacial como em qualquer outra tela — sem cursor virtual,
  // sem webview, sem preload injetado. O visual é o mesmo do modo desktop.
  const storeRef = useRef<HTMLDivElement>(null)
  const atalhosLoja = useRef<{
    voltar: () => boolean
    abrirTeclado: () => void
  } | null>(null)
  const setAtalhosLoja = useCallback((a: typeof atalhosLoja.current) => {
    atalhosLoja.current = a
  }, [])
  const extrasLoja = useMemo(
    () => ({
      // Y abre o teclado virtual pra digitar a busca sem depender de teclado
      // físico. O analógico esquerdo pertence ao cursor da loja; D-pad segue
      // com a navegação espacial normal e A/Cross confirma o cursor ou o foco.
      onY: () => atalhosLoja.current?.abrirTeclado(),
      dpadOnly: true,
    }),
    [],
  )
  // B na loja tem pilha própria (StoreView.voltar): fecha página/dialog/teclado
  // conforme o que estiver aberto. Só na raiz devolve false, e aí o B sai da
  // loja para a aba Jogos.
  const voltarLoja = useCallback(() => {
    if (atalhosLoja.current?.voltar()) return
    setActiveTab(1)
  }, [])
  // Overlay da loja aberto (página do jogo, teclado, escolha de destino): o
  // laço daqui SAI DE CENA. O overlay tem o próprio, e dois ativos disputariam
  // o mesmo direcional — e o mesmo B, que fechava a página e saía da loja
  // junto.
  const [lojaOverlay, setLojaOverlay] = useState(false)
  // Monta a loja só na primeira vez que a aba é aberta (não custa nada em quem
  // nunca usa) e nunca mais desmonta — ver o bloco persistente no render.
  const [lojaMontada, setLojaMontada] = useState(false)
  useEffect(() => {
    if (storeMode) setLojaMontada(true)
  }, [storeMode])
  useGamepadNav(
    storeRef,
    storeMode && !lojaOverlay && appFocused && !gameRunning,
    voltarLoja,
    false,
    extrasLoja,
  )

  // Overview do jogo: hub cinematográfico próprio, com navegação espacial e B
  // para voltar à biblioteca.
  const overviewNavActive =
    overviewOpen &&
    !overviewClosing &&
    appFocused &&
    !showEditProfile &&
    !menuOpen &&
    !showProfile &&
    !ctxGame &&
    !editGame &&
    !trailerPickGame
  useGamepadNav(overviewRef, overviewNavActive, closeOverview, false, { onUp: closeOverview })

  // Navegação por controle no perfil (D-pad move o foco, B fecha).
  const profileRef = useRef<HTMLDivElement>(null)
  useGamepadNav(profileRef, showProfile && appFocused && !showEditProfile && !gameRunning, () =>
    setShowProfile(false),
  )

  // Navegação por controle na seleção de perfil (só depois do boot sair).
  useGamepadNav(perfilRef, perfilGate && !boot && appFocused && !gameRunning)

  // Navegação por controle na tela de downloads. Também respeita o foco
  // autoritativo e uma sessão de jogo: gamescope mantém document.hasFocus()
  // verdadeiro, mas nenhum overlay do launcher deve consumir B/D-pad enquanto
  // o jogo fullscreen está na frente.
  useGamepadNav(
    dmRef,
    showDownloads && appFocused && !gameRunning,
    () => setShowDownloads(false),
  )

  // Diálogo do jogo sem manifesto: dois botões, então precisa do direcional e
  // do B para fechar como qualquer outro overlay.
  const semManifestoRef = useRef<HTMLDivElement>(null)
  useGamepadNav(
    semManifestoRef,
    Boolean(semManifesto) && appFocused && !gameRunning,
    () => setSemManifesto(null),
  )

  // Reseta a seleção ao trocar de aba.
  useEffect(() => {
    setSelectedIndex(0)
  }, [activeTab])

  // Mantém a seleção dentro dos limites quando a lista muda.
  useEffect(() => {
    setSelectedIndex((i) => Math.min(i, Math.max(0, viewGames.length - 1)))
  }, [viewGames.length])

  // Escape fecha o hub com a mesma animação do botão B.
  useEffect(() => {
    if (!overviewOpen) return
    const handleEscape = (event: KeyboardEvent) => {
      if (!appFocusedRef.current || gameRunningRef.current) return
      if (event.key === "Escape" || event.key === "ArrowUp") {
        event.preventDefault()
        closeOverview()
      }
    }
    window.addEventListener("keydown", handleEscape)
    return () => window.removeEventListener("keydown", handleEscape)
  }, [overviewOpen, closeOverview])

  const abrirJogo = useCallback(
    (game: Game, mode?: "steam" | "exe") => {
      if (!appFocusedRef.current || gameRunningRef.current || launchPendingRef.current) return
      launchPendingRef.current = true
      // Marca a sessão antes do IPC: o main só confirma game:running(true)
      // depois do poll, mas o lock local já bloqueia toques durante a abertura.
      const escolheModo = mode === undefined && game.launcher === "steam" && game.temExe
      if (!escolheModo) jogoAtivoRef.current.iniciar(game)
      void launchGame(game, mode)
        .then((result) => {
          launchPendingRef.current = false
          if (!result.ok || result.needsMode) {
            jogoAtivoRef.current.limpar()
            return
          }
          if (escolheModo) jogoAtivoRef.current.iniciar(game)
          setRecent((prev) => {
            const next = [game.id, ...prev.filter((id) => id !== game.id)].slice(0, 30)
            try {
              localStorage.setItem("gs_recent", JSON.stringify(next))
            } catch {
              /* ignore */
            }
            return next
          })
          setToast({ title: game.title, visible: true })
          setTimeout(() => setToast((current) => ({ ...current, visible: false })), 3000)
        })
        .catch(() => {
          // O bridge normalmente converte a falha em { ok: false }, mas a trava
          // também precisa ser liberada se um bridge customizado rejeitar.
          launchPendingRef.current = false
          jogoAtivoRef.current.limpar()
        })
    },
    [launchGame],
  )

  // Instalar OU abrir, conforme o estado do jogo. Instalar não trava o launcher
  // (não é sessão de jogo) — só abrir seta gameRunning.
  const _activate = useCallback(
    (game?: Game | null) => {
      if (!game) return
      // Um jogo fullscreen pode deixar a janela atrás do jogo; mesmo quando um
      // clique/tecla vaza até aqui, nunca inicie um segundo processo. O mesmo
      // jogo continua podendo usar o botão Jogar/Parar/Cancelar quando o
      // launcher está focado de propósito. A ação de parar não depende do
      // evento de foco chegar antes do clique.
      const sessao = jogoAtivoRef.current
      // Também cobre reload do renderer ou jogo externo, quando o booleano
      // rodando foi replayado mas o objeto Game ainda não existe localmente.
      if (launchPendingRef.current || gameRunningRef.current) {
        if (sessao.jogo?.id === game.id) {
          if (sessao.rodando) sessao.parar()
          else if (sessao.pendente) {
            sessao.cancelar()
            launchPendingRef.current = false
          }
        }
        return
      }
      if (!appFocusedRef.current) return
      if (
        sessao.jogo &&
        sessao.jogo.id !== game.id &&
        (sessao.rodando || sessao.pendente)
      ) return

      // Não instalado: redireciona para a instalação de cada loja.
      if (game.installed === false) {
        if (game.launcher === "epic") {
          // Pergunta o destino ANTES de baixar. Antes disto o download começava
          // na hora, na pasta padrão, sem nenhuma confirmação — o desktop já
          // perguntava (InstallDialog), o console não.
          setInstalarGame(game)
        } else if (game.launcher === "steam") {
          // Pelo NOSSO downloader: busca o manifesto nos provedores e baixa com
          // o DepotDownloader, igual ao botão Baixar da loja. O cliente da Steam
          // só entra se nenhum provedor tiver o manifesto (diálogo de saída).
          baixarSteamRef.current(game)
        } else {
          // heroic/lutris: cai no launch_cmd (o próprio runner trata).
          void launchCommand(game.launch_cmd, game.id)
        }
        return
      }

      // Este jogo já foi lançado? Rodando de fato, o botão é "Parar"; ainda
      // abrindo, o botão é "Cancelar" — um segundo toque não pode lançar duas
      // vezes nem matar o processo que está subindo sem uma ação explícita.
      if (jogoAtivoRef.current.jogo?.id === game.id) {
        if (jogoAtivoRef.current.rodando) jogoAtivoRef.current.parar()
        else if (jogoAtivoRef.current.pendente) {
          jogoAtivoRef.current.cancelar()
          launchPendingRef.current = false
        }
        return
      }

      abrirJogo(game)
    },
    [abrirJogo, launchCommand],
  )

  // Ao escolher um jogo no perfil, volta para a aba Jogos e abre o mesmo
  // overview usado pela biblioteca. O perfil não deve iniciar o jogo
  // imediatamente nem deixar o modal aberto por cima do overview.
  const _open_profile_game = useCallback(
    (game: Game) => {
      const nextViewGames = games.filter((item) => showHidden || !item.hidden)
      const index = nextViewGames.findIndex((item) => item.id === game.id)
      if (index < 0) return
      setActiveTab(1)
      setLibraryFilter("all")
      setLibrarySearch("")
      setSelectedIndex(index)
      setShowProfile(false)
      window.requestAnimationFrame(() => openOverview(true))
    },
    [games, openOverview, showHidden],
  )

  const _launch_selected = useCallback(() => {
    _activate(viewGames[selectedIndex])
  }, [viewGames, selectedIndex, _activate])

  // Salva metadados editados à mão. Mesmo caminho do ocultar: overrides.json.
  const _save_meta = useCallback(
    (game: Game, patch: Record<string, unknown>) => {
      if (!Object.keys(patch).length) return
      void saveMetadata(game, patch)
      setToast({ title: t("ps5.toast.metadados_salvos", { title: game.title }), visible: true })
      setTimeout(() => setToast((current) => ({ ...current, visible: false })), 2500)
    },
    [saveMetadata, t],
  )

  // As mutações da biblioteca ficam no hook compartilhado; estes wrappers só
  // cuidam do feedback visual específico do modo console.
  const _toggle_hidden = useCallback(
    (game: Game) => {
      const nowHidden = !game.hidden
      void toggleHidden(game)
      setToast({
        title: nowHidden
          ? t("ps5.toast.oculto", { title: game.title })
          : t("ps5.toast.reexibido", { title: game.title }),
        visible: true,
      })
      setTimeout(() => setToast((current) => ({ ...current, visible: false })), 2500)
    },
    [t, toggleHidden],
  )

  const _toggle_favorite = useCallback(
    (game: Game) => {
      void toggleFavorite(game)
    },
    [toggleFavorite],
  )

  const _refresh_library = useCallback(() => {
    void refreshLibrary()
    setToast({ title: t("ps5.toast.biblioteca_atualizada"), visible: true })
    setTimeout(() => setToast((current) => ({ ...current, visible: false })), 2500)
  }, [refreshLibrary, t])

  // Keyboard / gamepad navigation
  useEffect(() => {
    let lastNav = 0
    const COOLDOWN = 160

    const N = viewGames.length
    const step = (d: number) => setSelectedIndex((i) => Math.max(0, Math.min(N - 1, i + d)))

    const handleKey = (e: KeyboardEvent) => {
      // Em gamescope o Chromium continua dizendo que está focado e pode até
      // receber uma tecla enquanto o jogo fullscreen está na frente. Nunca
      // deixe essa tecla abrir outro jogo, alternar telas ou clicar no launcher.
      const acaoExplicita = e.key === "Enter" || e.key === " "
      // Stop/Cancel is an explicit user command. It remains available even if
      // the native focus flag says the game owns the surface; other navigation
      // keys stay blocked in that state.
      if (gameRunningRef.current || launchPendingRef.current) {
        if (
          acaoExplicita &&
          selectedGameRef.current?.id === jogoAtivoRef.current.jogo?.id
        ) {
          e.preventDefault()
          if (jogoAtivoRef.current.rodando) jogoAtivoRef.current.parar()
          else if (jogoAtivoRef.current.pendente) {
            jogoAtivoRef.current.cancelar()
            launchPendingRef.current = false
          }
        }
        if (!appFocusedRef.current || gameRunningRef.current || launchPendingRef.current) return
      }
      if (!appFocusedRef.current) return
      if (overviewClosing && e.key === "ArrowDown") {
        e.preventDefault()
        openOverview()
        return
      }
      if (e.key === "Escape") {
        // Esc abre o mesmo popup de ações do Start quando estamos na
        // biblioteca; overlays existentes continuam controlando seu próprio
        // fechamento.
        if (!modalOpenRef.current && selectedGameRef.current) {
          e.preventDefault()
          setCtxGame(selectedGameRef.current)
        }
        return
      }
      if (uiBlockedRef.current) return // painel de config aberto
      // Buttons and links already implement Enter/Space themselves. Ignoring
      // those targets avoids launching the selected game a second time when a
      // keyboard user activates a top-bar action or a roving rail card.
      const target = e.target instanceof HTMLElement ? e.target : null
      const nativeControl = Boolean(
        target?.closest(
          "button, a[href], input, select, textarea, [role=button], [contenteditable=true]",
        ),
      )
      const now = Date.now()
      if (now - lastNav < COOLDOWN) return

      if (e.key === "ArrowLeft") {
        lastNav = now
        step(-1)
      } else if (e.key === "ArrowRight") {
        lastNav = now
        step(1)
      } else if (e.key === "ArrowUp") {
        lastNav = now
      } else if (e.key === "ArrowDown") {
        lastNav = now
        if (selectedGameRef.current) openOverview()
      } else if (e.key === "Enter" || e.key === " ") {
        if (!nativeControl) _launch_selected()
      } else if (e.key === "F5" || e.key === "r") _refresh_library()
    }
    window.addEventListener("keydown", handleKey)
    return () => window.removeEventListener("keydown", handleKey)
  }, [viewGames.length, columns, _launch_selected, _refresh_library, openOverview, overviewClosing])

  // Navegação por controle (Gamepad API): D-pad/analógico, A=jogar, Start=atualizar.
  useEffect(() => {
    let raf = 0
    let prev: boolean[] = []
    let restAxes: number[] | null = null
    let sx = 0,
      sy = 0 // direção estável (x,y)
    let cx = 0,
      cy = 0 // candidata
    let candSince = 0
    let holdStart = 0
    let lastRepeat = 0
    let lastStep = 0
    let scrollVel = 0 // inércia do scroll do analógico direito
    const DEBOUNCE = 90
    const INITIAL_DELAY = 500
    const REPEAT = 260
    const MIN_GAP = 200

    // Direção 2D com calibração de repouso (mata drift/gatilhos e flicker).
    const direction = (gp: Gamepad): [number, number] => {
      let x = 0
      let y = 0
      if (gp.buttons[15]?.pressed) x = 1
      else if (gp.buttons[14]?.pressed) x = -1
      if (gp.buttons[13]?.pressed) y = 1
      else if (gp.buttons[12]?.pressed) y = -1
      if (!restAxes) restAxes = Array.from(gp.axes)
      const ax = (gp.axes[0] ?? 0) - (restAxes[0] ?? 0)
      const ay = (gp.axes[1] ?? 0) - (restAxes[1] ?? 0)
      if (!x) x = ax > 0.6 ? 1 : ax < -0.6 ? -1 : 0
      if (!y) y = ay > 0.6 ? 1 : ay < -0.6 ? -1 : 0
      const h = gp.axes[9]
      if (!x && !y && typeof h === "number" && h >= -1.05 && h <= 1.05) {
        const near = (t: number) => Math.abs(h - t) < 0.1
        if (near(-1)) y = -1
        else if (near(-0.714)) {
          x = 1
          y = -1
        } else if (near(-0.428)) x = 1
        else if (near(-0.142)) {
          x = 1
          y = 1
        } else if (near(0.142)) y = 1
        else if (near(0.428)) {
          x = -1
          y = 1
        } else if (near(0.714)) x = -1
        else if (near(1)) {
          x = -1
          y = -1
        }
      }
      return [x, y]
    }

    const N = viewGames.length
    const move = (dx: number, dy: number) => {
      // Para baixo abre o hub do jogo selecionado; o trilho continua horizontal.
      if (dy > 0) {
        if (selectedGameRef.current) openOverview()
        return
      }
      // Rail: só horizontal.
      let delta = 0
      if (dx !== 0) delta = dx
      if (delta === 0) return
      setSelectedIndex((i) => Math.max(0, Math.min(N - 1, i + delta)))
    }

    const loop = () => {
      // Janela sem foco (jogo em primeiro plano, alt-tab, gamescope): ignora
      // o controle — a Gamepad API entrega input mesmo desfocada.
      if (!appFocusedRef.current) {
        prev = [] // ressincroniza ao voltar (não dispara botão segurado)
        raf = requestAnimationFrame(loop)
        return
      }
      if (launchPendingRef.current) {
        // O IPC de launch é assíncrono; ignora o botão ainda pressionado até o
        // estado pendente/rodando estar refletido no renderer.
        prev = []
        raf = requestAnimationFrame(loop)
        return
      }
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      if (gp) {
        const now = Date.now()
        const primed = prev.length > 0
        if (overviewClosing && primed && gp.buttons[13]?.pressed && !prev[13]) {
          openOverview()
        }
        if (primed && gp.buttons[8]?.pressed && !prev[8] && !gameRunningRef.current) {
          setMenuOpen((v) => !v)
        }

        // L1/LB e R1/RB trocam de aba (Notícias ↔ Jogos ↔ Biblioteca) em QUALQUER
        // aba — só não quando há um modal aberto.
        if (!modalOpenRef.current) {
          // R2/RT (botão 7 no mapeamento padrão) abre a tela de downloads de
          // qualquer aba. L2 fica livre de propósito.
          if (primed && gp.buttons[7]?.pressed && !prev[7]) {
            setShowDownloads(true)
          }
          if (primed && gp.buttons[5]?.pressed && !prev[5]) {
            setActiveTab((t) => Math.min(TAB_COUNT - 1, t + 1))
          }
          if (primed && gp.buttons[4]?.pressed && !prev[4]) {
            setActiveTab((t) => Math.max(0, t - 1))
          }
        }

        if (!uiBlockedRef.current) {
          // Analógico DIREITO: rolagem suave do trilho de capas (estilo
          // navegador), igual à aba de Notícias.
          if (restAxes) {
            let sry = 0
            for (let ai = 2; ai < gp.axes.length; ai++) {
              const v = (gp.axes[ai] ?? 0) - (restAxes[ai] ?? 0)
              if (Math.abs(v) > Math.abs(sry)) sry = v
            }
            const target = Math.abs(sry) > 0.15 ? Math.sign(sry) * sry * sry * 46 : 0
            scrollVel += (target - scrollVel) * 0.25
          }
          const [rx, ry] = direction(gp)
          if (rx !== cx || ry !== cy) {
            cx = rx
            cy = ry
            candSince = now
          }
          // Descer abre uma tela, não percorre uma lista: responde no primeiro
          // quadro para parecer um gesto direto. As demais direções continuam
          // com debounce/intervalo contra drift do analógico.
          const aberturaOverview = cy > 0
          if (now - candSince >= (aberturaOverview ? 0 : DEBOUNCE) && (sx !== cx || sy !== cy)) {
            const wasNeutral = sx === 0 && sy === 0
            sx = cx
            sy = cy
            if ((sx || sy) && wasNeutral && now - lastStep >= (aberturaOverview ? 0 : MIN_GAP)) {
              move(sx, sy)
              lastStep = now
              holdStart = now
              lastRepeat = now
            }
          }
          if ((sx || sy) && now - holdStart > INITIAL_DELAY && now - lastRepeat > REPEAT) {
            move(sx, sy)
            lastRepeat = now
            lastStep = now
          }

          if (primed && gp.buttons[0]?.pressed && !prev[0]) _launch_selected() // A
          // Start abre as opções do jogo selecionado.
          if (primed && gp.buttons[9]?.pressed && !prev[9]) {
            setCtxGame(selectedGameRef.current)
          }
        } else {
          sx = 0
          sy = 0
          cx = 0
          cy = 0
        }
        prev = gp.buttons.map((b) => b.pressed)
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [viewGames.length, columns, _launch_selected, _refresh_library, openOverview, overviewClosing])

  const topBarNode = (
    <TopBar
      profile={
        conta?.perfil
          ? {
              ...profile,
              name: conta.perfil.display_name || conta.perfil.username || profile.name,
              avatar: conta.perfil.avatar_url ?? "",
            }
          : profile
      }
      activeTab={activeTab}
      onTab={setActiveTab}
      onRefresh={_refresh_library}
      onOpenProfile={() => setShowProfile(true)}
      menuOpen={menuOpen}
      onToggleMenu={() => setMenuOpen((v) => !v)}
      onCloseMenu={() => setMenuOpen(false)}
      showHidden={showHidden}
      onToggleShowHidden={() => setShowHidden((v) => !v)}
      libraryFilter={libraryFilter}
      onLibraryFilter={(filter) => {
        setLibraryFilter(filter)
        setSelectedIndex(0)
      }}
      search={librarySearch}
      onSearch={(value) => {
        setLibrarySearch(value)
        setSelectedIndex(0)
      }}
    />
  )
  const railNode =
    viewGames.length > 0 ? (
      <GameRail
        games={viewGames}
        selectedIndex={selectedIndex}
        cardScale={cardScale}
        onSelect={setSelectedIndex}
        onLaunch={_activate}
      />
    ) : (
      <div className="px-10 py-10 text-[#8a93a6]">{t("ps5.biblioteca.vazia")}</div>
    )
  const heroNode = (
    <HeroSection
      game={selectedGame}
      trailerUrl={appFocused && !gameRunning && !boot && !perfilGate ? trailerUrl : null}
      rodando={Boolean(selectedGame && jogoAtivo.rodando && jogoAtivo.jogo?.id === selectedGame.id)}
      abrindo={Boolean(
        selectedGame && jogoAtivo.pendente && jogoAtivo.jogo?.id === selectedGame.id,
      )}
      onLaunch={_launch_selected}
      onMore={() => selectedGame && setCtxGame(selectedGame)}
      onToggleFavorite={() => selectedGame && _toggle_favorite(selectedGame)}
    />
  )
  const footerNode = (
    <footer className="retro-console-footer flex h-7 shrink-0 items-center justify-between border-t px-10 text-[9px] font-black uppercase tracking-[0.16em]">
      <span>Press Start</span>
      <strong>Arcadia</strong>
      <span>Insert Coin</span>
    </footer>
  )

  return (
    <div
      className={`retro-big-picture relative flex min-h-screen flex-col select-none overflow-hidden ${storeMode ? "retro-store-active" : ""} ${posLogin ? "pos-login home-reveal" : ""} ${overviewOpen ? (overviewClosing ? "overview-returning" : "overview-active") : ""}`}
      onKeyDownCapture={(event) => {
        const action =
          event.target instanceof HTMLElement
            ? event.target.closest('[data-game-action="stop"], [data-game-action="cancel"]')
            : null
        if (
          !action &&
          !appFocusedRef.current &&
          (gameRunningRef.current || launchPendingRef.current)
        ) {
          event.preventDefault()
          event.stopPropagation()
        }
      }}
    >
      <ProfileBridge perfilLocal={profile} setPerfilLocal={setProfile} />
      {/* Tela de boot (vídeo em ~/.local/share/arcadia/boot.mp4) */}
      {boot && (
        <BootScreen
          src={`file://${window.launcherPaths?.dataDir}/boot.mp4`}
          saindo={bootSaindo}
          onEnded={() => {
            bootVideoFim.current = true
            tentarSairBoot()
          }}
          onError={() => {
            bootVideoFim.current = true // sem vídeo: pula direto
            tentarSairBoot()
          }}
        />
      )}
      {/* Fundo: tema do jogo em TELA CHEIA (crossfade real ao trocar).
          Antes, `key={selectedGame.id}` remontava o elemento a cada troca:
          o anterior sumia na hora e o novo entrava do zero (opacity: 0 →
          animação). Entre um e outro, o preto do container aparecia — a
          "piscada" clássica.
          Agora `HeroBackground` mantém DUAS camadas: a atual segue visível
          enquanto a nova entra em fade por cima; só quando a nova cobre é
          que a antiga sai. Sem gap preto. */}
      <HeroBackground
        preto={newsMode || storeMode}
        hero={trailerUrl || selectedGame?.hero || selectedGame?.cover}
        id={selectedGame ? `${selectedGame.id}:${trailerUrl ? "trailer" : "art"}` : null}
      />

      {/* Escurecimento p/ contraste: forte embaixo (trilho) e à esquerda (texto) */}
      <div
        className="retro-global-shade-bottom absolute inset-0 pointer-events-none"
        style={{
          background:
            "linear-gradient(to top, rgba(0,0,0,0.78) 0%, rgba(0,0,0,0.48) 28%, rgba(0,0,0,0.18) 62%, rgba(0,0,0,0.12) 100%)",
        }}
      />

      <div className="retro-crt-overlay pointer-events-none absolute inset-0 z-[19]" />

      <div
        className="retro-global-shade-side absolute inset-0 pointer-events-none"
        style={{
          background:
            "linear-gradient(to right, rgba(0,0,0,0.94) 0%, rgba(0,0,0,0.55) 38%, transparent 68%)",
        }}
      />

      {/* Gradiente sutil no topo, p/ legibilidade da barra transparente */}
      <div
        className="retro-global-shade-top absolute top-0 inset-x-0 h-32 z-20 pointer-events-none"
        style={{
          background:
            "linear-gradient(to bottom, rgba(0,0,0,0.7) 0%, rgba(0,0,0,0.35) 50%, transparent)",
        }}
      />

      {/* Top bar transparente, flutuando sobre o fundo */}
      {/* Editar perfil (Geral / Avatar / Plano de fundo) */}
      <EditProfile
        open={showEditProfile && !gameRunning}
        profile={profile}
        games={games}
        onClose={() => setShowEditProfile(false)}
        onChange={setProfile}
      />

      {/* Perfil (mesma tela do desktop): modal sobre o Big Picture */}
      {showProfile && (
        <div className="fixed inset-0 z-40 overflow-y-auto bg-black">
          <ProfilePage
            open
            embedded={false}
            navActive={appFocused && !gameRunning && !showEditProfile}
            profile={
              conta?.perfil
                ? {
                    ...profile,
                    name: conta.perfil.display_name || conta.perfil.username || profile.name,
                    avatar: conta.perfil.avatar_url ?? "",
                    background: conta.perfil.background_url ?? "",
                    banner: conta.perfil.banner_url ?? "",
                  }
                : profile
            }
            games={games}
            onClose={() => setShowProfile(false)}
            onEdit={() => setShowEditProfile(true)}
            onJogoClick={_open_profile_game}
          />
        </div>
      )}

      {overviewMounted && selectedGame && (
        <GameOverview
          ref={overviewRef}
          game={selectedGame}
          news={news}
          appFocused={appFocused}
          visible={overviewOpen}
          rodando={Boolean(
            selectedGame && jogoAtivo.rodando && jogoAtivo.jogo?.id === selectedGame.id,
          )}
          abrindo={Boolean(
            selectedGame && jogoAtivo.pendente && jogoAtivo.jogo?.id === selectedGame.id,
          )}
          closing={overviewClosing}
          onClose={closeOverview}
          onLaunch={(game) => {
            closeOverview()
            _activate(game)
          }}
          onOpenNews={(url) => window.launcherAPI?.openExternal(url)}
        />
      )}

      {/* Seleção de perfil (aparece depois do vídeo de boot, em crossfade) */}
      {perfilGate && (
        <div
          ref={perfilRef}
          className={`gp-scope fixed inset-0 z-[75] ${perfilSaindo ? "perfil-gate-out" : "perfil-gate-in"}`}
        >
          <ProfileSelect
            profiles={[
              {
                name:
                  conta?.perfil?.display_name ||
                  conta?.perfil?.username ||
                  profile?.name ||
                  t("profile.jogador"),
                avatar: conta?.perfil?.avatar_url ?? profile?.avatar,
                background: conta?.perfil?.background_url ?? profile?.background,
                banner: conta?.perfil?.banner_url ?? profile?.banner,
                owner: conta?.perfil?.username === OWNER_USERNAME,
              },
            ]}
            onSelect={confirmarPerfil}
            onAdd={() => {
              setPerfilGate(false)
              setShowEditProfile(true)
            }}
          />
        </div>
      )}

      {/* Conteúdo (acima do fundo). Em Notícias/Biblioteca a altura é travada na
          tela para o scroll acontecer dentro da view (ref p/ gamepad). */}
      {/* A key por aba remonta o conteudo, reiniciando a animacao de entrada:
          trocar Noticias/Jogos/Biblioteca era um corte seco. Durante o boot e a
          selecao de perfil fica de fora, para nao competir com a coreografia
          de abertura, que ja tem a sua propria sequencia. */}
      <div
        key={boot || perfilGate ? "intro" : activeTab}
        className={`retro-main-stage ${newsMode || storeMode ? "relative z-10 flex h-screen flex-col overflow-hidden" : "relative z-10 flex min-h-screen flex-col"}`}
      >
        {topBarNode}
        {storeMode /* A loja vive FORA deste bloco (que é remontado a cada troca de aba
             pela `key`): remontar destruía o webview e a loja da Steam
             recarregava do zero — vários segundos de tela preta a cada visita.
             Aqui fica só o espaço; o conteúdo é o bloco persistente abaixo. */ ? null : newsMode ? (
          <div className="min-h-0 flex-1 pt-12">
            <NewsView
              ref={newsRef}
              news={news}
              rotacao={newsSlot}
              loading={newsLoading}
              onOpen={(url) => window.launcherAPI?.openExternal(url)}
            />
          </div>
        ) : (
          <>
            {/* Espaço da barra superior */}
            <div className="h-[54px]" />

            {/* Trilho de capas no topo */}
            {railNode}

            {/* Hero embaixo à esquerda, com as ações */}
            {heroNode}
            {footerNode}
          </>
        )}
      </div>

      {/* Loja: montada na primeira visita e mantida viva daí em diante, só
          escondida quando o usuário está em outra aba. Agora é a StoreView
          (mesmo visual do modo desktop), com bigPicture=true habilitando
          teclado virtual e atalhos de gamepad. */}
      {lojaMontada && (
        <div
          ref={storeRef}
          className="retro-store-stage fixed bottom-0 right-0 top-0 z-10 overflow-hidden"
          style={{
            visibility: storeMode ? "visible" : "hidden",
            pointerEvents: storeMode ? "auto" : "none",
          }}
          aria-hidden={!storeMode}
        >
          <StoreView
            games={viewGames}
            bigPicture
            ativo={storeMode && appFocused && !gameRunning}
            appFocused={appFocused}
            gameRunning={gameRunning}
            runningGameId={jogoAtivo.jogo?.id}
            onOverlay={setLojaOverlay}
            onAtalhos={setAtalhosLoja}
            onLaunchGame={(game) => abrirJogo(game)}
          />
        </div>
      )}

      {/* Opções do jogo (Start ou botão "...") */}
      <GameContextMenu
        game={appFocused && !gameRunning ? ctxGame : null}
        onClose={() => setCtxGame(null)}
        onLaunch={() => _activate(ctxGame)}
        onEditMeta={() => setEditGame(ctxGame)}
        onToggleHidden={() => ctxGame && _toggle_hidden(ctxGame)}
        onDownloadTrailer={() => setTrailerPickGame(ctxGame)}
      />

      {/* Downloads (fila Epic) */}
      {showDownloads && <DownloadManager ref={dmRef} onClose={() => setShowDownloads(false)} />}

      {/* Destino da instalação (jogos Epic). Só depois de escolher é que o
          download entra na fila. */}
      {instalarGame && (
        <ConsoleDestinoDialog
          titulo={t("ps5.instalar.titulo", { title: instalarGame.title })}
          subtitulo={
            instalarGame.size != null
              ? t("ps5.instalar.subtitulo", { size: fmtMiB(instalarGame.size) })
              : t("ps5.instalar.escolher")
          }
          opcoes={destinosEpic}
          tamanho={instalarGame.size != null ? instalarGame.size / 1024 : undefined}
          onEscolher={(installPath) => {
            window.launcherAPI?.dmInstall({
              appid: instalarGame.id,
              title: instalarGame.title,
              cover: instalarGame.cover,
              installPath,
            })
            setInstalarGame(null)
            setShowDownloads(true)
          }}
          onFechar={() => setInstalarGame(null)}
        />
      )}

      {/* Jogo Steam: escolha da biblioteca de destino, já com o manifesto em mãos */}
      {acoesLoja.escolhendo && (
        <ConsoleDestinoDialog
          titulo={t("ps5.instalar.titulo", { title: acoesLoja.escolhendo.jogo.title })}
          subtitulo={t("ps5.steam_lib.subtitulo")}
          opcoes={acoesLoja.escolhendo.libs.map((l) => ({
            caminho: l.steamDir,
            rotulo: t("ps5.steam_lib.opcao"),
            livre: l.free,
          }))}
          onEscolher={(steamDir) => {
            if (!acoesLoja.escolhendo) return
            acoesLoja.confirmarBaixar(
              acoesLoja.escolhendo.jogo,
              acoesLoja.escolhendo.info,
              steamDir,
            )
            setShowDownloads(true)
          }}
          onFechar={() => acoesLoja.setEscolhendo(null)}
        />
      )}

      {/* Procura do manifesto: passa por vários provedores e pode demorar.
          Sem este aviso, apertar A parece não fazer nada e a pessoa aperta de
          novo (o guarda de pedido do hook cobre, mas a tela precisa responder). */}
      {acoesLoja.busy && !acoesLoja.escolhendo && (
        <div className="fixed inset-0 z-[88] flex items-center justify-center bg-black/70 backdrop-blur-sm">
          <div className="rounded-2xl border border-white/10 bg-[#0b0b0d] px-8 py-6 text-center">
            <div
              className="mx-auto mb-3 h-7 w-7 animate-spin rounded-full border-2 border-white/15"
              style={{ borderTopColor: "var(--accent)" }}
            />
            <p className="text-sm text-white/80">{t("ps5.manifesto.procurando")}</p>
            <p className="mt-1 text-[12px] text-white/40">{t("ps5.manifesto.consultando")}</p>
          </div>
        </div>
      )}

      {/* Nenhum provedor tem o manifesto: resta a Steam, se a pessoa quiser */}
      {semManifesto && (
        <div
          ref={semManifestoRef}
          className="gp-scope fixed inset-0 z-[90] flex items-center justify-center bg-black/85 backdrop-blur-sm"
        >
          <div className="w-[560px] max-w-[92vw] rounded-2xl border border-white/10 bg-[#0b0b0d] p-7">
            <h2 className="text-[22px] font-semibold text-white">{semManifesto.jogo.title}</h2>
            <p className="mt-2 text-[13px] leading-relaxed text-white/55">
              {t("ps5.sem_manifesto.explicacao", { motivo: semManifesto.motivo })}
            </p>
            <div className="mt-6 flex flex-col gap-2">
              <button
                autoFocus
                onClick={() => {
                  if (!appFocusedRef.current || gameRunningRef.current || launchPendingRef.current) return
                  void launchCommand(["steam", `steam://install/${semManifesto.jogo.appid}`])
                  setToast({
                    title: t("ps5.sem_manifesto.toast", { title: semManifesto.jogo.title }),
                    visible: true,
                  })
                  setTimeout(() => setToast((t) => ({ ...t, visible: false })), 3500)
                  setSemManifesto(null)
                }}
                className="rounded-xl px-5 py-3.5 text-[13px] font-semibold text-black outline-none transition-transform focus:scale-[1.02]"
                style={{ background: "var(--accent)" }}
              >
                {t("ps5.sem_manifesto.botao")}
              </button>
              <button
                onClick={() => setSemManifesto(null)}
                className="rounded-xl border border-white/10 py-3 text-[13px] text-white/55 outline-none transition-colors hover:text-white/85"
              >
                {t("common.cancelar")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Toast do hook da loja (fila, falhas, remoções) */}
      {acoesLoja.toast && (
        <div
          onClick={() => acoesLoja.setToast("")}
          className="fixed bottom-8 right-8 z-[95] max-w-[420px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-5 py-4 text-sm text-white/90 shadow-2xl backdrop-blur-md"
        >
          {acoesLoja.toast}
        </div>
      )}

      {/* Escolha manual do trailer (mostra os vídeos do YouTube) */}
      <TrailerPicker
        game={trailerPickGame}
        onClose={() => setTrailerPickGame(null)}
        onPicked={(gameId, path) => {
          setToast({ title: t("ps5.toast.trailer_aplicado"), visible: true })
          setTimeout(() => setToast((t) => ({ ...t, visible: false })), 2500)
          if (selectedGameRef.current?.id === gameId) setTrailerUrl(path)
        }}
      />

      {/* Editar metadados (capa, fundo, logo, título, descrição) */}
      <EditMetadata
        game={editGame}
        onClose={() => setEditGame(null)}
        onSave={(patch) => editGame && _save_meta(editGame, patch)}
      />

      {/* Atualização do Arcadia: A confirma, B adia. */}
      {atualizacao.info && (
        <UpdateDialog info={atualizacao.info} console onDepois={atualizacao.dispensar} />
      )}

      {escolhendoLaunch && (
        <LaunchModeDialog
          game={escolhendoLaunch}
          active={appFocused && !gameRunning}
          onEscolher={(mode) => {
            const g = escolhendoLaunch
            setEscolhendoLaunch(null)
            abrirJogo(g, mode)
          }}
          onClose={() => setEscolhendoLaunch(null)}
        />
      )}

      {/* Toast notification */}
      <Toast visible={toast.visible} title={toast.title} />
      <AchievementToast />
    </div>
  )
}

function Toast({ visible, title }: { visible: boolean; title: string }) {
  return (
    <div
      className="fixed bottom-16 right-8 flex items-center gap-3 px-6 py-4 rounded-xl z-50"
      style={{
        background: "rgba(10,10,10,0.95)",
        border: "1px solid rgba(0,168,255,0.3)",
        backdropFilter: "blur(20px)",
        boxShadow: "0 8px 30px rgba(0,0,0,0.6), 0 0 25px rgba(0,168,255,0.2)",
        opacity: visible ? 1 : 0,
        transform: visible ? "translateY(0) scale(1)" : "translateY(12px) scale(0.96)",
        transition:
          "opacity 0.35s cubic-bezier(0.22, 1, 0.36, 1), transform 0.35s cubic-bezier(0.22, 1, 0.36, 1)",
        pointerEvents: "none",
      }}
    >
      <div
        className="w-7 h-7 rounded-full flex items-center justify-center"
        style={{
          background: "rgba(0,168,255,0.2)",
          boxShadow: "0 0 15px rgba(0,168,255,0.3)",
        }}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="var(--accent)">
          <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
        </svg>
      </div>
      <span className="text-sm text-white font-semibold drop-shadow-lg">{title}</span>
    </div>
  )
}
