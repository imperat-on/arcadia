"use client"

import { useEffect, useState } from "react"
import type { TorrentItem } from "../../global"
import { fmtBytes } from "../tamanho"
import { useI18n } from "../../i18n/I18nContext"

// Seção de downloads torrent dentro da aba Downloads. Subsistema separado do
// dm (Steam/Epic): ids "tor:...", evento próprio, card próprio. Nunca mistura
// os dois nas mesmas listas — o dm não saberia pausar um torrent e vice-versa.
export function TorrentSection() {
  const { t } = useI18n()
  const [items, setItems] = useState<TorrentItem[]>([])

  useEffect(() => {
    window.launcherAPI?.torrentList().then((r) => {
      if (r?.downloads) setItems(r.downloads)
    })
    return window.launcherAPI?.onTorrentProgress((q) => {
      if (Array.isArray(q)) setItems(q)
    })
  }, [])

  if (!items.length) return null

  const ativos = items.filter((i) => !i.completo)
  const completos = items.filter((i) => i.completo)

  return (
    <div className="mt-8 flex max-w-[900px] flex-col gap-4">
      <h2 className="ui-section-title">{t("torrent.secao")}</h2>
      {ativos.map((it) => (
        <TorrentCard key={it.gameId} item={it} />
      ))}
      {completos.length > 0 && (
        <>
          <h3 className="mt-2 text-sm font-medium text-white/45">{t("torrent.concluidos")}</h3>
          {completos.map((it) => (
            <TorrentCard key={it.gameId} item={it} />
          ))}
        </>
      )}
    </div>
  )
}

function TorrentCard({ item: it }: { item: TorrentItem }) {
  const { t } = useI18n()
  const [coverQuebrou, setCoverQuebrou] = useState(false)
  const pct = it.completo ? 100 : Math.round((it.progress || 0) * 100)
  const baixando = !it.pausado && !it.completo
  // Concluído guarda o tamanho final no estado (o polling para e os campos
  // vivos somem) — sem isto o card mostrava "— / — · 0%".
  const bytes = it.completo ? it.fileSize : it.bytesDownloaded

  return (
    <div
      className={`flex items-center gap-5 rounded-2xl border p-4 transition-colors ${
        baixando ? "border-[color:var(--accent)]" : "border-white/10"
      } bg-white/[0.03]`}
      style={baixando ? { boxShadow: "0 0 30px -8px var(--accent)" } : undefined}
    >
      {it.cover && !coverQuebrou ? (
        <img
          src={it.cover}
          alt=""
          className="h-20 w-14 shrink-0 rounded-lg object-cover ring-1 ring-white/10"
          draggable={false}
          onError={() => setCoverQuebrou(true)}
        />
      ) : (
        <div className="flex h-20 w-14 shrink-0 items-center justify-center rounded-lg bg-white/5 ring-1 ring-white/10">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" className="text-white/40">
            <path d="M12 3v12" /><path d="m7 11 5 5 5-5" /><path d="M5 21h14" />
          </svg>
        </div>
      )}

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-4">
          <h3 className="truncate text-base font-medium">{it.title || it.folderName || it.gameId}</h3>
          <span className="shrink-0 text-xs text-white/50">
            {it.erro
              ? <span className="text-[#ff6b81]">{t("torrent.status.erro")}</span>
              : it.completo
                ? t("torrent.status.completo")
                : it.pausado
                  ? t("torrent.status.pausado")
                  : it.cacheando
                    ? t("torrent.status.cacheando")
                    : t("torrent.status.baixando")}
          </span>
        </div>

        <div className="mt-2.5 h-1.5 w-full overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{ width: `${pct}%`, background: "var(--accent)", boxShadow: baixando ? "0 0 12px var(--accent)" : "none" }}
          />
        </div>

        <div className="mt-2 flex items-baseline justify-between text-xs text-white/50">
          <span className="tabular-nums">
            {it.cacheando
              ? <span className="text-white/40">{it.erro || t("torrent.cacheando_desc")}</span>
              : <>{fmtBytes(bytes)} / {fmtBytes(it.fileSize)} · {pct}%</>}
          </span>
          {baixando && (
            <span className="tabular-nums text-white/70">
              {fmtBytes(it.downloadSpeed)}/s
              {it.engine !== "http" && <> · {t("torrent.peers", { count: String(it.numPeers || 0) })}</>}
              {it.engine === "http" && <> · HTTP</>}
            </span>
          )}
        </div>
      </div>

      <div className="flex shrink-0 flex-col gap-2">
        {baixando && (
          <Acao label={t("downloads.pausar")} onClick={() => window.launcherAPI?.torrentPause(it.gameId)} />
        )}
        {it.pausado && !it.completo && (
          <Acao label={t("downloads.retomar")} primaria onClick={() => window.launcherAPI?.torrentResume(it.gameId)} />
        )}
        <Acao
          label={it.completo ? t("common.remover") : t("common.cancelar")}
          onClick={() => window.launcherAPI?.torrentCancel(it.gameId)}
        />
      </div>
    </div>
  )
}

function Acao({ label, onClick, primaria }: { label: string; onClick: () => void; primaria?: boolean }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-lg px-3.5 py-1.5 text-[11px] font-semibold transition-colors ${
        primaria
          ? "text-black hover:scale-[1.03]"
          : "border border-white/15 text-white/70 hover:bg-white/[0.06] hover:text-white"
      }`}
      style={primaria ? { background: "var(--accent)" } : undefined}
    >
      {label}
    </button>
  )
}
