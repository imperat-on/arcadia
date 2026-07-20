"use client"

import { forwardRef, useEffect, useRef, useState } from "react"
import { corDominante } from "./corDominante"
import type { Game } from "./types"
import { StoreRow, type JogoLinha } from "./StoreRow"
import { StoreGamePage } from "./StoreGamePage"
import { StoreKeyboard } from "./StoreKeyboard"
import { useStoreActions } from "../useStoreActions"

interface StoreConsoleProps {
  games: Game[]
  /** Pausa o trailer do destaque quando a janela perde o foco. */
  ativo: boolean
  /** Recebe os atalhos X/Y para o laço de gamepad do PS5Launcher acionar. */
  onAtalhos?: (a: { baixar: (appid: string) => void; adicionar: (appid: string) => void }) => void
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

type Ficha = NonNullable<Awaited<ReturnType<NonNullable<typeof window.launcherAPI>["storeDetails"]>>["jogo"]>

export const StoreConsole = forwardRef<HTMLDivElement, StoreConsoleProps>(function StoreConsole(
  { games, ativo, onAtalhos },
  ref,
) {
  const acoes = useStoreActions(games)
  const [emAlta, setEmAlta] = useState<JogoLinha[]>([])
  const [linhas, setLinhas] = useState<Record<string, JogoLinha[]>>({})
  const [carregando, setCarregando] = useState(true)
  // O herói acompanha o jogo FOCADO. Antes mostrava um destaque escolhido no
  // carregamento e nunca mudava — percorrer as linhas não mexia em nada em
  // cima, que era a principal razão de a tela parecer inerte.
  const [destaque, setDestaque] = useState<JogoLinha | null>(null)
  const [ficha, setFicha] = useState<Ficha | null>(null)
  const [trailer, setTrailer] = useState<{ url: string; poster: string } | null>(null)
  const [cor, setCor] = useState("")
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

  // Ficha e trailer do jogo focado. A ficha espera 600ms parado e o trailer
  // 1,2s: sem essa espera, atravessar uma linha dispararia uma chamada por
  // capa e o appdetails bate no limite (~200 a cada 5 min). O cache de 24h
  // absorve o resto.
  useEffect(() => {
    if (!destaque) return
    let cancelado = false
    setTrailer(null)
    const t = setTimeout(() => {
      window.launcherAPI?.storeDetails(destaque.appid).then((r) => {
        if (cancelado || !r?.ok || !r.jogo) return
        setFicha(r.jogo)
      })
    }, 600)
    const tv = setTimeout(() => {
      window.launcherAPI?.storeDetails(destaque.appid).then((r) => {
        if (cancelado || !r?.ok || !r.jogo?.trailer) return
        setTrailer({ url: r.jogo.trailer.url, poster: r.jogo.trailer.poster })
      })
    }, 1200)
    return () => {
      cancelado = true
      clearTimeout(t)
      clearTimeout(tv)
    }
  }, [destaque])

  // Cor ambiente extraída da capa. Vem do cache na segunda vez, então voltar
  // num jogo já visto é instantâneo.
  useEffect(() => {
    if (!destaque) return
    let cancelado = false
    corDominante(`https://cdn.akamai.steamstatic.com/steam/apps/${destaque.appid}/library_600x900.jpg`).then((c) => {
      if (!cancelado) setCor(c)
    })
    return () => {
      cancelado = true
    }
  }, [destaque])

  // Parallax do herói: ele sobe a uma fração da rolagem, criando profundidade
  // entre o fundo e as linhas.
  const raiz = useRef<HTMLDivElement | null>(null)
  const [scroll, setScroll] = useState(0)

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

  // X e Y agem sobre a capa em foco, sem abrir a página. Procuramos o jogo em
  // todas as listas carregadas porque o foco pode estar em qualquer linha.
  useEffect(() => {
    if (!onAtalhos) return
    const achar = (appid: string): JogoLinha | undefined =>
      [emAlta, lista, resultados || [], ...Object.values(linhas)].flat().find((j) => j.appid === appid)
    onAtalhos({
      baixar: (appid) => {
        const j = achar(appid)
        if (j && !acoes.bloqueados.has(appid) && j.manifest !== false) acoes.baixar(j)
      },
      adicionar: (appid) => {
        const j = achar(appid)
        if (j && !acoes.bloqueados.has(appid) && j.manifest !== false) acoes.adicionar(j)
      },
    })
  }, [onAtalhos, emAlta, lista, resultados, linhas, acoes])

  return (
    <div
      ref={(el) => {
        raiz.current = el
        if (typeof ref === "function") ref(el)
        else if (ref) (ref as React.MutableRefObject<HTMLDivElement | null>).current = el
      }}
      onScroll={(e) => setScroll((e.target as HTMLElement).scrollTop)}
      className="loja h-full w-full overflow-y-auto overflow-x-hidden bg-[#08090b] text-white"
      // Tudo que reage à cor do jogo lê daqui: halo, borda da capa em foco,
      // barra de progresso das linhas e o sublinhado da aba ativa.
      style={cor ? ({ "--loja-cor": cor } as React.CSSProperties) : undefined}
    >
      {/* ── Herói (segue o foco) ─────────────────────────────────────────── */}
      <div className="relative h-[58vh] min-h-[360px] w-full overflow-hidden">
        <div
          className="absolute inset-0"
          // Parallax: o fundo sobe a 35% da rolagem, então as linhas parecem
          // deslizar POR CIMA dele em vez de junto.
          style={{ transform: `translateY(${scroll * -0.35}px) scale(1.06)` }}
        >
          {destaque && (
            <img
              key={destaque.appid}
              src={ficha?.fundo || `https://cdn.akamai.steamstatic.com/steam/apps/${destaque.appid}/header.jpg`}
              alt=""
              className="loja-heroi-arte absolute inset-0 h-full w-full object-cover"
            />
          )}
          {trailer && ativo && (
            <video
              key={trailer.url}
              src={trailer.url}
              poster={trailer.poster}
              autoPlay
              loop
              muted
              playsInline
              className="loja-heroi-arte absolute inset-0 h-full w-full object-cover"
            />
          )}
        </div>
        <div className="loja-halo absolute inset-0" />
        <div className="absolute inset-0 bg-gradient-to-t from-[#08090b] via-[#08090b]/60 to-transparent" />
        <div className="absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-[#08090b] to-transparent" />

        <div className="absolute bottom-9 left-12 right-12">
          {/* Linha de dados: só aparece quando a ficha chega, então não fica
              piscando enquanto se atravessa a linha depressa. */}
          <div className="mb-2 flex h-5 items-center gap-3 text-[13px] text-white/55">
            {ficha?.generos?.length ? <span>{ficha.generos.slice(0, 3).join(" · ")}</span> : null}
            {ficha?.metacritic ? (
              <span className="rounded px-1.5 py-0.5 text-[12px] font-semibold text-black" style={{ background: "var(--loja-cor)" }}>
                {ficha.metacritic}
              </span>
            ) : null}
            {ficha?.lancamento && <span className="text-white/35">{ficha.lancamento}</span>}
          </div>

          <h1 className="max-w-3xl text-5xl font-light tracking-wide drop-shadow-[0_2px_16px_rgba(0,0,0,0.85)]">
            {destaque?.title || (carregando ? "Carregando…" : "Loja")}
          </h1>

          <div className="mt-3 flex h-6 items-center gap-4 text-[13px]">
            {ficha?.preco && <span className="text-white/85">{ficha.preco}</span>}
            {destaque?.fontes?.length ? (
              <span className="text-white/40">Disponível em {destaque.fontes.join(", ")}</span>
            ) : destaque?.manifest === false ? (
              <span className="text-white/35">Sem manifesto</span>
            ) : null}
          </div>
        </div>
      </div>

      {/* ── Filtros ──────────────────────────────────────────────────────── */}
      {/* Abas com sublinhado que desliza entre elas, em vez de seis pílulas
          soltas — o movimento contínuo é o que liga uma categoria à outra. */}
      <div className="relative flex gap-7 overflow-x-auto px-12 pt-7 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {CATEGORIAS.map((c) => (
          <button
            key={c.id}
            onClick={() => {
              setCategoria(c.id)
              setResultados(null) // sair da busca ao escolher um filtro
            }}
            className={`relative shrink-0 pb-2 text-[14px] font-medium outline-none transition-colors focus:text-white ${
              categoria === c.id ? "text-white" : "text-white/40 hover:text-white/70"
            }`}
          >
            {c.rotulo}
            {categoria === c.id && (
              <span
                className="absolute inset-x-0 -bottom-px h-[2px] rounded-full"
                style={{ background: "var(--loja-cor)", boxShadow: "0 0 12px var(--loja-cor)" }}
              />
            )}
          </button>
        ))}
      </div>
      <div className="mx-12 h-px bg-white/[0.07]" />

