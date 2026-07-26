"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useStoreActions, StoreGamePage, type ItemLoja, CartaoLoja, useI18n } from "./storeShared"
import type { Game } from "../ps5-launcher/types"

// Aba Lojas: busca no catálogo (Hubcap + Steam). Página de detalhe estilo
// Hydra (StoreGamePage) abre ao clicar no card.

export function StoreView({ games = [] }: { games?: Game[] }) {
  const { t } = useI18n()
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
  } = useStoreActions(games)

  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<ItemLoja[] | null>(null)
  const [sugestoes, setSugestoes] = useState<{ appid: string; title: string }[]>([])
  const [sugSel, setSugSel] = useState(-1)
  const [buscando, setBuscando] = useState(false)
  const [msg, setMsg] = useState("")
  const [pagina, setPagina] = useState<ItemLoja | null>(null)
  const esqueletos = useMemo(() => Array.from({ length: 8 }, (_, i) => i), [])

  // Catálogo navegável (sem busca): lista "Em alta" paginada via store:recent.
  const POR_PAGINA = 24
  const [catalogo, setCatalogo] = useState<ItemLoja[]>([])
  const [catTotal, setCatTotal] = useState(0)
  const [catPag, setCatPag] = useState(0) // 0-based
  const [catCarregando, setCatCarregando] = useState(false)
  const gerCat = useRef(0)

  useEffect(() => {
    const meu = ++gerCat.current
    setCatCarregando(true)
    window.launcherAPI?.storeRecent?.("all", POR_PAGINA, catPag * POR_PAGINA).then((r) => {
      if (meu !== gerCat.current) return
      setCatCarregando(false)
      if (r?.ok) { setCatalogo(r.jogos || []); setCatTotal(r.total || 0) }
    })
  }, [catPag])

  const gerSug = useRef(0)
  const gerBusca = useRef(0)
  const ignorarSug = useRef(false)
  const caixaRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    window.launcherAPI?.storeWarm?.()
  }, [])

  useEffect(() => {
    if (ignorarSug.current) {
      ignorarSug.current = false
      return
    }
    const q = busca.trim()
    const meu = ++gerSug.current
    if (q.length < 2) {
      setSugestoes([])
      setSugSel(-1)
      return
    }
    const timer = setTimeout(async () => {
      const r = await window.launcherAPI?.storeSuggest(q)
      if (meu !== gerSug.current) return
      setSugestoes(r?.ok ? r.jogos || [] : [])
      setSugSel(-1)
    }, 120)
    return () => clearTimeout(timer)
  }, [busca])

  const fecharSugestoes = useCallback(() => {
    gerSug.current++
    setSugestoes([])
    setSugSel(-1)
  }, [])

  useEffect(() => {
    if (!sugestoes.length) return
    const fora = (e: MouseEvent) => {
      if (!caixaRef.current?.contains(e.target as Node)) fecharSugestoes()
    }
    document.addEventListener("mousedown", fora)
    return () => document.removeEventListener("mousedown", fora)
  }, [sugestoes.length, fecharSugestoes])

  const pesquisar = useCallback(
    async (termo?: string) => {
      const q = (termo ?? busca).trim()
      if (!q) return
      if (termo !== undefined && termo !== busca) {
        ignorarSug.current = true
        setBusca(termo)
      }
      fecharSugestoes()
      const meu = ++gerBusca.current
      setBuscando(true)
      setMsg("")
      const r = await window.launcherAPI?.storeSearch(q)
      if (meu !== gerBusca.current) return
      setBuscando(false)
      if (!r?.ok) {
        setResultados([])
        setMsg(r?.error || t("store.busca_falhou"))
        return
      }
      setResultados(r.jogos || [])
      setMsg(r.jogos?.length ? "" : t("store.nada_encontrado"))
    },
    [busca, fecharSugestoes, t],
  )

  const limpar = useCallback(() => {
    gerBusca.current++
    ignorarSug.current = true
    setBusca("")
    setResultados(null)
    setMsg("")
    setBuscando(false)
    fecharSugestoes()
    inputRef.current?.focus()
  }, [fecharSugestoes])

  const aoTeclar = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && sugestoes.length) {
      e.preventDefault()
      setSugSel((i) => (i + 1) % sugestoes.length)
    } else if (e.key === "ArrowUp" && sugestoes.length) {
      e.preventDefault()
      setSugSel((i) => (i <= 0 ? sugestoes.length : i) - 1)
    } else if (e.key === "Escape") {
      if (sugestoes.length) fecharSugestoes()
      else limpar()
    } else if (e.key === "Enter") {
      pesquisar(sugSel >= 0 ? sugestoes[sugSel]?.title : undefined)
    }
  }

  const buscou = resultados !== null
  const grade = buscou ? resultados : []
  const carregandoGrade = buscando

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="ui-title mb-1">{t("store.titulo")}</h1>
      <p className="ui-subtitle mb-6">{t("store.descricao")}</p>

      <div className="mb-4 flex max-w-[860px] gap-2">
        <div ref={caixaRef} className="relative flex-1">
          <input
            ref={inputRef}
            value={busca}
            onChange={(e) => { ignorarSug.current = false; setBusca(e.target.value) }}
            onKeyDown={aoTeclar}
            placeholder={t("store.buscar_placeholder")}
            spellCheck={false}
            className="ui-input w-full py-2.5 pl-3.5 pr-9 text-[13px] placeholder:text-white/25"
          />
          {busca && (
            <button
              onClick={limpar}
              title={t("common.cancelar")}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-white/35 transition-colors hover:text-white"
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          )}
          {sugestoes.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-20 mt-1 overflow-hidden rounded-lg border border-white/10 bg-[#15181d] shadow-2xl shadow-black/60">
              {sugestoes.map((s, i) => (
                <button
                  key={s.appid}
                  onMouseDown={(e) => { e.preventDefault(); pesquisar(s.title) }}
                  onMouseEnter={() => setSugSel(i)}
                  className={`block w-full truncate px-3.5 py-2 text-left text-[13px] transition-colors ${i === sugSel ? "bg-white/[0.09] text-white" : "text-white/80 hover:bg-white/[0.07] hover:text-white"}`}
                >
                  {s.title}
                </button>
              ))}
            </div>
          )}
        </div>
        <button
          onClick={() => pesquisar()}
          disabled={buscando || !busca.trim()}
          className="ui-btn-primary rounded-lg px-4 py-2.5 text-[12px] disabled:opacity-50"
          style={{ background: "var(--accent)" }}
        >
          {buscando ? t("store.buscando") : t("store.buscar")}
        </button>
      </div>

      {msg && <p className="mb-4 text-[12px] text-white/55">{msg}</p>}

      {buscou && (
        <h2 className="mb-3 text-sm font-medium text-white/60">
          {t("store.resultados_count", { count: grade.length })}
        </h2>
      )}

      {(() => {
        const mostraCat = !buscou
        const itens = mostraCat ? catalogo : grade
        const carregando = mostraCat ? catCarregando : carregandoGrade
        return (
          <>
            <div className="grid-stagger grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3 pb-6">
              {carregando && esqueletos.map((i) => (
                <div key={`sk${i}`} className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
                  <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
                  <div className="p-3">
                    <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
                    <div className="h-8 animate-pulse rounded-lg bg-white/[0.04]" />
                  </div>
                </div>
              ))}
              {!carregando && itens.map((j) => (
                <CartaoLoja
                  key={j.appid}
                  jogo={j}
                  naBiblioteca={bloqueados.has(j.appid)}
                  adicionado={jaAdicionados.has(j.appid)}
                  onOpen={() => setPagina(j)}
                  t={t}
                />
              ))}
            </div>
            {mostraCat && !carregando && catTotal > POR_PAGINA && (
              <Paginacao
                pag={catPag}
                totalPags={Math.ceil(catTotal / POR_PAGINA)}
                onIr={(p) => { setCatPag(p); window.scrollTo?.(0, 0) }}
              />
            )}
          </>
        )
      })()}

      {pagina && (
        <StoreGamePage
          jogo={pagina}
          onClose={() => setPagina(null)}
          onBaixar={() => baixar(pagina)}
          onAdicionar={() => adicionar(pagina)}
          onRemover={() => remover(pagina)}
          naBiblioteca={bloqueados.has(pagina.appid)}
          ocupado={acaoBusy !== ""}
        />
      )}

      {escolhendo && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm" onClick={() => setEscolhendo(null)}>
          <div className="w-[440px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <h3 className="mb-1 text-base font-semibold text-white">{t("store.instalar_em", { title: escolhendo.jogo.title })}</h3>
            <p className="mb-4 text-[12px] text-white/40">{t("store.escolher_biblioteca")}</p>
            <div className="flex flex-col gap-2">
              {escolhendo.libs.map((l, i) => (
                <button
                  key={l.steamDir}
                  onClick={() => confirmarBaixar(escolhendo.jogo, escolhendo.info, l.steamDir)}
                  className={`flex items-center justify-between rounded-xl border px-4 py-3 text-left transition-colors ${i === 0 ? "border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)]" : "border-white/10 hover:border-white/25"}`}
                >
                  <span className="text-[13px] font-medium text-white/90">{l.steamDir.replace(/^\/home\/[^/]+/, "~")}</span>
                  <span className="text-[11px] font-semibold text-white/50">{t("store.gb_livres", { free: l.free.toFixed(2) })}</span>
                </button>
              ))}
            </div>
            <button onClick={() => setEscolhendo(null)} className="mt-3 w-full rounded-lg border border-white/10 py-2 text-[12px] font-semibold text-white/50 transition-colors hover:border-white/25 hover:text-white/80">{t("common.cancelar")}</button>
          </div>
        </div>
      )}

      {toast && (
        <div className="fixed bottom-5 right-5 z-[80] max-w-[360px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-4 py-3 text-[13px] text-white/90 shadow-2xl shadow-black/60 backdrop-blur-md" onClick={() => setToast("")}>{toast}</div>
      )}
    </div>
  )
}

