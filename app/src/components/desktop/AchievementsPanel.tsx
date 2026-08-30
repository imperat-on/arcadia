"use client"

// Painel de conquistas: lista com ícone 64px (colorido se desbloqueada,
// cinza se não), contador done/total no título e atualização em tempo real.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { Panel } from "./GameDetailPanels"

type ItemConquista = {
  title: string
  desc?: string
  icon?: string
  icongray?: string
  apiname?: string
  block?: number | null
  bit?: number | null
  achieved?: boolean
  unlock?: number
  percent?: number
}

export function AchievementsPanel({ appid }: { appid: string }) {
  const { t } = useI18n()
  const [items, setItems] = useState<ItemConquista[] | null>(null)
  const [feedback, setFeedback] = useState<{ texto: string; cor: "ok" | "erro" } | null>(null)

  // Recarrega apiname/título/desc/ícones dos itens a partir dos schemas da Steam.
  const recarregarSchema = async () => {
    setFeedback(null)
    const r = await window.launcherAPI?.achievementsSchemasLoad()
    setFeedback({
      texto: r?.ok
        ? t("conquistas.schemas_ok", { n: String(r.updated ?? 0) })
        : t("conquistas.schemas_erro"),
      cor: r?.ok ? "ok" : "erro",
    })
    setTimeout(() => setFeedback(null), 3000)
    // Itens podem ter ganho apiname/ícone novo — recarrega a lista.
    const arr = await window.launcherAPI?.achievementsGet(appid)
    setItems(arr || null)
  }

  // Fetch inicial: null = carregando; [] = sem conquistas (após fetch).
  useEffect(() => {
    let vivo = true
    setItems(null)
    window.launcherAPI?.achievementsGet(appid).then((arr) => {
      if (vivo) setItems(arr || [])
    })
    return () => {
      vivo = false
    }
  }, [appid])

  // Tempo real: marca a conquista desbloqueada no painel aberto.
  useEffect(() => {
    const off = window.launcherAPI?.onAchievementUnlocked((payload) => {
      if (payload.appid !== appid) return
      setItems((prev) => {
        if (!prev) return prev
        const idx = prev.findIndex(
          (it) =>
            (it.block != null && it.bit != null && `${it.block}|${it.bit}` === payload.key) ||
            it.title === payload.title,
        )
        if (idx === -1) {
          // Não achou no índice (ex.: scrape da loja sem block/bit): cria o item.
          return [
            ...prev,
            {
              title: payload.title,
              desc: payload.desc || "",
              icon: payload.icon || "",
              icongray: payload.icon || "",
              apiname: undefined,
              block: null,
              bit: null,
              achieved: true,
              unlock: payload.unlock || 0,
              percent: payload.percent || 0,
            },
          ]
        }
        return prev.map((x, i) =>
          i === idx ? { ...x, achieved: true, unlock: payload.unlock } : x,
        )
      })
    })
    return off
  }, [appid])

  const done = items ? items.filter((x) => x.achieved).length : 0
  const total = items ? items.length : 0

  const deleteAchievement = async (apiname: string) => {
    if (!window.confirm(`Remover "${apiname}" do servidor?`)) return
    try {
      await window.launcherAPI?.syncDeleteAchievement(appid, apiname)
      // Remove from local
      setItems((prev) => prev?.filter((x) => x.apiname !== apiname) || [])
      window.alert("Conquista removida do servidor!")
    } catch (e) {
      window.alert("Erro ao remover: " + String(e))
    }
  }

  return (
    <Panel
      title={t("conquistas.titulo")}
      right={
        <span className="flex items-center gap-2">
          <span className="flex items-center gap-1.5">
            {items ? t("conquistas.contador", { done: String(done), total: String(total) }) : "…"}
            <button
              onClick={recarregarSchema}
              title={t("conquistas.atualizar_schema")}
              className="rounded-md border border-white/10 px-1.5 py-0.5 text-[10px] text-white/50 transition-colors hover:border-white/25 hover:text-white"
            >
              {t("conquistas.atualizar_schema")}
            </button>
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
      {items === null && total === 0 && (
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

      {items !== null && items.length === 0 && (
        <p className="text-[12px] text-white/45">{t("conquistas.vazio")}</p>
      )}

      {items && items.length > 0 && (
        <div className="max-h-[420px] space-y-1 overflow-y-auto pr-1">
          {items.map((it) => (
            <div
              key={it.title + (it.block ?? "") + (it.bit ?? "")}
              className="flex items-center gap-3 rounded-md p-2 hover:bg-white/[0.04]"
            >
              {it.icon || it.icongray ? (
                <img
                  src={it.achieved ? it.icon : it.icongray || it.icon}
                  alt=""
                  loading="lazy"
                  className="h-12 w-12 shrink-0 rounded-md object-cover"
                  style={!it.achieved ? { filter: "grayscale(0.85) opacity(0.5)" } : undefined}
                />
              ) : (
                <div className="h-12 w-12 shrink-0 rounded-md bg-white/5 ring-1 ring-white/10" />
              )}
              <div
                className={`min-w-0 flex-1 truncate text-[13px] ${it.achieved ? "font-semibold text-white" : "text-white/40"}`}
              >
                {it.title}
              </div>
              <button
                onClick={() => deleteAchievement(it.apiname!)}
                className="shrink-0 rounded p-1 text-white/30 transition-colors hover:bg-red-500/20 hover:text-red-400"
                title="Remover do servidor"
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                </svg>
              </button>
            </div>
          ))}
        </div>
      )}
    </Panel>
  )
}
