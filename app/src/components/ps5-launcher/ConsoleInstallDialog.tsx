"use client"

import { useEffect, useRef, useState } from "react"
import { useGamepadNav } from "./useGamepadNav"
import type { Game } from "./types"

interface Destino {
  caminho: string
  rotulo: string
  livre?: number
  total?: number
}

interface ConsoleInstallDialogProps {
  game: Game
  onFechar: () => void
  onInstalar: (installPath: string) => void
}

const encurtar = (p: string) => p.replace(/^\/home\/[^/]+/, "~")
const gib = (v?: number) => (v == null || Number.isNaN(v) ? "—" : `${v.toFixed(1)} GB`)

// Escolha do destino antes de instalar, no modo console.
//
// O equivalente do desktop (InstallDialog) pede a pasta num campo de texto com
// seletor nativo — inútil com o controle na mão. Aqui os destinos viram uma
// lista navegável pelo direcional: a pasta padrão e uma por disco onde já
// existe biblioteca Steam, que são justamente os discos reservados a jogos.
export function ConsoleInstallDialog({ game, onFechar, onInstalar }: ConsoleInstallDialogProps) {
  const [destinos, setDestinos] = useState<Destino[]>([])
  const ref = useRef<HTMLDivElement>(null)

  useGamepadNav(ref, true, onFechar)

  useEffect(() => {
    let cancelado = false
    ;(async () => {
      const api = window.launcherAPI
      const cfg = await api?.getConfig()
      const home = window.launcherPaths?.home || "~"
      const padrao = cfg?.default_install_path || `${home}/Games/Arcadia`

      const libs = (await api?.storeLibraries()) || []
      const caminhos = [
        { caminho: padrao, rotulo: "Pasta padrão" },
        // Uma pasta Arcadia na raiz de cada biblioteca Steam: cai no mesmo
        // disco sem se misturar com o steamapps da Steam.
        ...libs.map((l: { steamDir: string }) => ({
          caminho: `${l.steamDir}/Arcadia`,
          rotulo: "Disco da biblioteca Steam",
        })),
      ].filter((d, i, arr) => arr.findIndex((o) => o.caminho === d.caminho) === i)

      const comEspaco = await Promise.all(
        caminhos.map(async (d) => {
          const r = await api?.diskSpace(d.caminho)
          return r?.ok ? { ...d, livre: r.free, total: r.total } : d
        }),
      )
      if (!cancelado) setDestinos(comEspaco)
    })()
    return () => {
      cancelado = true
    }
  }, [])

  // Foca o primeiro destino assim que a lista existe, para o direcional já ter
  // onde começar.
  useEffect(() => {
    if (!destinos.length) return
    ref.current?.querySelector<HTMLButtonElement>("button[data-destino]")?.focus()
  }, [destinos.length])

  const tamanho = game.size != null ? game.size / 1024 : undefined // size vem em MiB

  return (
    <div
      ref={ref}
      className="gp-scope fixed inset-0 z-[90] flex items-center justify-center bg-black/85 backdrop-blur-sm"
    >
      <div className="w-[620px] max-w-[92vw] rounded-2xl border border-white/10 bg-[#0b0b0d] p-7">
        <h2 className="text-[22px] font-semibold text-white">Instalar {game.title}</h2>
        <p className="mt-1 text-[13px] text-white/45">
          {tamanho != null ? `Download de ${gib(tamanho)} · ` : ""}Escolha onde instalar.
        </p>

        <div className="mt-6 flex flex-col gap-2">
          {destinos.length === 0 ? (
            <p className="py-6 text-center text-[13px] text-white/35">Procurando destinos…</p>
          ) : (
            destinos.map((d) => {
              const cabe = d.livre == null || tamanho == null || d.livre >= tamanho
              return (
                <button
                  key={d.caminho}
                  data-destino
                  disabled={!cabe}
                  onClick={() => onInstalar(d.caminho)}
                  className="flex items-center justify-between rounded-xl border border-white/10 px-5 py-4 text-left outline-none transition-colors hover:border-white/25 focus:border-[color:var(--accent)] disabled:opacity-40"
                >
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-white/90">{encurtar(d.caminho)}</span>
                    <span className="block text-[11px] text-white/40">{d.rotulo}</span>
                  </span>
                  <span className="shrink-0 pl-4 text-xs text-white/45">
                    {cabe ? `${gib(d.livre)} livres` : "Sem espaço"}
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