// Paginação numérica (1-based na UI, 0-based no estado). Mostra vizinhas +
// elipse + primeira/última, no estilo do print.
function Paginacao({ pag, totalPags, onIr }: { pag: number; totalPags: number; onIr: (p: number) => void }) {
  const atual = pag + 1
  const nums: (number | "…")[] = []
  const push = (n: number | "…") => nums.push(n)
  const janela = new Set<number>([1, totalPags, atual, atual - 1, atual + 1, atual - 2, atual + 2])
  let ant = 0
  for (let n = 1; n <= totalPags; n++) {
    if (!janela.has(n)) continue
    if (n - ant > 1) push("…")
    push(n)
    ant = n
  }
  const Btn = ({ children, on, ativo, off }: { children: React.ReactNode; on?: () => void; ativo?: boolean; off?: boolean }) => (
    <button
      onClick={on}
      disabled={off}
      className={`min-w-9 rounded-lg px-3 py-2 text-[13px] font-medium transition-colors disabled:opacity-30 ${
        ativo ? "bg-[color:var(--accent)] text-black" : "border border-white/10 text-white/70 hover:border-white/25 hover:text-white"
      }`}
    >
      {children}
    </button>
  )
  return (
    <div className="flex flex-wrap items-center justify-center gap-2 pb-8 pt-2">
      <Btn on={() => onIr(pag - 1)} off={atual <= 1}>‹</Btn>
      {nums.map((n, i) =>
        n === "…" ? (
          <span key={`e${i}`} className="px-1 text-white/30">…</span>
        ) : (
          <Btn key={n} on={() => onIr(n - 1)} ativo={n === atual}>{n}</Btn>
        ),
      )}
      <Btn on={() => onIr(pag + 1)} off={atual >= totalPags}>›</Btn>
    </div>
  )
}
