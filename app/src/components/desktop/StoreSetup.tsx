"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

export function StoreSetup() {
  const { t } = useI18n()
  const [apiKey, setApiKey] = useState("")
  const [depotOk, setDepotOk] = useState<boolean | null>(null)
  const [depotBusy, setDepotBusy] = useState(false)
  const [msg, setMsg] = useState("")

  const recarregarStatus = async () => {
    try {
      const status = await window.launcherAPI?.storeStatus()
      if (status) setDepotOk(Boolean(status.depotdownloader))
    } catch {}
  }

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setApiKey(c?.hubcap_api_key || ""))
    recarregarStatus()
  }, [])

  const salvarKey = async () => {
    await window.launcherAPI?.setConfig({ hubcap_api_key: apiKey.trim() } as Record<
      string,
      unknown
    >)
    setMsg(t("common.salvo"))
    setTimeout(() => setMsg(""), 2500)
  }

  const instalarDepot = async () => {
    setDepotBusy(true)
    setMsg("")
    try {
      const result = await window.launcherAPI?.storeEnsureDepotDownloader()
      setMsg(
        result?.ok ? t("store_setup.instalado") : result?.error || t("store_setup.falha_depot"),
      )
    } catch (error) {
      setMsg(`${t("store_setup.falha_depot")}: ${error}`)
    } finally {
      await recarregarStatus()
      setDepotBusy(false)
    }
  }

  return (
    <section className="mb-8">
      <div className="flex flex-col gap-4 rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4">
        <div>
          <label className="mb-1.5 block text-[13px] text-white/70">
            {t("store_setup.chave_hubcap")}
          </label>
          <div className="flex gap-2">
            <input
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              type="password"
              spellCheck={false}
              placeholder={t("store_setup.hubcap_placeholder")}
              className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
            <button
              onClick={salvarKey}
              className="rounded-lg px-4 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
              style={{ background: "var(--accent)" }}
            >
              {t("common.salvar")}
            </button>
          </div>
        </div>

        <div className="rounded-xl border border-white/[0.08] bg-white/[0.02] p-4">
          <div className="mb-1 flex items-center justify-between">
            <h3 className="text-sm font-medium text-white">DepotDownloader</h3>
            <span
              className="rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
              style={{
                color: depotOk ? "#4adf9a" : "#ffb86b",
                background: depotOk ? "rgba(74,223,154,0.12)" : "rgba(255,184,107,0.12)",
              }}
            >
              {depotOk === null
                ? "…"
                : depotOk
                  ? t("store_setup.instalado")
                  : t("store_setup.faltando")}
            </span>
          </div>
          <p className="text-xs text-white/45">{t("store_setup.depotdownloader_desc")}</p>
          {depotOk === false && (
            <button
              onClick={instalarDepot}
              disabled={depotBusy}
              className="mt-2.5 rounded-lg px-3.5 py-1.5 text-[11px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
              style={{ background: "var(--accent)" }}
            >
              {depotBusy ? t("store_setup.instalando") : t("contextmenu.instalar")}
            </button>
          )}
        </div>

        {msg && <p className="text-[12px] text-white/55">{msg}</p>}
      </div>
    </section>
  )
}
