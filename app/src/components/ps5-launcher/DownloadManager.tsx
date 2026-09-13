"use client"

import { forwardRef } from "react"
import { useDownloadsFeed } from "../downloads/useDownloadsFeed"
import { DownloadCard } from "../downloads/DownloadCard"
import { useI18n } from "../../i18n/I18nContext"

interface DownloadManagerProps {
  onClose: () => void
}

// Tela de downloads do console (estilo PS5). Mesma lista unificada do desktop:
// fila Epic/Steam + torrent/HTTP/debrid no MESMO card — antes o P2P era
// invisível aqui e o usuário do Big Picture não via download de magnet nenhum.
export const DownloadManager = forwardRef<HTMLDivElement, DownloadManagerProps>(
  function DownloadManager({ onClose }, ref) {
    const { t } = useI18n()
    const feed = useDownloadsFeed()
    const baixando = feed.ativos.some((i) => i.status === "active")

    return (
      <div
        ref={ref}
        className="retro-download-shell gp-scope fixed inset-0 z-50 overflow-y-auto bg-black/95 text-white antialiased backdrop-blur-xl"
      >
        {/* Glow ambiente de acento no topo (assinatura da tela) */}
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-64"
          style={{
            background:
              "radial-gradient(ellipse 60% 100% at 50% 0%, color-mix(in oklab, var(--accent) 10%, transparent), transparent 70%)",
          }}
        />
        <div className="relative mx-auto max-w-[1100px] px-10 py-8">
          {/* Cabeçalho */}
          <div className="mb-2 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.26em] text-white/55">
            <span
              className="inline-block h-[6px] w-[6px] rounded-full shadow-[0_0_8px_var(--accent)]"
              style={{ background: "var(--accent)" }}
            />
            {t("downloads.titulo")}
          </div>
          <div className="mb-8 flex items-baseline justify-between">
            <h1
              className="game-name text-3xl font-bold"
              style={{
                background:
                  "linear-gradient(120deg, #fff 55%, color-mix(in oklab, var(--accent) 80%, #fff))",
                WebkitBackgroundClip: "text",
                backgroundClip: "text",
                color: "transparent",
              }}
            >
              {baixando
                ? t("downloads.baixando_agora")
                : feed.ativosCount
                  ? t("downloads.status.na_fila")
                  : t("downloads.fila")}
            </h1>
            <span className="text-sm text-white/45">
              {t("downloads.ativos", { count: String(feed.ativosCount) })}
              {feed.falhasCount > 0 &&
                ` · ${t("downloads.com_falha", { count: String(feed.falhasCount) })}`}
            </span>
          </div>

          {feed.total === 0 ? (
            <div className="flex min-h-[300px] flex-col items-center justify-center gap-2 text-center">
              <p className="text-lg font-semibold text-white/60">{t("downloads.vazio_titulo")}</p>
              <p className="max-w-[420px] text-sm text-white/35">{t("downloads.vazio_sub")}</p>
            </div>
          ) : (
            <div className="flex flex-col gap-4 pb-10">
              {feed.ativos.map((it) => (
                <DownloadCard key={it.id} item={it} />
              ))}
              {feed.concluidos.length > 0 && (
                <>
                  <h2 className="mt-4 flex items-center gap-2 text-sm font-semibold text-white/55">
                    <span
                      className="inline-block h-1 w-1 rounded-full"
                      style={{ background: "var(--accent)" }}
                    />
                    {t("downloads.secao.concluidos")}
                  </h2>
                  {feed.concluidos.map((it) => (
                    <DownloadCard key={it.id} item={it} />
                  ))}
                </>
              )}
              {feed.falhas.length > 0 && (
                <>
                  <h2 className="mt-4 flex items-center gap-2 text-sm font-semibold text-white/55">
                    <span
                      className="inline-block h-1 w-1 rounded-full"
                      style={{ background: "var(--state-danger)" }}
                    />
                    {t("downloads.secao.falhas")}
                  </h2>
                  {feed.falhas.map((it) => (
                    <DownloadCard key={it.id} item={it} />
                  ))}
                </>
              )}
            </div>
          )}

          <div className="flex items-center justify-end gap-6 pb-2 text-xs text-white/40">
            <button
              onClick={onClose}
              className="outline-none transition-colors hover:text-white/70 focus-visible:text-[color:var(--accent)]"
            >
              {t("gameoverview.controle.voltar")}
            </button>
          </div>
        </div>
      </div>
    )
  },
)
