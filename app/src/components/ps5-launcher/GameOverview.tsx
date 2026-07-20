"use client"

import { forwardRef, useEffect, useMemo, useRef, useState } from "react"
import type { Game } from "./types"
import type { AchievementItem, NewsItem } from "../../global"

interface GameOverviewProps {
  game: Game
  news: NewsItem[]
  appFocused?: boolean // foco real da janela (gamescope)
  /** Este jogo é o que está rodando agora — o botão vira "Parar jogo". */
  rodando?: boolean
  /** Lançado, esperando o processo subir. */
  abrindo?: boolean
  closing?: boolean
  onClose: () => void
  onLaunch: (game: Game) => void
  onOpenNews: (url: string) => void
}

// Notícias relacionadas: casa palavras significativas do título do jogo
// com o título da notícia (ex.: "Silksong", "Diablo").
function noticiasRelacionadas(game: Game, news: NewsItem[]): NewsItem[] {
  const palavras = game.title
    .toLowerCase()
    .split(/[^a-z0-9à-ÿ]+/i)
    .filter((w) => w.length >= 4)
  if (!palavras.length) return []
  return news
    .filter((n) => {
      const t = n.title.toLowerCase()
      return palavras.some((w) => t.includes(w))
    })
    .slice(0, 3)
}

function tempoRelativo(iso: string): string {
  if (!iso) return ""
  const diff = Date.now() - new Date(iso).getTime()
  if (isNaN(diff)) return ""
  const h = Math.floor(diff / 3600000)
  if (h < 1) return "agora"
  if (h < 24) return `há ${h} h`
  const d = Math.floor(h / 24)
  return d === 1 ? "há 1 dia" : `há ${d} dias`
}

// "20,3 h", "45 min", "1 h 20 min"
function tempoDeJogo(mins: number): string {
  if (mins < 60) return `${mins} min`
  const h = Math.floor(mins / 60)
  const m = mins % 60
  if (h < 10 && m > 0) return `${h} h ${m} min`
  return `${String(h).replace(".", ",")} h`
}

function Tag({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex h-7 items-center justify-center rounded-full border border-white/15 bg-white/[0.06] px-3 text-xs font-medium tracking-wide text-white/80 backdrop-blur-sm">
      {children}
    </span>
  )
}

