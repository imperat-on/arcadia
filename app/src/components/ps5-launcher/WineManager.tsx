"use client"

import { useEffect, useMemo, useState } from "react"
import type { WineVer } from "../../global"
import { fmtMiB } from "../tamanho"
import { userLocale } from "../../i18n/locale"
import { useI18n } from "../../i18n/I18nContext"

// Wine Manager estilo Heroic: abas GE-Proton / Wine-GE / Steam, lista com
// busca, data de lançamento, tamanho e ações (baixar / remover / abrir pasta).

type Tab = "ge-proton" | "wine-ge" | "steam"

function fmtDate(iso?: string, t?: (k: string) => string) {
  if (!iso) return t ? t("winemanager.data_fallback") : "—"
  const d = new Date(iso)
  if (Number.isNaN(+d)) return t ? t("winemanager.data_fallback") : "—"
  return d.toLocaleDateString(userLocale(), { day: "2-digit", month: "2-digit", year: "numeric" })
}

export function WineSection() {
  const { t } = useI18n()

  const TABS: { id: Tab; label: string; hint: string }[] = [
    { id: "ge-proton", label: t("winemanager.tab.ge_proton"), hint: t("winemanager.tab.ge_proton_hint") },
    { id: "wine-ge", label: t("winemanager.tab.wine_ge"), hint: t("winemanager.tab.wine_ge_hint") },
    { id: "steam", label: t("winemanager.tab.proton_steam"), hint: t("winemanager.tab.proton_steam_hint") },
  ]

  const [tab, setTab] = useState<Tab>("ge-proton")
  const [query, setQuery] = useState("")
  const [installed, setInstalled] = useState<WineVer[]>([])
  const [available, setAvailable] = useState<WineVer[]>([])
  const [busy, setBusy] = useState("")
  const [progress, setProgress] = useState<{ id: string; pct: number } | null>(null)
  const [err, setErr] = useState("")

  const recarregar = () => {
    window.launcherAPI?.wineList().then((r) => {
      setInstalled(r?.installed || [])
      setAvailable(r?.available || [])
      if (r?.error) setErr(r.error)
    })
  }

  useEffect(recarregar, [])

  useEffect(() => {
    return window.launcherAPI?.onWineProgress?.((p) => {
      const pct = p.total ? Math.round((p.done / p.total) * 100) : 0
      setProgress({ id: p.id, pct })
    })
  }, [])

  const instDaAba = useMemo(() => {
    const q = query.trim().toLowerCase()
    return installed
      .filter((v) => (tab === "steam" ? v.kind === "steam" : (v.kind || "ge-proton") === tab))
      .filter((v) => !q || v.name.toLowerCase().includes(q))
  }, [installed, tab, query])

  const availDaAba = useMemo(() => {
    const q = query.trim().toLowerCase()
    return available
      .filter((v) => v.kind === tab)
      .filter((v) => !q || v.name.toLowerCase().includes(q) || v.id.toLowerCase().includes(q))
  }, [available, tab, query])

  const tabInfo = TABS.find((t) => t.id === tab)!

  return (
    <div>
      {/* Cabeçalho com busca */}
      <div className="mb-5 flex items-center justify-between gap-4">
        <h2 className="text-3xl font-light tracking-wide text-white">{t("winemanager.titulo")}</h2>
        <div className="relative w-64">
          <svg
            className="absolute left-3 top-1/2 -translate-y-1/2 text-white/35"
            width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          >
            <circle cx="11" cy="11" r="8" /><path d="m21 21-4.35-4.35" />
          </svg>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("winemanager.buscar")}
            spellCheck={false}
            className="w-full rounded-lg border border-white/10 bg-white/[0.04] py-2 pl-9 pr-3 text-[13px] text-white placeholder-white/30 outline-none transition-colors focus:border-[color:var(--accent)]"
          />
        </div>
      </div>

      {/* Abas */}
      <div className="mb-4 flex gap-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`rounded-lg px-4 py-1.5 text-[13px] font-medium transition-colors ${
              tab === t.id
                ? "bg-white/[0.08] text-white"
                : "text-white/45 hover:bg-white/[0.04] hover:text-white/80"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Dica da aba */}
      <div className="mb-5 flex items-start gap-2 text-[12px] leading-snug text-[color:var(--accent)]">
        <svg className="mt-0.5 shrink-0" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12" />
        </svg>
        <span className="text-white/60">{tabInfo.hint}</span>
      </div>

      {/* Tabela */}
      <div className="overflow-hidden rounded-xl border border-white/[0.08]">
        <div className="grid grid-cols-[1fr_160px_110px_120px] gap-2 border-b border-white/[0.08] bg-white/[0.03] px-5 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-white/40">
          <span>{t("winemanager.coluna.versao")}</span>
          <span>{t("winemanager.coluna.data")}</span>
          <span>{t("winemanager.coluna.tamanho")}</span>
          <span className="text-right">{t("winemanager.coluna.acoes")}</span>
        </div>

        {/* Instaladas desta aba */}
        {instDaAba.map((v) => (
          <Row key={v.id}>
            <Name name={v.name} installed />
            <Cell>{t("winemanager.data_fallback")}</Cell>
            <Cell>{t("winemanager.data_fallback")}</Cell>
            <Actions>
              {v.path && (
                <IconBtn
                  title={t("winemanager.abrir_pasta")}
                  onClick={() => window.launcherAPI?.openExternal(`file://${v.path}`)}
                  icon={
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                  }
                />
              )}
              {v.kind !== "steam" && (
                <IconBtn
                  title={t("common.remover")}
                  danger
                  onClick={async () => {
                    await window.launcherAPI?.wineRemove(v.id)
                    recarregar()
                  }}
                  icon={
                    <>
                      <path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
                      <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                    </>
                  }
                />
              )}
            </Actions>
          </Row>
        ))}

        {/* Disponíveis desta aba */}
        {tab !== "steam" &&
          availDaAba.map((v) => {
            const baixando = busy === v.id
            const pct = baixando && progress?.id === v.id ? progress.pct : 0
            return (
              <Row key={v.id}>
                <Name name={v.name} />
                <Cell>{fmtDate(v.releaseDate, t)}</Cell>
                <Cell>{fmtMiB(v.size)}</Cell>
                <Actions>
                  <button
                    onClick={async () => {
                      setBusy(v.id)
                      setErr("")
                      const r = await window.launcherAPI?.wineInstall(v.id, v.kind as "ge-proton" | "wine-ge")
                      setBusy("")
                      setProgress(null)
                      if (!r?.ok) setErr(r?.error || t("winemanager.falha_baixar"))
                      recarregar()
                    }}
                    disabled={baixando}
                    title={baixando ? t("winemanager.baixando_pct", { pct: String(pct) }) : t("winemanager.baixar")}
                    className="flex h-8 w-8 items-center justify-center rounded-full transition-transform hover:scale-110 disabled:opacity-70"
                    style={{ background: "var(--accent)", color: "#000" }}
                  >
                    {baixando ? (
                      <span className="text-[10px] font-bold tabular-nums">{pct}%</span>
                    ) : (
                      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="7 10 12 15 17 10" />
                        <line x1="12" x2="12" y1="15" y2="3" />
                      </svg>
                    )}
                  </button>
                </Actions>
              </Row>
            )
          })}

        {/* Vazio */}
        {instDaAba.length === 0 && (tab === "steam" || availDaAba.length === 0) && (
          <div className="px-5 py-8 text-center text-[13px] text-white/35">
            {query
              ? t("winemanager.sem_resultados")
              : tab === "steam"
                ? t("winemanager.sem_proton")
                : t("winemanager.sem_versoes")}
          </div>
        )}
      </div>

      {err && <p className="mt-3 text-xs text-[#ff6b81]">{err}</p>}
      <p className="mt-6 text-xs text-white/35">
        {t("winemanager.footer_dica")}
      </p>
    </div>
  )
}

