"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

export function StoreSetup() {
  const { t } = useI18n()
  const [apiKey, setApiKey] = useState("")
  const [msg, setMsg] = useState("")

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setApiKey(c?.hubcap_api_key || ""))
  }, [])

  const salvarKey = async () => {
    await window.launcherAPI?.setConfig({ hubcap_api_key: apiKey.trim() } as Record<
      string,
      unknown
    >)
    setMsg(t("common.salvo"))
    setTimeout(() => setMsg(""), 2500)
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

        {msg && <p className="text-[12px] text-white/55">{msg}</p>}
      </div>
    </section>
  )
}
