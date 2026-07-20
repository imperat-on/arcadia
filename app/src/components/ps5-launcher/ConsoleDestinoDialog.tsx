"use client"

import { useEffect, useRef } from "react"
import { useGamepadNav } from "./useGamepadNav"

export interface DestinoOpcao {
  /** Caminho de instalação, usado como valor e como rótulo principal. */
  caminho: string
  /** Linha secundária: de onde veio esta opção. */
  rotulo?: string
  /** GB livres no disco, quando conhecidos. */
  livre?: number
}

interface ConsoleDestinoDialogProps {
  titulo: string
  subtitulo?: string
  opcoes: DestinoOpcao[]
  /** GB do download, para desabilitar destino que não comporta. */
  tamanho?: number
  /** Texto enquanto a lista de destinos ainda não chegou. */
  carregando?: string
  onEscolher: (caminho: string) => void
  onFechar: () => void
}

const encurtar = (p: string) => p.replace(/^\/home\/[^/]+/, "~")
const gb = (v?: number) => (v == null || Number.isNaN(v) ? "—" : `${v.toFixed(1)} GB`)

// Escolha do destino de instalação no modo console, para Epic, Steam e loja.
//
// O diálogo do desktop (InstallDialog) pede a pasta num campo de texto com
// seletor nativo — inútil com o controle na mão. Aqui os destinos são uma
// lista navegável pelo direcional: A confirma, B cancela.
export function ConsoleDestinoDialog({
  titulo,
  subtitulo,
  opcoes,
  tamanho,
  carregando = "Procurando destinos…",
  onEscolher,
  onFechar,
}: ConsoleDestinoDialogProps) {
  const ref = useRef<HTMLDivElement>(null)

  useGamepadNav(ref, true, onFechar)

  // Foca o primeiro destino assim que a lista existe, para o direcional já ter
  // de onde partir.
  useEffect(() => {
    if (!opcoes.length) return
    ref.current?.querySelector<HTMLButtonElement>("button[data-destino]")?.focus()
  }, [opcoes.length])

  return (
    <div
      ref={ref}
      className="gp-scope fixed inset-0 z-[90] flex items-center justify-center bg-black/85 backdrop-blur-sm"
    >
      <div className="w-[620px] max-w-[92vw] rounded-2xl border border-white/10 bg-[#0b0b0d] p-7">
        <h2 className="text-[22px] font-semibold text-white">{titulo}</h2>
        {subtitulo && <p className="mt-1 text-[13px] text-white/45">{subtitulo}</p>}

        <div className="mt-6 flex flex-col gap-2">
          {opcoes.length === 0 ? (
            <p className="py-6 text-center text-[13px] text-white/35">{carregando}</p>
          ) : (
            opcoes.map((o) => {
              const cabe = o.livre == null || tamanho == null || o.livre >= tamanho
              return (
                <button
                  key={o.caminho}
                  data-destino
                  disabled={!cabe}
                  onClick={() => onEscolher(o.caminho)}
                  className="flex items-center justify-between rounded-xl border border-white/10 px-5 py-4 text-left outline-none transition-colors hover:border-white/25 focus:border-[color:var(--accent)] disabled:opacity-40"
                >
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-white/90">{encurtar(o.caminho)}</span>
                    {o.rotulo && <span className="block text-[11px] text-white/40">{o.rotulo}</span>}
                  </span>
                  <span className="shrink-0 pl-4 text-xs text-white/45">
                    {cabe ? `${gb(o.livre)} livres` : "Sem espaço"}
                  </span>
                </button>
              )
            })
          )}
        </div>

        <button
          onClick={onFechar}
          className="mt-5 w-full rounded-lg border border-white/10 py-2.5 text-[13px] text-white/55 outline-none transition-colors hover:text-white/85"
        >
          Cancelar
        </button>
      </div>
    </div>
  )
}
