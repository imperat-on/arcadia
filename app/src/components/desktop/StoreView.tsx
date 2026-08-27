"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useStoreActions, StoreGamePage, type ItemLoja, CartaoLoja, useI18n } from "./storeShared"
import { GameSettingsDialog } from "./GameSettingsDialog"
import { MetodoDownloadDialog } from "./MetodoDownloadDialog"
import { EscolhaDownloadDialog } from "../DepotPicker"
import type { Game } from "../ps5-launcher/types"
import { StoreKeyboard, type SugestaoLoja } from "../ps5-launcher/StoreKeyboard"
import { RetroStoreView } from "./RetroStoreView"

// Aba Lojas: busca no catálogo (Hubcap + Steam). Página de detalhe estilo
// Hydra (StoreGamePage) abre ao clicar no card.
//
// bigPicture=true: modo Big Picture. Habilita StoreKeyboard (teclado na tela),
// avisa o host quando abre/fecha overlay (StoreGamePage / dialogs) e expõe
// atalhos de gamepad (voltar / abrirTeclado) via onAtalhos.

interface StoreAtalhos {
  voltar: () => boolean
  abrirTeclado: () => void
}

export function StoreView({
  games = [],
  bigPicture = false,
  ativo = true,
  onOverlay,
  onAtalhos,
  onOpenDownloads,
  onLaunchGame,
}: {
  games?: Game[]
  bigPicture?: boolean
  ativo?: boolean
  onOverlay?: (aberto: boolean) => void
  onAtalhos?: (a: StoreAtalhos) => void
  onOpenDownloads?: () => void
  onLaunchGame?: (game: Game) => void
}) {
  const { t } = useI18n()
  const {
    bloqueados,
    jaAdicionados,
    escolhendo,
    setEscolhendo,
    metodo,
    setMetodo,
    busy: acaoBusy,
    toast,
    setToast,
    baixar,
    baixarDepot,
    confirmarTorrent,
    confirmarBaixar,
    adicionar,
    remover,
    slsAtivo,
  } = useStoreActions(games)

  const [aba, setAba] = useState<"steam" | "retro">("steam")
  const [retroDetailOpen, setRetroDetailOpen] = useState(false)
  const [retroDownloadOpen, setRetroDownloadOpen] = useState(false)
  const [retroBackRequest, setRetroBackRequest] = useState(0)
  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<ItemLoja[] | null>(null)
  const [sugestoes, setSugestoes] = useState<{ appid: string; title: string; cover?: string }[]>([])
  const [sugSel, setSugSel] = useState(-1)
  const [buscando, setBuscando] = useState(false)
  const [msg, setMsg] = useState("")
  const [pagina, setPagina] = useState<ItemLoja | null>(null)
  const [configGame, setConfigGame] = useState<Game | null>(null)
  const [tecladoAberto, setTecladoAberto] = useState(false)
  const esqueletos = useMemo(() => Array.from({ length: 8 }, (_, i) => i), [])
  const containerLojaRef = useRef<HTMLDivElement>(null)
  const cursorGamepadRef = useRef<HTMLDivElement>(null)
  const cursorPosRef = useRef({ x: 0, y: 0 })
  const cursorVisivelRef = useRef(false)

  // Catálogo navegável (sem busca): lista "Em alta" paginada via store:recent.
  const POR_PAGINA = 24
  const [catalogo, setCatalogo] = useState<ItemLoja[]>([])
  const [catTotal, setCatTotal] = useState(0)
  const [catPag, setCatPag] = useState(0) // 0-based
  const [catCarregando, setCatCarregando] = useState(false)
  const gerCat = useRef(0)

  useEffect(() => {
    const meu = ++gerCat.current
    setCatCarregando(true)
    window.launcherAPI?.storeRecent?.("all", POR_PAGINA, catPag * POR_PAGINA).then((r) => {
      if (meu !== gerCat.current) return
      setCatCarregando(false)
      if (r?.ok) {
        setCatalogo(r.jogos || [])
        setCatTotal(r.total || 0)
      }
    })
  }, [catPag])

  const gerSug = useRef(0)
  const gerBusca = useRef(0)
  const ignorarSug = useRef(false)
  const caixaRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    window.launcherAPI?.storeWarm?.()
  }, [])

  const gamepadSuspenso = Boolean(
    configGame || metodo || escolhendo || tecladoAberto || retroDownloadOpen,
  )

  useEffect(() => {
    if (!bigPicture || !ativo || gamepadSuspenso) return

    let raf = 0
    let ultimoFrame = 0
    let repouso: number[] | null = null
    const deadzone = 0.18
    const seletorClicavel =
      "button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [role='button'], [tabindex]:not([tabindex='-1'])"

    const ocultarCursor = () => {
      cursorVisivelRef.current = false
      containerLojaRef.current?.classList.remove("cursor-none")
      if (cursorGamepadRef.current) cursorGamepadRef.current.style.display = "none"
    }

    const superficieCursor = () => {
      const container = containerLojaRef.current
      if (!container) return null
      const superficies = Array.from(
        container.querySelectorAll<HTMLElement>("[data-gamepad-cursor-surface]"),
      )
      for (let i = superficies.length - 1; i >= 0; i--) {
        const superficie = superficies[i]
        const rect = superficie.getBoundingClientRect()
        if (rect.width > 0 && rect.height > 0) return superficie
      }
      return container
    }

    const alvoNoCursor = () => {
      const { x, y } = cursorPosRef.current
      const elemento = document.elementFromPoint(x, y)
      const alvo = elemento?.closest<HTMLElement>(seletorClicavel) || null
      return alvo && superficieCursor()?.contains(alvo) ? alvo : null
    }

    const atualizarCursor = () => {
      const cursor = cursorGamepadRef.current
      if (!cursor) return
      const clicavel = Boolean(alvoNoCursor())
      cursor.style.width = clicavel ? "28px" : "20px"
      cursor.style.height = clicavel ? "28px" : "20px"
      cursor.style.background = clicavel ? "var(--accent)" : "rgba(255, 255, 255, 0.22)"
      cursor.style.filter = clicavel
        ? "drop-shadow(0 0 7px var(--accent)) drop-shadow(0 2px 2px rgba(0,0,0,.8))"
        : "drop-shadow(0 2px 2px rgba(0,0,0,.85))"
    }

    const confirmarCursor = (evento: Event) => {
      if (!cursorVisivelRef.current) return
      evento.preventDefault()
      const alvo = alvoNoCursor()
      if (!alvo) return
      alvo.focus({ preventScroll: true })
      alvo.click()
    }

    const normalizarEixo = (valor: number) => {
      const absoluto = Math.abs(valor)
      if (absoluto <= deadzone) return 0
      const normalizado = (absoluto - deadzone) / (1 - deadzone)
      return Math.sign(valor) * normalizado * normalizado
    }

    const loop = (agora: number) => {
      const container = containerLojaRef.current
      const gamepad = Array.from(navigator.getGamepads?.() || []).find(
        (controle): controle is Gamepad => Boolean(controle),
      )
      const pausado = !document.hasFocus()
      const deltaTempo = ultimoFrame ? Math.min(0.05, (agora - ultimoFrame) / 1000) : 1 / 60
      ultimoFrame = agora

      if (!pausado && container && gamepad) {
        if (!repouso) repouso = Array.from(gamepad.axes)
        const cursorX = normalizarEixo((gamepad.axes[0] ?? 0) - (repouso[0] ?? 0))
        const cursorY = normalizarEixo((gamepad.axes[1] ?? 0) - (repouso[1] ?? 0))
        if (cursorX || cursorY) {
          const limite = (superficieCursor() || container).getBoundingClientRect()
          if (!cursorVisivelRef.current) {
            cursorPosRef.current = {
              x: limite.left + limite.width / 2,
              y: limite.top + limite.height / 2,
            }
          }
          cursorPosRef.current.x = Math.max(
            limite.left + 10,
            Math.min(limite.right - 10, cursorPosRef.current.x + cursorX * 1200 * deltaTempo),
          )
          cursorPosRef.current.y = Math.max(
            limite.top + 10,
            Math.min(limite.bottom - 10, cursorPosRef.current.y + cursorY * 1200 * deltaTempo),
          )
          cursorVisivelRef.current = true
          container.classList.add("cursor-none")
          if (cursorGamepadRef.current) {
            cursorGamepadRef.current.style.display = "block"
            cursorGamepadRef.current.style.left = `${cursorPosRef.current.x}px`
            cursorGamepadRef.current.style.top = `${cursorPosRef.current.y}px`
          }
          atualizarCursor()
        } else if (cursorVisivelRef.current) {
          atualizarCursor()
        }

        if (gamepad.buttons.slice(12, 16).some((botao) => botao?.pressed)) ocultarCursor()
      } else if (pausado) {
        ocultarCursor()
      }

      raf = requestAnimationFrame(loop)
    }

    window.addEventListener("mousemove", ocultarCursor)
    window.addEventListener("arcadia:gamepad-confirm", confirmarCursor)
    raf = requestAnimationFrame(loop)
    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener("mousemove", ocultarCursor)
      window.removeEventListener("arcadia:gamepad-confirm", confirmarCursor)
      ocultarCursor()
    }
  }, [bigPicture, ativo, gamepadSuspenso])

  // Big Picture: avisa o host que um overlay próprio está no ar (página do
  // jogo, teclado, dialog de config/método/escolha), pra ele suspender o laço
  // de gamepad do launcher. Sem isso, dois laços disputam o D-pad e o B fecha
  // a página junto com a loja.
  useEffect(() => {
    if (!bigPicture) return
    onOverlay?.(ativo && gamepadSuspenso)
  }, [bigPicture, ativo, gamepadSuspenso, onOverlay])

  useEffect(() => {
    if (ignorarSug.current) {
      ignorarSug.current = false
      return
    }
    const q = busca.trim()
    const meu = ++gerSug.current
    if (q.length < 2) {
      setSugestoes([])
      setSugSel(-1)
      return
    }
    const timer = setTimeout(async () => {
      const r = await window.launcherAPI?.storeSuggest(q)
      if (meu !== gerSug.current) return
      setSugestoes(r?.ok ? r.jogos || [] : [])
      setSugSel(-1)
    }, 120)
    return () => clearTimeout(timer)
  }, [busca])

  const fecharSugestoes = useCallback(() => {
    gerSug.current++
    setSugestoes([])
    setSugSel(-1)
  }, [])

  useEffect(() => {
    if (!sugestoes.length) return
    const fora = (e: MouseEvent) => {
      if (!caixaRef.current?.contains(e.target as Node)) fecharSugestoes()
    }
    document.addEventListener("mousedown", fora)
    return () => document.removeEventListener("mousedown", fora)
  }, [sugestoes.length, fecharSugestoes])

  const pesquisar = useCallback(
    async (termo?: string) => {
      const q = (termo ?? busca).trim()
      if (!q) return
      if (termo !== undefined && termo !== busca) {
        ignorarSug.current = true
        setBusca(termo)
      }
      fecharSugestoes()
      const meu = ++gerBusca.current
      setBuscando(true)
      setMsg("")
      const r = await window.launcherAPI?.storeSearch(q)
      if (meu !== gerBusca.current) return
      setBuscando(false)
      if (!r?.ok) {
        setResultados([])
        setMsg(r?.error || t("store.busca_falhou"))
        return
      }
      setResultados(r.jogos || [])
      setMsg(r.jogos?.length ? "" : t("store.nada_encontrado"))
    },
    [busca, fecharSugestoes, t],
  )

  const limpar = useCallback(() => {
    gerBusca.current++
    ignorarSug.current = true
    setBusca("")
    setResultados(null)
    setMsg("")
    setBuscando(false)
    fecharSugestoes()
    inputRef.current?.focus()
  }, [fecharSugestoes])

  // Big Picture: registra os atalhos que o laço do host consome. `voltar` tem
  // pilha própria (página aberta → fecha; senão pede pro host sair da loja).
  // `abrirTeclado` é o Y do controle.
  useEffect(() => {
    if (!bigPicture) return
    onAtalhos?.({
      voltar: () => {
        if (tecladoAberto) {
          setTecladoAberto(false)
          return true
        }
        if (aba === "retro") {
          if (retroDetailOpen) {
            setRetroBackRequest((value) => value + 1)
            return true
          }
          setAba("steam")
          return true
        }
        if (pagina) {
          setPagina(null)
          return true
        }
        if (configGame) {
          setConfigGame(null)
          return true
        }
        if (metodo) {
          setMetodo(null)
          return true
        }
        if (escolhendo) {
          setEscolhendo(null)
          return true
        }
        if (busca) {
          limpar()
          return true
        }
        return false
      },
      abrirTeclado: () => {
        if (aba === "steam") setTecladoAberto(true)
      },
    })
  }, [
    bigPicture,
    onAtalhos,
    aba,
    retroDetailOpen,
    tecladoAberto,
    pagina,
    configGame,
    metodo,
    escolhendo,
    busca,
    limpar,
    setMetodo,
    setEscolhendo,
  ])

  // Sugestões pro teclado no formato SugestaoLoja (com img/preco quando existirem).
  const sugestoesKB = useMemo<SugestaoLoja[]>(() => {
    return sugestoes.map((s) => ({
      appid: s.appid,
      title: s.title,
      img: s.cover,
    }))
  }, [sugestoes])

  const aoTeclar = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && sugestoes.length) {
      e.preventDefault()
      setSugSel((i) => (i + 1) % sugestoes.length)
    } else if (e.key === "ArrowUp" && sugestoes.length) {
      e.preventDefault()
      setSugSel((i) => (i <= 0 ? sugestoes.length : i) - 1)
    } else if (e.key === "Escape") {
      if (sugestoes.length) fecharSugestoes()
      else limpar()
    } else if (e.key === "Enter") {
      pesquisar(sugSel >= 0 ? sugestoes[sugSel]?.title : undefined)
    }
  }

  const buscou = resultados !== null
  const grade = buscou ? resultados : []
  const carregandoGrade = buscando
  const gamePorAppid = useMemo(() => {
    const m = new Map<string, Game>()
    for (const g of games) {
      const appid = String(g.id).replace(/^steam:/, "")
      if (appid) m.set(appid, g)
    }
    return m
  }, [games])

  if (pagina) {
    const naBiblioteca = bloqueados.has(pagina.appid)
    const game = naBiblioteca ? gamePorAppid.get(pagina.appid) : undefined
    return (
      <>
        <StoreGamePage
          jogo={pagina}
          game={game}
          onClose={() => setPagina(null)}
          onBaixar={() => baixar(pagina)}
          onAdicionar={() => adicionar(pagina)}
          onRemover={() => remover(pagina)}
          onConfig={game ? () => setConfigGame(game) : undefined}
          onJogar={game && game.installed !== false && onLaunchGame ? () => onLaunchGame(game) : undefined}
          naBiblioteca={naBiblioteca}
          ocupado={acaoBusy !== ""}
          slssteamAtivo={slsAtivo}
          bigPicture={bigPicture}
        />
        {metodo && (
          <MetodoDownloadDialog
            jogo={metodo.jogo}
            opcoes={metodo.opcoes}
            onDepot={() => { const j = metodo.jogo; setMetodo(null); baixarDepot(j) }}
            onTorrent={(magnet, pasta) => confirmarTorrent(metodo.jogo, magnet, pasta)}
            onClose={() => setMetodo(null)}
            depotDisponivel={slsAtivo}
          />
        )}
        {escolhendo && (
          <EscolhaDownloadDialog
            escolhendo={escolhendo}
            onCancel={() => setEscolhendo(null)}
            onConfirm={(steamDir, sel) => confirmarBaixar(escolhendo.jogo, escolhendo.info, steamDir, sel)}
            titulo={t("store.instalar_em", { title: escolhendo.jogo.title })}
          />
        )}
        {configGame && <GameSettingsDialog game={configGame} onClose={() => setConfigGame(null)} />}
      </>
    )
  }

  if (aba === "retro") {
    return (
      <div
        ref={containerLojaRef}
        data-gamepad-scroll
        className={`h-full ${retroDetailOpen ? "overflow-hidden" : "overflow-y-auto px-8 py-6"} ${bigPicture ? "retro-console-store" : ""}`}
      >
        {bigPicture && (
          <div
            ref={cursorGamepadRef}
            aria-hidden="true"
            className="pointer-events-none fixed z-[9999] hidden h-5 w-5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white transition-[width,height,background-color,filter] duration-75"
          />
        )}
        {!retroDetailOpen && <StoreTabs
          aba={aba}
          onAba={(next) => {
            setAba(next)
            if (next !== "retro") setRetroDetailOpen(false)
          }}
          t={t}
        />}
        <RetroStoreView
          backRequest={retroBackRequest}
          onDetailChange={setRetroDetailOpen}
          onDownloadDialogChange={setRetroDownloadOpen}
          onOpenDownloads={onOpenDownloads}
          onLaunchGame={onLaunchGame}
        />
      </div>
    )
  }

  return (
    <div ref={containerLojaRef} data-gamepad-scroll className={`h-full overflow-y-auto px-8 py-6 ${bigPicture ? "retro-console-store" : ""}`}>
      {bigPicture && (
        <div
          ref={cursorGamepadRef}
          aria-hidden="true"
          className="pointer-events-none fixed z-[9999] hidden h-5 w-5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white transition-[width,height,background-color,filter] duration-75"
        />
      )}
      <StoreTabs aba={aba} onAba={setAba} t={t} />
      <h1 className="ui-title mb-1">{t("store.titulo")}</h1>
      <p className="ui-subtitle mb-6">{t("store.descricao")}</p>

      <div className="mb-4 flex max-w-[860px] gap-2">
        <div ref={caixaRef} className="relative flex-1">
          <input
            ref={inputRef}
            value={busca}
            onChange={(e) => {
              ignorarSug.current = false
              setBusca(e.target.value)
            }}
            onKeyDown={aoTeclar}
            onClick={() => {
              if (!bigPicture) return
              inputRef.current?.blur()
              setTecladoAberto(true)
            }}
            placeholder={t("store.buscar_placeholder")}
            spellCheck={false}
            className="ui-input w-full py-2.5 pl-3.5 pr-9 text-[13px] placeholder:text-white/25"
          />
          {busca && (
            <button
              onClick={limpar}
              title={t("common.cancelar")}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-white/35 transition-colors hover:text-white"
            >
              <svg
                width="13"
                height="13"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
              >
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          )}
          {sugestoes.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-white/10 bg-[#15181d] shadow-2xl shadow-black/60">
              {sugestoes.map((s, i) => (
                <button
                  key={s.appid}
                  onMouseDown={(e) => {
                    e.preventDefault()
                    pesquisar(s.title)
                  }}
                  onMouseEnter={() => setSugSel(i)}
                  className={`flex w-full items-center gap-3 px-2.5 py-2 text-left transition-colors ${i === sugSel ? "bg-white/[0.09] text-white" : "text-white/80 hover:bg-white/[0.07] hover:text-white"}`}
                >
                  {s.cover ? (
                    <img
                      src={s.cover}
                      alt=""
                      className="h-9 w-9 shrink-0 rounded-md object-cover ring-1 ring-white/10"
                      loading="lazy"
                      onError={(e) => {
                        ;(e.target as HTMLImageElement).style.display = "none"
                      }}
                    />
                  ) : (
                    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-white/[0.05] text-sm font-bold text-white/40">
                      {s.title[0]?.toUpperCase()}
                    </div>
                  )}
                  <span className="min-w-0 truncate text-[13px]">
                    <TermoDestacado texto={s.title} termo={busca.trim()} />
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
        <button
          onClick={() => pesquisar()}
          disabled={buscando || !busca.trim()}
          className="ui-btn-primary rounded-lg px-4 py-2.5 text-[12px] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {buscando ? t("store.buscando") : t("store.buscar")}
        </button>
      </div>

      {msg && <p className="mb-4 text-[12px] text-white/55">{msg}</p>}

      {buscou && (
        <h2 className="mb-3 text-sm font-medium text-white/60">
          {t("store.resultados_count", { count: grade.length })}
        </h2>
      )}

      {(() => {
        const mostraCat = !buscou
        const itens = mostraCat ? catalogo : grade
        const carregando = mostraCat ? catCarregando : carregandoGrade
        return (
          <>
            <div className="retro-store-grid grid-stagger grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3 pb-6">
              {carregando &&
                esqueletos.map((i) => (
                  <div
                    key={`sk${i}`}
                    className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]"
                  >
                    <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
                    <div className="p-3">
                      <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
                      <div className="h-8 animate-pulse rounded-lg bg-white/[0.04]" />
                    </div>
                  </div>
                ))}
              {!carregando &&
                itens.map((j) => (
                  <CartaoLoja
                    key={j.appid}
                    jogo={j}
                    naBiblioteca={bloqueados.has(j.appid)}
                    adicionado={jaAdicionados.has(j.appid)}
                    onOpen={() => setPagina(j)}
                    t={t}
                  />
                ))}
            </div>
            {mostraCat && !carregando && catTotal > POR_PAGINA && (
              <Paginacao
                pag={catPag}
                totalPags={Math.ceil(catTotal / POR_PAGINA)}
                onIr={(p) => {
                  setCatPag(p)
                  window.scrollTo?.(0, 0)
                }}
              />
            )}
          </>
        )
      })()}


      {metodo && (
        <MetodoDownloadDialog
          jogo={metodo.jogo}
          opcoes={metodo.opcoes}
          onDepot={() => {
            const j = metodo.jogo
            setMetodo(null)
            baixarDepot(j)
          }}
          onTorrent={(magnet, pasta) => confirmarTorrent(metodo.jogo, magnet, pasta)}
          onClose={() => setMetodo(null)}
          depotDisponivel={slsAtivo}
        />
      )}

      {escolhendo && (
        <EscolhaDownloadDialog
          escolhendo={escolhendo}
          onCancel={() => setEscolhendo(null)}
          onConfirm={(steamDir, sel) =>
            confirmarBaixar(escolhendo.jogo, escolhendo.info, steamDir, sel)
          }
          titulo={t("store.instalar_em", { title: escolhendo.jogo.title })}
        />
      )}

      {configGame && <GameSettingsDialog game={configGame} onClose={() => setConfigGame(null)} />}

      {toast && (
        <div
          className="fixed bottom-5 right-5 z-[80] max-w-[360px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-4 py-3 text-[13px] text-white/90 shadow-2xl shadow-black/60 backdrop-blur-md"
          onClick={() => setToast("")}
        >
          {toast}
        </div>
      )}

      {/* Teclado virtual: só no Big Picture. No desktop existe teclado físico. */}
      {bigPicture && (
        <StoreKeyboard
          aberto={tecladoAberto}
          inicial={busca}
          sugestoes={sugestoesKB}
          onTexto={(v) => setBusca(v)}
          onEscolherSugestao={(appid) => {
            const s = sugestoes.find((x) => x.appid === appid)
            if (s) {
              setTecladoAberto(false)
              pesquisar(s.title)
            }
          }}
          onConfirmar={(texto) => {
            setTecladoAberto(false)
            if (texto) pesquisar(texto)
          }}
          onFechar={() => setTecladoAberto(false)}
        />
      )}
    </div>
  )
}

function StoreTabs({
  aba,
  onAba,
  t,
}: {
  aba: "steam" | "retro"
  onAba: (aba: "steam" | "retro") => void
  t: (key: string, vars?: Record<string, string | number>) => string
}) {
  return (
    <div
      role="tablist"
      aria-label={t("store.tabs_label")}
      className="mb-6 flex w-fit items-center gap-1 rounded-xl border border-white/[0.08] bg-white/[0.025] p-1"
    >
      <button
        type="button"
        role="tab"
        aria-selected={aba === "steam"}
        onClick={() => onAba("steam")}
        className={`rounded-lg px-4 py-2 text-[12px] font-medium transition-colors ${aba === "steam" ? "bg-white/[0.12] text-white" : "text-white/50 hover:text-white"}`}
      >
        {t("store.steam_tab")}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={aba === "retro"}
        onClick={() => onAba("retro")}
        className={`rounded-lg px-4 py-2 text-[12px] font-medium transition-colors ${aba === "retro" ? "bg-white/[0.12] text-white" : "text-white/50 hover:text-white"}`}
      >
        {t("store.retro_tab")}
      </button>
    </div>
  )
}

// Destaca o termo digitado dentro do título (case-insensitive, sem HTML).
function TermoDestacado({ texto, termo }: { texto: string; termo: string }) {
  const t = termo.trim().toLowerCase()
  if (!t) return <>{texto}</>
  const idx = texto.toLowerCase().indexOf(t)
  if (idx < 0) return <>{texto}</>
  return (
    <>
      {texto.slice(0, idx)}
      <span className="font-bold text-white">{texto.slice(idx, idx + t.length)}</span>
      {texto.slice(idx + t.length)}
    </>
  )
}

// Paginação numérica (1-based na UI, 0-based no estado). Mostra vizinhas +
// elipse + primeira/última, no estilo do print.
function Paginacao({
  pag,
  totalPags,
  onIr,
}: {
  pag: number
  totalPags: number
  onIr: (p: number) => void
}) {
  const atual = pag + 1
  const nums: (number | "…")[] = []
  const push = (n: number | "…") => nums.push(n)
  const janela = new Set<number>([1, totalPags, atual, atual - 1, atual + 1, atual - 2, atual + 2])
  let ant = 0
  for (let n = 1; n <= totalPags; n++) {
    if (!janela.has(n)) continue
    if (n - ant > 1) push("…")
    push(n)
    ant = n
  }
  const Btn = ({
    children,
    on,
    ativo,
    off,
  }: {
    children: React.ReactNode
    on?: () => void
    ativo?: boolean
    off?: boolean
  }) => (
    <button
      onClick={on}
      disabled={off}
      className={`min-w-9 rounded-lg px-3 py-2 text-[13px] font-medium transition-colors disabled:opacity-30 ${
        ativo
          ? "bg-[color:var(--accent)] text-black"
          : "border border-white/10 text-white/70 hover:border-white/25 hover:text-white"
      }`}
    >
      {children}
    </button>
  )
  return (
    <div className="flex flex-wrap items-center justify-center gap-2 pb-8 pt-2">
      <Btn on={() => onIr(pag - 1)} off={atual <= 1}>
        ‹
      </Btn>
      {nums.map((n, i) =>
        n === "…" ? (
          <span key={`e${i}`} className="px-1 text-white/30">
            …
          </span>
        ) : (
          <Btn key={n} on={() => onIr(n - 1)} ativo={n === atual}>
            {n}
          </Btn>
        ),
      )}
      <Btn on={() => onIr(pag + 1)} off={atual >= totalPags}>
        ›
      </Btn>
    </div>
  )
}
