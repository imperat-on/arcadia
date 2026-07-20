"use client"

import { forwardRef, useCallback, useEffect, useRef, useState } from "react"
import { corDominante } from "./corDominante"
import type { FichaJogo, Game, JogoLinha } from "./types"
import { StoreShowcase, type SecaoVitrine } from "./StoreShowcase"
import { StoreCategoria } from "./StoreCategoria"
import { StoreGamePage } from "./StoreGamePage"
import { StoreKeyboard } from "./StoreKeyboard"
import { StoreHUD } from "./StoreHUD"
import { useStoreActions } from "../useStoreActions"

const TAMANHO_PAGINA = 40

// Seções que compõem a vitrine inicial, na ordem em que aparecem.
const VITRINE = ["alta", "new_releases", "top_sellers", "specials", "jogados"]
// Quantos itens cada trilho mostra. Um trilho não rola infinito — quem quer
// mais entra na categoria.
const TAMANHO_TRILHO = 24

interface StoreConsoleProps {
  games: Game[]
  /** Pausa o trailer do destaque quando a janela perde o foco. */
  ativo: boolean
  /** Recebe os atalhos do laço de gamepad do PS5Launcher (X, Y e B). */
  onAtalhos?: (a: {
    baixar: (appid: string) => void
    adicionar: (appid: string) => void
    /** B: sai da categoria para a vitrine. Devolve true se consumiu o botão. */
    voltar: () => boolean
  }) => void
}

// A loja tem dois modos na mesma tela: a VITRINE (herói + um trilho por
// categoria) e a CATEGORIA aberta (grade densa com rolagem infinita). A régua
// de chips no topo alterna entre eles; B volta da categoria para a vitrine.
// As chaves de gênero são as que o SteamSpy aceita (inglês); o rótulo é nosso.
type Categoria = {
  id: string
  rotulo: string
  fonte: { tipo: "steamspy"; lista?: string; genero?: string } | { tipo: "featured"; secao: string }
}

const CATEGORIAS: Categoria[] = [
  { id: "alta", rotulo: "Em alta", fonte: { tipo: "steamspy" } },
  { id: "new_releases", rotulo: "Lançamentos", fonte: { tipo: "featured", secao: "new_releases" } },
  { id: "top_sellers", rotulo: "Mais vendidos", fonte: { tipo: "featured", secao: "top_sellers" } },
  { id: "jogados", rotulo: "Mais jogados", fonte: { tipo: "steamspy", lista: "top100forever" } },
  { id: "specials", rotulo: "Promoções", fonte: { tipo: "featured", secao: "specials" } },
  { id: "coming_soon", rotulo: "Em breve", fonte: { tipo: "featured", secao: "coming_soon" } },
  { id: "acao", rotulo: "Ação", fonte: { tipo: "steamspy", genero: "Action" } },
  { id: "rpg", rotulo: "RPG", fonte: { tipo: "steamspy", genero: "RPG" } },
  { id: "indie", rotulo: "Indie", fonte: { tipo: "steamspy", genero: "Indie" } },
  { id: "aventura", rotulo: "Aventura", fonte: { tipo: "steamspy", genero: "Adventure" } },
  { id: "estrategia", rotulo: "Estratégia", fonte: { tipo: "steamspy", genero: "Strategy" } },
  { id: "corrida", rotulo: "Corrida", fonte: { tipo: "steamspy", genero: "Racing" } },
]

