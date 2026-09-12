"use client"

import { useEffect, useMemo, useState } from "react"
import type { DmItem, TorrentItem } from "../../global"
import { buildFeed } from "./normalize"
import type { DownloadsFeed } from "./normalize"

// Feed único de downloads para as duas UIs: assina a fila do dm E o
// subsistema torrent, e devolve a lista já normalizada/agrupada.
export function useDownloadsFeed(): DownloadsFeed {
  const [dm, setDm] = useState<DmItem[]>([])
  const [tor, setTor] = useState<TorrentItem[]>([])

  useEffect(() => {
    // Carga inicial + assinatura dos dois canais. Cada um atualiza a própria
    // fatia — nenhum dos dois pode apagar o outro.
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setDm(q)
    })
    window.launcherAPI?.torrentList().then((r) => {
      if (Array.isArray(r?.downloads)) setTor(r.downloads)
    })
    const offDm = window.launcherAPI?.onDmProgress((q) => {
      if (Array.isArray(q)) setDm(q)
    })
    const offTor = window.launcherAPI?.onTorrentProgress((q) => {
      if (Array.isArray(q)) setTor(q)
    })
    return () => {
      offDm?.()
      offTor?.()
    }
  }, [])

  return useMemo(() => buildFeed(dm, tor), [dm, tor])
}
