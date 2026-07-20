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

  const raiz = useRef<HTMLDivElement | null>(null)

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

  const cat = CATEGORIAS.find((c) => c.id === categoria)
  const semManifesto = destaque?.manifest === false
  const jaTem = bloqueado(destaque)

  return (
    <div
      ref={(el) => {
        raiz.current = el
        if (typeof ref === "function") ref(el)
        else if (ref) (ref as React.MutableRefObject<HTMLDivElement | null>).current = el
      }}
      // Cockpit: três colunas, e só a última rola. O herói horizontal saiu — o
      // jogo em foco mora num painel fixo, então a informação não some quando
      // se desce pelas linhas.
      className="loja grid h-full w-full overflow-hidden bg-[#08090b] text-white"
      style={{
        gridTemplateColumns: "168px clamp(300px, 22vw, 380px) 1fr",
        ...(cor ? ({ "--loja-cor": cor } as React.CSSProperties) : {}),
      }}
    >
      {/* ── Coluna 1: trilho de categorias ───────────────────────────────── */}
      <nav className="flex flex-col gap-1 border-r border-white/[0.06] py-9 pl-8 pr-3">
        <button
          onClick={() => setTeclado(true)}
          className="mb-5 flex items-center gap-2 text-left text-[13px] text-white/45 outline-none transition-colors hover:text-white focus:text-white"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          Buscar
        </button>

        {CATEGORIAS.map((c) => (
          <button
            key={c.id}
            onClick={() => {
              setCategoria(c.id)
              setResultados(null) // sair da busca ao escolher um filtro
            }}
            className={`relative py-2 pl-3 text-left text-[14px] font-medium outline-none transition-colors focus:text-white ${
              categoria === c.id ? "text-white" : "text-white/40 hover:text-white/70"
            }`}
          >
            {/* O indicador que antes deslizava na horizontal agora é uma barra
                vertical — mesmo mecanismo, girado com a coluna. */}
            {categoria === c.id && (
              <span
                className="absolute inset-y-1 left-0 w-[2px] rounded-full"
                style={{ background: "var(--loja-cor)", boxShadow: "0 0 10px var(--loja-cor)" }}
              />
            )}
            {c.rotulo}
          </button>
        ))}

        <div className="mt-auto text-[11px] leading-relaxed text-white/25">
          A abrir
          <br />
          X baixar
          <br />
          Y adicionar
          <br />
          B voltar
        </div>
      </nav>

      {/* ── Coluna 2: painel do jogo em foco ─────────────────────────────── */}
      <aside className="relative flex flex-col overflow-hidden border-r border-white/[0.06] px-6 py-9">
        <div className="loja-halo absolute inset-0" />

        <div className="relative">
          {/* O trailer ocupa o MESMO retângulo da capa. Cortar as laterais de um
              16:9 é proposital: trocar retrato por widescreen faria o painel
              inteiro saltar a cada jogo focado. */}
          <div
            className="relative w-full overflow-hidden rounded-xl bg-[#111114] ring-1 ring-white/10"
            style={{ aspectRatio: "2 / 3" }}
          >
            {destaque && (
              <img
                key={destaque.appid}
                src={`https://cdn.akamai.steamstatic.com/steam/apps/${destaque.appid}/library_600x900.jpg`}
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

          <h1 className="mt-5 text-2xl font-light leading-tight tracking-wide">
            {destaque?.title || (carregando ? "Carregando…" : "Loja")}
          </h1>

          <div className="mt-2 flex min-h-[20px] flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-white/45">
            {ficha?.generos?.length ? <span>{ficha.generos.slice(0, 3).join(" · ")}</span> : null}
            {ficha?.lancamento && <span className="text-white/30">{ficha.lancamento}</span>}
          </div>

          <div className="mt-3 flex min-h-[24px] items-center gap-3 text-[13px]">
            {ficha?.preco && <span className="text-white/85">{ficha.preco}</span>}
            {ficha?.metacritic ? (
              <span
                className="rounded px-1.5 py-0.5 text-[12px] font-semibold text-black"
                style={{ background: "var(--loja-cor)" }}
              >
                {ficha.metacritic}
              </span>
            ) : null}
          </div>

          <p className="mt-2 min-h-[16px] text-[12px] text-white/35">
            {destaque?.fontes?.length
              ? `Disponível em ${destaque.fontes.join(", ")}`
              : semManifesto
                ? "Sem manifesto"
                : ""}
          </p>
        </div>

        {/* Ações no rodapé do painel: ficam no mesmo lugar em todos os jogos,
            então a mão aprende o caminho. */}
        <div className="relative mt-auto flex flex-col gap-2 pt-6">
          {jaTem ? (
            <>
              <div
                className="rounded-xl border py-3 text-center text-[13px] font-semibold"
                style={{ borderColor: "color-mix(in oklab, var(--loja-cor) 45%, transparent)", color: "var(--loja-cor)" }}
              >
                Na biblioteca
              </div>
              <BotaoPainel
                rotulo="Remover"
                perigo
                desabilitado={Boolean(acoes.busy)}
                onClick={() => destaque && acoes.remover(destaque)}
              />
            </>
          ) : semManifesto ? (
            <div className="rounded-xl border border-white/10 py-3 text-center text-[13px] text-white/30">
              Sem manifesto
            </div>
          ) : (
            <>
              <BotaoPainel
                rotulo={acoes.busy ? "…" : "Baixar"}
                primario
                desabilitado={!destaque || Boolean(acoes.busy)}
                onClick={() => destaque && acoes.baixar(destaque)}
              />
              <BotaoPainel
                rotulo="Adicionar à Steam"
                desabilitado={!destaque || Boolean(acoes.busy)}
                onClick={() => destaque && acoes.adicionar(destaque)}
              />
            </>
          )}
          <BotaoPainel
            rotulo="Ver detalhes"
            desabilitado={!destaque}
            onClick={() => destaque && setAberto(destaque)}
          />
        </div>
      </aside>

      {/* ── Coluna 3: linhas ─────────────────────────────────────────────── */}
      <div className="overflow-y-auto overflow-x-hidden py-9">
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
            titulo={cat?.rotulo || ""}
            jogos={lista}
            carregando={carregandoLista}
            onAbrir={setAberto}
            onFocar={setDestaque}
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
