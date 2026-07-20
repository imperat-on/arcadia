"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { Game } from "./types"
import { HeroSection } from "./HeroSection"
import { GameRail } from "./GameRail"
import { NewsView } from "./NewsView"
import { AchievementToasts } from "./AchievementToasts"
import { BootScreen } from "./BootScreen"
import ProfileSelect from "./ProfileSelect"
import { DownloadManager } from "./DownloadManager"
import { GameOverview } from "./GameOverview"
import { useGamepadNav } from "./useGamepadNav"
import { GameContextMenu } from "./GameContextMenu"
import { TrailerPicker } from "./TrailerPicker"
import { EditMetadata } from "./EditMetadata"
import { LibraryGrid } from "./LibraryGrid"
import { TopBar, TABS } from "./TopBar"
import { StoreConsole } from "./StoreConsole"
import { SettingsPanel } from "./SettingsPanel"
import { ProfilePage } from "./ProfilePage"
import { EditProfile } from "./EditProfile"
import type { Profile, NewsItem } from "../../global"

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

// Detecta fundo em vídeo (live wallpaper). GIF anima sozinho via background-image;
// vídeo precisa de um <video>. Ignora query string (?w=…) ao olhar a extensão.
function isVideoBg(url?: string): boolean {
  if (!url) return false
  return /\.(webm|mp4|m4v|mov)$/i.test(url.split("?")[0])
}

interface LaunchToast {
  title: string
  visible: boolean
}