export const StoreConsole = forwardRef<HTMLDivElement, StoreConsoleProps>(function StoreConsole(
  { games, ativo, onAtalhos },
  ref,
) {
  const acoes = useStoreActions(games)
  // Paginação: cada índice é uma resposta do backend. Concatenadas, viram a
  // lista visível para a grade. Manter em páginas facilita reset ao trocar
  // categoria e evita concat linear a cada `setLista`.
  const [paginas, setPaginas] = useState<JogoLinha[][]>([])
  const [total, setTotal] = useState<number | null>(null)
  const [carregando, setCarregando] = useState(true)
  const carregandoRef = useRef(false)
  const categoriaRef = useRef("alta")
  const cacheCategorias = useRef<Map<string, { paginas: JogoLinha[][]; total: number | null }>>(new Map())
  const [categoria, setCategoria] = useState("alta")
  // "vitrine" é a entrada (herói + trilhos); "categoria" é uma categoria
  // aberta em grade. B volta de uma para a outra.
  const [modo, setModo] = useState<"vitrine" | "categoria">("vitrine")
  // Jogos de cada trilho da vitrine, por id de categoria.
  const [vitrine, setVitrine] = useState<Record<string, JogoLinha[]>>({})
  const [carregandoVitrine, setCarregandoVitrine] = useState(true)
  // O herói e o HUD leem estes três, sempre do jogo em foco.
  const [destaque, setDestaque] = useState<JogoLinha | null>(null)
  const [ficha, setFicha] = useState<FichaJogo | null>(null)
  const [trailer, setTrailer] = useState<{ url: string; poster: string } | null>(null)
  const [cor, setCor] = useState("")
  const [aberto, setAberto] = useState<JogoLinha | null>(null)
  const [teclado, setTeclado] = useState(false)
  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<JogoLinha[] | null>(null)

  // Busca uma página de uma categoria sem tocar no estado da UI. Usada tanto
  // pela categoria ativa quanto pela pré-carga silenciosa dos vizinhos.
  const buscarCategoria = useCallback(async (catId: string, offset: number) => {
    const cat = CATEGORIAS.find((c) => c.id === catId)
    if (!cat) return null
    const api = window.launcherAPI
    const r =
      cat.fonte.tipo === "featured"
        ? await api?.storeFeatured(cat.fonte.secao, TAMANHO_PAGINA, offset)
        : cat.fonte.genero
          ? await api?.storeGenre(cat.fonte.genero, TAMANHO_PAGINA, offset)
          : await api?.storeRecent(cat.fonte.lista, TAMANHO_PAGINA, offset)
    if (!r?.ok) return null
    return {
      jogos: (r.jogos || []) as JogoLinha[],
      total: typeof r.total === "number" ? r.total : null,
    }
  }, [])

  // Carrega a categoria ativa. Mantém um cache em memória por categoria para
  // que voltar num filtro já visitado (ou pré-carregado) seja instantâneo.
  const carregar = useCallback(async (catId: string, offset: number) => {
    if (carregandoRef.current) return
    carregandoRef.current = true
    setCarregando(true)
    try {
      const r = await buscarCategoria(catId, offset)
      if (!r || categoriaRef.current !== catId) return
      setPaginas((atual) => {
        const novo = offset === 0 ? [r.jogos] : [...atual, r.jogos]
        cacheCategorias.current.set(catId, { paginas: novo, total: r.total })
        return novo
      })
      setTotal(r.total)
    } finally {
      carregandoRef.current = false
      if (categoriaRef.current === catId) setCarregando(false)
    }
  }, [buscarCategoria])

  // Monta a vitrine. As seções entram em DUAS ondas: as duas primeiras (as
  // que aparecem sem rolar) e, só depois, o resto. Disparar as cinco de uma
  // vez daria uma rajada de requisições para pintar coisa fora da tela.
  //
  // Tudo o que chega vai para o mesmo cacheCategorias da grade, então abrir a
  // categoria depois é instantâneo.
  useEffect(() => {
    let cancelado = false
    const puxar = async (ids: string[]) => {
      await Promise.all(
        ids.map(async (id) => {
          const cached = cacheCategorias.current.get(id)
          const r = cached ? { jogos: cached.paginas.flat(), total: cached.total } : await buscarCategoria(id, 0)
          if (!r || cancelado) return
          if (!cached) cacheCategorias.current.set(id, { paginas: [r.jogos], total: r.total })
          setVitrine((v) => ({ ...v, [id]: r.jogos.slice(0, TAMANHO_TRILHO) }))
        }),
      )
    }
    setCarregandoVitrine(true)
    puxar(VITRINE.slice(0, 2))
      .then(() => {
        if (!cancelado) setCarregandoVitrine(false)
        return puxar(VITRINE.slice(2))
      })
      .catch(() => {
        if (!cancelado) setCarregandoVitrine(false)
      })
    return () => {
      cancelado = true
    }
  }, [buscarCategoria])

  // Ao abrir/trocar de categoria, usa o cache em memória se existir; senão
  // carrega. Na vitrine não há categoria ativa, então nada é buscado aqui.
  useEffect(() => {
    if (modo !== "categoria") return
    categoriaRef.current = categoria
    setDestaque(null)
    const cached = cacheCategorias.current.get(categoria)
    if (cached) {
      setPaginas(cached.paginas)
      setTotal(cached.total)
      setCarregando(false)
    } else {
      setPaginas([])
      setTotal(null)
      carregar(categoria, 0)
    }
  }, [modo, categoria, carregar])

  // Pré-carrega as categorias vizinhas em segundo plano para a troca de filtro
  // ser instantânea. Usa buscarCategoria diretamente para não disputar a trava
  // de loading da categoria ativa.
  useEffect(() => {
    if (modo !== "categoria") return
    if (carregando) return
    if (!paginas.length) return
    const idx = CATEGORIAS.findIndex((c) => c.id === categoria)
    if (idx < 0) return
    const vizinhos = [idx - 1, idx + 1]
      .filter((i) => i >= 0 && i < CATEGORIAS.length)
      .map((i) => CATEGORIAS[i].id)
    for (const id of vizinhos) {
      if (cacheCategorias.current.has(id)) continue
      buscarCategoria(id, 0).then((r) => {
        if (!r) return
        cacheCategorias.current.set(id, { paginas: [r.jogos], total: r.total })
      })
    }
  }, [modo, carregando, paginas, categoria, buscarCategoria])

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

  const raiz = useRef<HTMLDivElement | null>(null)


  const pesquisar = async (termo: string) => {
    setTeclado(false)
    const q = termo.trim()
    setBusca(q)
    if (!q) return setResultados(null)
    const r = await window.launcherAPI?.storeSearch(q)
    setResultados(r?.ok ? ((r.jogos || []) as JogoLinha[]) : [])
  }

  const bloqueado = (j: JogoLinha | null) => Boolean(j && acoes.bloqueados.has(j.appid))

  // X e Y agem sobre o card em foco, sem abrir a página. Procuramos o jogo em
  // tudo que está na tela — trilhos da vitrine, páginas da categoria e
  // resultados de busca —, porque o foco pode estar em qualquer um.
  useEffect(() => {
    if (!onAtalhos) return
    const achar = (appid: string): JogoLinha | undefined =>
      [...Object.values(vitrine).flat(), ...paginas.flat(), ...(resultados || [])].find((j) => j.appid === appid)
    onAtalhos({
      // B sai da busca ou da categoria; na vitrine devolve false para o
      // PS5Launcher tratar o botão como "sair da loja".
      voltar: () => {
        if (resultados) {
          setResultados(null)
          setBusca("")
          return true
        }
        if (modo === "categoria") {
          setModo("vitrine")
          setDestaque(null)
          return true
        }
        return false
      },
      baixar: (appid) => {
        const j = achar(appid)
        if (j && !acoes.bloqueados.has(appid) && j.manifest !== false) acoes.baixar(j)
      },
      adicionar: (appid) => {
        const j = achar(appid)
        if (j && !acoes.bloqueados.has(appid) && j.manifest !== false) acoes.adicionar(j)
      },
    })
  }, [onAtalhos, vitrine, paginas, resultados, modo, acoes])

  const semManifesto = destaque?.manifest === false
  const jaTem = bloqueado(destaque)

  // Quantos itens já carregamos vs. quanto o backend disse que existe.
  const carregados = paginas.reduce((n, p) => n + p.length, 0)
  const temMais = total == null ? carregados > 0 && !carregando : carregados < total

  // Pedido de próxima página vindo do IntersectionObserver da grade. Só
  // dispara se tem mais e a categoria atual não é resultado de busca.
  const pedirMais = useCallback(() => {
    if (resultados) return
    if (carregandoRef.current) return
    if (total != null && carregados >= total) return
    if (carregados === 0) return // primeira página é responsabilidade do useEffect
    carregar(categoriaRef.current, carregados)
  }, [carregar, resultados, total, carregados])

  // Só entram na vitrine as seções que já chegaram — um trilho vazio no meio
  // da coluna abriria um buraco enquanto a segunda onda não responde.
  const secoesVitrine: SecaoVitrine[] = VITRINE.map((id) => ({
    id,
    rotulo: CATEGORIAS.find((c) => c.id === id)?.rotulo || id,
    jogos: vitrine[id] || [],
  })).filter((s, i) => s.jogos.length || i < 2)

  return (
    <div
      ref={(el) => {
        raiz.current = el
        if (typeof ref === "function") ref(el)
        else if (ref) (ref as React.MutableRefObject<HTMLDivElement | null>).current = el
      }}
      // Preto OLED puro, igual à aba Notícias: sem palco, sem vinheta, sem
      // tint. A cor dominante ainda pinta ring do card, chip ativo e preço
      // no HUD — só o FUNDO fica preto absoluto.
      className="loja flex h-full w-full flex-col overflow-hidden bg-black text-white"
      style={cor ? ({ "--loja-cor": cor } as React.CSSProperties) : undefined}
    >
      {/* ── Régua de categorias ──────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-2.5 overflow-x-auto px-12 pb-4 pt-8 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <button
          onClick={() => setTeclado(true)}
          className="loja-chip"
          aria-label="Buscar"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="mr-1.5">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          Buscar
        </button>

        <span className="mx-1 h-4 w-px shrink-0 bg-white/10" />

        {/* "Vitrine" volta para a entrada; os demais abrem a categoria em
            grade. Sem este chip, quem entrasse numa categoria com o mouse não
            teria como voltar (B é só do controle). */}
        <button
          onClick={() => {
            setModo("vitrine")
            setResultados(null)
            setDestaque(null)
          }}
          className={`loja-chip${!resultados && modo === "vitrine" ? " -ativo" : ""}`}
        >
          Vitrine
        </button>

        {CATEGORIAS.map((c) => (
          <button
            key={c.id}
            onClick={() => {
              setCategoria(c.id)
              setModo("categoria")
              setResultados(null) // sair da busca ao escolher uma categoria
            }}
            className={`loja-chip${!resultados && modo === "categoria" && categoria === c.id ? " -ativo" : ""}`}
          >
            {c.rotulo}
          </button>
        ))}
      </div>

      {resultados && (
        <p className="shrink-0 px-12 pb-3 text-[13px] text-white/60">
          Resultados para "{busca}" ({resultados.length})
        </p>
      )}

      {/* ── Corpo: vitrine, categoria ou resultados ──────────────────────── */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pt-2">
        {resultados ? (
          // A busca reaproveita a grade da categoria, sem paginação: os
          // resultados são curtos e cabem numa resposta só.
          <StoreCategoria
            paginas={[resultados]}
            carregando={false}
            temMais={false}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onPedirMais={() => {}}
          />
        ) : modo === "vitrine" ? (
          <StoreShowcase
            secoes={secoesVitrine}
            carregando={carregandoVitrine}
            focado={destaque}
            ficha={ficha}
            trailer={trailer}
            ativo={ativo}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onVerCategoria={(id) => {
              setCategoria(id)
              setModo("categoria")
            }}
          />
        ) : (
          <StoreCategoria
            paginas={paginas}
            carregando={carregando}
            temMais={temMais}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onPedirMais={pedirMais}
          />
        )}
      </div>

      {/* ── HUD contextual ───────────────────────────────────────────────── */}
      <div>
        <StoreHUD
          destaque={destaque}
          ficha={ficha}
          bloqueado={jaTem}
          semManifesto={Boolean(semManifesto)}
        />
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