export const GameOverview = forwardRef<HTMLDivElement, GameOverviewProps>(function GameOverview(
  { game, news, appFocused = true, rodando, abrindo, closing, onClose, onLaunch, onOpenNews },
  ref,
) {
  const relacionadas = useMemo(() => noticiasRelacionadas(game, news), [game, news])
  const destaque = relacionadas[0] ?? null
  const [somTrailer, setSomTrailer] = useState(false)

  // Conquistas detalhadas (estilo SuccessStory): busca ao abrir o overview.
  const [achievements, setAchievements] = useState<AchievementItem[] | null>(null)
  const achievementsScrollRef = useRef<HTMLDivElement>(null)
  const appFocusedRef = useRef(appFocused)
  appFocusedRef.current = appFocused

  // Analógico DIREITO rola a coluna de conquistas (mesma física do
  // useGamepadNav: zona morta, resposta quadrática e inércia).
  useEffect(() => {
    let raf = 0
    let rest: number[] | null = null
    let vel = 0
    const loop = () => {
      // Janela sem foco: não rola (input iria para o jogo E para o overview).
      if (!appFocusedRef.current || !document.hasFocus()) {
        vel = 0
        raf = requestAnimationFrame(loop)
        return
      }
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      const el = achievementsScrollRef.current
      if (gp && el) {
        if (!rest) rest = Array.from(gp.axes)
        let ry = 0
        for (let ai = 2; ai < gp.axes.length; ai++) {
          const v = (gp.axes[ai] ?? 0) - (rest[ai] ?? 0)
          if (Math.abs(v) > Math.abs(ry)) ry = v
        }
        const target = Math.abs(ry) > 0.15 ? Math.sign(ry) * ry * ry * 46 : 0
        vel += (target - vel) * 0.25
        if (Math.abs(vel) > 0.05) el.scrollTop += vel
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [])
  useEffect(() => {
    let vivo = true
    const api = window.launcherAPI
    const appid = game.id.startsWith("steam:") ? game.id.split(":")[1] : ""
    if (!api || !appid) {
      setAchievements([])
      return
    }
    api.achievementsGet(appid).then((items) => {
      if (vivo) setAchievements(Array.isArray(items) ? items : [])
    }).catch(() => {
      if (vivo) setAchievements([])
    })
    return () => {
      vivo = false
    }
  }, [game.id])

  // Trailer local resolvido NA HORA (sem o delay de 1,5s da home e sem
  // baixar nada): undefined = carregando, null = não existe, string = path.
  const [trailer, setTrailer] = useState<string | null | undefined>(undefined)
  useEffect(() => {
    let vivo = true
    const api = window.launcherAPI
    if (!api) {
      setTrailer(null)
      return
    }
    api.trailerPath(game.id).then((r) => {
      if (vivo) setTrailer(r?.path || null)
    }).catch(() => {
      if (vivo) setTrailer(null)
    })
    return () => {
      vivo = false
    }
  }, [game.id])

  const detalhes: [string, string | number | undefined][] = [
    ["Desenvolvedora", game.developer],
    ["Publicadora", game.publisher],
    ["Gênero", game.genre],
    ["Lançamento", game.year],
    ["Jogadores", game.players],
    ["Tempo de jogo", game.playtime_minutes ? tempoDeJogo(game.playtime_minutes) : undefined],
    ["Conquistas", game.achievements_total ? (game.achievements_done != null ? `${game.achievements_done} / ${game.achievements_total}` : `${game.achievements_total}`) : undefined],
    ["Metacritic", game.metacritic ? `${game.metacritic} / 100` : undefined],
    ["Fonte", game.launcher],
  ]

  return (
    <div ref={ref} className="gp-scope fixed inset-0 z-40 overflow-hidden bg-black text-white antialiased">
      {/* Fundo: hero à DIREITA, afundando num gradiente OLED pesado — a arte
          fica como clima, nunca atrapalha a leitura */}
      <div className={closing ? "ov-out absolute inset-0" : "ov-bg-in absolute inset-0"}>
        {game.hero && (
          <img
            src={game.hero}
            alt=""
            className="absolute inset-y-0 right-0 h-full w-[70%] object-cover object-right"
            style={{ maskImage: "linear-gradient(to left, black 30%, transparent 95%)", WebkitMaskImage: "linear-gradient(to left, black 30%, transparent 95%)" }}
            draggable={false}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-r from-black via-black/85 to-black/40" />
        <div className="absolute inset-0 bg-gradient-to-t from-black via-transparent to-black/60" />
        {/* Fase 2: sombra sobe da borda inferior p/ legibilidade dos painéis */}
        {!closing && (
          <div className="ov-shade absolute inset-x-0 bottom-0 h-[55%] bg-gradient-to-t from-black via-black/70 to-transparent" />
        )}
      </div>

      <div className="relative z-10 mx-auto flex h-full max-w-[1900px] flex-col px-12 py-10">
        {/* Cabeçalho: capa + meta + ação + nota */}
        <section className={`flex items-start gap-8 ${closing ? "ov-out" : ""}`}>
          {game.cover && (
            <img
              src={game.cover}
              alt={game.title}
              className={`h-[190px] w-[142px] shrink-0 rounded-xl object-cover shadow-2xl shadow-black/80 ring-1 ring-white/15 ${closing ? "" : "ov-hero-card"}`}
              draggable={false}
            />
          )}

          <div className={`min-w-0 flex-1 pt-1 ${closing ? "" : "ov-hero-text"}`}>
            {game.logo ? (
              <img src={game.logo} alt={game.title} className="max-h-16 max-w-[380px] object-contain object-left" draggable={false} />
            ) : (
              <h1 className="truncate text-4xl font-light tracking-wide">{game.title}</h1>
            )}
            <p className="mt-3 line-clamp-3 max-w-[560px] text-[15px] font-light leading-relaxed text-white/65">
              {game.description || "Sem descrição disponível."}
            </p>
            <div className="mt-4 flex flex-wrap items-center gap-2">
              {game.year && <Tag>{game.year}</Tag>}
              {game.genre && <Tag>{game.genre}</Tag>}
              {game.players && <Tag>{game.players}</Tag>}
            </div>
            <button
              onClick={() => onLaunch(game)}
              className={`group mt-6 inline-flex items-center gap-3 rounded-full py-3 pl-5 pr-7 text-sm font-semibold outline-none transition-all hover:scale-[1.04] focus-visible:shadow-[0_0_0_2px_var(--accent),0_0_30px_var(--accent)] ${
                rodando ? "bg-[#e8703a] text-white" : "bg-white text-black"
              }`}
              style={{ boxShadow: "0 10px 40px -10px rgba(255,255,255,0.35)" }}
            >
              <svg viewBox="0 0 24 24" fill="currentColor" className="h-4 w-4" aria-hidden="true">
                {rodando ? <rect x="6" y="6" width="12" height="12" rx="1.5" /> : <path d="M8 5v14l11-7z" />}
              </svg>
              {rodando ? "Parar jogo" : abrindo ? "Abrindo…" : "Jogar agora"}
            </button>
          </div>
        </section>

        {/* Corpo: trailer + detalhes (+ coluna de conquistas quando houver) */}
        <section className={`mt-8 grid min-h-0 flex-1 gap-6 ${achievements?.length ? "grid-cols-[1.5fr_1fr_1fr]" : "grid-cols-[1.6fr_1fr]"} ${closing ? "ov-out" : ""}`}>
          {/* Trailer — clicar liga/desliga o som. Sem trailer local, mostra a
              notícia relacionada. */}
          {trailer !== null ? (
            <button
              onClick={() => setSomTrailer((v) => !v)}
              className={`group relative flex flex-col overflow-hidden rounded-2xl border border-white/10 bg-black text-left outline-none transition-colors hover:border-white/25 focus-visible:border-[color:var(--accent)] ${closing ? "" : "ov-w1"}`}
            >
              <div className="relative min-h-0 flex-1 overflow-hidden">
                {trailer && appFocused && (
                  <video
                    key={trailer}
                    src={trailer}
                    autoPlay
                    loop
                    muted={!somTrailer}
                    playsInline
                    className="h-full w-full object-cover"
                  />
                )}
                <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-black/20" />
                <span className="absolute right-4 top-4 flex h-9 w-9 items-center justify-center rounded-lg bg-black/50 ring-1 ring-white/15 backdrop-blur-md">
                  {somTrailer ? (
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="h-4 w-4"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.5 5.5a9 9 0 0 1 0 13" /></svg>
                  ) : (
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className="h-4 w-4"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><line x1="22" x2="16" y1="9" y2="15" /><line x1="16" x2="22" y1="9" y2="15" /></svg>
                  )}
                </span>
                <span className="absolute bottom-4 left-5 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/70">
                  <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
                  Trailer
                </span>
              </div>
            </button>
          ) : (
            <button
              onClick={() => destaque && onOpenNews(destaque.url)}
              className={`group relative flex flex-col overflow-hidden rounded-2xl border border-white/10 bg-white/[0.03] text-left outline-none backdrop-blur-xl transition-colors hover:border-white/25 focus-visible:border-[color:var(--accent)] ${closing ? "" : "ov-w1"}`}
            >
              <div className="relative min-h-0 flex-1 overflow-hidden">
                {destaque?.image || game.hero ? (
                  <img
                    src={destaque?.image || game.hero}
                    alt=""
                    className="h-full w-full object-cover transition-transform duration-700 group-hover:scale-[1.03]"
                    draggable={false}
                  />
                ) : (
                  <div className="flex h-full items-center justify-center text-sm font-light text-white/40">
                    Sem notícias relacionadas no momento.
                  </div>
                )}
                {destaque && (
                  <>
                    <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/20 to-transparent" />
                    <div className="absolute inset-x-0 bottom-0 p-6">
                      <h5 className="line-clamp-2 max-w-[90%] text-lg font-normal text-white/95">{destaque.title}</h5>
                      <p className="mt-1 text-xs tracking-wide text-white/50">{tempoRelativo(destaque.date)}</p>
                    </div>
                  </>
                )}
                <span className="absolute left-5 top-4 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/70">
                  <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
                  Notícias
                </span>
              </div>
            </button>
          )}

          {/* Detalhes — fundo quase sólido para leitura perfeita sobre a arte */}
          <div className={`flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/10 bg-black/60 backdrop-blur-2xl ${closing ? "" : "ov-w2"}`}>
            <span className="flex items-center gap-2 px-6 pt-5 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
              <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
              Detalhes
            </span>
            <div className="mt-4 flex-1 space-y-0 overflow-y-auto px-6 pb-4">
              {detalhes.filter(([, v]) => v).map(([label, valor], i, arr) => (
                <div key={label} className={`flex items-baseline justify-between gap-4 py-3 text-sm ${i < arr.length - 1 ? "border-b border-white/[0.07]" : ""}`}>
                  <span className="shrink-0 text-white/45">{label}</span>
                  <span className="text-right font-light text-white/90">{valor}</span>
                </div>
              ))}
              {/* Barra de progresso das conquistas do jogador */}
              {game.achievements_total != null && game.achievements_total > 0 && game.achievements_done != null && (
                <div className="pt-4">
                  <div className="h-1 w-full overflow-hidden rounded-full bg-white/10">
                    <div
                      className="h-full rounded-full"
                      style={{ width: `${Math.round((game.achievements_done / game.achievements_total) * 100)}%`, background: "var(--accent)" }}
                    />
                  </div>
                  <span className="mt-2 block text-right text-[11px] text-white/40">
                    {Math.round((game.achievements_done / game.achievements_total) * 100)}% das conquistas
                  </span>
                </div>
              )}
            </div>
          </div>

          {/* Conquistas — coluna própria com progresso e lista rolável */}
          {achievements != null && achievements.length > 0 && (
            <div className={`flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/10 bg-black/60 backdrop-blur-2xl ${closing ? "" : "ov-w3"}`}>
              <div className="flex items-center gap-4 px-6 pt-5">
                <span className="flex shrink-0 items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
                  <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
                  Conquistas
                </span>
                <span className="ml-auto shrink-0 text-xs font-light tabular-nums text-white/50">
                  {achievements.filter((a) => a.achieved).length} / {achievements.length}
                </span>
              </div>
              <div className="mx-6 mt-3 h-1 overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full"
                  style={{ width: `${Math.round((achievements.filter((a) => a.achieved).length / achievements.length) * 100)}%`, background: "var(--accent)" }}
                />
              </div>
              <div ref={achievementsScrollRef} className="mt-2 min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3 pb-4 pt-2">
                {[...achievements]
                  .sort((a, b) => {
                    if (a.achieved !== b.achieved) return a.achieved ? -1 : 1
                    if (a.achieved) return (b.unlock || 0) - (a.unlock || 0)
                    return (a.percent || 999) - (b.percent || 999)
                  })
                  .map((a) => (
                    <AchievementRow key={a.name} a={a} />
                  ))}
              </div>
            </div>
          )}
        </section>

        {/* Dica de controle */}
        <div className={`flex items-center justify-end gap-6 pt-5 text-xs text-white/40 ${closing ? "ov-out" : "ov-w4"}`}>
          <span>A — jogar / som do trailer</span>
          <button onClick={onClose} className="outline-none transition-colors hover:text-white/70 focus-visible:text-[color:var(--accent)]">B — voltar</button>
        </div>
      </div>
    </div>
  )
})

function AchievementRow({ a }: { a: AchievementItem }) {
  const pct = typeof a.percent === "number" ? a.percent : parseFloat(String(a.percent)) || 0
  const rara = pct > 0 && pct <= 10
  return (
    <div
      tabIndex={0}
      className={`flex items-start gap-4 rounded-xl px-3 py-3 outline-none transition-colors focus:bg-white/[0.06] ${a.achieved ? "" : "opacity-50"}`}
    >
      <img
        src={a.achieved ? a.icon : a.icongray || a.icon}
        alt=""
        className={`h-12 w-12 shrink-0 rounded-lg ring-1 ${a.achieved ? "ring-white/15" : "ring-white/10"}`}
        loading="lazy"
        draggable={false}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <h5 className={`truncate text-sm ${a.achieved ? "font-medium text-white/95" : "font-normal text-white/70"}`}>
            {a.title}
          </h5>
          <span className="shrink-0 text-[11px] tabular-nums text-white/40">
            {a.achieved && a.unlock
              ? new Date(a.unlock * 1000).toLocaleDateString("pt-BR")
              : "Bloqueada"}
          </span>
        </div>
        {a.desc && (
          <p className="mt-0.5 line-clamp-2 text-xs font-light leading-relaxed text-white/55">{a.desc}</p>
        )}
        <div className="mt-1.5 flex items-center gap-2">
          <div className="h-[3px] w-20 overflow-hidden rounded-full bg-white/10">
            <div
              className="h-full rounded-full"
              style={{ width: `${Math.min(100, pct)}%`, background: rara ? "#ffd23f" : "rgba(255,255,255,0.35)" }}
            />
          </div>
          <span className={`text-[10px] ${rara ? "text-[#ffd23f]/80" : "text-white/35"}`}>
            {pct.toFixed(1).replace(".", ",")}% dos jogadores{rara ? " — rara" : ""}
          </span>
        </div>
      </div>
    </div>
  )
}
