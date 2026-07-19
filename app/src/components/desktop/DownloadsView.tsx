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

  const atual = items.find((i) => i.status === "downloading")

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <div className="mb-6 flex items-baseline justify-between">
        <h1 className="text-2xl font-light tracking-wide text-white">
          {atual ? "Baixando agora" : "Fila de downloads"}
        </h1>
        <span className="text-sm text-white/40">
          {items.filter((i) => ["downloading", "queued", "paused"].includes(i.status)).length} ativo(s)
        </span>
      </div>

      {items.length === 0 ? (
        <div className="flex min-h-[280px] items-center justify-center text-white/35">
          Nenhum download por aqui. Jogos Epic que você mandar instalar aparecem nesta tela.
        </div>
      ) : (
        <div className="flex max-w-[900px] flex-col gap-4 pb-8">
          {items.map((it) => (
            <DmCard key={it.appid} item={it} />
          ))}
        </div>
      )}
    </div>
  )
}
