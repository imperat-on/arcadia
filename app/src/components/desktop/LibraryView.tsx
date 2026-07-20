"use client"

import { useEffect, useMemo, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { StoreBadge } from "./StoreBadge"
import { InstallDialog } from "./InstallDialog"
import { GameSettingsDialog } from "./GameSettingsDialog"
import { GameContextMenu, type CtxActions } from "./GameContextMenu"
import { GameDetailsDialog } from "./GameDetailsDialog"
import { GamePage } from "./GamePage"
import { UninstallDialog } from "./UninstallDialog"
import { EditMetadata } from "../ps5-launcher/EditMetadata"
import { avisarJogando } from "./PlayingBadge"

import { AddGameDialog } from "./AddGameDialog"

const LOJAS = ["todas", "steam", "epic", "custom"] as const

// Biblioteca do modo desktop: busca, filtros e grade de capas 2:3.
export function LibraryView({ games, tilesColor, alwaysTitles, onRefresh }: { games: Game[]; tilesColor?: boolean; alwaysTitles?: boolean; onRefresh?: () => void }) {
  const [busca, setBusca] = useState("")
  const [loja, setLoja] = useState<(typeof LOJAS)[number]>("todas")
  const [catFiltro, setCatFiltro] = useState("todas")
  const [soInstalados, setSoInstalados] = useState(false)
  const [instalando, setInstalando] = useState<Game | null>(null)
  const [configurando, setConfigurando] = useState<Game | null>(null)
  const [detalhes, setDetalhes] = useState<Game | null>(null)
  const [editandoCustom, setEditandoCustom] = useState<Game | null>(null)
  const [metaEdit, setMetaEdit] = useState<Game | null>(null)
  const [desinstalando, setDesinstalando] = useState<Game | null>(null)
  const [pagina, setPagina] = useState<Game | null>(null)
  const [adicionando, setAdicionando] = useState(false)
  const [menu, setMenu] = useState<{ g: Game; x: number; y: number } | null>(null)
  const [recemDesinstalados, setRecemDesinstalados] = useState<Set<string>>(new Set())

  // Quando a biblioteca real chega (reindex/refresh), o estado otimista já
  // cumpriu seu papel — limpa para não marcar jogo reinstalado como removido.
  useEffect(() => {
    setRecemDesinstalados((prev) => {
      if (!prev.size) return prev
      const vivos = new Set(games.map((g) => g.id))
      const next = new Set([...prev].filter((id) => vivos.has(id) && games.find((g) => g.id === id)?.installed === false))
      return next.size === prev.size ? prev : next
    })
  }, [games])

  const categorias = useMemo(() => {
    const s = new Set<string>()
    for (const g of games) for (const c of g.categories || []) s.add(c)
    return Array.from(s).sort((a, b) => a.localeCompare(b))
  }, [games])

  const lista = useMemo(() => {
    let l = games.filter((g) => !g.hidden)
    if (loja !== "todas") l = l.filter((g) => g.launcher === loja)
    if (catFiltro !== "todas") l = l.filter((g) => (g.categories || []).includes(catFiltro))
    if (soInstalados) l = l.filter((g) => g.installed !== false && !recemDesinstalados.has(g.id))
    const q = busca.trim().toLowerCase()
    if (q) l = l.filter((g) => g.title.toLowerCase().includes(q))
    // Favoritos primeiro, depois ordem alfabética.
    return [...l].sort((a, b) => Number(b.favorite || false) - Number(a.favorite || false) || a.title.localeCompare(b.title))
  }, [games, loja, catFiltro, soInstalados, busca, recemDesinstalados])


  const salvar = (id: string, patch: Record<string, unknown>) =>
    window.launcherAPI?.setOverride(id, patch).then(() => onRefresh?.())

  // Instalar um jogo não instalado, conforme a loja. Steam não tem fila
  // interna: redireciona pro cliente (steam://install abre o diálogo da Steam).
  // Epic/outros passam pelo InstallDialog (escolhe pasta, usa a fila).
  const instalar = (g: Game) => {
    if (g.launcher === "steam") {
      const appid = String(g.id).replace(/^steam:/, "")
      window.launcherAPI?.launch(["steam", `steam://install/${appid}`])
      return
    }
    setInstalando(g)
  }

  // Lança o jogo, mostra o card "jogando" e registra a última jogatina
  // (a menos que "Desativar a sincronização do tempo de jogo" esteja ligada).
  const jogar = (g: Game) => {
    window.launcherAPI?.launch(g.launch_cmd, g.id).then((r) => {
      if (r?.warnings?.length) console.warn("arcadia:", r.warnings.join("; "))
    })
    avisarJogando(g)
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.disable_playtime_tracking !== true) salvar(g.id, { last_played: Date.now() })
    })
  }

  const acoesMenu = (g: Game): CtxActions => ({
    jogar: () => {
      if (g.installed !== false) jogar(g)
      else instalar(g)
    },
    detalhes: () => setDetalhes(g),
    configuracoes: () => setConfigurando(g),
    registros: async () => Boolean((await window.launcherAPI?.gamelogOpen(g.id))?.ok),
    editar: () => setEditandoCustom(g),
    metadados: () => setMetaEdit(g),
    ocultar: () => salvar(g.id, { hidden: true }),
    favorito: () => salvar(g.id, { favorite: !g.favorite }),
    categorias: (cats) => salvar(g.id, { categories: cats }),
    desinstalar: () => {
      if (g.launcher === "steam") {
        // A Steam mostra o diálogo de confirmação dela.
        window.launcherAPI?.gameUninstall(g)
        setTimeout(() => onRefresh?.(), 5000)
        return
      }
      setDesinstalando(g)
    },
  })

  const confirmarDesinstalar = (opts: { removePrefix: boolean; removeSettings: boolean }) => {
    const g = desinstalando
    setDesinstalando(null)
    if (!g) return
    // Otimista: já tira o jogo da lista de instalados na hora.
    setRecemDesinstalados((prev) => new Set(prev).add(g.id))
    // O main espera o uninstall + reindex antes de responder.
    window.launcherAPI?.gameUninstall(g, opts).then((r) => {
      if (!r?.ok) {
        setRecemDesinstalados((prev) => {
          const n = new Set(prev)
          n.delete(g.id)
          return n
        })
        window.alert(r?.error || "Falha ao desinstalar")
      }
      onRefresh?.()
    })
  }

  return (
    <div className="flex h-full flex-col">
      {/* Topbar: busca + filtros */}
      <div className="flex items-center gap-3 px-8 pb-4 pt-6">
        <div className="relative flex-1">
          <svg className="absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-white/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" />
          </svg>
          <input
            value={busca}
            onChange={(e) => setBusca(e.target.value)}
            placeholder="Buscar jogos"
            spellCheck={false}
            className="w-full rounded-xl border border-white/10 bg-white/[0.04] py-2.5 pl-10 pr-4 text-sm text-white outline-none transition-colors placeholder:text-white/30 focus:border-[color:var(--accent)]"
          />
        </div>
        {categorias.length > 0 && (
          <select
            value={catFiltro}
            onChange={(e) => setCatFiltro(e.target.value)}
            className="appearance-none rounded-xl border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-xs font-medium text-white/70 outline-none transition-colors focus:border-[color:var(--accent)]"
          >
            <option value="todas" className="bg-[#16161a]">Categorias</option>
            {categorias.map((c) => (
              <option key={c} value={c} className="bg-[#16161a]">{c}</option>
            ))}
          </select>
        )}
        <div className="flex gap-1 rounded-xl border border-white/10 bg-white/[0.04] p-1">
          {LOJAS.map((l) => (
            <button
              key={l}
              onClick={() => setLoja(l)}
              className={`rounded-lg px-3 py-1.5 text-xs font-medium capitalize transition-colors ${
                loja === l ? "bg-white text-black" : "text-white/55 hover:text-white"
              }`}
            >
              {l === "todas" ? "Todas" : l}
            </button>
          ))}
        </div>
        <button
          onClick={() => setSoInstalados((v) => !v)}
          className={`rounded-xl border px-3.5 py-2.5 text-xs font-medium transition-colors ${
            soInstalados ? "border-[color:var(--accent)] text-white" : "border-white/10 text-white/55 hover:text-white"
          }`}
        >
          Só instalados
        </button>
        <button
          onClick={() => setAdicionando(true)}
          className="rounded-xl px-4 py-2.5 text-xs font-bold tracking-wide text-black transition-transform hover:scale-[1.03]"
          style={{ background: "var(--accent)" }}
        >
          ADICIONAR JOGO
        </button>
      </div>

      {/* Cabeçalho da grade */}
      <div className="flex items-center gap-4 px-8 pb-2">
        <h2 className="text-lg font-light text-white">
          Todos os jogos <span className="ml-1 rounded-md bg-white/10 px-2 py-0.5 text-xs text-white/70">{lista.length}</span>
        </h2>
      </div>

      {/* Grade */}
      <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-8 pt-2">
        <div className="grid-stagger grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-4">
          {lista.map((g) => {
            // Recém-desinstalado: já mostra como não instalado (sem botão jogar).
            const g2 = recemDesinstalados.has(g.id) ? { ...g, installed: false } : g
            return (
              <Card
                key={g.id}
                game={g2}
                tilesColor={tilesColor}
                alwaysTitles={alwaysTitles}
                onInstall={() => instalar(g)}
                onConfig={() => setConfigurando(g)}
                onMenu={(x, y) => setMenu({ g, x, y })}
                onOpen={() => setPagina(g2)}
              />
            )
          })}
        </div>
        {lista.length === 0 && (
          <div className="flex h-64 items-center justify-center text-white/35">Nada por aqui com esses filtros.</div>
        )}
      </div>

      {/* Diálogo de instalação (escolher pasta, ver espaço em disco) */}
      {instalando && <InstallDialog game={instalando} onClose={() => setInstalando(null)} />}

      {/* Adicionar jogo manualmente */}
      {adicionando && <AddGameDialog onClose={() => setAdicionando(false)} onAdded={() => onRefresh?.()} />}

      {/* Editar jogo custom (título, executável, imagens, instalador) */}
      {editandoCustom && (
        <AddGameDialog editGame={editandoCustom} onClose={() => setEditandoCustom(null)} onAdded={() => onRefresh?.()} />
      )}

      {/* Diálogo de configurações do jogo (estilo Heroic) */}
      {configurando && <GameSettingsDialog game={configurando} onClose={() => setConfigurando(null)} />}

      {/* Página do jogo (clique no card) */}
      {pagina && (
        <GamePage
          game={pagina}
          onClose={() => setPagina(null)}
          onJogar={() => {
            setPagina(null)
            jogar(pagina)
          }}
          onInstalar={() => {
            setPagina(null)
            instalar(pagina)
          }}
          onImportar={() => {
            window.launcherAPI?.gameImport(pagina).then((r) => {
              if (!r?.ok && r?.error !== "cancelado") window.alert(r?.error || "Falha ao importar")
            })
          }}
          onConfig={() => setConfigurando(pagina)}
        />
      )}

      {/* Detalhes do jogo */}
      {detalhes && <GameDetailsDialog game={detalhes} onClose={() => setDetalhes(null)} />}

      {/* Desinstalação (não-Steam): opções de prefixo/configs */}
      {desinstalando && (
        <UninstallDialog game={desinstalando} onConfirm={confirmarDesinstalar} onClose={() => setDesinstalando(null)} />
      )}

      {/* Metadados (edição + busca online de descrição/artes) */}
      <EditMetadata
        game={metaEdit}
        onClose={() => setMetaEdit(null)}
        onSave={(patch) => metaEdit && salvar(metaEdit.id, patch)}
      />

      {/* Menu de contexto (botão direito) */}
      {menu && (
        <GameContextMenu
          game={menu.g}
          x={menu.x}
          y={menu.y}
          allCategories={categorias}
          actions={acoesMenu(menu.g)}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  )
}

function Card({ game: g, tilesColor, alwaysTitles, onInstall, onConfig, onMenu, onOpen }: { game: Game; tilesColor?: boolean; alwaysTitles?: boolean; onInstall?: () => void; onConfig?: () => void; onMenu?: (x: number, y: number) => void; onOpen?: () => void }) {
  const instalado = g.installed !== false
  const epicNaoInstalado = g.launcher === "epic" && !instalado

  const acao = () => {
    // Não instalado (qualquer loja): o pai decide como instalar (Steam abre
    // steam://install, Epic abre o diálogo de download).
    if (!instalado) {
      onInstall?.()
      return
    }
    window.launcherAPI?.launch(g.launch_cmd, g.id).then((r) => {
      if (r?.warnings?.length) console.warn("arcadia:", r.warnings.join("; "))
    })
    avisarJogando(g)
    window.launcherAPI?.getConfig().then((c) => {
      if (c?.disable_playtime_tracking !== true) window.launcherAPI?.setOverride(g.id, { last_played: Date.now() })
    })
  }

  return (
    <div
      onClick={onOpen}
      onMouseEnter={() => window.launcherAPI?.gameSysinfo(g)} // esquenta o cache da página
      onContextMenu={(e) => {
        e.preventDefault()
        onMenu?.(e.clientX, e.clientY)
      }}
      className="group relative flex cursor-pointer flex-col overflow-hidden rounded-xl border border-white/[0.07] bg-[#121216] transition-colors hover:border-white/25"
    >
      <div className="relative aspect-[2/3] w-full overflow-hidden bg-black">
        {g.cover ? (
          <img
            src={g.cover}
            alt={g.title}
            loading="lazy"
            className={`h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.04] ${instalado || tilesColor ? "" : "grayscale-[0.4]"}`}
            draggable={false}
          />
        ) : (
          <div className="flex h-full items-center justify-center px-3 text-center text-xs text-white/30">{g.title}</div>
        )}
        <StoreBadge launcher={g.launcher} />
        {g.favorite && (
          <div className="absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full bg-black/60">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="#e8703a">
              <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z" />
            </svg>
          </div>
        )}

        {/* Overlay de ação: ícone download (não instalado) ou play+config (instalado) */}
        <div className="absolute inset-0 flex items-center justify-center gap-3 bg-black/60 opacity-0 transition-opacity group-hover:opacity-100">
          {instalado ? (
            <>
              {/* Configurações do jogo */}
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onConfig?.()
                }}
                title="Configurações do jogo"
                className="flex h-11 w-11 items-center justify-center rounded-full bg-white/[0.12] text-white backdrop-blur-sm transition-all hover:scale-110 hover:bg-white/[0.2]"
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="4" y1="6" x2="20" y2="6" /><circle cx="9" cy="6" r="2" fill="currentColor" stroke="none" />
                  <line x1="4" y1="12" x2="20" y2="12" /><circle cx="15" cy="12" r="2" fill="currentColor" stroke="none" />
                  <line x1="4" y1="18" x2="20" y2="18" /><circle cx="7" cy="18" r="2" fill="currentColor" stroke="none" />
                </svg>
              </button>
              {/* Jogar */}
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  acao()
                }}
                title="Jogar"
                className="flex h-12 w-12 items-center justify-center rounded-full text-black transition-transform hover:scale-110"
                style={{ background: "var(--accent)", boxShadow: "0 0 20px color-mix(in srgb, var(--accent) 50%, transparent)" }}
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" stroke="none">
                  <path d="M8 5.14v13.72a1 1 0 0 0 1.52.86l11-6.86a1 1 0 0 0 0-1.72l-11-6.86a1 1 0 0 0-1.52.86z" />
                </svg>
              </button>
            </>
          ) : (
            /* Baixar (não instalado) */
            <button
              onClick={(e) => {
                e.stopPropagation()
                acao()
              }}
              title={epicNaoInstalado ? "Baixar" : "Instalar"}
              className="flex h-12 w-12 items-center justify-center rounded-full text-black transition-transform hover:scale-110"
              style={{ background: "var(--accent)", boxShadow: "0 0 20px color-mix(in srgb, var(--accent) 50%, transparent)" }}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="7 10 12 15 17 10" />
                <line x1="12" x2="12" y1="15" y2="3" />
              </svg>
            </button>
          )}
        </div>
      </div>
      <div
        className={`truncate px-3 py-2.5 text-[13px] text-white/85 ${alwaysTitles === false ? "opacity-0 transition-opacity group-hover:opacity-100" : ""}`}
        title={g.title}
      >
        {g.title}
      </div>
    </div>
  )
}
