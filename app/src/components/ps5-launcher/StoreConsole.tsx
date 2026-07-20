"use client"

import { forwardRef, useEffect, useRef, useState } from "react"
import { corDominante } from "./corDominante"
import type { Game } from "./types"
import { StoreGrid, type JogoLinha } from "./StoreGrid"
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

// Uma régua só de categorias, e cada uma é UMA grade. O antigo modo
// "Destaques", que empilhava seis linhas dentro de si, não faz sentido no
// formato de ladrilhos — assim toda categoria se comporta igual.
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

type Ficha = NonNullable<Awaited<ReturnType<NonNullable<typeof window.launcherAPI>["storeDetails"]>>["jogo"]>

export const StoreConsole = forwardRef<HTMLDivElement, StoreConsoleProps>(function StoreConsole(
  { games, ativo, onAtalhos },
  ref,
) {
  const acoes = useStoreActions(games)
  const [lista, setLista] = useState<JogoLinha[]>([])
  const [carregando, setCarregando] = useState(true)
  const [categoria, setCategoria] = useState("alta")
  // O painel expandido vive no ladrilho em foco e lê estes três.
  const [destaque, setDestaque] = useState<JogoLinha | null>(null)
  const [ficha, setFicha] = useState<Ficha | null>(null)
  const [trailer, setTrailer] = useState<{ url: string; poster: string } | null>(null)
  const [cor, setCor] = useState("")
  const [aberto, setAberto] = useState<JogoLinha | null>(null)
  const [teclado, setTeclado] = useState(false)
  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<JogoLinha[] | null>(null)

  // Cada categoria é uma chamada só. A lista da busca, quando existe, tem
  // precedência sobre a categoria.
  useEffect(() => {
    const cat = CATEGORIAS.find((c) => c.id === categoria)
    if (!cat) return
    let cancelado = false
    setCarregando(true)
    setLista([])
    const api = window.launcherAPI
    const p =
      cat.fonte.tipo === "featured"
        ? api?.storeFeatured(cat.fonte.secao, 40)
        : cat.fonte.genero
          ? api?.storeGenre(cat.fonte.genero, 40)
          : api?.storeRecent(cat.fonte.lista)
    p?.then((r) => {
      if (cancelado) return
      setLista(r?.ok ? ((r.jogos || []) as JogoLinha[]) : [])
    }).finally(() => !cancelado && setCarregando(false))
    return () => {
      cancelado = true
    }
  }, [categoria])

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

  // X e Y agem sobre a capa em foco, sem abrir a página. Procuramos o jogo em
  // todas as listas carregadas porque o foco pode estar em qualquer linha.
  useEffect(() => {
    if (!onAtalhos) return
    const achar = (appid: string): JogoLinha | undefined =>
      [lista, resultados || []].flat().find((j) => j.appid === appid)
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
  }, [onAtalhos, lista, resultados, acoes])

  const cat = CATEGORIAS.find((c) => c.id === categoria)
  const semManifesto = destaque?.manifest === false
  const jaTem = bloqueado(destaque)

  const jogos = resultados ?? lista

  return (
    <div
      ref={(el) => {
        raiz.current = el
        if (typeof ref === "function") ref(el)
        else if (ref) (ref as React.MutableRefObject<HTMLDivElement | null>).current = el
      }}
      // Sem barra lateral e sem herói: a régua de categorias em cima e a grade
      // ocupando o resto. O jogo em foco não tem lugar fixo na tela — ele
      // expande onde está.
      className="loja flex h-full w-full flex-col overflow-hidden bg-[#08090b] text-white"
      style={cor ? ({ "--loja-cor": cor } as React.CSSProperties) : undefined}
    >
      {/* ── Régua de categorias ──────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-6 overflow-x-auto px-12 pb-4 pt-8 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <button
          onClick={() => setTeclado(true)}
          className="flex shrink-0 items-center gap-1.5 text-[14px] text-white/40 outline-none transition-colors hover:text-white focus:text-white"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          Buscar
        </button>

        <span className="h-4 w-px shrink-0 bg-white/10" />

        {CATEGORIAS.map((c) => (
          <button
            key={c.id}
            onClick={() => {
              setCategoria(c.id)
              setResultados(null) // sair da busca ao escolher uma categoria
            }}
            className={`relative shrink-0 pb-1 text-[14px] font-medium outline-none transition-colors focus:text-white ${
              !resultados && categoria === c.id ? "text-white" : "text-white/40 hover:text-white/70"
            }`}
          >
            {c.rotulo}
            {!resultados && categoria === c.id && (
              <span
                className="absolute inset-x-0 -bottom-0.5 h-[2px] rounded-full"
                style={{ background: "var(--loja-cor)", boxShadow: "0 0 10px var(--loja-cor)" }}
              />
            )}
          </button>
        ))}
      </div>

      {resultados && (
        <p className="shrink-0 px-12 pb-3 text-[13px] text-white/45">
          Resultados para "{busca}" ({resultados.length})
        </p>
      )}

      {/* ── Grade ────────────────────────────────────────────────────────── */}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pt-2">
        <StoreGrid
          jogos={jogos}
          carregando={carregando && !resultados}
          focado={destaque}
          ficha={ficha}
          trailer={trailer}
          ativo={ativo}
          onFocar={setDestaque}
          onAbrir={setAberto}
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

function BotaoPainel({
  rotulo,
  onClick,
  primario,
  perigo,
  desabilitado,
}: {
  rotulo: string
  onClick: () => void
  primario?: boolean
  perigo?: boolean
  desabilitado?: boolean
}) {
  return (
    <button
      onClick={onClick}
      disabled={desabilitado}
      className={`rounded-xl py-3 text-[13px] font-semibold outline-none transition-all disabled:opacity-40 focus:ring-2 focus:ring-white ${
        primario
          ? "text-black enabled:hover:scale-[1.02]"
          : perigo
            ? "border border-[#ff6b81]/40 text-[#ff6b81] enabled:hover:bg-[#ff6b81]/10"
            : "border border-white/15 text-white/75 enabled:hover:bg-white/[0.07]"
      }`}
      style={primario ? { background: "var(--loja-cor)" } : undefined}
    >
      {rotulo}
    </button>
  )
}