export function PS5Launcher() {
  // Fora do Electron (dev no navegador) cai no mock; dentro, carrega o real.
  const [games, setGames] = useState<Game[]>(
    typeof window !== "undefined" && window.launcherAPI ? [] : MOCK_GAMES,
  )
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [toast, setToast] = useState<LaunchToast>({ title: "", visible: false })
  const [showSettings, setShowSettings] = useState(false)
  const [showProfile, setShowProfile] = useState(false)
  const [showEditProfile, setShowEditProfile] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const [profile, setProfile] = useState<Profile>({})
  const [cardScale, setCardScale] = useState(1)
  // Abas: 0 Jogos · 1 Biblioteca
  const [activeTab, setActiveTab] = useState(1) // abre em Jogos (Notícias é a aba 0)
  // Ids já iniciados (persistido): alimenta o selo "Nunca jogado".
  const [recent, setRecent] = useState<string[]>([])

  // Menu de contexto do jogo (Start) e visibilidade dos ocultos.
  const [ctxGame, setCtxGame] = useState<Game | null>(null)
  const [trailerPickGame, setTrailerPickGame] = useState<Game | null>(null)
  const [editGame, setEditGame] = useState<Game | null>(null)
  const [showHidden, setShowHidden] = useState(false)

  // Jogo em execução: trava o input do launcher e impede iniciar duas vezes.
  const [gameRunning, setGameRunning] = useState(false)
  const gameRunningRef = useRef(false)
  gameRunningRef.current = gameRunning

  // Trailer no fundo (estilo PS5): toca ao focar o jogo por um instante.
  const [trailerUrl, setTrailerUrl] = useState<string | null>(null)
  const trailerAutoRef = useRef(true)

  // Aba de notícias (RSS PT-BR).
  const [news, setNews] = useState<NewsItem[]>([])
  const [newsLoading, setNewsLoading] = useState(false)
  const newsRef = useRef<HTMLDivElement>(null)
  const gridScrollRef = useRef<HTMLDivElement>(null)
  const [overviewOpen, setOverviewOpen] = useState(false)
  const [overviewClosing, setOverviewClosing] = useState(false)
  const overviewRef = useRef<HTMLDivElement>(null)
  // Fecha com animação de saída: mantém montado ~180ms antes de desmontar.
  const closeOverview = useCallback(() => {
    setOverviewClosing(true)
    setTimeout(() => {
      setOverviewOpen(false)
      setOverviewClosing(false)
    }, 180)
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
  const [dmAtivos, setDmAtivos] = useState(0)
  const dmRef = useRef<HTMLDivElement>(null)

  // Badge da fila na TopBar (ativos = downloading/queued/paused).
  useEffect(() => {
    const conta = (items: { status?: string }[]) =>
      items.filter((i) => i.status === "downloading" || i.status === "queued" || i.status === "paused").length
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setDmAtivos(conta(q))
    })
    return window.launcherAPI?.onDmProgress((q) => setDmAtivos(conta(q)))
  }, [])

  // modalOpenRef: algum overlay/modal aberto → bloqueia TUDO (inclusive trocar aba).
  const modalOpenRef = useRef(false)
  modalOpenRef.current =
    boot ||
    perfilGate ||
    showDownloads ||
    showSettings ||
    showProfile ||
    showEditProfile ||
    menuOpen ||
    gameRunning ||
    overviewOpen ||
    Boolean(ctxGame) ||
    Boolean(editGame) ||
    Boolean(trailerPickGame)

  // uiBlockedRef: pausa a navegação de JOGOS (D-pad/A). Vale também na aba de
  // Notícias, que tem foco próprio — mas o L1/R1 (trocar aba) segue funcionando.
  const uiBlockedRef = useRef(false)
  // A Loja tem navegação própria por foco, igual às Notícias: sem isto o
  // direcional moveria a seleção do trilho de jogos por trás da loja.
  uiBlockedRef.current = modalOpenRef.current || activeTab === 0 || activeTab === 3

  // Ambas as abas mostram a biblioteca inteira; muda só a forma de exibir.
  // Jogos ocultos só aparecem com "Mostrar ocultos" ligado (menu do Select).
  const GRID_COLUMNS = 7
  const viewGames = useMemo(
    () => (showHidden ? games : games.filter((g) => !g.hidden)),
    [games, showHidden],
  )
  // Abas: 0 Notícias · 1 Jogos (trilho) · 2 Biblioteca (grade) · 3 Loja
  const newsMode = activeTab === 0
  const gridMode = activeTab === 2
  const storeMode = activeTab === 3
  const columns = GRID_COLUMNS

  const selectedGame = viewGames[selectedIndex] ?? viewGames[0] ?? null

  // O loop do gamepad não re-registra a cada troca de seleção; lê por aqui.
  const selectedGameRef = useRef<Game | null>(null)
  selectedGameRef.current = selectedGame

  // Carrega a biblioteca real (library.json) via ponte do Electron.
  const bootVideoFim = useRef(false)
  const bootLibOk = useRef(false)
  const tentarSairBoot = () => {
    // Boot em vídeo: sai quando o vídeo terminou (ou não existe) E a
    // biblioteca já carregou. A seleção de perfil entra JUNTO com o fade do
    // vídeo (crossfade) — sem corte seco entre as duas telas.
    if (!bootVideoFim.current || !bootLibOk.current) return
    setBootSaindo(true)
    setTimeout(() => setBoot(false), 900)
  }
  useEffect(() => {
    const api = window.launcherAPI
    if (!api) return
    api.getLibrary().then((g) => {
      if (Array.isArray(g) && g.length) setGames(g)
      bootLibOk.current = true
      tentarSairBoot()
    })
    api.getConfig().then((c) => {
      setProfile(c?.profile ?? {})
      if (c?.console_ui_scale) api.setZoom(c.console_ui_scale)
      trailerAutoRef.current = c?.trailer_auto !== false
      applyUiPrefs(c)
    })
    try {
      const r = JSON.parse(localStorage.getItem("gs_recent") || "[]")
      if (Array.isArray(r)) setRecent(r)
    } catch {
      /* ignore */
    }
  }, [])

  // Aplica preferências visuais (escala das capas + cor de destaque).
  function applyUiPrefs(c: { card_scale?: number; accent?: string }) {
    setCardScale(c?.card_scale ?? 1)
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
    if (showSettings || showProfile || showEditProfile || menuOpen || gameRunning || ctxGame || trailerPickGame) return
    if (boot || perfilGate) return // boot/seleção de perfil: nada de trailer
    if (gridMode || newsMode || storeMode) return // essas abas têm visual próprio
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
  }, [selId, showSettings, showProfile, showEditProfile, menuOpen, gameRunning, ctxGame, trailerPickGame, gridMode, newsMode, storeMode, boot, perfilGate])

  // Notícias: busca alinhada ao RELÓGIO (marcos de 5 min — :00,:05,:10…).
  // O slot também gira o destaque da aba (rotação a cada 5 min).
  const SLOT_5 = 5 * 60 * 1000
  const [newsSlot, setNewsSlot] = useState(() => Math.floor(Date.now() / SLOT_5))

  // Foco real da janela (eventos blur/focus do Electron — no gamescope o
  // document.hasFocus() mente). Trava gamepad e silencia trailer.
  const [appFocused, setAppFocused] = useState(() => document.hasFocus())
  const appFocusedRef = useRef(true)
  appFocusedRef.current = appFocused && document.hasFocus()
  useEffect(() => {
    return window.launcherAPI?.onAppFocus((f) => setAppFocused(f))
  }, [])
  useEffect(() => {
    if (!newsMode && !overviewOpen) return
    const api = window.launcherAPI
    if (!api) return
    let timer = 0
    const buscar = () => {
      if (!news.length) setNewsLoading(true)
      api.getNews().then((n) => {
        if (Array.isArray(n) && n.length) setNews(n)
      }).finally(() => setNewsLoading(false))
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
  useGamepadNav(newsRef, newsMode, undefined, true)
  // A loja navega POR FOCO (as capas são botões), diferente das notícias, que
  // só rolam. Por isso não usa o modo scrollOnly.
  const storeRef = useRef<HTMLDivElement>(null)
  useGamepadNav(storeRef, storeMode)

  // Navegação por controle no overview (D-pad move o foco, A ativa, B fecha).
  const overviewNavActive =
    overviewOpen && appFocused &&
    !showSettings && !showProfile && !showEditProfile && !menuOpen &&
    !ctxGame && !editGame && !trailerPickGame && !gameRunning
  useGamepadNav(overviewRef, overviewNavActive, () => closeOverview())

  // Navegação por controle na seleção de perfil (só depois do boot sair).
  useGamepadNav(perfilRef, perfilGate && !boot && appFocused)

  // Navegação por controle na tela de downloads. NÃO depende de appFocused:
  // era a única superfície cuja navegação morria com um blur enquanto seguia
  // aberta — e como o onBack vive no mesmo hook, o B também parava de fechar,
  // deixando a tela presa. O laço principal do gamepad já é travado pelo
  // modalOpenRef, que inclui showDownloads.
  useGamepadNav(dmRef, showDownloads, () => setShowDownloads(false))

  // Reseta a seleção ao trocar de aba.
  useEffect(() => {
    setSelectedIndex(0)
  }, [activeTab])

  // Escape fecha o overview.
  useEffect(() => {
    if (!overviewOpen) return
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeOverview()
    }
    window.addEventListener("keydown", h)
    return () => window.removeEventListener("keydown", h)
  }, [overviewOpen])

  // Mantém a seleção dentro dos limites quando a lista muda.
  useEffect(() => {
    setSelectedIndex((i) => Math.min(i, Math.max(0, viewGames.length - 1)))
  }, [viewGames.length])

  // Instalar OU abrir, conforme o estado do jogo. Instalar não trava o launcher
  // (não é sessão de jogo) — só abrir seta gameRunning.
  const _activate = useCallback((game?: Game | null) => {
    if (!game) return

    // Não instalado: redireciona para a instalação de cada loja.
    if (game.installed === false) {
      if (game.launcher === "epic") {
        window.launcherAPI?.dmInstall({ appid: game.id, title: game.title, cover: game.cover })
        setShowDownloads(true)
      } else if (game.launcher === "steam") {
        // rungameid NÃO instala; steam://install abre o diálogo de instalação.
        const appid = String(game.id).replace(/^steam:/, "")
        window.launcherAPI?.launch(["steam", `steam://install/${appid}`])
        setToast({ title: `Instalando ${game.title} pela Steam…`, visible: true })
        setTimeout(() => setToast((t) => ({ ...t, visible: false })), 3500)
      } else {
        // heroic/lutris: cai no launch_cmd (o próprio runner trata).
        window.launcherAPI?.launch(game.launch_cmd)
      }
      return
    }

    // Instalado: abre o jogo.
    if (gameRunningRef.current) return // evita abrir duas vezes
    setGameRunning(true)
    window.launcherAPI?.launch(game.launch_cmd)
    // Registra em "Recentes".
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
    setTimeout(() => setToast((t) => ({ ...t, visible: false })), 3000)
  }, [])

  const _launch_selected = useCallback(() => {
    _activate(viewGames[selectedIndex])
  }, [viewGames, selectedIndex, _activate])

  // Enquanto o jogo roda, o launcher fica travado. Como o jogo é lançado
  // solto (steam://… retorna na hora), quem diz que voltamos é o foco da
  // janela: o jogo rouba o foco ao abrir e devolve ao fechar.
  useEffect(() => {
    if (!gameRunning) return
    let stoleFocus = false
    const onBlur = () => {
      stoleFocus = true
    }
    const onFocus = () => {
      if (stoleFocus) setGameRunning(false)
    }
    window.addEventListener("blur", onBlur)
    window.addEventListener("focus", onFocus)
    // Se nada tomou o foco, o jogo provavelmente nem abriu: destrava sozinho.
    const fallback = setTimeout(() => {
      if (!stoleFocus) setGameRunning(false)
    }, 20000)
    return () => {
      window.removeEventListener("blur", onBlur)
      window.removeEventListener("focus", onFocus)
      clearTimeout(fallback)
    }
  }, [gameRunning])

  // Salva metadados editados à mão. Mesmo caminho do ocultar: overrides.json.
  const _save_meta = useCallback(
    (game: Game, patch: Record<string, unknown>) => {
      if (!Object.keys(patch).length) return // nada mudou
      const api = window.launcherAPI
      if (api) {
        api.setOverride(game.id, patch).then((lib) => {
          if (Array.isArray(lib)) setGames(lib)
        })
      } else {
        setGames((prev) =>
          prev.map((g) => (g.id === game.id ? { ...g, ...patch } : g)),
        )
      }
      setToast({ title: `${game.title} — metadados salvos`, visible: true })
      setTimeout(() => setToast((t) => ({ ...t, visible: false })), 2500)
    },
    [],
  )

  // Oculta/reexibe um jogo. Persiste em overrides.json e sobrevive ao re-scan.
  const _toggle_hidden = useCallback((game: Game) => {
    const nowHidden = !game.hidden
    const api = window.launcherAPI
    if (api) {
      api.setOverride(game.id, { hidden: nowHidden || null }).then((lib) => {
        if (Array.isArray(lib)) setGames(lib)
      })
    } else {
      // Modo navegador (mock): reflete só na memória.
      setGames((prev) =>
        prev.map((g) => (g.id === game.id ? { ...g, hidden: nowHidden } : g)),
      )
    }
    setToast({
      title: nowHidden ? `${game.title} — oculto` : `${game.title} — reexibido`,
      visible: true,
    })
    setTimeout(() => setToast((t) => ({ ...t, visible: false })), 2500)
  }, [])

  const _refresh_library = useCallback(() => {
    const api = window.launcherAPI
    if (api) {
      api.refresh().then((g) => {
        if (Array.isArray(g)) setGames(g)
      })
    }
    setToast({ title: "Biblioteca atualizada!", visible: true })
    setTimeout(() => setToast((t) => ({ ...t, visible: false })), 2500)
  }, [])

  // Keyboard / gamepad navigation
  useEffect(() => {
    let lastNav = 0
    const COOLDOWN = 160

    const N = viewGames.length
    const step = (d: number) =>
      setSelectedIndex((i) => Math.max(0, Math.min(N - 1, i + d)))

    const handleKey = (e: KeyboardEvent) => {
      if (uiBlockedRef.current) return // painel de config aberto
      const now = Date.now()
      if (now - lastNav < COOLDOWN) return

      if (e.key === "ArrowLeft") { lastNav = now; step(-1) }
      else if (e.key === "ArrowRight") { lastNav = now; step(1) }
      else if (e.key === "ArrowUp") { lastNav = now; if (gridMode) step(-columns) }
      else if (e.key === "ArrowDown") {
        lastNav = now
        if (gridMode) step(columns)
        else if (selectedGameRef.current) setOverviewOpen(true) // trilho: abre overview
      }
      else if (e.key === "Enter" || e.key === " ") _launch_selected()
      else if (e.key === "F5" || e.key === "r") _refresh_library()
    }
    window.addEventListener("keydown", handleKey)
    return () => window.removeEventListener("keydown", handleKey)
  }, [viewGames.length, gridMode, columns, _launch_selected, _refresh_library])

  // Navegação por controle (Gamepad API): D-pad/analógico, A=jogar, Start=atualizar.
  useEffect(() => {
    let raf = 0
    let prev: boolean[] = []
    let restAxes: number[] | null = null
    let sx = 0, sy = 0 // direção estável (x,y)
    let cx = 0, cy = 0 // candidata
    let candSince = 0
    let holdStart = 0
    let lastRepeat = 0
    let lastStep = 0
    let scrollVel = 0 // inércia do scroll do analógico direito (Biblioteca)
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
        else if (near(-0.714)) { x = 1; y = -1 }
        else if (near(-0.428)) x = 1
        else if (near(-0.142)) { x = 1; y = 1 }
        else if (near(0.142)) y = 1
        else if (near(0.428)) { x = -1; y = 1 }
        else if (near(0.714)) x = -1
        else if (near(1)) { x = -1; y = -1 }
      }
      return [x, y]
    }

    const N = viewGames.length
    const move = (dx: number, dy: number) => {
      // Aba Jogos (trilho): para baixo abre o overview do jogo selecionado.
      if (dy > 0 && !gridMode) {
        if (selectedGameRef.current) setOverviewOpen(true)
        return
      }
      // Rail: só horizontal. Grade: horizontal ±1, vertical ±columns.
      let delta = 0
      if (dx !== 0) delta = dx
      else if (dy !== 0 && gridMode) delta = dy * columns
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
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      if (gp) {
        const now = Date.now()
        const primed = prev.length > 0
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
          // Analógico DIREITO: rolagem suave da grade (estilo navegador),
          // igual à aba de Notícias. Eixo detectado varrendo além do esquerdo.
          if (gridMode && restAxes) {
            let sry = 0
            for (let ai = 2; ai < gp.axes.length; ai++) {
              const v = (gp.axes[ai] ?? 0) - (restAxes[ai] ?? 0)
              if (Math.abs(v) > Math.abs(sry)) sry = v
            }
            const target = Math.abs(sry) > 0.15 ? Math.sign(sry) * sry * sry * 46 : 0
            scrollVel += (target - scrollVel) * 0.25
            const el = gridScrollRef.current
            if (el && Math.abs(scrollVel) > 0.05) el.scrollTop += scrollVel
          }
          const [rx, ry] = direction(gp)
          if (rx !== cx || ry !== cy) {
            cx = rx
            cy = ry
            candSince = now
          }
          if (now - candSince >= DEBOUNCE && (sx !== cx || sy !== cy)) {
            const wasNeutral = sx === 0 && sy === 0
            sx = cx
            sy = cy
            if ((sx || sy) && wasNeutral && now - lastStep >= MIN_GAP) {
              move(sx, sy)
              lastStep = now
              holdStart = now
              lastRepeat = now
            }
          }
          if (
            (sx || sy) &&
            now - holdStart > INITIAL_DELAY &&
            now - lastRepeat > REPEAT
          ) {
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
          sx = 0; sy = 0; cx = 0; cy = 0
        }
        prev = gp.buttons.map((b) => b.pressed)
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [viewGames.length, gridMode, columns, _launch_selected, _refresh_library])

  return (
    <div
      className={`relative flex flex-col min-h-screen select-none overflow-hidden ${posLogin ? "pos-login home-reveal" : ""}`}
      style={{ background: "#000000" }}
    >
      {/* Toasts de conquista (estilo PS5), por cima de tudo */}
      <AchievementToasts />

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
      {/* Fundo: tema do jogo em TELA CHEIA (crossfade ao trocar).
          Vídeo (.webm/.mp4) vira live wallpaper; imagem/GIF via background.
          Na aba de Notícias o fundo é preto (o NewsView tem visual próprio). */}
      {newsMode || storeMode ? (
        <div className="absolute inset-0" style={{ background: "#000000" }} />
      ) : selectedGame?.hero && isVideoBg(selectedGame.hero) ? (
        <video
          key={selectedGame.id}
          className="absolute inset-0 w-full h-full object-cover animate-bg-fade"
          src={selectedGame.hero}
          autoPlay
          loop
          muted
          playsInline
        />
      ) : selectedGame?.hero ? (
        <div
          key={selectedGame.id}
          className="absolute inset-0 animate-bg-fade"
          style={{
            backgroundImage: `url(${selectedGame.hero})`,
            backgroundSize: "cover",
            backgroundPosition: "top center",
          }}
        />
      ) : (
        <div
          className="absolute inset-0"
          style={{ background: "linear-gradient(135deg, #000000, #161619)" }}
        />
      )}

      {/* Trailer do jogo por cima do fundo (estilo PS5), com fade de entrada.
          Só toca com a janela focada e sem jogo rodando — desmontar o <video>
          corta o som na hora (gamescope/alt-tab). */}
      {trailerUrl && !gridMode && appFocused && !gameRunning && !boot && !perfilGate && (
        <video
          key={trailerUrl}
          className="absolute inset-0 w-full h-full object-cover animate-bg-fade"
          src={trailerUrl}
          autoPlay
          loop
          playsInline
          ref={(el) => {
            if (el) el.volume = 0.4
          }}
        />
      )}

      {/* Escurecimento p/ contraste: forte embaixo (trilho) e à esquerda (texto) */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            "linear-gradient(to top, #000000 2%, rgba(0,0,0,0.86) 26%, rgba(0,0,0,0.42) 60%, rgba(0,0,0,0.22) 100%)",
        }}
      />
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            "linear-gradient(to right, rgba(0,0,0,0.9) 0%, rgba(0,0,0,0.45) 42%, transparent 72%)",
        }}
      />

      {/* Gradiente sutil no topo, p/ legibilidade da barra transparente */}
      <div
        className="absolute top-0 inset-x-0 h-28 z-20 pointer-events-none"
        style={{
          background: "linear-gradient(to bottom, rgba(3,5,10,0.5), transparent)",
        }}
      />

      {/* Top bar transparente, flutuando sobre o fundo */}
      <div className="absolute top-0 inset-x-0 z-30">
        <TopBar
          profile={profile}
          activeTab={activeTab}
          onTab={setActiveTab}
          onRefresh={_refresh_library}
          onOpenSettings={() => setShowSettings(true)}
          onOpenProfile={() => setShowProfile(true)}
          menuOpen={menuOpen}
          onToggleMenu={() => setMenuOpen((v) => !v)}
          onCloseMenu={() => setMenuOpen(false)}
          showHidden={showHidden}
          onToggleShowHidden={() => setShowHidden((v) => !v)}
          downloadsActive={dmAtivos}
          onOpenDownloads={() => setShowDownloads(true)}
        />
      </div>

      {/* Painel de configurações (chave da Steam API etc.) */}
      <SettingsPanel
        open={showSettings}
        onClose={() => setShowSettings(false)}
        onSaved={_refresh_library}
        onUiChange={applyUiPrefs}
      />

      {/* Página de perfil (estilo Steam) */}
      <ProfilePage
        open={showProfile}
        navActive={!showEditProfile}
        profile={profile}
        games={games}
        onClose={() => setShowProfile(false)}
        onEdit={() => setShowEditProfile(true)}
      />

      {/* Editar perfil (Geral / Avatar / Plano de fundo) */}
      <EditProfile
        open={showEditProfile}
        profile={profile}
        games={games}
        onClose={() => setShowEditProfile(false)}
        onChange={setProfile}
      />

      {/* Overview do jogo selecionado (seta para baixo na aba Jogos) */}
      {overviewOpen && selectedGame && (
        <GameOverview
          ref={overviewRef}
          game={selectedGame}
          news={news}
          appFocused={appFocused}
          closing={overviewClosing}
          onClose={() => closeOverview()}
          onLaunch={(g) => {
            // Instala se não instalado, abre se instalado (mesma regra do resto).
            closeOverview()
            _activate(g)
          }}
          onOpenNews={(url) => window.launcherAPI?.openExternal(url)}
        />
      )}

      {/* Seleção de perfil (aparece depois do vídeo de boot, em crossfade) */}
      {perfilGate && (
        <div ref={perfilRef} className={`gp-scope fixed inset-0 z-[75] ${perfilSaindo ? "perfil-gate-out" : "perfil-gate-in"}`}>
          <ProfileSelect
            profiles={[{ name: profile?.name || "Jogador", avatar: profile?.avatar, background: profile?.background, owner: true }]}
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
        className={`${boot || perfilGate ? "" : "tab-in "}${newsMode || gridMode || storeMode ? "relative z-10 flex h-screen flex-col overflow-hidden" : "relative z-10 flex flex-col min-h-screen"}`}
      >
        {storeMode ? (
          <div className="flex-1 min-h-0 pt-20">
            <StoreConsole ref={storeRef} games={viewGames} ativo={appFocused && !gameRunning} />
          </div>
        ) : newsMode ? (
          <div className="flex-1 min-h-0 pt-20">
            <NewsView
              ref={newsRef}
              news={news}
              rotacao={newsSlot}
              loading={newsLoading}
              onOpen={(url) => window.launcherAPI?.openExternal(url)}
            />
          </div>
        ) : gridMode ? (
          <>
            {/* Grade completa (Biblioteca) */}
            <div className="pt-6" />
            <LibraryGrid
              games={viewGames}
              selectedIndex={selectedIndex}
              columns={columns}
              onSelect={setSelectedIndex}
              onLaunch={_activate}
              emptyMessage="Sua biblioteca está vazia."
              scrollRef={gridScrollRef}
            />
          </>
        ) : (
          <>
            {/* Espaço da barra superior */}
            <div className="pt-20" />

            {/* Trilho de capas no topo */}
            {viewGames.length > 0 ? (
              <GameRail
                games={viewGames}
                selectedIndex={selectedIndex}
                cardScale={cardScale}
                onSelect={setSelectedIndex}
                onLaunch={_activate}
              />
            ) : (
              <div className="px-10 py-10 text-[#8a93a6]">
                Sua biblioteca está vazia.
              </div>
            )}

            {/* Hero embaixo à esquerda, com as ações */}
            <HeroSection
              game={selectedGame}
              onLaunch={_launch_selected}
              onMore={() => setCtxGame(selectedGame)}
            />

            {/* Selo "Nunca jogado" (canto inferior direito) */}
            {selectedGame && !recent.includes(selectedGame.id) && (
              <div
                className="absolute bottom-16 right-10 flex items-center gap-2 px-4 py-2.5 rounded-lg text-white text-[15px] font-medium"
                style={{
                  background: "rgba(0,0,0,0.55)",
                  backdropFilter: "blur(8px)",
                }}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 2a10 10 0 100 20 10 10 0 000-20zm0 18a8 8 0 110-16 8 8 0 010 16zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z" />
                </svg>
                Nunca jogado
              </div>
            )}
          </>
        )}
      </div>

      {/* Opções do jogo (Start ou botão "...") */}
      <GameContextMenu
        game={ctxGame}
        onClose={() => setCtxGame(null)}
        onLaunch={() => _activate(ctxGame)}
        onEditMeta={() => setEditGame(ctxGame)}
        onToggleHidden={() => ctxGame && _toggle_hidden(ctxGame)}
        onDownloadTrailer={() => setTrailerPickGame(ctxGame)}
      />

      {/* Downloads (fila Epic) */}
      {showDownloads && (
        <DownloadManager ref={dmRef} onClose={() => setShowDownloads(false)} />
      )}

      {/* Escolha manual do trailer (mostra os vídeos do YouTube) */}
      <TrailerPicker
        game={trailerPickGame}
        onClose={() => setTrailerPickGame(null)}
        onPicked={(gameId, path) => {
          setToast({ title: "Trailer baixado e aplicado!", visible: true })
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

      {/* Toast notification */}
      <Toast visible={toast.visible} title={toast.title} />
    </div>
  )
}

function Toast({ visible, title }: { visible: boolean; title: string }) {
  return (
    <div
      className="fixed bottom-16 right-8 flex items-center gap-3 px-5 py-3 rounded-xl z-50"
      style={{
        background: "rgba(18,25,46,0.95)",
        border: "1px solid rgba(0,168,255,0.3)",
        backdropFilter: "blur(20px)",
        boxShadow: "0 8px 32px rgba(0,0,0,0.5), 0 0 20px rgba(0,114,206,0.2)",
        opacity: visible ? 1 : 0,
        transform: visible ? "translateY(0)" : "translateY(12px)",
        transition: "opacity 0.3s, transform 0.3s cubic-bezier(0.22,1,0.36,1)",
        pointerEvents: "none",
      }}
    >
      <div
        className="w-6 h-6 rounded-full flex items-center justify-center"
        style={{ background: "rgba(0,114,206,0.3)" }}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="#00a8ff">
          <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
        </svg>
      </div>
      <span className="text-sm text-white font-medium">{title}</span>
    </div>
  )
}
