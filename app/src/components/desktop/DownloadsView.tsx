"use client"

import { useEffect, useState } from "react"
import type { DmItem } from "../../global"
import { DmCard } from "../ps5-launcher/DownloadManager"
import { useI18n } from "../../i18n/I18nContext"

export function DownloadsView() {
  const { t } = useI18n()
  const [items, setItems] = useState<DmItem[]>([])

  useEffect(() => {
    window.launcherAPI?.dmQueue().then((q) => {
      if (Array.isArray(q)) setItems(q)
    })
    return window.launcherAPI?.onDmProgress((q) => {
      if (Array.isArray(q)) setItems(q)
    })
  }, [])

  const ativos = items.filter((i) => ["downloading", "queued", "paused"].includes(i.status))
  const parados = items.filter((i) => !["downloading", "queued", "paused"].includes(i.status))
  const baixando = ativos.some((i) => i.status === "downloading")

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <div className="mb-6 flex items-baseline justify-between">
        <h1 className="text-2xl font-light tracking-wide text-white">
          {baixando ? t("downloads.baixando_agora") : t("downloads.fila")}
        </h1>
        <span className="text-sm text-white/40">
          {t("downloads.ativos", { count: String(ativos.length) })}
          {parados.length > 0 && ` · ${t("downloads.com_falha", { count: String(parados.length) })}`}
        </span>
      </div>

      {items.length === 0 ? (
        <div className="flex min-h-[280px] items-center justify-center text-white/35">
          {t("downloads.vazio")}
        </div>
      ) : (
        <div className="flex max-w-[900px] flex-col gap-4 pb-8">
          {ativos.map((it) => (
            <DmCard key={it.appid} item={it} />
          ))}
          {parados.length > 0 && (
            <>
              <h2 className="mt-4 text-sm font-medium text-white/45">{t("downloads.nao_concluidos")}</h2>
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
