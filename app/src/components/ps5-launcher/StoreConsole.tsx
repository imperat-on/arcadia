"use client"

import { forwardRef, useCallback, useEffect, useRef, useState } from "react"
import { corDominante } from "./corDominante"
import type { FichaJogo, Game, JogoLinha } from "./types"
import { StoreShowcase, type SecaoVitrine } from "./StoreShowcase"
import { StoreCategoria } from "./StoreCategoria"
import { StoreGamePage } from "./StoreGamePage"
import { StoreKeyboard } from "./StoreKeyboard"
import { ConsoleDestinoDialog } from "./ConsoleDestinoDialog"
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
  // Estado do herói, separado do foco: ele roda sozinho pelos destaques.
  const [heroiIdx, setHeroiIdx] = useState(0)
  const [fichaHeroi, setFichaHeroi] = useState<FichaJogo | null>(null)
  const [trailerHeroi, setTrailerHeroi] = useState<{ url: string } | null>(null)
  // O herói e o HUD leem estes três, sempre do jogo em foco.
  const [destaque, setDestaque] = useState<JogoLinha | null>(null)
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

  // Herói: os primeiros "Mais vendidos" em rodízio. Se essa seção ainda não
  // respondeu, cai em "Em alta" para a tela não abrir vazia.
  const destaques = (vitrine.top_sellers?.length ? vitrine.top_sellers : vitrine.alta || []).slice(0, 5)
  const heroi = destaques[heroiIdx] || null

  // Ficha e trailer do herói. Independentes do jogo em foco: o herói tem vida
  // própria (roda sozinho) e trocar de capa não pode apagar a arte dele.
  useEffect(() => {
    if (!heroi) return
    let cancelado = false
    setTrailerHeroi(null)
    window.launcherAPI?.storeDetails(heroi.appid).then((r) => {
      if (cancelado || !r?.ok || !r.jogo) return
      setFichaHeroi(r.jogo)
      // O herói ocupa a largura da tela: em movie480 o vídeo fica mais borrado
      // que a imagem parada atrás dele. Os ladrilhos seguem em 480p.
      if (r.jogo.trailer) {
        setTrailerHeroi({ url: r.jogo.trailer.alta || r.jogo.trailer.url })
      }
    })
    return () => {
      cancelado = true
    }
  }, [heroi])

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
      // O navy profundo (definido em .loja) substitui o preto absoluto: no
      // preto puro os cards não tinham chão do qual se destacar.
      className="loja flex h-full w-full flex-col overflow-hidden text-white"
      style={cor ? ({ "--loja-cor": cor } as React.CSSProperties) : undefined}
    >
      {/* ── Barra superior ───────────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-6 border-b border-[var(--loja-fio)] px-12">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto py-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {/* "Vitrine" volta para a entrada; as demais abrem a categoria em
              grade. Sem esta aba, quem entrasse numa categoria com o mouse não
              teria como voltar (B é só do controle). */}
          <button
            onClick={() => {
              setModo("vitrine")
              setResultados(null)
              setDestaque(null)
            }}
            className={`loja-aba${!resultados && modo === "vitrine" ? " -ativo" : ""}`}
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
              className={`loja-aba${!resultados && modo === "categoria" && categoria === c.id ? " -ativo" : ""}`}
            >
              {c.rotulo}
            </button>
          ))}
        </div>

        <button
          onClick={() => setTeclado(true)}
          aria-label="Buscar"
          className="grid h-9 w-9 shrink-0 place-items-center rounded-full text-[var(--loja-apagado)] outline-none transition-colors hover:bg-[var(--loja-sup-2)] hover:text-white"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
        </button>
      </div>

      {resultados && (
        <p className="shrink-0 px-12 pb-3 pt-5 text-[13px] text-[var(--loja-apagado)]">
          Resultados para "{busca}" ({resultados.length})
        </p>
      )}

      {/* ── Corpo: vitrine, categoria ou resultados ──────────────────────── */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
        {resultados ? (
          // A busca reaproveita a grade da categoria, sem paginação: os
          // resultados são curtos e cabem numa resposta só.
          <StoreCategoria
            paginas={[resultados]}
            carregando={false}
            temMais={false}
            naBiblioteca={bloqueado}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onPedirMais={() => {}}
            onAdicionar={acoes.adicionar}
          />
        ) : modo === "vitrine" ? (
          <StoreShowcase
            secoes={secoesVitrine}
            carregando={carregandoVitrine}
            destaques={destaques}
            heroiIdx={heroiIdx}
            onHeroiIdx={setHeroiIdx}
            fichaHeroi={fichaHeroi}
            trailerHeroi={trailerHeroi}
            ativo={ativo}
            ocupado={Boolean(acoes.busy)}
            naBiblioteca={bloqueado}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onBaixar={acoes.baixar}
            onAdicionar={acoes.adicionar}
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
            naBiblioteca={bloqueado}
            onFocar={setDestaque}
            onAbrir={setAberto}
            onPedirMais={pedirMais}
            onAdicionar={acoes.adicionar}
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

      {/* Escolha da biblioteca Steam — mesmo diálogo usado pela biblioteca do
          modo console, alimentado pelo hook */}
      {acoes.escolhendo && (
        <ConsoleDestinoDialog
          titulo={`Instalar ${acoes.escolhendo.jogo.title}`}
          subtitulo="Escolha a biblioteca Steam de destino."
          opcoes={acoes.escolhendo.libs.map((l) => ({
            caminho: l.steamDir,
            rotulo: "Biblioteca Steam",
            livre: l.free,
          }))}
          onEscolher={(steamDir) =>
            acoes.escolhendo && acoes.confirmarBaixar(acoes.escolhendo.jogo, acoes.escolhendo.info, steamDir)
          }
          onFechar={() => acoes.setEscolhendo(null)}
        />
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
