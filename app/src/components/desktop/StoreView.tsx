"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useStoreActions } from "../useStoreActions"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"

// Aba Lojas: busca no catálogo (Hubcap + Steam) e download direto para a
// biblioteca Steam (DepotDownloader + SLSsteam). O setup (API key, .NET,
// SLSsteam) fica em Configurações → Integrações.

type ItemLoja = {
  appid: string
  title: string
  cover?: string
  manifest?: boolean
}

type Sugestao = { appid: string; title: string }

// Imagem da loja com fallback: header.jpg → capsule → placeholder com título
// (demos/playtests novos ainda não têm header no CDN da Steam).
function StoreImg({ appid, cover, title }: { appid: string; cover?: string; title: string }) {
  const [fase, setFase] = useState(0)
  // Reinicia a cascata quando o card é reaproveitado para outro jogo (a chave
  // do React é o appid, mas o `cover` pode mudar sozinho ao revalidar).
  useEffect(() => setFase(0), [appid, cover])
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
  const { t } = useI18n()
  // Ações (Baixar/Add/Remover/reiniciar Steam), estado de bloqueio e escolha de
  // disco vêm do hook compartilhado com a loja do modo console.
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
  const [resultados, setResultados] = useState<ItemLoja[] | null>(null)
  const [recentes, setRecentes] = useState<ItemLoja[]>([])
  const [sugestoes, setSugestoes] = useState<Sugestao[]>([])
  const [sugSel, setSugSel] = useState(-1)
  const [carregandoRec, setCarregandoRec] = useState(true)
  const [buscando, setBuscando] = useState(false)
  const [msg, setMsg] = useState("")

  // ── Contadores de geração ────────────────────────────────────────────────
  // Toda resposta assíncrona carrega o número do pedido que a originou; se
  // esse número não é mais o atual, a resposta é descartada. É o que impede a
  // lista de "piscar" com o resultado de um termo que o usuário já abandonou.
  const gerSug = useRef(0)
  const gerBusca = useRef(0)
  // Ligado quando somos NÓS que preenchemos o campo (ao escolher uma
  // sugestão): sem isso o efeito de sugestões rodava de novo e reabria a lista
  // logo após o clique — o famoso "tenho que clicar duas vezes".
  const ignorarSug = useRef(false)
  const caixaRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  // Abre a conexão com a Steam assim que a aba monta. A primeira requisição do
  // processo custa ~3s de DNS + TLS; pagando aqui, a primeira tecla digitada
  // já encontra o socket pronto (~250ms).
  useEffect(() => {
    window.launcherAPI?.storeWarm?.()
  }, [])

  useEffect(() => {
    let vivo = true
    window.launcherAPI
      ?.storeRecent()
      .then((r) => {
        if (vivo && r?.ok) setRecentes(r.jogos || [])
      })
      .finally(() => {
        if (vivo) setCarregandoRec(false)
      })
    return () => {
      vivo = false
    }
  }, [])

  // ── Sugestões enquanto digita ────────────────────────────────────────────
  // Usam storeSuggest (uma chamada à Steam, só títulos), nunca a busca
  // completa — esta sonda a disponibilidade de cada resultado e custa segundos.
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
      if (meu !== gerSug.current) return // chegou tarde: já digitaram mais
      setSugestoes(r?.ok ? r.jogos || [] : [])
      setSugSel(-1)
    }, 120)
    return () => clearTimeout(timer)
  }, [busca])

  const fecharSugestoes = useCallback(() => {
    gerSug.current++ // invalida qualquer resposta ainda em voo
    setSugestoes([])
    setSugSel(-1)
  }, [])

  // Fecha a lista ao clicar fora. Substitui o onBlur com setTimeout, que
  // fechava a lista antes do clique registrar em alguns casos.
  useEffect(() => {
    if (!sugestoes.length) return
    const fora = (e: MouseEvent) => {
      if (!caixaRef.current?.contains(e.target as Node)) fecharSugestoes()
    }
    document.addEventListener("mousedown", fora)
    return () => document.removeEventListener("mousedown", fora)
  }, [sugestoes.length, fecharSugestoes])

  // ── Busca completa ───────────────────────────────────────────────────────
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
      if (meu !== gerBusca.current) return // outra busca começou depois desta
      setBuscando(false)
      if (!r?.ok) {
        setResultados([])
        setMsg(r?.error || t("store.busca_falhou"))
        return
      }
      const jogos = r.jogos || []
      setResultados(jogos)
      setMsg(jogos.length ? "" : t("store.nada_encontrado"))
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

  // Grade exibida: resultados da busca (quando houve uma) ou os "em alta".
  // `resultados === null` distingue "ainda não buscou" de "buscou e deu zero" —
  // antes as duas situações eram o mesmo array vazio e o cabeçalho mentia.
  const buscou = resultados !== null
  const grade = buscou ? resultados : recentes
  const carregandoGrade = buscando || (!buscou && carregandoRec && recentes.length === 0)
  const esqueletos = useMemo(() => Array.from({ length: 8 }, (_, i) => i), [])

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      <h1 className="mb-1 text-2xl font-light tracking-wide text-white">{t("store.titulo")}</h1>
      <p className="mb-6 text-sm text-white/40">{t("store.descricao")}</p>

      {/* Busca + Restart Steam */}
      <div className="mb-4 flex max-w-[860px] gap-2">
        <div ref={caixaRef} className="relative flex-1">
          <input
            ref={inputRef}
            value={busca}
            onChange={(e) => {
              // Digitou de verdade: volta a sugerir (o flag pode ter ficado
              // ligado se a sugestão escolhida era igual ao texto do campo).
              ignorarSug.current = false
              setBusca(e.target.value)
            }}
            onKeyDown={aoTeclar}
            placeholder={t("store.buscar_placeholder")}
            spellCheck={false}
            className="w-full rounded-lg border border-white/10 bg-white/[0.04] py-2.5 pl-3.5 pr-9 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
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
                  // mousedown (e não click): dispara antes de o input perder o
                  // foco, então a escolha nunca é engolida pelo fechamento.
                  onMouseDown={(e) => {
                    e.preventDefault()
                    pesquisar(s.title)
                  }}
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
          disabled={buscando || !busca.trim()}
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
          esqueletos.map((i) => (
            <div key={`sk${i}`} className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
              <div className="aspect-[460/215] w-full animate-pulse bg-white/[0.05]" />
              <div className="p-3">
                <div className="mb-2 h-3.5 w-3/4 animate-pulse rounded bg-white/[0.07]" />
                <div className="h-8 animate-pulse rounded-lg bg-white/[0.04]" />
              </div>
            </div>
          ))}
        {!carregandoGrade &&
          grade.map((j) => (
            <CartaoLoja
              key={j.appid}
              jogo={j}
              naBiblioteca={bloqueados.has(j.appid)}
              adicionado={jaAdicionados.has(j.appid)}
              ocupado={acaoBusy !== ""}
              nesteJogo={acaoBusy === j.appid}
              onBaixar={() => baixar(j)}
              onAdicionar={() => adicionar(j)}
              onRemover={() => remover(j)}
              t={t}
            />
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

// Card extraído do corpo da lista: com 24 resultados na tela, deixar tudo
// inline fazia cada tecla digitada na busca re-renderizar os 24 cards.
function CartaoLoja({
  jogo,
  naBiblioteca,
  adicionado,
  ocupado,
  nesteJogo,
  onBaixar,
  onAdicionar,
  onRemover,
  t,
}: {
  jogo: ItemLoja
  naBiblioteca: boolean
  adicionado: boolean
  ocupado: boolean
  nesteJogo: boolean
  onBaixar: () => void
  onAdicionar: () => void
  onRemover: () => void
  t: (k: string, v?: Record<string, string | number>) => string
}) {
  return (
    <div className="overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.02]">
      <div className="aspect-[460/215] w-full bg-black">
        <StoreImg appid={jogo.appid} cover={jogo.cover} title={jogo.title} />
      </div>
      <div className="p-3">
        <div className="mb-2 truncate text-[13px] font-medium text-white" title={jogo.title}>
          {jogo.title}
        </div>
        {naBiblioteca ? (
          <div className="flex gap-2">
            <div
              className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-[color:var(--accent)]/40 py-2 text-[12px] font-semibold"
              style={{ color: "var(--accent)" }}
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="20 6 9 17 4 12" />
              </svg>
              {t("store.na_biblioteca")}
            </div>
            {adicionado && (
              <button
                onClick={onRemover}
                disabled={ocupado}
                title={t("store.remover_tooltip")}
                className="rounded-lg border border-[#ff6b81]/40 px-3 py-2 text-[12px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
              >
                {nesteJogo ? "…" : t("common.remover")}
              </button>
            )}
          </div>
        ) : jogo.manifest === false ? (
          /* A busca já sabe se algum provedor tem o manifesto. Sem usar esse
             dado, o botão Baixar ficava ativo em jogo indisponível: o clique
             consultava todos os provedores e terminava num toast no canto, sem
             nunca chegar à escolha de disco — parecia que não fazia nada. */
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
              onClick={onBaixar}
              disabled={ocupado}
              className="flex-1 rounded-lg px-3 py-2 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.02] disabled:opacity-50"
              style={{ background: "var(--accent)" }}
            >
              {nesteJogo ? "…" : t("store.baixar")}
            </button>
            <button
              onClick={onAdicionar}
              disabled={ocupado}
              title={t("store.add_tooltip")}
              className="flex-1 rounded-lg border border-white/20 px-3 py-2 text-[12px] font-semibold text-white/80 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
            >
              {t("store.add")}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
