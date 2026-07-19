"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { TextCandidate } from "../../global"
import { useGamepadNav } from "./useGamepadNav"

interface TextSearchProps {
  gameId: string
  titulo: string
  onClose: () => void
  onPicked: (texto: string) => void
}

const CONSOLE = typeof window !== "undefined" && window.launcherMode !== "desktop"

export function TextSearch({ gameId, titulo, onClose, onPicked }: TextSearchProps) {
  const ref = useRef<HTMLDivElement>(null)
  useGamepadNav(ref, true, onClose)

  const [termo, setTermo] = useState(titulo)
  const [carregando, setCarregando] = useState(false)
  const [textos, setTextos] = useState<TextCandidate[]>([])
  const [erros, setErros] = useState<string[]>([])

  const buscar = useCallback(
    async (q: string) => {
      setCarregando(true)
      setErros([])
      const res = await window.launcherAPI?.searchText(gameId, q)
      setTextos(res?.textos ?? [])
      setErros(res?.erros ?? [])
      setCarregando(false)
    },
    [gameId],
  )

  useEffect(() => {
    buscar(titulo)
  }, [buscar, titulo])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])

  return (
    <>
      <div className="fixed inset-0 z-[60]" style={{ background: "rgba(0,0,0,0.75)" }} />
      <div
        ref={ref}
        className="gp-scope fixed z-[61] left-1/2 top-1/2 w-[780px] max-h-[86vh] flex flex-col rounded-2xl overflow-hidden"
        style={{
          transform: "translate(-50%, -50%)",
          background: CONSOLE ? "rgba(10,22,54,0.98)" : "var(--sidebar-bg)",
          border: "1px solid rgba(255,255,255,0.14)",
          boxShadow: "0 24px 70px rgba(0,0,0,0.7)",
        }}
        role="dialog"
        aria-label="Buscar descrição"
      >
        <div className="px-6 py-4 flex items-center gap-3" style={{ background: "rgba(0,0,0,0.5)" }}>
          <span className="text-white text-[15px] font-semibold tracking-wide uppercase whitespace-nowrap">
            Buscar descrição
          </span>
          <input
            value={termo}
            onChange={(e) => setTermo(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && buscar(termo)}
            placeholder="Nome do jogo"
            className="flex-1 px-3 py-2 rounded-lg text-white text-[14px] outline-none"
            style={{
              background: "rgba(255,255,255,0.07)",
              border: "1px solid rgba(255,255,255,0.12)",
            }}
          />
          <button
            onClick={() => buscar(termo)}
            className="px-4 py-2 rounded-lg text-[14px] font-semibold text-white"
            style={{ background: "var(--accent)" }}
          >
            Buscar
          </button>
          <button
            onClick={onClose}
            className="px-3 py-2 rounded-lg text-[14px] text-white/70 hover:text-white"
            style={{ background: "rgba(255,255,255,0.08)" }}
          >
            Fechar
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-6 flex flex-col gap-3">
          {carregando ? (
            <p className="text-center text-[#8a93a6] py-12">Procurando…</p>
          ) : textos.length === 0 ? (
            <p className="text-center text-[#8a93a6] py-12">
              Nenhuma descrição encontrada para “{termo}”.
            </p>
          ) : (
            textos.map((t, i) => (
              <button
                key={`${t.fonte}-${i}`}
                onClick={() => {
                  onPicked(t.texto)
                  onClose()
                }}
                className="text-left rounded-xl p-4 transition-colors hover:bg-white/10"
                style={{
                  background: "rgba(255,255,255,0.05)",
                  border: "1px solid rgba(255,255,255,0.10)",
                }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <span
                    className="px-2 py-0.5 rounded text-[10px] font-bold text-white"
                    style={{ background: "rgba(0,0,0,0.6)" }}
                  >
                    {t.fonte}
                  </span>
                  <span className="text-[11px] text-[#8a93a6]">
                    {t.texto.length} caracteres
                  </span>
                </div>
                {/* Prévia limitada: a completa da Steam passa de 8 mil chars */}
                <p className="text-[14px] text-white/85 leading-relaxed line-clamp-4 whitespace-pre-line">
                  {t.texto}
                </p>
              </button>
            ))
          )}

          {erros.length > 0 && (
            <div className="flex flex-col gap-1 pt-2">
              {erros.map((e) => (
                <p key={e} className="text-[12px] text-[#ffa07a]">
                  {e}
                </p>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  )
}
