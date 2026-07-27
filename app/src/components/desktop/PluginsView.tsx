"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

type Plugin = { id: string; name: string; descKey: string; installed: boolean; enabled: boolean }

// Chave no config.json onde fica o caminho manual de cada plugin (quando o
// usuário colocou o arquivo fora do caminho padrão). Só slssteam/slscheevo.
const CAMINHO_KEY: Record<string, string> = {
  slssteam: "slssteam_path",
  slscheevo: "slscheevo_path",
}

export function PluginsView() {
  const { t } = useI18n()
  const [plugins, setPlugins] = useState<Plugin[]>([])
  const [busy, setBusy] = useState("")
  const [msg, setMsg] = useState("")
  const [caminhos, setCaminhos] = useState<Record<string, string>>({})

  const carregar = () => window.launcherAPI?.pluginsList().then((r) => setPlugins(r?.plugins || []))
  useEffect(() => {
    carregar()
    window.launcherAPI?.getConfig().then((c) => {
      setCaminhos({ slssteam: c?.slssteam_path || "", slscheevo: c?.slscheevo_path || "" })
    })
  }, [])

  // Salva o caminho no config e tenta detectar/ativar. Não baixa nada.
  const verificar = async (id: string) => {
    setBusy(id); setMsg("")
    const key = CAMINHO_KEY[id]
    if (key) await window.launcherAPI?.setConfig({ [key]: (caminhos[id] || "").trim() })
    const r = await window.launcherAPI?.pluginsInstall(id)
    if (!r?.ok) setMsg(r?.error === "not_detected" ? t("plugins.nao_detectado_msg") : r?.error || t("plugins.falha"))
    await carregar()
    setBusy("")
  }

  const desativar = async (id: string) => {
    setBusy(id); setMsg("")
    const r = await window.launcherAPI?.pluginsRemove(id)
    if (!r?.ok) setMsg(r?.error || t("plugins.falha"))
    await carregar()
    setBusy("")
  }

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="ui-title mb-1">{t("plugins.titulo")}</h1>
      <p className="ui-subtitle mb-6 max-w-3xl">{t("plugins.descricao")}</p>
      {msg && <p className="mb-4 rounded-lg border border-[#ff6b81]/30 bg-[#ff6b81]/10 px-3 py-2 text-[13px] text-[#ffb3c0]">{msg}</p>}
      <div className="grid max-w-4xl gap-3">
        {plugins.map((p) => {
          const temCaminho = p.id in CAMINHO_KEY
          return (
            <div key={p.id} className="ui-card p-5">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <div className="mb-1 flex items-center gap-2">
                    <h3 className="text-base font-semibold text-white">{t(p.name)}</h3>
                    <span className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${
                      p.enabled
                        ? "bg-[color:var(--accent)]/20 text-[color:var(--accent)]"
                        : "bg-white/10 text-white/45"
                    }`}>
                      {p.enabled ? t("plugins.ativo") : t("plugins.desativado")}
                    </span>
                    {/* Status de detecção só faz sentido p/ plugins com arquivo físico. */}
                    {temCaminho && !p.installed && (
                      <span className="rounded-full bg-[#ffb86b]/15 px-2 py-0.5 text-[11px] font-semibold text-[#ffb86b]">
                        {t("plugins.arquivo_nao_encontrado")}
                      </span>
                    )}
                  </div>
                  <p className="max-w-xl text-[13px] leading-relaxed text-white/45">{t(p.descKey)}</p>
                </div>
                {p.enabled ? (
                  <button
                    onClick={() => desativar(p.id)}
                    disabled={busy === p.id}
                    className="ui-btn-secondary shrink-0 rounded-lg px-4 py-2 text-[12px] font-semibold text-[#ff6b81] disabled:opacity-50"
                  >
                    {busy === p.id ? "…" : t("plugins.desativar")}
                  </button>
                ) : (
                  <button
                    onClick={() => verificar(p.id)}
                    disabled={busy === p.id}
                    className="ui-btn-primary shrink-0 rounded-lg px-4 py-2 text-[12px] disabled:opacity-50"
                    style={{ background: "var(--accent)" }}
                  >
                    {busy === p.id ? "…" : t("plugins.verificar")}
                  </button>
                )}
              </div>
              {/* Caminho manual: só p/ plugins baseados em arquivo, quando não detectados. */}
              {temCaminho && !p.installed && (
                <div className="mt-4">
                  <label className="mb-1.5 block text-[12px] text-white/50">{t("plugins.caminho_label")}</label>
                  <input
                    value={caminhos[p.id] || ""}
                    onChange={(e) => setCaminhos((c) => ({ ...c, [p.id]: e.target.value }))}
                    placeholder={t("plugins.caminho_placeholder")}
                    spellCheck={false}
                    className="ui-input w-full px-3 py-2 text-[13px] placeholder:text-white/25"
                  />
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
