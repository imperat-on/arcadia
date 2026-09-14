"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

export function StoreSetup() {
  const { t } = useI18n()
  const [apiKey, setApiKey] = useState("")
  const [temChave, setTemChave] = useState(false)
  const [validando, setValidando] = useState(false)
  const [teste, setTeste] = useState<"ok" | "invalida" | "indisponivel" | "sem_chave" | "">("")
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
    window.launcherAPI?.getConfig().then((c) => {
      setApiKey(c?.hubcap_api_key || "")
      setTemChave(Boolean(c?.hubcap_api_key))
    })
    recarregarStatus()
  }, [])

  // Testa a chave de verdade (uma requisição ao Hubcap, que para no status).
  // Salvar sem saber se a chave presta deixava a pessoa achando que resolveu.
  const validar = async (valor?: string) => {
    setValidando(true)
    setTeste("")
    try {
      const r = await window.launcherAPI?.storeValidarChaveHubcap(valor ?? apiKey)
      if (r?.ok) setTeste("ok")
      else if (r?.motivo === "chave_invalida") setTeste("invalida")
      else if (r?.motivo === "sem_chave") setTeste("sem_chave")
      else setTeste("indisponivel")
    } catch {
      setTeste("indisponivel")
    } finally {
      setValidando(false)
    }
  }

  const salvarKey = async () => {
    const valor = apiKey.trim()
    // null apaga de verdade. Mandar "" não apagava: o merge do writeConfig
    // trazia a chave antiga de volta e ela "continuava ativa".
    await window.launcherAPI?.setConfig({ hubcap_api_key: valor === "" ? null : valor } as Record<
      string,
      unknown
    >)
    setTemChave(valor !== "")
    setMsg(t("common.salvo"))
    setTimeout(() => setMsg(""), 2500)
    await validar(valor)
  }

  const removerKey = async () => {
    await window.launcherAPI?.setConfig({ hubcap_api_key: null } as Record<string, unknown>)
    setApiKey("")
    setTemChave(false)
    setTeste("")
    setMsg(t("store_setup.chave_removida"))
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
            <button
              onClick={() => validar()}
              disabled={validando}
              className="rounded-lg border border-white/15 px-3.5 py-2.5 text-[12px] text-white/80 transition-colors hover:border-white/30 disabled:opacity-50"
            >
              {validando ? t("store_setup.validando") : t("store_setup.validar")}
            </button>
            {temChave && (
              <button
                onClick={removerKey}
                className="rounded-lg border border-white/15 px-3.5 py-2.5 text-[12px] text-white/60 transition-colors hover:border-red-400/40 hover:text-red-300"
              >
                {t("store_setup.remover_chave")}
              </button>
            )}
          </div>
          <p className="mt-1.5 text-[11px] text-white/35">{t("store_setup.hubcap_dica")}</p>
          {teste && (
            <p
              role="status"
              className={`text-[11px] ${teste === "ok" ? "text-emerald-300/80" : "text-amber-200/80"}`}
            >
              {t(`store_setup.teste_${teste}`)}
            </p>
          )}
        </div>

        <div className="rounded-xl border border-white/[0.08] bg-white/[0.02] p-4">
          <div className="mb-1 flex items-center justify-between">
            <h3 className="text-sm font-medium text-white">DepotDownloader</h3>
            <span
              className="rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
              style={{
                color: depotOk ? "var(--state-success)" : "var(--state-warn-soft)",
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
