"use client"

import { forwardRef, useEffect, useState } from "react"
import type { Game } from "./types"
import { StoreRow, type JogoLinha } from "./StoreRow"
import { StoreGamePage } from "./StoreGamePage"
import { StoreKeyboard } from "./StoreKeyboard"
import { useStoreActions } from "../useStoreActions"

interface StoreConsoleProps {
  games: Game[]
  /** Pausa o trailer do destaque quando a janela perde o foco. */
  ativo: boolean
}

// Gêneros das linhas da home. Os nomes são os que o SteamSpy aceita (inglês);
// o rótulo mostrado é em português.
const GENEROS: { chave: string; rotulo: string }[] = [
  { chave: "Action", rotulo: "Ação" },
  { chave: "RPG", rotulo: "RPG" },
  { chave: "Indie", rotulo: "Indie" },
  { chave: "Adventure", rotulo: "Aventura" },
  { chave: "Strategy", rotulo: "Estratégia" },
  { chave: "Racing", rotulo: "Corrida" },
]

// Filtros do topo. "Destaques" é a home completa; os outros trocam as linhas
// por uma lista só. `fonte` diz de onde a lista vem.
type Categoria = {
  id: string
  rotulo: string
  fonte?: { tipo: "featured"; secao: string } | { tipo: "steamspy"; lista: string }
}
const CATEGORIAS: Categoria[] = [
  { id: "destaques", rotulo: "Destaques" },
  { id: "new_releases", rotulo: "Lançamentos", fonte: { tipo: "featured", secao: "new_releases" } },
  { id: "top_sellers", rotulo: "Mais vendidos", fonte: { tipo: "featured", secao: "top_sellers" } },
  { id: "jogados", rotulo: "Mais jogados", fonte: { tipo: "steamspy", lista: "top100forever" } },
  { id: "specials", rotulo: "Promoções", fonte: { tipo: "featured", secao: "specials" } },
  { id: "coming_soon", rotulo: "Em breve", fonte: { tipo: "featured", secao: "coming_soon" } },
]

