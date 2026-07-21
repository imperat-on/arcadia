"use client"

import { useEffect, useRef, useState } from "react"
import { useStoreActions } from "../useStoreActions"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

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

type ManifestInfo = {
  depots: { depotId: string; manifestId: string; key: string }[]
  token?: string
  dlcs?: string[]
  fonte?: string
}

export function StoreView({ games = [] }: { games?: Game[] }) {
  const { t } = useI18n()
  // Ações (Baixar/Add/Remover/reiniciar Steam), estado de bloqueio e escolha de
  // disco vêm do hook compartilhado com a loja do modo console — antes essa
  // lógica morava só aqui e teria de ser duplicada lá.
  const {
    bloqueados,
    jaAdicionados,
    escolhendo,
    setEscolhendo,
    busy: acaoBusy,
    toast,
    setToast,
    baixar,
    confirmarBaixar,
    adicionar,
    remover,
    reiniciarSteam,
  } = useStoreActions(games)

  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<{ appid: string; title: string; cover?: string; manifest?: boolean }[]>([])
  const [recentes, setRecentes] = useState<{ appid: string; title: string; cover?: string; manifest?: boolean }[]>([])
  const [sugestoes, setSugestoes] = useState<{ appid: string; title: string }[]>([])
  // Item destacado nas sugestões (setas do teclado); -1 = nenhum.
  const [sugSel, setSugSel] = useState(-1)
  const [carregandoRec, setCarregandoRec] = useState(true)
  const [buscando, setBuscando] = useState(false)
  const [msg, setMsg] = useState("")

  useEffect(() => {
    window.launcherAPI?.storeRecent()
      .then((r) => {
        if (r?.ok) setRecentes(r.jogos || [])
      })
      .finally(() => setCarregandoRec(false))
  }, [])

  // Sugestões enquanto digita. Usam storeSuggest (só títulos da Steam), não a
  // busca completa: esta confere a disponibilidade de cada resultado e leva
  // 1–2s, então uma por tecla disparava dezenas de sondagens ao Ryuu e as
  // respostas voltavam fora de ordem, fazendo a lista "piscar" com resultados
  // de um termo antigo. O contador descarta qualquer resposta atrasada.
  const pedidoSug = useRef(0)
  useEffect(() => {
    const q = busca.trim()
    if (q.length < 2) {
      setSugestoes([])
      setSugSel(-1)
      return
    }
    const meu = ++pedidoSug.current
    const t = setTimeout(async () => {
      const r = await window.launcherAPI?.storeSuggest(q)
      if (meu !== pedidoSug.current) return // chegou tarde: já digitaram mais
      if (r?.ok) {
        setSugestoes(r.jogos || [])
        setSugSel(-1)
      }
    }, 220)
    return () => clearTimeout(t)
  }, [busca])

  const pesquisar = async (termo?: string) => {
    const q = (termo ?? busca).trim()
    if (!q) return
    if (termo) setBusca(termo)
    setSugestoes([])
    setBuscando(true)
    setMsg("")
    const r = await window.launcherAPI?.storeSearch(q)
    setBuscando(false)
    if (!r?.ok) {
      setResultados([])
      setMsg(r?.error || t("store.busca_falhou"))
      return
    }
    setResultados(r.jogos || [])
    if ((r.jogos || []).length === 0) setMsg(t("store.nada_encontrado"))
  }

  // Grade exibida: resultados da busca ou os adicionados recentemente.
  const buscou = resultados.length > 0 || msg !== ""
  const grade = buscou ? resultados : recentes
  // Esqueleto: buscando, ou o "Em alta" ainda não chegou.
  const carregandoGrade = buscando || (!buscou && carregandoRec && recentes.length === 0)

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="mb-1 text-2xl font-light tracking-wide text-white">{t("store.titulo")}</h1>
      <p className="mb-6 text-sm text-white/40">{t("store.descricao")}</p>

      {/* Busca + Restart Steam */}
      <div className="mb-4 flex max-w-[860px] gap-2">
        <div className="relative flex-1">
          <input
            value={busca}
            onChange={(e) => setBusca(e.target.value)}
            onKeyDown={(e) => {
              // Setas percorrem as sugestões; Enter aceita a destacada (ou
              // busca o que está digitado); Esc só fecha a lista.
              if (e.key === "ArrowDown" && sugestoes.length) {
                e.preventDefault()
                setSugSel((i) => (i + 1) % sugestoes.length)
              } else if (e.key === "ArrowUp" && sugestoes.length) {
                e.preventDefault()
                setSugSel((i) => (i <= 0 ? sugestoes.length : i) - 1)
              } else if (e.key === "Escape") {
                setSugestoes([])
                setSugSel(-1)
              } else if (e.key === "Enter") {
                pesquisar(sugSel >= 0 ? sugestoes[sugSel]?.title : undefined)
              }
            }}
            onBlur={() => setTimeout(() => setSugestoes([]), 150)}
            placeholder={t("store.buscar_placeholder")}
            spellCheck={false}
            className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)] disabled:opacity-50"
          />
          {/* Sugestões enquanto digita */}
          {sugestoes.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-white/10 bg-[#15181d] shadow-2xl shadow-black/60">
              {sugestoes.map((s, i) => (
                <button
                  key={s.appid}
                  onMouseDown={() => pesquisar(s.title)}
                  onMouseEnter={() => setSugSel(i)}
                  className={`block w-full truncate px-3.5 py-2 text-left text-[13px] transition-colors ${
                    i === sugSel ? "bg-white/[0.09] text-white" : "text-white/80 hover:bg-white/[0.07] hover:text-white"
                  }`}
                >
                  {s.title}
                </button>
              ))}
            </div>
          )}
        </div>
        <button
          onClick={() => pesquisar()}
          disabled={buscando}
          className="rounded-lg px-4 py-2.5 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {buscando ? t("store.buscando") : t("store.buscar")}
        </button>
        <button
          onClick={reiniciarSteam}
          title={t("store.restart_steam_tooltip")}
          className="flex items-center gap-2 rounded-lg border border-white/15 px-4 py-2.5 text-[12px] font-semibold text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M23 4v6h-6" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
          </svg>
          {t("store.restart_steam")}
        </button>
      </div>
      {msg && <p className="mb-4 text-[12px] text-white/55">{msg}</p>}

      {/* Grade: resultados da busca ou adicionados recentemente */}
      <h2 className="mb-3 text-sm font-medium text-white/60">
        {buscou ? t("store.resultados_count", { count: grade.length }) : t("store.em_alta_agora")}
      </h2>
      <div className="grid-stagger grid max-w-[1100px] grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3 pb-8">
        {/* Enquanto a lista não chega, cartões-fantasma: a área ficava
            totalmente preta e parecia que a loja tinha quebrado. */}
        {carregandoGrade &&
          Array.from({ length: 8 }).map((_, i) => (
            <div key={`sk${i}`} className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
              <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
              <div className="p-3">
                <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
                <div className="h-8 animate-pulse rounded-lg bg-white/[0.04]" />
              </div>
            </div>
          ))}
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
                    {t("store.na_biblioteca")}
                  </div>
                  {jaAdicionados.has(j.appid) && (
                    <button
                      onClick={() => remover(j)}
                      disabled={acaoBusy !== ""}
                      title={t("store.remover_tooltip")}
                      className="rounded-lg border border-[#ff6b81]/40 px-3 py-2 text-[12px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
                    >
                      {acaoBusy === j.appid ? "…" : t("common.remover")}
                    </button>
                  )}
                </div>
              ) : (
                /* A busca já sabe se algum provedor tem o manifesto. Sem usar
                   esse dado, o botão Baixar ficava ativo em jogo indisponível:
                   o clique consultava todos os provedores e terminava num
                   toast no canto, sem nunca chegar à escolha de disco — dava a
                   impressão de que o botão não fazia nada. */
                j.manifest === false ? (
                  <div
                    title={t("store.sem_manifesto_tooltip")}
                    className="flex items-center justify-center gap-1.5 rounded-lg border border-white/10 py-2 text-[12px] font-semibold text-white/35"
                  >
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <circle cx="12" cy="12" r="10" /><path d="m4.9 4.9 14.2 14.2" />
                    </svg>
                    {t("store.sem_manifesto")}
                  </div>
                ) : (
                <div className="flex gap-2">
                  <button
                    onClick={() => baixar(j)}
                    disabled={acaoBusy !== ""}
                    className="flex-1 rounded-lg px-3 py-2 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.02] disabled:opacity-50"
                    style={{ background: "var(--accent)" }}
                  >
                    {acaoBusy === j.appid ? "…" : t("store.baixar")}
                  </button>
                  <button
                    onClick={() => adicionar(j)}
                    disabled={acaoBusy !== ""}
                    title={t("store.add_tooltip")}
                    className="flex-1 rounded-lg border border-white/20 px-3 py-2 text-[12px] font-semibold text-white/80 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
                  >
                    {t("store.add")}
                  </button>
                </div>
                )
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
            <h3 className="mb-1 text-base font-semibold text-white">{t("store.instalar_em", { title: escolhendo.jogo.title })}</h3>
            <p className="mb-4 text-[12px] text-white/40">{t("store.escolher_biblioteca")}</p>
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
                  <span className="text-[11px] font-semibold text-white/50">{t("store.gb_livres", { free: l.free.toFixed(2) })}</span>
                </button>
              ))}
            </div>
            <button
              onClick={() => setEscolhendo(null)}
              className="mt-3 w-full rounded-lg border border-white/10 py-2 text-[12px] font-semibold text-white/50 transition-colors hover:border-white/25 hover:text-white/80"
            >
              {t("common.cancelar")}
            </button>
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
