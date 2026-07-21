"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import type { YoutubeResult } from "../../global"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"

interface TrailerPickerProps {
  game: Game | null
  onClose: () => void
  onPicked: (gameId: string, path: string) => void
}

function fmtDur(s: number): string {
  if (!s) return ""
  const m = Math.floor(s / 60)
  const sec = String(s % 60).padStart(2, "0")
  return `${m}:${sec}`
}

export function TrailerPicker({ game, onClose, onPicked }: TrailerPickerProps) {
  const ref = useRef<HTMLDivElement>(null)
  const { t } = useI18n()
  const open = Boolean(game)
  // Sem player aberto, a navegação por controle vale para a grade.
  const [preview, setPreview] = useState<YoutubeResult | null>(null)
  const [downloading, setDownloading] = useState(false)
  // Voltar (B/Esc): da prévia volta pra grade; da grade fecha o seletor.
  const back = useCallback(() => {
    setPreview((p) => {
      if (p) return null
      onClose()
      return p
    })
  }, [onClose])
  // Navegação por controle ativa na grade E na prévia (para alcançar "Baixar");
  // só desliga durante o download.
  useGamepadNav(ref, open && !downloading, back)

  const [results, setResults] = useState<YoutubeResult[]>([])
  const [loading, setLoading] = useState(true)
  const [percent, setPercent] = useState(0)
  const [stage, setStage] = useState("download")
  const [streamUrl, setStreamUrl] = useState<string | null>(null)
  const [streamLoading, setStreamLoading] = useState(false)
  const [streamErr, setStreamErr] = useState<string | null>(null)
  const [dlError, setDlError] = useState<string | null>(null)
  const [searchErr, setSearchErr] = useState<string | null>(null)

  const AGE_MSG = t("trailer.restricao_idade")

  // Busca os vídeos do YouTube ao abrir.
  useEffect(() => {
    if (!game) return
    setLoading(true)
    setResults([])
    setPreview(null)
    setDownloading(false)
    setSearchErr(null)
    window.launcherAPI
      ?.trailerSearch(game.title)
      .then((r) => {
        // Antes fazíamos só `r?.results ?? []`, o que transformava QUALQUER
        // falha em "Nenhum vídeo encontrado" — a tela mentia sobre a causa.
        setResults(r?.results ?? [])
        if (r && !r.ok) setSearchErr(r.error || "erro desconhecido")
      })
      .finally(() => setLoading(false))
  }, [game])

  // Progresso do download (só do jogo atual).
  useEffect(() => {
    if (!game) return
    return window.launcherAPI?.onTrailerDlProgress((d) => {
      if (d.id !== game.id) return
      setPercent(d.percent)
      setStage(d.stage)
    })
  }, [game])

  // Esc: fecha o player se aberto; senão fecha tudo.
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return
      if (downloading) return
      if (preview) setPreview(null)
      else onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, preview, downloading, onClose])

  // Ao abrir o player, pega a URL direta (o embed do YouTube recusa file://).
  const abrirPreview = async (v: YoutubeResult) => {
    setPreview(v)
    setStreamUrl(null)
    setStreamErr(null)
    setDlError(null)
    setStreamLoading(true)
    const r = await window.launcherAPI?.trailerStreamUrl(v.url)
    setStreamLoading(false)
    if (r?.ok && r.url) setStreamUrl(r.url)
    else setStreamErr(r?.error === "age" ? AGE_MSG : null)
  }

  if (!game) return null

  const baixar = async (v: YoutubeResult) => {
    setDownloading(true)
    setDlError(null)
    setPercent(0)
    setStage("download")
    const r = await window.launcherAPI?.trailerDownloadUrl(game.id, v.url)
    setDownloading(false)
    if (r?.ok && r.path) {
      onPicked(game.id, r.path) // aplica automaticamente
      onClose()
    } else {
      setDlError(r?.error === "age" ? AGE_MSG : r?.error || t("trailer.falha_baixar"))
    }
  }

  return (
    <>
      <div className="fixed inset-0 z-[60]" style={{ background: "rgba(0,0,0,0.8)" }} />
      <div
        ref={ref}
        className="gp-scope fixed z-[61] left-1/2 top-1/2 w-[980px] max-w-[95vw] max-h-[90vh] rounded-2xl overflow-hidden flex flex-col"
        style={{
          transform: "translate(-50%, -50%)",
          background: "rgba(10,22,54,0.98)",
          border: "1px solid rgba(255,255,255,0.14)",
          boxShadow: "0 24px 70px rgba(0,0,0,0.7)",
        }}
      >
        {/* Cabeçalho */}
        <div className="px-6 py-4 flex items-center justify-between shrink-0" style={{ background: "rgba(0,0,0,0.5)" }}>
          <div className="min-w-0">
            <h2 className="text-white text-lg font-bold truncate">
              {downloading ? t("trailer.baixando_trailer") : preview ? t("trailer.preview") : t("trailer.escolher")}
            </h2>
            <p className="text-xs text-[#8a93a6] truncate">
              {game.title} —               {downloading || preview ? "" : t("trailer.resultados_youtube")}
            </p>
          </div>
          {!downloading && (
            <button
              onClick={() => (preview ? setPreview(null) : onClose())}
              className="px-4 py-2 rounded-xl text-sm font-semibold text-white shrink-0"
              style={{ background: "rgba(255,255,255,0.08)" }}
            >
              {preview ? t("trailer.voltar") : t("trailer.fechar")}
            </button>
          )}
        </div>

        {/* ── Painel de download ─────────────────────────────────────────── */}
        {downloading && preview ? (
          <div className="p-6 flex gap-5 items-center">
            <img
              src={preview.thumbnail}
              alt=""
              className="w-56 h-[126px] object-cover rounded-xl shrink-0"
            />
            <div className="flex-1 min-w-0">
              <div className="text-white font-semibold line-clamp-2">{preview.title}</div>
              <div className="text-sm text-[#8a93a6] mt-1">
                {preview.channel}
                {preview.duration ? ` · ${fmtDur(preview.duration)}` : ""}
              </div>
              <div className="mt-4">
                <div className="h-2.5 rounded-full overflow-hidden" style={{ background: "rgba(255,255,255,0.1)" }}>
                  <div
                    className="h-full rounded-full transition-all"
                    style={{ width: `${percent}%`, background: "var(--accent)" }}
                  />
                </div>
                <div className="text-xs text-[#8a93a6] mt-2">
                  {stage === "processando"
                    ? t("trailer.processando")
                    : stage === "done"
                      ? t("trailer.aplicando")
                      : t("trailer.baixando_pct", { pct: String(Math.round(percent)) })}
                </div>
              </div>
            </div>
          </div>
        ) : preview ? (
          /* ── Player embutido (assiste como no YouTube) ─────────────────── */
          <div className="p-5">
            <div
              className="relative w-full rounded-xl overflow-hidden flex items-center justify-center"
              style={{ height: "58vh", background: "#000" }}
            >
              {streamLoading ? (
                <div className="text-[#8a93a6]">{t("trailer.carregando_video")}</div>
              ) : streamUrl ? (
                <video
                  className="w-full h-full object-contain"
                  src={streamUrl}
                  controls
                  autoPlay
                  playsInline
                />
              ) : (
                <div className="text-[#8a93a6] px-8 text-center leading-relaxed">
                  {streamErr || t("trailer.sem_previa")}
                </div>
              )}
            </div>
            <div className="flex items-center justify-between gap-4 mt-4 shrink-0">
              <div className="min-w-0">
                <div className="text-white font-semibold line-clamp-1">{preview.title}</div>
                <div className="text-sm text-[#8a93a6]">
                  {preview.channel}
                  {preview.duration ? ` · ${fmtDur(preview.duration)}` : ""}
                </div>
                {dlError && (
                  <div className="text-xs text-[#ff6b81] mt-1 line-clamp-2">{dlError}</div>
                )}
              </div>
              <button
                onClick={() => baixar(preview)}
                className="px-6 py-2.5 rounded-xl text-sm font-bold text-white shrink-0"
                style={{ background: "linear-gradient(135deg, #ff4d6d, #c81e45)" }}
              >
                {dlError ? t("trailer.tentar_novamente") : t("trailer.baixar_aplicar")}
              </button>
            </div>
          </div>
        ) : (
          /* ── Grade de resultados ───────────────────────────────────────── */
          <div className="overflow-y-auto p-5">
            {loading ? (
              <div className="text-center text-[#8a93a6] py-16">{t("trailer.buscando_youtube")}</div>
            ) : searchErr ? (
              <div className="text-center py-16 px-6">
                <div className="text-[#ff6b6b]">{t("trailer.falha_busca")}</div>
                <div className="text-[#8a93a6] text-sm mt-2 break-words">{searchErr}</div>
              </div>
            ) : results.length === 0 ? (
              <div className="text-center text-[#8a93a6] py-16">{t("trailer.nenhum_video")}</div>
            ) : (
              <div className="grid grid-cols-2 gap-4">
                {results.map((v) => (
                  <button
                    key={v.id}
                    onClick={() => abrirPreview(v)}
                    className="flex gap-3 p-2 rounded-xl text-left outline-none focus:ring-2"
                    style={{ background: "rgba(255,255,255,0.05)" }}
                  >
                    <div className="relative flex-shrink-0" style={{ width: 160 }}>
                      {v.thumbnail ? (
                        <img src={v.thumbnail} alt="" className="w-40 h-[90px] object-cover rounded-lg" loading="lazy" />
                      ) : (
                        <div className="w-40 h-[90px] rounded-lg" style={{ background: "#000" }} />
                      )}
                      {v.duration > 0 && (
                        <span
                          className="absolute bottom-1 right-1 text-[11px] text-white px-1.5 py-0.5 rounded"
                          style={{ background: "rgba(0,0,0,0.8)" }}
                        >
                          {fmtDur(v.duration)}
                        </span>
                      )}
                      {/* Selo de "play" para deixar claro que abre o vídeo */}
                      <span className="absolute inset-0 flex items-center justify-center opacity-0 hover:opacity-100 transition-opacity" style={{ background: "rgba(0,0,0,0.3)" }}>
                        <svg width="34" height="34" viewBox="0 0 24 24" fill="#fff"><path d="M8 5v14l11-7z" /></svg>
                      </span>
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="text-white text-sm font-medium line-clamp-2">{v.title}</div>
                      <div className="text-xs text-[#8a93a6] mt-1 truncate">{v.channel}</div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </>
  )
}
