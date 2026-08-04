"use client"

// Painel de conquistas estilo Steam: banner de aviso offline, lista com ícone
// 64px (colorido se desbloqueada, cinza se não), contador done/total no título.
// Atualiza em tempo real via onAchievementUnlocked (watcher do main process).
// Botão de cadeado força o desbloqueio escrevendo no .bin do Steam.
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

  // Força desbloqueio escrevendo no .bin do Steam (sem cliente Steam rodando).
  const forcarDesbloqueio = async (it: ItemConquista) => {
    if (!window.confirm(t("conquistas.desbloquear_confirmar", { titulo: it.title }))) return
    const r = await window.launcherAPI?.achievementsForceUnlock(appid, it.apiname!)
    if (r?.ok) {
      setItems((prev) =>
        prev?.map((x) => (x === it ? { ...x, achieved: true, unlock: r.epoch } : x)),
      )
      window.alert(t("conquistas.desbloquear_ok"))
    } else {
      // Erro do main pode ser chave i18n (ex.: bin nunca criado) — traduz se for.
      const msg = r?.error && r.error.startsWith("conquistas.") ? t(r.error) : r?.error || ""
      window.alert(msg)
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
      {/* Aviso estilo Steam: conquistas offline não sincronizam */}
      <div className="mb-3 flex items-center gap-2 rounded-md border border-[#f5a623]/40 bg-[#f5a623]/15 px-3 py-2">
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="currentColor"
          className="h-3.5 w-3.5 shrink-0 text-[#f5a623]"
        >
          <polyline points="12 2 22 22 2 22" />
        </svg>
        <span className="text-[12px] text-[#f5a623]">{t("conquistas.aviso_offline")}</span>
      </div>

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
              {!it.achieved && it.apiname && (
                <button
                  title={t("conquistas.desbloquear")}
                  onClick={() => forcarDesbloqueio(it)}
                  className="ml-auto shrink-0 rounded-md p-1.5 text-white/40 transition-colors hover:bg-white/10 hover:text-white"
                >
                  {/* cadeado aberto */}
                  <svg
                    width="12"
                    height="12"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    className="h-3 w-3"
                  >
                    <rect x="3" y="11" width="18" height="11" rx="2" />
                    <path d="M7 11V7a5 5 0 0 1 9.9-1" />
                  </svg>
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </Panel>
  )
}
