"use client"

// Painel de conquistas: lista com ícone 64px (colorido se desbloqueada,
// cinza se não) e atualização em tempo real.
import { useCallback, useEffect, useRef, useState } from "react"
import type { TrophySummary } from "../../global"
import { useI18n } from "../../i18n/I18nContext"
import { useAccountOptional } from "../account/AccountContext"
import { TrophyDetails, TrophyStatus, unavailableTrophies } from "../TrophyDetails"
import { Panel } from "./GameDetailPanels"

export function AchievementsPanel({ appid, title }: { appid: string; title?: string }) {
  const { t } = useI18n()
  const account = useAccountOptional()?.session?.user?.id
  const scope = `${account ?? ""}:${appid}`
  const [result, setResult] = useState<{ scope: string; summary: TrophySummary } | null>(null)
  const summary = result?.scope === scope ? result.summary : null
  const items = summary?.items ?? null
  const [allOpen, setAllOpen] = useState(false)
  const closeTrophies = useCallback(() => setAllOpen(false), [])
  const [feedback, setFeedback] = useState<{ texto: string; cor: "ok" | "erro" } | null>(null)
  const request = useRef(0)
  const refresh = useCallback(async () => {
    const id = ++request.current
    let next
    try { next = await window.launcherAPI?.achievementsTrophySummary(appid) }
    catch { next = unavailableTrophies }
    if (id !== request.current || next === null) return
    const data = next ?? unavailableTrophies
    setResult(old => ({ scope, summary: data.stale && data.progress === "unknown" && old?.scope === scope && old.summary.items.length
      ? { ...old.summary, stale: true } : data }))
  }, [appid, scope])

  useEffect(() => {
    setAllOpen(false)
    const api = window.launcherAPI
    void refresh()
    const offUnlock = api?.onAchievementUnlocked(payload => { if (payload.appid === appid) void refresh() })
    const offSync = api?.onSyncState?.(() => { void refresh() })
    const offLibrary = api?.onLibraryChanged?.(() => { void refresh() })
    return () => { request.current++; offUnlock?.(); offSync?.(); offLibrary?.() }
  }, [appid, refresh])

  const recarregarSchema = async () => {
    setFeedback(null)
    const r = await window.launcherAPI?.achievementsSchemasLoad().catch(() => null)
    setFeedback({
      texto: r?.ok ? t("conquistas.schemas_ok", { n: r.updated ?? 0 }) : t("conquistas.schemas_erro"),
      cor: r?.ok ? "ok" : "erro",
    })
    setTimeout(() => setFeedback(null), 3000)
    await refresh()
  }

  return (
    <>
    <Panel
      title={t("trofeus.titulo")}
      right={
        <span className="flex items-center gap-2">
          <span className="flex items-center gap-1.5">
            <button
              onClick={recarregarSchema}
              title={t("conquistas.atualizar_schema")}
              className="rounded-md border border-white/10 px-1.5 py-0.5 text-[10px] text-white/50 transition-colors hover:border-white/25 hover:text-white"
            >
              {t("conquistas.atualizar_schema")}
            </button>
            {summary && (
              <button
                type="button"
                onClick={() => setAllOpen(true)}
                className="detail-achievements-all"
              >
                {t("trofeus.abrir")}
              </button>
            )}
          </span>
          {feedback && (
            <span
              className={`text-[11px] ${feedback.cor === "ok" ? "text-emerald-400" : "text-red-400"}`}
            >
              {feedback.texto}
            </span>
          )}
        </span>
      }
    >
      {items === null && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="flex items-center gap-3">
              <div className="h-12 w-12 shrink-0 animate-pulse rounded-md bg-white/5" />
              <div className="flex flex-col gap-2">
                <div className="h-3 w-32 animate-pulse rounded bg-white/5" />
                <div className="h-3 w-20 animate-pulse rounded bg-white/5" />
              </div>
            </div>
          ))}
        </div>
      )}

      {summary && <p role="status" className="text-xs text-[var(--text-2)]"><TrophyStatus summary={summary} /></p>}
      {summary?.stale && <p className="text-xs text-[var(--text-2)]">{t("trofeus.leitura_falhou")}</p>}

      {items && items.length > 0 && (
        <div className="max-h-[420px] space-y-1 overflow-y-auto pr-1">
          {items.map((it, i) => (
            <div
              key={`${it.apiname ?? it.title}:${i}`}
              className="flex items-center gap-3 rounded-md p-2 hover:bg-white/[0.04]"
            >
              {(!it.hidden || it.achieved) && (it.icon || it.icongray) ? (
                <img
                  src={it.achieved ? it.icon || it.icongray : it.icongray || it.icon}
                  alt=""
                  loading="lazy"
                  className="h-12 w-12 shrink-0 rounded-md object-cover"
                  style={!it.achieved ? { filter: "grayscale(0.85) opacity(0.5)" } : undefined}
                />
              ) : (
                <div className="h-12 w-12 shrink-0 rounded-md bg-white/5 ring-1 ring-white/10" />
              )}
              <div
                className={`min-w-0 truncate text-[13px] ${it.achieved ? "font-semibold text-white" : "text-white/40"}`}
              >
                {it.hidden && !it.achieved ? t("trofeus.secreto") : it.title}
              </div>
            </div>
          ))}
        </div>
      )}
    </Panel>

    {allOpen && <TrophyDetails key={scope} title={title} summary={summary} source="steam" onClose={closeTrophies} onCapture={async () => {
      const r = await window.launcherAPI?.steamCapturarAgora()
      if (r?.ok) await refresh()
      return r?.ok === true
    }} />}
    </>
  )
}
