"use client"

import { forwardRef, useEffect, useMemo, useState } from "react"
import type { Game } from "./types"
import type { NewsItem } from "../../global"
import { useI18n } from "../../i18n/I18nContext"

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

function tempoRelativo(iso: string, t: (k: string, v?: any) => string): string {
  if (!iso) return ""
  const diff = Date.now() - new Date(iso).getTime()
  if (isNaN(diff)) return ""
  const h = Math.floor(diff / 3600000)
  if (h < 1) return t("gameoverview.agora")
  if (h < 24) return t("gameoverview.horas_atras", { h: String(h) })
  const d = Math.floor(h / 24)
  return d === 1 ? t("gameoverview.um_dia_atras") : t("gameoverview.dias_atras", { d: String(d) })
}

// "20,3 h", "45 min", "1 h 20 min"
function tempoDeJogo(mins: number, t: (k: string, v?: any) => string): string {
  if (mins < 60) return t("gameoverview.tempo.minutos", { mins: String(mins) })
  const h = Math.floor(mins / 60)
  const m = mins % 60
  if (h < 10 && m > 0) return t("gameoverview.tempo.horas_minutos", { h: String(h), m: String(m) })
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
  const { t } = useI18n()
  const relacionadas = useMemo(() => noticiasRelacionadas(game, news), [game, news])
  const destaque = relacionadas[0] ?? null
  const [somTrailer, setSomTrailer] = useState(false)

  // Trailer local resolvido NA HORA (sem o delay de 1,5s da home e sem
  // baixar nada): undefined = carregando, null = não existe, string = path.
  const [trailer, setTrailer] = useState<string | null | undefined>(undefined)

  // Fallback remoto (Steam sysinfo) pros campos de detalhe vazios.
  const [meta, setMeta] = useState<{
    short_description?: string
    about?: string
    developers?: string[]
    publishers?: string[]
    release_date?: string
    movies?: Array<{ id?: string | number; name?: string; thumb?: string; mp4?: string; webm?: string; hls?: string }>
    header?: string
  } | null>(null)

  useEffect(() => {
    let vivo = true
    setMeta(null)
    const precisaEnriquecer = !game.description || !game.developer || !game.publisher || !game.genre
    if (!precisaEnriquecer) return
    const api = window.launcherAPI
    if (!api?.gameSysinfo) return
    api.gameSysinfo(game).then((r: any) => {
      if (vivo && r && typeof r === "object") setMeta(r?.info ?? null)
    }).catch(() => {})
    return () => { vivo = false }
  }, [game.id])
  // HowLongToBeat: tempos de jogo (horas). Falha silenciosa — sem linha na UI.
  const [hltb, setHltb] = useState<{ main: number; mainExtra: number; completionist: number } | null>(null)

  useEffect(() => {
    let vivo = true
    setHltb(null)
    const api = window.launcherAPI
    if (!api?.hltbGet || !game.title) return
    api.hltbGet(game.title).then((r) => {
      if (!vivo || !r) return
      setHltb({ main: r.main || 0, mainExtra: r.mainExtra || 0, completionist: r.completionist || 0 })
    }).catch(() => {})
    return () => { vivo = false }
  }, [game.id])
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

  // Fallback: sem trailer local, usa o primeiro vídeo remoto do sysinfo.
  useEffect(() => {
    if (trailer !== null) return
    const m = meta?.movies?.[0]
    if (!m) return
    const remoto = m.mp4 || m.webm || m.hls
    if (remoto) setTrailer(remoto)
  }, [meta, trailer])

  const detalhes: [string, string | number | undefined][] = [
    [t("gameoverview.detalhes.desenvolvedora"), game.developer || meta?.developers?.[0]],
    [t("gameoverview.detalhes.publicadora"), game.publisher || meta?.publishers?.[0]],
    [t("gameoverview.detalhes.genero"), game.genre],
    [t("gameoverview.detalhes.lancamento"), game.year || meta?.release_date],
    [t("gameoverview.detalhes.jogadores"), game.players],
    [t("gameoverview.detalhes.tempo_jogo"), game.playtime_minutes ? tempoDeJogo(game.playtime_minutes, t) : undefined],
    [t("gameoverview.detalhes.hltb_main"), hltb?.main ? tempoDeJogo(hltb.main, t) : undefined],
    [t("gameoverview.detalhes.hltb_main_extra"), hltb?.mainExtra ? tempoDeJogo(hltb.mainExtra, t) : undefined],
    [t("gameoverview.detalhes.hltb_100"), hltb?.completionist ? tempoDeJogo(hltb.completionist, t) : undefined],
    [t("gameoverview.detalhes.metacritic"), game.metacritic ? `${game.metacritic} / 100` : undefined],
    [t("gameoverview.detalhes.fonte"), game.launcher],
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
              <h1 className="game-name truncate text-4xl font-light tracking-wide">{game.title}</h1>
            )}
            <p className="mt-3 line-clamp-3 max-w-[560px] text-[15px] font-light leading-relaxed text-white/65">
              {game.description || meta?.short_description || t("gameoverview.sem_descricao")}
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
              {rodando ? t("gameoverview.parar_jogo") : abrindo ? t("common.abrindo") : t("gameoverview.jogar_agora")}
            </button>
          </div>
        </section>

        {/* Corpo: trailer + detalhes */}
        <section className={`mt-8 grid min-h-0 flex-1 gap-6 grid-cols-[1.6fr_1fr] ${closing ? "ov-out" : ""}`}>
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
                    onError={() => setTrailer(null)}
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
                  {t("gameoverview.trailer")}
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
                    {t("gameoverview.sem_noticias")}
                  </div>
                )}
                {destaque && (
                  <>
                    <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/20 to-transparent" />
                    <div className="absolute inset-x-0 bottom-0 p-6">
                      <h5 className="line-clamp-2 max-w-[90%] text-lg font-normal text-white/95">{destaque.title}</h5>
                      <p className="mt-1 text-xs tracking-wide text-white/50">{tempoRelativo(destaque.date, t)}</p>
                    </div>
                  </>
                )}
                <span className="absolute left-5 top-4 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.24em] text-white/70">
                  <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
                  {t("gameoverview.noticias")}
                </span>
              </div>
            </button>
          )}

          {/* Detalhes — fundo quase sólido para leitura perfeita sobre a arte */}
          <div className={`flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/10 bg-black/60 backdrop-blur-2xl ${closing ? "" : "ov-w2"}`}>
            <span className="flex items-center gap-2 px-6 pt-5 text-[11px] font-medium uppercase tracking-[0.24em] text-white/50">
              <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
              {t("gameoverview.detalhes")}
            </span>
            <div className="mt-4 flex-1 space-y-0 overflow-y-auto px-6 pb-4">
              {detalhes.filter(([, v]) => v).length <= 1 && game.launcher === "steam" && meta === null && (
                <div className="space-y-3 py-3">
                  {[0,1,2,3].map(i => (
                    <div key={i} className="flex items-center justify-between gap-4">
                      <div className="h-3 w-20 rounded bg-white/5 animate-pulse" />
                      <div className="h-3 w-32 rounded bg-white/5 animate-pulse" />
                    </div>
                  ))}
                </div>
              )}
              {detalhes.filter(([, v]) => v).map(([label, valor], i, arr) => (
                <div key={label} className={`flex items-baseline justify-between gap-4 py-3 text-sm ${i < arr.length - 1 ? "border-b border-white/[0.07]" : ""}`}>
                  <span className="shrink-0 text-white/45">{label}</span>
                  <span className="text-right font-light text-white/90">{valor}</span>
                </div>
              ))}
            </div>
          </div>
        </section>

        {/* Dica de controle */}
        <div className={`flex items-center justify-end gap-6 pt-5 text-xs text-white/60 ${closing ? "ov-out" : "ov-w4"}`}>
          <span className="flex items-center gap-2">
            <Glyph kind="cross" />
            <span>{t("gameoverview.controle.jogar")}</span>
          </span>
          <button onClick={onClose} className="flex items-center gap-2 outline-none transition-colors hover:text-white focus-visible:text-[color:var(--accent)]">
            <Glyph kind="circle" />
            <span>{t("gameoverview.controle.voltar")}</span>
          </button>
        </div>
      </div>
    </div>
  )
})

function Glyph({ kind }: { kind: "cross" | "circle" }) {
  const cfg = kind === "cross"
    ? { ch: "✕", bg: "rgba(0,114,206,0.22)", border: "rgba(0,114,206,0.55)", fg: "#7ec8ff" }
    : { ch: "○", bg: "rgba(240,53,59,0.22)", border: "rgba(240,53,59,0.55)", fg: "#ff8085" }
  return (
    <span
      aria-hidden
      className="inline-flex h-6 w-6 items-center justify-center rounded-full border text-[13px] font-semibold leading-none"
      style={{ background: cfg.bg, borderColor: cfg.border, color: cfg.fg }}
    >
      {cfg.ch}
    </span>
  )
}
