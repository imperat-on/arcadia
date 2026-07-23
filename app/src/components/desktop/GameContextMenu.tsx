"use client"

import { useEffect, useRef, useState, type ReactNode } from "react"
import { createPortal } from "react-dom"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

// Menu de contexto (botão direito) dos cards da biblioteca desktop.
// As ações vêm do LibraryView; "Categorias" abre um subpainel interno.

export interface CtxActions {
  jogar: () => void
  detalhes: () => void
  configuracoes: () => void
  registros: () => Promise<boolean>
  editar: () => void
  metadados: () => void
  ocultar: () => void
  favorito: () => void
  categorias: (cats: string[]) => void
  desinstalar: () => void
}

const I = (d: string) => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" className="shrink-0 opacity-80">
    <path d={d} />
  </svg>
)

export function GameContextMenu({
  game,
  x,
  y,
  allCategories,
  actions,
  onClose,
}: {
  game: Game
  x: number
  y: number
  allCategories: string[]
  actions: CtxActions
  onClose: () => void
}) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const [verCategorias, setVerCategorias] = useState(false)
  const [novaCat, setNovaCat] = useState("")
  const [aviso, setAviso] = useState("")

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
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
  }, [onClose])

  // Mantém o menu dentro da janela.
  const W = 240
  const H = verCategorias ? 320 : 380
  const px = Math.min(x, window.innerWidth - W - 8)
  const py = Math.min(y, window.innerHeight - H - 8)

  const run = (fn: () => void) => () => {
    onClose()
    fn()
  }

  const Item = ({ icon, label, onClick }: { icon: ReactNode; label: string; onClick: () => void }) => (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-3 px-4 py-2.5 text-left text-[13px] text-white/90 transition-colors hover:bg-white/[0.07]"
    >
      {icon}
      {label}
    </button>
  )

  const catsDoJogo = game.categories || []
  const toggleCat = (c: string) => {
    const nova = catsDoJogo.includes(c) ? catsDoJogo.filter((k) => k !== c) : [...catsDoJogo, c]
    actions.categorias(nova)
  }

  // Portal para o body: se ficasse dentro da árvore da view, qualquer ancestral
  // com transform/filter viraria o bloco contedor do position:fixed e o menu
  // abria deslocado (não onde o mouse clicou).
  return createPortal(
    <div
      ref={ref}
      className="fixed z-[80] w-[240px] overflow-hidden rounded-xl border border-white/10 bg-[#15181d] py-1.5 shadow-2xl shadow-black/70"
      style={{ left: px, top: py, animation: "ctx-in 0.12s ease-out" }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {verCategorias ? (
        <>
          <button onClick={() => setVerCategorias(false)} className="flex w-full items-center gap-2 px-4 py-2 text-[12px] text-white/50 transition-colors hover:text-white">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><path d="M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z" /></svg>
            {t("library.categorias")}
          </button>
          <div className="max-h-[220px] overflow-y-auto">
            {allCategories.length === 0 && <p className="px-4 py-2 text-[12px] text-white/35">{t("library.sem_categorias")}</p>}
            {allCategories.map((c) => (
              <label key={c} className="flex cursor-pointer items-center gap-3 px-4 py-2 text-[13px] text-white/85 transition-colors hover:bg-white/[0.06]">
                <input
                  type="checkbox"
                  checked={catsDoJogo.includes(c)}
                  onChange={() => toggleCat(c)}
                  className="h-[15px] w-[15px] cursor-pointer appearance-none rounded-[4px] border border-white/30 checked:border-transparent"
                  style={
                    catsDoJogo.includes(c)
                      ? {
                          background: "var(--accent)",
                          backgroundImage: "url(\"data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='3.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='20 6 9 17 4 12'/></svg>\")",
                          backgroundSize: "11px",
                          backgroundPosition: "center",
                          backgroundRepeat: "no-repeat",
                        }
                      : undefined
                  }
                />
                {c}
              </label>
            ))}
          </div>
          <div className="mt-1 flex gap-2 border-t border-white/[0.07] px-3 py-2">
            <input
              value={novaCat}
              onChange={(e) => setNovaCat(e.target.value)}
              placeholder={t("library.nova_categoria")}
              spellCheck={false}
              className="min-w-0 flex-1 rounded-md border border-white/10 bg-white/[0.04] px-2.5 py-1.5 text-[12px] text-white outline-none placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
            <button
              onClick={() => {
                const c = novaCat.trim()
                if (!c) return
                if (!catsDoJogo.includes(c)) actions.categorias([...catsDoJogo, c])
                setNovaCat("")
              }}
              disabled={!novaCat.trim()}
              className="flex h-7 w-7 items-center justify-center rounded-md text-black transition-transform enabled:hover:scale-110 disabled:opacity-40"
              style={{ background: "var(--accent)" }}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
                <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
              </svg>
            </button>
          </div>
        </>
      ) : (
        <>
          {game.installed !== false ? (
            <Item onClick={run(actions.jogar)} icon={I("M8 5v14l11-7z")} label={t("library.jogar")} />
          ) : (
            <Item onClick={run(actions.jogar)} icon={I("M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z")} label={t("library.instalar")} />
          )}
          <Item onClick={run(actions.detalhes)} icon={I("M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z")} label={t("library.detalhes")} />
          <Item onClick={run(actions.configuracoes)} icon={I("M19.14 12.94a7.07 7.07 0 000-1.88l2.03-1.58a.5.5 0 00.12-.64l-1.92-3.32a.5.5 0 00-.61-.22l-2.39.96a7.03 7.03 0 00-1.62-.94l-.36-2.54a.5.5 0 00-.5-.42h-3.84a.5.5 0 00-.5.42l-.36 2.54c-.59.24-1.13.56-1.62.94l-2.39-.96a.5.5 0 00-.61.22L2.65 8.84a.5.5 0 00.12.64l2.03 1.58a7.07 7.07 0 000 1.88l-2.03 1.58a.5.5 0 00-.12.64l1.92 3.32c.14.24.42.34.61.22l2.39-.96c.49.38 1.03.7 1.62.94l.36 2.54c.05.24.25.42.5.42h3.84c.25 0 .45-.18.5-.42l.36-2.54a7.03 7.03 0 001.62-.94l2.39.96c.19.12.47.02.61-.22l1.92-3.32a.5.5 0 00-.12-.64l-2.03-1.58zM12 15.5a3.5 3.5 0 110-7 3.5 3.5 0 010 7z")} label={t("library.config_jogo")} />
          <Item
            onClick={async () => {
              const ok = await actions.registros()
              if (ok) onClose()
              else {
                setAviso(t("library.sem_registros"))
                setTimeout(() => setAviso(""), 3500)
              }
            }}
            icon={I("M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8l-6-6zm-1 7V3.5L18.5 9H13zM8 12h8v2H8v-2zm0 4h8v2H8v-2z")}
            label={t("library.registros")}
          />
          <Item onClick={run(actions.editar)} icon={I("M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 000-1.41l-2.34-2.34a1 1 0 00-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z")} label={t("library.editar_jogo")} />
          <Item onClick={run(actions.metadados)} icon={I("M19 3H5a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2V5a2 2 0 00-2-2zM9 17H7v-7h2v7zm4 0h-2V7h2v10zm4 0h-2v-4h2v4z")} label={t("library.metadados")} />
          <Item onClick={run(actions.ocultar)} icon={I("M12 7a5 5 0 015 5c0 .65-.13 1.26-.36 1.83l2.92 2.92A11.8 11.8 0 0023 12c-1.73-4.39-6-7.5-11-7.5-1.27 0-2.49.2-3.64.57l2.17 2.16C11.1 7.09 11.54 7 12 7zM2.71 3.16L1.39 4.47l2.17 2.17A11.77 11.77 0 001 12c1.73 4.39 6 7.5 11 7.5 1.55 0 3.03-.3 4.38-.84l3.15 3.15 1.31-1.31L2.71 3.16zM12 17a5 5 0 01-4.55-7.06l1.55 1.55A3 3 0 0012 15c.28 0 .55-.04.8-.11l1.55 1.55c-.72.36-1.51.56-2.35.56z")} label={t("library.ocultar_jogo")} />
          <Item onClick={run(actions.favorito)} icon={I(game.favorite ? "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z" : "M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55l-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z")} label={game.favorite ? t("library.remover_favoritos") : t("library.adicionar_favoritos")} />
          <Item onClick={() => setVerCategorias(true)} icon={I("M3 13h2v-2H3v2zm0 4h2v-2H3v2zm0-8h2V7H3v2zm4 4h14v-2H7v2zm0 4h14v-2H7v2zM7 7v2h14V7H7z")} label={t("library.categorias")} />
          {game.installed !== false && (
            <Item onClick={run(actions.desinstalar)} icon={I("M6 19a2 2 0 002 2h8a2 2 0 002-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z")} label={t("library.desinstalar")} />
          )}
          {aviso && <p className="border-t border-white/[0.07] px-4 py-2 text-[11px] leading-snug text-white/50">{aviso}</p>}
        </>
      )}
      <style>{`
        @keyframes ctx-in {
          from { opacity: 0; transform: scale(0.97); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>,
    document.body,
  )
}