      {/* ── Linhas ───────────────────────────────────────────────────────── */}
      <div className="pt-6">
        {resultados && (
          <StoreRow
            titulo={`Resultados para "${busca}" (${resultados.length})`}
            jogos={resultados}
            onAbrir={setAberto}
            onFocar={setDestaque}
          />
        )}
        {categoria === "destaques" ? (
          <>
            <StoreRow titulo="Em alta agora" jogos={emAlta} carregando={carregando} onAbrir={setAberto} onFocar={setDestaque} />
            {GENEROS.map((g) => (
              <StoreRow
                key={g.chave}
                titulo={g.rotulo}
                jogos={linhas[g.chave] || []}
                carregando={!linhas[g.chave]}
                onAbrir={setAberto}
                onFocar={setDestaque}
              />
            ))}
          </>
        ) : (
          <StoreRow
            titulo={CATEGORIAS.find((c) => c.id === categoria)?.rotulo || ""}
            jogos={lista}
            carregando={carregandoLista}
            onAbrir={setAberto}
            onFocar={setDestaque}
          />
        )}
      </div>

      {/* Dicas dos botões: os atalhos X/Y não teriam como ser descobertos. */}
      <div className="pointer-events-none fixed inset-x-0 bottom-0 z-[60] flex justify-center pb-4">
        <div className="rounded-full border border-white/10 bg-black/70 px-5 py-2 text-[12px] text-white/45">
          A abrir · X baixar · Y adicionar · B voltar
        </div>
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