/* ------------------------------------------------------------------ */

function Row({ children }: { children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[1fr_160px_110px_120px] items-center gap-2 border-b border-white/[0.04] px-5 py-3 transition-colors last:border-0 hover:bg-white/[0.02]">
      {children}
    </div>
  )
}

function Name({ name, installed }: { name: string; installed?: boolean }) {
  return (
    <div className="flex items-center gap-2.5 truncate">
      <span
        className="h-2 w-2 shrink-0 rounded-full"
        style={{ background: installed ? "#4adf9a" : "rgba(255,255,255,0.25)" }}
      />
      <span className="truncate text-[13px] text-white/90">{name}</span>
    </div>
  )
}

function Cell({ children }: { children: React.ReactNode }) {
  return <span className="text-[12px] text-white/55">{children}</span>
}

function Actions({ children }: { children: React.ReactNode }) {
  return <div className="flex items-center justify-end gap-1.5">{children}</div>
}

function IconBtn({
  title,
  danger,
  onClick,
  icon,
}: {
  title: string
  danger?: boolean
  onClick: () => void
  icon: React.ReactNode
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      className={`flex h-7 w-7 items-center justify-center rounded-md transition-colors ${
        danger ? "text-white/40 hover:bg-red-500/15 hover:text-red-400" : "text-white/50 hover:bg-white/[0.07] hover:text-white"
      }`}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        {icon}
      </svg>
    </button>
  )
}
