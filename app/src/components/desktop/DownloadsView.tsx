"use client"

import { useEffect, useState } from "react"
import type { DmItem } from "../../global"
import { DmCard } from "../ps5-launcher/DownloadManager"

// Downloads no modo desktop: mesmo corpo da tela do console, mas inline.
export function DownloadsView() {
  const [items, setItems] = useState<DmItem[]>([])

  useEffect(() => {
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setItems(q)
    })
    return window.launcherAPI?.onDmProgress((q) => {
      if (Array.isArray(q)) setItems(q)
    })
  }, [])

  // Ativos e falhados em seções separadas: antes vinham misturados sob o
  // título "Baixando agora", com o contador dizendo "1 ativo(s)" ao lado de
  // três cards — a tela contradizia a si mesma.
  const ativos = items.filter((i) => ["downloading", "queued", "paused"].includes(i.status))
  const parados = items.filter((i) => !["downloading", "queued", "paused"].includes(i.status))
  const baixando = ativos.some((i) => i.status === "downloading")

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <div className="mb-6 flex items-baseline justify-between">
        <h1 className="text-2xl font-light tracking-wide text-white">
          {baixando ? "Baixando agora" : "Fila de downloads"}
        </h1>
        <span className="text-sm text-white/40">
          {ativos.length} ativo(s)
          {parados.length > 0 && ` · ${parados.length} com falha`}
        </span>
      </div>

      {items.length === 0 ? (
        <div className="flex min-h-[280px] items-center justify-center text-white/35">
          Nenhum download por aqui. O que você mandar instalar pela Biblioteca ou pela aba Lojas aparece nesta tela.
        </div>
      ) : (
        <div className="flex max-w-[900px] flex-col gap-4 pb-8">
          {ativos.map((it) => (
            <DmCard key={it.appid} item={it} />
          ))}
          {parados.length > 0 && (
            <>
              <h2 className="mt-4 text-sm font-medium text-white/45">Não concluídos</h2>
              {parados.map((it) => (
                <DmCard key={it.appid} item={it} />
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