export const StoreConsole = forwardRef<HTMLDivElement, StoreConsoleProps>(function StoreConsole(
  { games, ativo },
  ref,
) {
  const acoes = useStoreActions(games)
  const [emAlta, setEmAlta] = useState<JogoLinha[]>([])
  const [linhas, setLinhas] = useState<Record<string, JogoLinha[]>>({})
  const [carregando, setCarregando] = useState(true)
  const [destaque, setDestaque] = useState<JogoLinha | null>(null)
  const [trailer, setTrailer] = useState<{ url: string; poster: string } | null>(null)
  const [aberto, setAberto] = useState<JogoLinha | null>(null)
  const [teclado, setTeclado] = useState(false)
  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<JogoLinha[] | null>(null)
  const [categoria, setCategoria] = useState("destaques")
  const [lista, setLista] = useState<JogoLinha[]>([])
  const [carregandoLista, setCarregandoLista] = useState(false)

  // "Em alta" primeiro: é a linha que define o destaque, então a home tem
  // conteúdo antes de as outras chegarem.
  useEffect(() => {
    window.launcherAPI
      ?.storeRecent()
      .then((r) => {
        if (!r?.ok) return
        const jogos = (r.jogos || []) as JogoLinha[]
        setEmAlta(jogos)
        setDestaque(jogos.find((j) => j.manifest) || jogos[0] || null)
      })
      .finally(() => setCarregando(false))
  }, [])

  // As linhas por gênero entram uma a uma, em sequência. Em paralelo seriam
  // seis chamadas ao SteamSpy mais dezenas de sondagens de disponibilidade de
  // uma vez — a home congelaria até tudo responder.
  useEffect(() => {
    let cancelado = false
    ;(async () => {
      for (const g of GENEROS) {
        const r = await window.launcherAPI?.storeGenre(g.chave, 24)
        if (cancelado) return
        if (r?.ok) setLinhas((prev) => ({ ...prev, [g.chave]: (r.jogos || []) as JogoLinha[] }))
      }
    })()
    return () => {
      cancelado = true
    }
  }, [])

  // O trailer do destaque só é buscado quando o destaque muda — nunca para a
  // linha inteira, senão o appdetails bate no limite de requisições.
  useEffect(() => {
    if (!destaque) return
    let cancelado = false
    setTrailer(null)
    window.launcherAPI?.storeDetails(destaque.appid).then((r) => {
      if (cancelado || !r?.ok || !r.jogo?.trailer) return
      setTrailer({ url: r.jogo.trailer.url, poster: r.jogo.trailer.poster })
    })
    return () => {
      cancelado = true
    }
  }, [destaque])

  // Troca de filtro: busca a lista da categoria escolhida. "Destaques" não
  // busca nada — é a home, que já está carregada.
  useEffect(() => {
    const cat = CATEGORIAS.find((c) => c.id === categoria)
    if (!cat?.fonte) return setLista([])
    let cancelado = false
    setCarregandoLista(true)
    setLista([])
    const api = window.launcherAPI
    const p =
      cat.fonte.tipo === "featured"
        ? api?.storeFeatured(cat.fonte.secao, 24)
        : api?.storeRecent(cat.fonte.lista)
    p?.then((r) => {
      if (cancelado) return
      setLista(r?.ok ? ((r.jogos || []) as JogoLinha[]) : [])
    }).finally(() => !cancelado && setCarregandoLista(false))
    return () => {
      cancelado = true
    }
  }, [categoria])

  const pesquisar = async (termo: string) => {
    setTeclado(false)
    const q = termo.trim()
    setBusca(q)
    if (!q) return setResultados(null)
    const r = await window.launcherAPI?.storeSearch(q)
    setResultados(r?.ok ? ((r.jogos || []) as JogoLinha[]) : [])
  }

  const bloqueado = (j: JogoLinha | null) => Boolean(j && acoes.bloqueados.has(j.appid))

  return (
    <div ref={ref} className="h-full w-full overflow-y-auto overflow-x-hidden bg-[#08090b] text-white">
      {/* ── Destaque ─────────────────────────────────────────────────────── */}
      <div className="relative h-[62vh] min-h-[380px] w-full overflow-hidden">
        {trailer && ativo ? (
          <video
            key={trailer.url}
            src={trailer.url}
            poster={trailer.poster}
            autoPlay
            loop
            muted
            playsInline
            className="absolute inset-0 h-full w-full object-cover"
          />
        ) : destaque ? (
          <img
            src={`https://cdn.akamai.steamstatic.com/steam/apps/${destaque.appid}/header.jpg`}
            alt=""
            className="absolute inset-0 h-full w-full object-cover"
          />
        ) : null}
        <div className="absolute inset-0 bg-gradient-to-t from-[#08090b] via-[#08090b]/55 to-transparent" />

        <div className="absolute bottom-10 left-12 right-12">
          <h1 className="max-w-3xl text-5xl font-light tracking-wide drop-shadow-[0_2px_16px_rgba(0,0,0,0.8)]">
            {destaque?.title || (carregando ? "Carregando…" : "Loja")}
          </h1>
          <div className="mt-5 flex gap-3">
            <button
              onClick={() => destaque && setAberto(destaque)}
              disabled={!destaque}
              className="rounded-xl px-7 py-3 text-sm font-bold text-black outline-none transition-transform enabled:hover:scale-[1.03] disabled:opacity-40 focus:ring-2 focus:ring-white"
              style={{ background: "var(--accent)" }}
            >
              Ver na loja
            </button>
            <button
              onClick={() => setTeclado(true)}
              className="rounded-xl border border-white/20 px-7 py-3 text-sm font-semibold text-white/85 outline-none transition-colors hover:bg-white/[0.08] focus:ring-2 focus:ring-white"
            >
              Buscar
            </button>
          </div>
        </div>
      </div>

      {/* ── Filtros ──────────────────────────────────────────────────────── */}
      <div className="flex gap-2 overflow-x-auto px-12 pt-8 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {CATEGORIAS.map((c) => (
          <button
            key={c.id}
            onClick={() => {
              setCategoria(c.id)
              setResultados(null) // sair da busca ao escolher um filtro
            }}
            className={`shrink-0 rounded-full px-5 py-2 text-[13px] font-medium outline-none transition-colors focus:ring-2 focus:ring-white ${
              categoria === c.id
                ? "bg-white text-black"
                : "border border-white/12 text-white/65 hover:bg-white/[0.08]"
            }`}
          >
            {c.rotulo}
          </button>
        ))}
      </div>

      {/* ── Linhas ───────────────────────────────────────────────────────── */}
      <div className="pt-6">
        {resultados && (
          <StoreRow
            titulo={`Resultados para "${busca}" (${resultados.length})`}
            jogos={resultados}
            onAbrir={setAberto}
          />
        )}
        {categoria === "destaques" ? (
          <>
            <StoreRow titulo="Em alta agora" jogos={emAlta} carregando={carregando} onAbrir={setAberto} />
            {GENEROS.map((g) => (
              <StoreRow
                key={g.chave}
                titulo={g.rotulo}
                jogos={linhas[g.chave] || []}
                carregando={!linhas[g.chave]}
                onAbrir={setAberto}
              />
            ))}
          </>
        ) : (
          <StoreRow
            titulo={CATEGORIAS.find((c) => c.id === categoria)?.rotulo || ""}
            jogos={lista}
            carregando={carregandoLista}
            onAbrir={setAberto}
          />
        )}
      </div>

      <StoreGamePage
        jogo={aberto}
        bloqueado={bloqueado(aberto)}
        ocupado={Boolean(acoes.busy)}
        onBaixar={() => aberto && acoes.baixar(aberto)}
        onAdicionar={() => aberto && acoes.adicionar(aberto)}
        onRemover={() => aberto && acoes.remover(aberto)}
        onFechar={() => setAberto(null)}
      />

      <StoreKeyboard
        aberto={teclado}
        inicial={busca}
        onConfirmar={pesquisar}
        onFechar={() => setTeclado(false)}
      />

      {/* Escolha de disco — o mesmo diálogo do desktop, alimentado pelo hook */}
      {acoes.escolhendo && (
        <div className="gp-scope fixed inset-0 z-[90] flex items-center justify-center bg-black/80 backdrop-blur-sm">
          <div className="w-[520px] max-w-[92vw] rounded-2xl border border-white/10 bg-[#0d0d10] p-6">
            <h3 className="mb-1 text-lg font-semibold">Instalar "{acoes.escolhendo.jogo.title}" em:</h3>
            <p className="mb-5 text-[13px] text-white/40">Escolha a biblioteca Steam de destino.</p>
            <div className="flex flex-col gap-2">
              {acoes.escolhendo.libs.map((l, i) => (
                <button
                  key={l.steamDir}
                  onClick={() =>
                    acoes.escolhendo &&
                    acoes.confirmarBaixar(acoes.escolhendo.jogo, acoes.escolhendo.info, l.steamDir)
                  }
                  className={`flex items-center justify-between rounded-xl border px-5 py-4 text-left outline-none transition-colors ${
                    i === 0 ? "border-[color:var(--accent)]" : "border-white/10 hover:border-white/25"
                  }`}
                >
                  <span className="text-sm text-white/90">{l.steamDir.replace(/^\/home\/[^/]+/, "~")}</span>
                  <span className="text-xs text-white/45">{l.free.toFixed(2)} GB livres</span>
                </button>
              ))}
            </div>
            <button
              onClick={() => acoes.setEscolhendo(null)}
              className="mt-4 w-full rounded-lg border border-white/10 py-2.5 text-[13px] text-white/55 outline-none hover:text-white/85"
            >
              Cancelar
            </button>
          </div>
        </div>
      )}

      {acoes.toast && (
        <div
          className="fixed bottom-8 right-8 z-[95] max-w-[420px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-5 py-4 text-sm text-white/90 shadow-2xl backdrop-blur-md"
          onClick={() => acoes.setToast("")}
        >
          {acoes.toast}
        </div>
      )}
    </div>
  )
})
