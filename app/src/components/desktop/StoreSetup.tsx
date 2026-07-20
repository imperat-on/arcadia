"use client"

import { useEffect, useState } from "react"

// Setup da Loja Steam (API key Hubcap + status .NET/DepotDownloader/SLSsteam).
// Usado em Configurações → Integrações; a aba Lojas fica só com a busca.
export function StoreSetup() {
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
    setMsg("API key salva.")
    setTimeout(() => setMsg(""), 2500)
  }

  const instalarDotnet = async () => {
    setBusy("dotnet")
    const r = await window.launcherAPI?.storeEnsureDotnet()
    setBusy("")
    if (!r?.ok) setMsg(r?.error || "Falha ao instalar o .NET")
    recarregar()
  }

  const instalarSls = async () => {
    setBusy("sls")
    setMsg("Instalando a SLSsteam (pode demorar)…")
    const r = await window.launcherAPI?.slssteamInstall()
    setBusy("")
    setMsg(r?.ok ? "SLSsteam instalada!" : r?.error || "Falha ao instalar a SLSsteam")
    recarregar()
  }

  const StatusCard = ({ titulo, ok, detalhe, acao, onAcao }: { titulo: string; ok: boolean; detalhe: string; acao?: string; onAcao?: () => void }) => (
    <div className="rounded-2xl border border-white/[0.08] bg-white/[0.02] p-4">
      <div className="mb-1 flex items-center justify-between">
        <h3 className="text-sm font-medium text-white">{titulo}</h3>
        {/* Sem pill: a palavra basta, a cor só separa pronto de pendente. */}
        <span className="text-[11px] font-medium" style={{ color: ok ? "#4adf9a" : "#ffb86b" }}>
          {ok ? "Instalado" : "Faltando"}
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
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-white/40">Loja Steam</h3>
      <div className="flex flex-col gap-4 rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4">
        <div>
          <label className="mb-1.5 block text-[13px] text-white/70">Chave do Hubcap</label>
          <div className="flex gap-2">
            <input
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              type="password"
              spellCheck={false}
              placeholder="Opcional — acrescenta o catálogo do Hubcap"
              className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
            <button
              onClick={salvarKey}
              className="rounded-lg px-4 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
              style={{ background: "var(--accent)" }}
            >
              Salvar
            </button>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3 xl:grid-cols-3">
          <StatusCard titulo=".NET 9" ok={Boolean(status?.dotnet)} detalhe="Runtime do DepotDownloader." acao={busy === "dotnet" ? "Instalando…" : "Instalar"} onAcao={instalarDotnet} />
          <StatusCard titulo="DepotDownloader" ok={Boolean(status?.depotdownloader)} detalhe="Baixa os depots da Steam." />
          <StatusCard titulo="SLSsteam" ok={Boolean(status?.slssteam)} detalhe="Faz a Steam reconhecer os jogos baixados." acao={busy === "sls" ? "Instalando…" : "Instalar"} onAcao={instalarSls} />
        </div>
        {msg && <p className="text-[12px] text-white/55">{msg}</p>}
      </div>
    </section>
  )
}
