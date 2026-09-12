"use client"

import { useDownloadsFeed } from "../downloads/useDownloadsFeed"
import { DownloadCard } from "../downloads/DownloadCard"
import { useI18n } from "../../i18n/I18nContext"
import type { DownloadsFeed } from "../downloads/normalize"

// Aba Downloads (desktop). Uma lista só para os dois subsistemas (fila
// Epic/Steam + torrent/HTTP/debrid) — antes a seção de torrent era um bloco
// separado com contador e card próprios, e a tela se contradizia ("0 ativo"
// com torrent baixando na frente).
export function DownloadsView({ feed: feedProp }: { feed?: DownloadsFeed }) {
  const { t } = useI18n()
  const ownFeed = useDownloadsFeed(!feedProp)
  const feed = feedProp ?? ownFeed
  const baixando = feed.ativos.some((i) => i.status === "active")

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <div className="mb-6 flex items-baseline justify-between">
        <h1 className="ui-title">
          {baixando ? t("downloads.baixando_agora") : t("downloads.fila")}
        </h1>
        <span className="text-sm text-white/40">
          {t("downloads.ativos", { count: String(feed.ativosCount) })}
          {feed.falhasCount > 0 &&
            ` · ${t("downloads.com_falha", { count: String(feed.falhasCount) })}`}
        </span>
      </div>

      {feed.total === 0 ? (
        <div className="flex min-h-[320px] flex-col items-center justify-center gap-2 text-center">
          <p className="text-lg font-semibold text-white/70">{t("downloads.vazio_titulo")}</p>
          <p className="max-w-[380px] text-sm text-white/35">{t("downloads.vazio_sub")}</p>
        </div>
      ) : (
        <div className="desktop-fluid-column flex max-w-[900px] flex-col gap-4 pb-8">
          {feed.ativos.map((it) => (
            <DownloadCard key={it.id} item={it} />
          ))}
          {feed.concluidos.length > 0 && (
            <>
              <h2 className="ui-section-title mt-4">{t("downloads.secao.concluidos")}</h2>
              {feed.concluidos.map((it) => (
                <DownloadCard key={it.id} item={it} />
              ))}
            </>
          )}
          {feed.falhas.length > 0 && (
            <>
              <h2 className="ui-section-title mt-4">{t("downloads.secao.falhas")}</h2>
              {feed.falhas.map((it) => (
                <DownloadCard key={it.id} item={it} />
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
