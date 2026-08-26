"use client"

import { useEffect, useState } from "react"

const ativos = (items: { status?: string }[]) =>
  items.filter((item) => ["downloading", "queued", "paused"].includes(item.status || "")).length

export function useDownloadBadges({ includeTorrents = false } = {}) {
  const [dmCount, setDmCount] = useState(0)
  const [torrentCount, setTorrentCount] = useState(0)
  useEffect(() => {
    const update = (items: { status?: string }[]) => setDmCount(ativos(items))
    window.launcherAPI?.dmQueue().then((items) => Array.isArray(items) && update(items))
    const offDm = window.launcherAPI?.onDmProgress(update)
    const offTorrent = includeTorrents
      ? window.launcherAPI?.onTorrentProgress((items) =>
          setTorrentCount(items.filter((item) => !item.completo && !item.erro).length),
        )
      : undefined
    if (!includeTorrents) setTorrentCount(0)
    else window.launcherAPI?.torrentList().then((result) => {
      if (Array.isArray(result?.downloads)) setTorrentCount(result.downloads.filter((item) => !item.completo && !item.erro).length)
    })
    return () => {
      offDm?.()
      offTorrent?.()
    }
  }, [includeTorrents])
  return dmCount + torrentCount
}
