"use client"

import { useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"

// Aba Lojas: busca no catálogo Hubcap e download direto para a biblioteca
// Steam (DepotDownloader + SLSsteam). O setup (API key, .NET, SLSsteam) fica
// em Configurações → Integrações.
// Imagem da loja com fallback: header.jpg → capsule → placeholder com título
// (demos/playtests novos ainda não têm header no CDN da Steam).
function StoreImg({ appid, cover, title }: { appid: string; cover?: string; title: string }) {
  const [fase, setFase] = useState(0)
  const fontes = [
    cover || `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/header.jpg`,
    `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/capsule_231x87.jpg`,
  ]
  if (fase >= fontes.length) {
    return (
      <div className="flex h-full w-full flex-col items-center justify-center gap-1.5 bg-[#121216] px-3 text-center">
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.25)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="8.5" cy="8.5" r="1.5" /><path d="m21 15-5-5L5 21" />
        </svg>
        <span className="text-[11px] leading-tight text-white/30">{title}</span>
      </div>
    )
  }
  return (
    <img
      src={fontes[fase]}
      alt=""
      loading="lazy"
      className="h-full w-full object-cover"
      draggable={false}
      onError={() => setFase((f) => f + 1)}
    />
  )
}

export function StoreView({ games = [] }: { games?: Game[] }) {
  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<{ appid: string; title: string; cover?: string; manifest?: boolean }[]>([])
  const [recentes, setRecentes] = useState<{ appid: string; title: string; cover?: string; manifest?: boolean }[]>([])
  const [sugestoes, setSugestoes] = useState<{ appid: string; title: string }[]>([])
  const [jaAdicionados, setJaAdicionados] = useState<Set<string>>(new Set())
  // Diálogo "onde instalar" (bibliotecas Steam em vários drives).
  const [escolhendo, setEscolhendo] = useState<{
    jogo: { appid: string; title: string }
    info: { depots: { depotId: string; manifestId: string; key: string }[]; token?: string; dlcs?: string[]; fonte?: string }
    libs: { path: string; steamDir: string; free: number }[]
  } | null>(null)
  const [busy, setBusy] = useState("")
  const [msg, setMsg] = useState("")
  // Toast de feedback (Add/Baixar/Restart) — popup visível, some sozinho.
  const [toast, setToast] = useState("")
  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(""), 5000)
    return () => clearTimeout(t)
  }, [toast])

  useEffect(() => {
    const status = () =>
      window.launcherAPI?.storeStatus().then((s) => {
        setJaAdicionados(new Set(s?.adicionados || []))
      })
    status()
    // Adicionar/remover mexe no registro da SLSsteam. Relendo o status a cada
    // mudança de biblioteca, os cards refletem o estado real mesmo quando a
    // alteração vem de outra aba (ou de um download que terminou).
    const off = window.launcherAPI?.onLibraryChanged(() => status())
    // "Em alta" vem do SteamSpy e a busca cobre Ryuu/Sushi — nenhum dos dois
    // precisa da chave do Morrenus. Carregamos sempre; antes isto ficava atrás
    // de `if (hubcapKey)` e a loja inteira parecia quebrada para quem não tinha.
    window.launcherAPI?.storeRecent().then((r) => {
      if (r?.ok) setRecentes(r.jogos || [])
    })
    return off
  }, [])

  // Jogos já adicionados/instalados (SLSsteam config + biblioteca do Arcadia):
  // não deixar adicionar/baixar de novo.
  const bloqueados = new Set([
    ...jaAdicionados,
    ...games.map((g) => String(g.id).replace(/^steam:/, "")),
  ])

  // Sugestões em tempo real (150ms; mantém as anteriores até a nova chegar).
  useEffect(() => {
    const q = busca.trim()
    if (q.length < 3) {
      setSugestoes([])
      return
    }
    const t = setTimeout(async () => {
      const r = await window.launcherAPI?.storeSearch(q)
      if (r?.ok) setSugestoes((r.jogos || []).slice(0, 6))
    }, 150)
    return () => clearTimeout(t)
  }, [busca])

  const pesquisar = async (termo?: string) => {
    const q = (termo ?? busca).trim()
    if (!q) return
    if (termo) setBusca(termo)
    setSugestoes([])
    setBusy("busca")
    setMsg("")
    const r = await window.launcherAPI?.storeSearch(q)
    setBusy("")
    if (!r?.ok) {
      setResultados([])
      setMsg(r?.error || "Busca falhou")
      return
    }
    setResultados(r.jogos || [])
    if ((r.jogos || []).length === 0) setMsg("Nada encontrado.")
  }

  // Grade exibida: resultados da busca ou os adicionados recentemente.
  const buscou = resultados.length > 0 || msg !== ""
  const grade = buscou ? resultados : recentes

  const baixar = async (jogo: { appid: string; title: string }) => {
    setBusy(jogo.appid)
    setMsg("")
    const info = await window.launcherAPI?.storeInstallInfo(jogo.appid)
    setBusy("")
    if (!info?.ok || !info.depots?.length) {
      setToast(info?.error || "Sem manifesto para este jogo.")
      return
    }
    // Pergunta ONDE instalar: bibliotecas Steam detectadas (multi-drive).
    const libs = (await window.launcherAPI?.storeLibraries()) || []
    if (libs.length <= 1) {
      return confirmarBaixar(jogo, info as Parameters<typeof confirmarBaixar>[1], libs[0]?.steamDir)
    }
    setEscolhendo({ jogo, info: info as Parameters<typeof confirmarBaixar>[1], libs })
  }

  const confirmarBaixar = async (
    jogo: { appid: string; title: string },
    info: { depots: { depotId: string; manifestId: string; key: string }[]; token?: string; dlcs?: string[]; fonte?: string },
    steamDir?: string,
  ) => {
    setEscolhendo(null)
    setBusy(jogo.appid)
    const r = await window.launcherAPI?.storeInstall({
      appid: jogo.appid,
      title: jogo.title,
      cover: `https://cdn.akamai.steamstatic.com/steam/apps/${jogo.appid}/header.jpg`,
      installdir: jogo.title.replace(/[^A-Za-z0-9]/g, ""),
      depots: info.depots,
      token: info.token,
      dlcs: info.dlcs,
      steamDir,
    })
    setBusy("")
    const via = info.fonte ? ` (via ${info.fonte})` : ""
    setToast(r?.ok ? `"${jogo.title}" entrou na fila de downloads${via}.` : r?.error || "Falha ao enfileirar")
  }

  // "Add": registra o jogo na Steam (lua + AdditionalApps) sem baixar — a
  // própria Steam baixa depois pela CDN dela (estilo luatools-moon).
  const adicionar = async (jogo: { appid: string; title: string }) => {
    setBusy(jogo.appid)
    setMsg("")
    const info = await window.launcherAPI?.storeInstallInfo(jogo.appid)
    if (!info?.ok || !info.depots?.length) {
      setBusy("")
      setToast(info?.error || "Sem manifesto para este jogo.")
      return
    }
    const r = await window.launcherAPI?.storeAddToSteam({ appid: jogo.appid, token: info.token, dlcs: info.dlcs })
    setBusy("")
    if (r?.ok) setJaAdicionados((prev) => new Set(prev).add(jogo.appid))
    setToast(
      r?.ok
        ? `"${jogo.title}" adicionado! Clique em Restart Steam para baixar por lá.`
        : r?.error || "Falha ao adicionar",
    )
  }

  const remover = async (jogo: { appid: string; title: string }) => {
    setBusy(jogo.appid)
    // Remove de tudo: pasta + appmanifest (downloads) + registro SLSsteam (Adds).
    const r = await window.launcherAPI?.storeRemoveDownloaded(jogo.appid)
    setBusy("")
    if (r?.ok) {
      setJaAdicionados((prev) => {
        const n = new Set(prev)
        n.delete(jogo.appid)
        return n
      })
    }
    setToast(r?.ok ? `"${jogo.title}" removido da Steam (SLSsteam).` : r?.error || "Falha ao remover")
  }

  const abrirSteam = async () => {
    const r = await window.launcherAPI?.slssteamLaunch()
    if (!r?.ok) setToast(r?.error || "Falha ao abrir a Steam")
    else setToast("Reiniciando a Steam com a SLSsteam…")
  }

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="mb-1 text-2xl font-light tracking-wide text-white">Loja Steam</h1>
      <p className="mb-6 text-sm text-white/40">Busque jogos no catálogo e baixe direto para a sua biblioteca Steam (via DepotDownloader + SLSsteam).</p>

      {/* Busca + Restart Steam */}
      <div className="mb-4 flex max-w-[860px] gap-2">
        <div className="relative flex-1">
          <input
            value={busca}
            onChange={(e) => setBusca(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && pesquisar()}
            onBlur={() => setTimeout(() => setSugestoes([]), 150)}
            placeholder="Buscar jogo na loja…"
            spellCheck={false}
            className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)] disabled:opacity-50"
          />
          {/* Sugestões enquanto digita */}
          {sugestoes.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-white/10 bg-[#15181d] shadow-2xl shadow-black/60">
              {sugestoes.map((s) => (
                <button
                  key={s.appid}
                  onMouseDown={() => pesquisar(s.title)}
                  className="block w-full truncate px-3.5 py-2 text-left text-[13px] text-white/80 transition-colors hover:bg-white/[0.07] hover:text-white"
                >
                  {s.title}
                </button>
              ))}
            </div>
          )}
        </div>
        <button
          onClick={() => pesquisar()}
          disabled={busy === "busca"}
          className="rounded-lg px-4 py-2.5 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {busy === "busca" ? "Buscando…" : "Buscar"}
        </button>
        <button
          onClick={abrirSteam}
          title="Reinicia a Steam com a SLSsteam carregada"
          className="flex items-center gap-2 rounded-lg border border-white/15 px-4 py-2.5 text-[12px] font-semibold text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M23 4v6h-6" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
          </svg>
          Restart Steam
        </button>
      </div>
      {msg && <p className="mb-4 text-[12px] text-white/55">{msg}</p>}

      {/* Grade: resultados da busca ou adicionados recentemente */}
      <h2 className="mb-3 text-sm font-medium text-white/60">
        {buscou ? `Resultados (${grade.length})` : "Em alta agora"}
      </h2>
      <div className="grid max-w-[1100px] grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3 pb-8">
        {grade.map((j) => (
          <div key={j.appid} className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
            <div className="aspect-[460/215] w-full bg-black">
              <StoreImg appid={j.appid} cover={j.cover} title={j.title} />
            </div>
            <div className="p-3">
              <div className="mb-2 truncate text-[13px] font-medium text-white" title={j.title}>{j.title}</div>
              {bloqueados.has(j.appid) ? (
                <div className="flex gap-2">
                  <div className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-[color:var(--accent)]/40 py-2 text-[12px] font-semibold" style={{ color: "var(--accent)" }}>
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                    Na biblioteca
                  </div>
                  {jaAdicionados.has(j.appid) && (
                    <button
                      onClick={() => remover(j)}
                      disabled={busy === j.appid}
                      title="Remover da SLSsteam (desfaz o Add)"
                      className="rounded-lg border border-[#ff6b81]/40 px-3 py-2 text-[12px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
                    >
                      {busy === j.appid ? "…" : "Remover"}
                    </button>
                  )}
                </div>
              ) : (
                <div className="flex gap-2">
                  <button
                    onClick={() => baixar(j)}
                    disabled={busy === j.appid}
                    className="flex-1 rounded-lg px-3 py-2 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.02] disabled:opacity-50"
                    style={{ background: "var(--accent)" }}
                  >
                    {busy === j.appid ? "…" : "Baixar"}
                  </button>
                  <button
                    onClick={() => adicionar(j)}
                    disabled={busy === j.appid}
                    title="Adiciona à Steam sem baixar — a Steam baixa pela CDN dela"
                    className="flex-1 rounded-lg border border-white/20 px-3 py-2 text-[12px] font-semibold text-white/80 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
                  >
                    Add
                  </button>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* Diálogo "onde instalar" (bibliotecas Steam em vários drives) */}
      {escolhendo && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={() => setEscolhendo(null)}>
          <div
            className="w-[440px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-5 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="mb-1 text-base font-semibold text-white">Instalar "{escolhendo.jogo.title}" em:</h3>
            <p className="mb-4 text-[12px] text-white/40">Escolha a biblioteca Steam de destino.</p>
            <div className="flex flex-col gap-2">
              {escolhendo.libs.map((l, i) => (
                <button
                  key={l.steamDir}
                  onClick={() => confirmarBaixar(escolhendo.jogo, escolhendo.info, l.steamDir)}
                  className={`flex items-center justify-between rounded-xl border px-4 py-3 text-left transition-colors ${
                    i === 0 ? "border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)]" : "border-white/10 hover:border-white/25"
                  }`}
                >
                  <span className="flex items-center gap-2.5 text-[13px] font-medium text-white/90">
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="text-white/50">
                      <rect x="2" y="3" width="20" height="7" rx="2" /><rect x="2" y="14" width="20" height="7" rx="2" />
                    </svg>
                    {l.steamDir.replace(/^\/home\/[^/]+/, "~")}
                  </span>
                  <span className="text-[11px] font-semibold text-white/50">{l.free.toFixed(2)} GB livres</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Toast de feedback (Add/Baixar/Restart) */}
      {toast && (
        <div
          className="fixed bottom-5 right-5 z-[80] max-w-[360px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-4 py-3 text-[13px] text-white/90 shadow-2xl shadow-black/60 backdrop-blur-md"
          style={{ animation: "toast-in 0.25s ease-out" }}
          onClick={() => setToast("")}
        >
          {toast}
        </div>
      )}
      <style>{`
        @keyframes toast-in {
          from { opacity: 0; transform: translateY(10px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  )
}
