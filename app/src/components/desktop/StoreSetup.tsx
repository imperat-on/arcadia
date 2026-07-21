"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

export function StoreSetup() {
  const { t } = useI18n()
  const [status, setStatus] = useState<{ dotnet?: string; depotdownloader: boolean; hubcapKey: boolean; slssteam: boolean; steamDir: string } | null>(null)
  const [apiKey, setApiKey] = useState("")
  const [busy, setBusy] = useState("")
  const [msg, setMsg] = useState("")

  const recarregar = () => window.launcherAPI?.storeStatus().then((s) => setStatus(s as typeof status))
  useEffect(() => {
    recarregar()
    window.launcherAPI?.getConfig().then((c) => setApiKey(c?.hubcap_api_key || ""))
  }, [])

  const salvarKey = async () => {
    await window.launcherAPI?.setConfig({ hubcap_api_key: apiKey.trim() } as Record<string, unknown>)
    recarregar()
    setMsg(t("common.salvo"))
    setTimeout(() => setMsg(""), 2500)
  }

  const instalarDotnet = async () => {
    setBusy("dotnet")
    const r = await window.launcherAPI?.storeEnsureDotnet()
    setBusy("")
    if (!r?.ok) setMsg(r?.error || t("store_setup.falha_dotnet"))
    recarregar()
  }

  const instalarSls = async () => {
    setBusy("sls")
    setMsg(t("store_setup.instalando_sls"))
    const r = await window.launcherAPI?.slssteamInstall()
    setBusy("")
    setMsg(r?.ok ? t("store_setup.sls_instalada") : r?.error || t("store_setup.falha_sls"))
    recarregar()
  }

  const StatusCard = ({ titulo, ok, detalhe, acao, onAcao }: { titulo: string; ok: boolean; detalhe: string; acao?: string; onAcao?: () => void }) => (
    <div className="rounded-2xl border border-white/[0.08] bg-white/[0.02] p-4">
      <div className="mb-1 flex items-center justify-between">
        <h3 className="text-sm font-medium text-white">{titulo}</h3>
        <span className="text-[11px] font-medium" style={{ color: ok ? "#4adf9a" : "#ffb86b" }}>
          {ok ? t("store_setup.instalado") : t("store_setup.faltando")}
        </span>
      </div>
      <p className="text-xs text-white/45">{detalhe}</p>
      {acao && !ok && (
        <button
          onClick={onAcao}
          disabled={Boolean(busy)}
          className="mt-2.5 rounded-lg px-3.5 py-1.5 text-[11px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {acao}
        </button>
      )}
    </div>
  )

  return (
    <section className="mb-8">
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-white/40">{t("settings.steam")}</h3>
      <div className="flex flex-col gap-4 rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4">
        <div>
          <label className="mb-1.5 block text-[13px] text-white/70">{t("store_setup.chave_hubcap")}</label>
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

        <div className="grid grid-cols-2 gap-3 xl:grid-cols-3">
          <StatusCard titulo=".NET 9" ok={Boolean(status?.dotnet)} detalhe={t("store_setup.dotnet_desc")} acao={busy === "dotnet" ? t("store_setup.instalando") : t("contextmenu.instalar")} onAcao={instalarDotnet} />
          <StatusCard titulo="DepotDownloader" ok={Boolean(status?.depotdownloader)} detalhe={t("store_setup.depotdownloader_desc")} />
          <StatusCard titulo="SLSsteam" ok={Boolean(status?.slssteam)} detalhe={t("store_setup.slssteam_desc")} acao={busy === "sls" ? t("store_setup.instalando") : t("contextmenu.instalar")} onAcao={instalarSls} />
        </div>
        {msg && <p className="text-[12px] text-white/55">{msg}</p>}
      </div>
    </section>
  )
}
