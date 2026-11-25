"use client"

// Painel de conquistas: lista com ícone 64px (colorido se desbloqueada,
// cinza se não), contador done/total no título e atualização em tempo real.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { Panel } from "./GameDetailPanels"
import { AchievementsFullScreen } from "./AchievementsFullScreen"

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
  // Tela cheia com TODAS as conquistas (mesma do painel Retro).
  const [allOpen, setAllOpen] = useState(false)

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
    setAllOpen(false)
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
  const progress = total ? Math.round((done / total) * 100) : 0

  return (
    <>
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
            {items && items.length > 6 && (
              <button
                type="button"
                onClick={() => setAllOpen(true)}
                className="detail-achievements-all"
              >
                {t("conquistas.ver_todas")}
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
                className={`min-w-0 truncate text-[13px] ${it.achieved ? "font-semibold text-white" : "text-white/40"}`}
              >
                {it.title}
              </div>
            </div>
          ))}
        </div>
      )}
    </Panel>

    {allOpen && items && items.length > 0 && (
      <AchievementsFullScreen
        done={done}
        total={total}
        progress={progress}
        onClose={() => setAllOpen(false)}
      >
        <div className="detail-achievement-full-grid grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
          {items.map((it, i) => (
            <article
              key={it.title + (it.block ?? "") + (it.bit ?? "") + i}
              className={`detail-achievement-card flex min-h-[230px] min-w-0 flex-col overflow-hidden rounded-[6px] border px-3 py-3 text-center ${it.achieved ? "border-[var(--desktop-green)]/45 bg-[var(--desktop-green)]/[.035]" : "border-white/[.08] bg-white/[.015]"}`}
            >
              <div className="detail-achievement-icon relative mx-auto mb-4 aspect-square w-20 shrink-0 overflow-hidden rounded-[5px] bg-white/5">
                {it.icon || it.icongray ? (
                  <img
                    src={it.achieved ? it.icon : it.icongray || it.icon}
                    alt=""
                    loading="lazy"
                    className={`h-full w-full object-cover ${it.achieved ? "" : "opacity-55 sepia"}`}
                  />
                ) : null}
              </div>
              <h4
                className={`line-clamp-2 text-[11px] font-semibold leading-[1.35] ${it.achieved ? "text-white/90" : "text-white/65"}`}
              >
                {it.title}
              </h4>
              <p className="mt-2 line-clamp-4 text-[9px] leading-[1.45] text-white/35">
                {it.desc || (it.achieved ? t("conquistas.concluido") : "")}
              </p>
            </article>
          ))}
        </div>
      </AchievementsFullScreen>
    )}
    </>
  )
}
