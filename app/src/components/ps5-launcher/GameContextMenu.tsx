"use client"

import { useEffect, useRef, useState, type ReactNode } from "react"
import type { Game } from "./types"
import { useGamepadNav } from "./useGamepadNav"

interface GameContextMenuProps {
  game: Game | null
  onClose: () => void
  onLaunch: () => void
  onEditMeta: () => void
  onToggleHidden: () => void
  onDownloadTrailer: () => void
}

const FERRAMENTAS = [
  { id: "winecfg", label: "winecfg (configurações do Wine)" },
  { id: "regedit", label: "regedit (registro do Windows)" },
  { id: "explorer", label: "explorer.exe (arquivos do prefixo)" },
  { id: "winetricks", label: "winetricks (bibliotecas extras)" },
] as const

export function GameContextMenu({
  game,
  onClose,
  onLaunch,
  onEditMeta,
  onToggleHidden,
  onDownloadTrailer,
}: GameContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null)
  const open = Boolean(game)
  useGamepadNav(ref, open, onClose)
  const [ferramentas, setFerramentas] = useState(false)
  const [toolBusy, setToolBusy] = useState("")

  // Fecha ao clicar fora ou apertar Esc (clique-fora desligável na Acessibilidade).
  useEffect(() => {
    if (!open) return
    setFerramentas(false)
    let clickFora = true
    window.launcherAPI?.getConfig().then((c) => {
      clickFora = !c?.no_click_outside
    })
    const onDown = (e: MouseEvent) => {
      if (clickFora && ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("mousedown", onDown)
    window.addEventListener("keydown", onKey)
    return () => {
      window.removeEventListener("mousedown", onDown)
      window.removeEventListener("keydown", onKey)
    }
  }, [open, onClose])

  if (!game) return null

  const run = (fn: () => void) => () => {
    onClose()
    fn()
  }

  return (
    <>
      {/* Véu: escurece o fundo e captura cliques fora */}
      <div className="fixed inset-0 z-40" style={{ background: "rgba(0,0,0,0.55)" }} />

      <div
        ref={ref}
        className="gp-scope fixed z-50 left-1/2 top-1/2 w-[420px] rounded-xl overflow-hidden"
        style={{
          transform: "translate(-50%, -50%)",
          background: "rgba(10,22,54,0.98)",
          border: "1px solid rgba(255,255,255,0.14)",
          boxShadow: "0 24px 70px rgba(0,0,0,0.7)",
          backdropFilter: "blur(16px)",
        }}
        role="menu"
        aria-label={`Opções de ${game.title}`}
      >
        {/* Cabeçalho: nome do jogo */}
        <div
          className="px-5 py-3 text-white text-[15px] font-semibold tracking-wide uppercase truncate"
          style={{ background: "rgba(0,0,0,0.35)" }}
        >
          {game.title}
        </div>

        <div className="py-2">
          {ferramentas ? (
            <>
              <div className="px-5 pb-2 text-xs uppercase tracking-wider text-white/50">
                Ferramentas do prefixo
              </div>
              {FERRAMENTAS.map((f) => (
                <Item
                  key={f.id}
                  onClick={async () => {
                    setToolBusy(f.id)
                    await window.launcherAPI?.prefixTool(game.id, f.id)
                    setToolBusy("")
                  }}
                  icon={<path d="M14.7 6.3a4 4 0 00-5.4 5.4L3 18v3h3l6.3-6.3a4 4 0 005.4-5.4l-2.6 2.6-2-2 2.6-2.6z" />}
                  label={toolBusy === f.id ? "Abrindo…" : f.label}
                />
              ))}
              <Item
                onClick={() => setFerramentas(false)}
                icon={<path d="M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z" />}
                label="Voltar"
              />
            </>
          ) : (
            <>
          <Item
            onClick={run(onLaunch)}
            icon={
              game.installed === false ? (
                <path d="M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z" />
              ) : (
                <path d="M8 5v14l11-7z" />
              )
            }
            label={game.installed === false ? "Instalar" : "Iniciar jogo"}
          />
          <Item
            onClick={run(onEditMeta)}
            icon={
              <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 000-1.41l-2.34-2.34a1 1 0 00-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z" />
            }
            label="Editar metadados"
          />
          <Item
            onClick={run(onDownloadTrailer)}
            icon={
              <path d="M10 16.5l6-4.5-6-4.5v9zM12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z" />
            }
            label="Baixar trailer"
          />
          <Item
            onClick={() => setFerramentas(true)}
            icon={<path d="M14.7 6.3a4 4 0 00-5.4 5.4L3 18v3h3l6.3-6.3a4 4 0 005.4-5.4l-2.6 2.6-2-2 2.6-2.6z" />}
            label="Ferramentas do prefixo"
          />
          <Item
            onClick={run(onToggleHidden)}
            icon={
              game.hidden ? (
                <path d="M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zm0 12.5a5 5 0 110-10 5 5 0 010 10zm0-8a3 3 0 100 6 3 3 0 000-6z" />
              ) : (
                <path d="M12 7a5 5 0 015 5c0 .65-.13 1.26-.36 1.83l2.92 2.92A11.8 11.8 0 0023 12c-1.73-4.39-6-7.5-11-7.5-1.27 0-2.49.2-3.64.57l2.17 2.16C11.1 7.09 11.54 7 12 7zM2.71 3.16L1.39 4.47l2.17 2.17A11.77 11.77 0 001 12c1.73 4.39 6 7.5 11 7.5 1.55 0 3.03-.3 4.38-.84l3.15 3.15 1.31-1.31L2.71 3.16zM12 17a5 5 0 01-4.55-7.06l1.55 1.55A3 3 0 0012 15c.28 0 .55-.04.8-.11l1.55 1.55c-.72.36-1.51.56-2.35.56z" />
              )
            }
            label={game.hidden ? "Reexibir este jogo" : "Ocultar este jogo"}
          />
            </>
          )}
        </div>
      </div>
    </>
  )
}

function Item({
  icon,
  label,
  onClick,
}: {
  icon: ReactNode
  label: string
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      role="menuitem"
      className="w-full flex items-center gap-4 px-5 py-3 text-left text-white text-[17px] transition-colors"
      onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.10)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" className="flex-shrink-0 opacity-90">
        {icon}
      </svg>
      {label}
    </button>
  )
}
