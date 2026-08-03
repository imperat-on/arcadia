"use client"

// Painéis reaproveitados pela página do jogo da LOJA (StoreGamePage) e da
// BIBLIOTECA (GamePage), no estilo Hydra: galeria (trailer+screenshots) e
// ProtonDB. Todos os dados vêm de IPC já existente (gameSysinfo, gameProtonDb).

import { useEffect, useMemo, useRef, useState } from "react"
import Hls from "hls.js"
import { useI18n } from "../../i18n/I18nContext"
import { fmtNotaSteam, fmtNum } from "../format"

type Movie = { id: number; name: string; thumb: string; mp4: string; webm: string; hls?: string }
type Shot = { thumb: string; full: string }

// Galeria: player de trailer + faixa de miniaturas (trailers e screenshots).
export function GameMediaGallery({ movies = [], screenshots = [] }: { movies?: Movie[]; screenshots?: Shot[] }) {
  // Item selecionado: { tipo: "movie"|"shot", idx }. Começa no 1º trailer, ou
  // no 1º screenshot se não houver trailer.
  const temMovie = movies.length > 0
  const [sel, setSel] = useState<{ tipo: "movie" | "shot"; idx: number }>(
    temMovie ? { tipo: "movie", idx: 0 } : { tipo: "shot", idx: 0 },
  )
  const vidRef = useRef<HTMLVideoElement>(null)

  useEffect(() => {
    setSel(temMovie ? { tipo: "movie", idx: 0 } : { tipo: "shot", idx: 0 })
  }, [temMovie, movies.length, screenshots.length])

  // Steam trocou trailers p/ HLS (.m3u8); mp4 só existe em jogos antigos.
  // Sem mp4 → anexa hls.js ao <video> (Chromium não toca HLS nativo).
  const mov = sel.tipo === "movie" ? movies[sel.idx] : undefined
  const direto = mov?.mp4 || mov?.webm || ""
  useEffect(() => {
    const v = vidRef.current
    if (!v || !mov || direto || !mov.hls) return
    if (!Hls.isSupported()) { v.src = mov.hls; return }
    const hls = new Hls()
    hls.loadSource(mov.hls)
    hls.attachMedia(v)
    return () => hls.destroy()
  }, [mov?.id, direto])

  if (!temMovie && screenshots.length === 0) return null

  const thumbs = [
    ...movies.map((m, i) => ({ tipo: "movie" as const, idx: i, src: m.thumb, play: true })),
    ...screenshots.map((s, i) => ({ tipo: "shot" as const, idx: i, src: s.thumb, play: false })),
  ]

  return (
    <div className="flex flex-col gap-2">
      <div className="relative aspect-video w-full overflow-hidden rounded-xl bg-black">
        {sel.tipo === "movie" && movies[sel.idx] ? (
          <video
            ref={vidRef}
            key={movies[sel.idx].id}
            src={direto || undefined}
            poster={movies[sel.idx].thumb}
            controls
            autoPlay
            muted
            className="h-full w-full object-contain"
          />
        ) : screenshots[sel.idx] ? (
          <img src={screenshots[sel.idx].full} alt="" className="h-full w-full object-contain" draggable={false} />
        ) : null}
      </div>
      <div className="flex gap-2 overflow-x-auto pb-1">
        {thumbs.map((th) => {
          const ativo = th.tipo === sel.tipo && th.idx === sel.idx
          return (
            <button
              key={`${th.tipo}-${th.idx}`}
              onClick={() => setSel({ tipo: th.tipo, idx: th.idx })}
              className={`relative aspect-video h-14 shrink-0 overflow-hidden rounded-md border transition-colors ${
                ativo ? "border-[color:var(--accent)]" : "border-white/10 hover:border-white/30"
              }`}
            >
              <img src={th.src} alt="" className="h-full w-full object-cover" draggable={false} />
              {th.play && (
                <span className="absolute inset-0 flex items-center justify-center bg-black/30">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="white"><path d="M8 5v14l11-7z" /></svg>
                </span>
              )}
            </button>
          )
        })}
      </div>
    </div>
  )
}

const PROTON_CORES: Record<string, string> = {
  platinum: "#b4c7dc", gold: "#cfb53b", silver: "#a7a7ad", bronze: "#cd7f32", borked: "#ff6b81",
}

// Painel ProtonDB: tier, Steam Deck e score. Só aparece se houver dados.
export function ProtonDBPanel({ appid }: { appid: string }) {
  const { t } = useI18n()
  const [data, setData] = useState<{ tier?: string; score?: number | null; deckCompatibility?: string; total?: number; url?: string } | null>(null)
  const [carregou, setCarregou] = useState(false)

  useEffect(() => {
    let vivo = true
    setCarregou(false)
    window.launcherAPI?.gameProtonDb(appid).then((r) => {
      if (vivo) { setData(r?.info || null); setCarregou(true) }
    })
    return () => { vivo = false }
  }, [appid])

  if (!carregou || !data) return null
  const tier = (data.tier || "").toLowerCase()
  const cor = PROTON_CORES[tier] || "#9aa0a6"
  const score = typeof data.score === "number" ? `${Math.round(data.score * 100)}%` : "—"
  const deck = data.deckCompatibility ? t(`protondb.deck.${data.deckCompatibility.toLowerCase()}`) : ""

  return (
    <Panel title="ProtonDB">
      <div className="flex flex-col gap-3 text-[13px]">
        <Linha label={t("protondb.nivel")}>
          <span className="rounded-md px-2.5 py-0.5 text-[12px] font-bold uppercase text-black" style={{ background: cor }}>
            {tier ? t(`protondb.tier.${tier}`) : "—"}
          </span>
        </Linha>
        {deck && <Linha label={t("protondb.steam_deck")}><span className="text-white/80">{deck}</span></Linha>}
        <Linha label={t("protondb.pontuacao")}><span className="text-white/80">{score}</span></Linha>
      </div>
      <button
        onClick={() => window.launcherAPI?.openExternal(data.url || "https://www.protondb.com")}
        className="mt-4 flex items-center gap-1.5 text-[12px] text-white/50 transition-colors hover:text-white/80"
      >
        {t("protondb.ver")}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><polyline points="15 3 21 3 21 9" /><line x1="10" y1="14" x2="21" y2="3" />
        </svg>
      </button>
    </Panel>
  )
}

// Card colapsável: clica no título oculta/mostra o corpo. Padrão: aberto.
// `right` = conteúdo extra à direita do título.
export function Panel({ title, right, children }: { title: string; right?: React.ReactNode; children: React.ReactNode }) {
  const [aberto, setAberto] = useState(true)
  return (
    <div
      className="overflow-hidden rounded-2xl transition-colors"
      style={{
        background: "rgba(255,255,255,0.025)",
        boxShadow: `inset 0 0 0 1px rgba(255,255,255,${aberto ? 0.07 : 0.05})`,
      }}
    >
      <button
        onClick={() => setAberto((v) => !v)}
        className="group flex w-full items-center justify-between gap-2 px-5 py-3.5 text-left transition-colors hover:bg-white/[0.03]"
      >
        <span className="flex items-center gap-2.5 text-[11px] font-bold uppercase tracking-[0.14em] text-white/70">
          <span
            className="h-3 w-[2px] rounded-full transition-colors"
            style={{ background: aberto ? "var(--accent)" : "rgba(255,255,255,0.2)" }}
          />
          {title}
          {right && <span className="ml-1 font-normal normal-case tracking-normal text-white/45">{right}</span>}
        </span>
        <svg
          width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4"
          strokeLinecap="round" strokeLinejoin="round"
          className={`shrink-0 text-white/40 transition-transform ${aberto ? "" : "-rotate-90"}`}
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {aberto && <div className="px-5 pb-5 pt-1">{children}</div>}
    </div>
  )
}

function Linha({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between">
      <span className="flex items-center gap-2 text-white/55">{label}</span>
      {children}
    </div>
  )
}

// HTML → texto puro. Compartilhado com StoreGamePage (requisitos, descrição).
export function stripHtml(s: string) {
  return String(s || "")
    .replace(/<br\s*\/?>/gi, "\n").replace(/<\/li>/gi, "\n").replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "").replace(/&amp;/g, "&").replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/\n{3,}/g, "\n\n").trim()
}

// Sanitiza o HTML da descrição da Steam mantendo o mix imagem+texto.
// Remove script/style/iframe e atributos on*/href javascript: (sem dep nova).
function sanitizeHtml(raw: string) {
  let s = String(raw || "")
  s = s.replace(/<(script|style|iframe|object|embed)[\s\S]*?<\/\1>/gi, "")
  s = s.replace(/<\/?(script|style|iframe|object|embed)[^>]*>/gi, "")
  s = s.replace(/\son\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "") // handlers
  s = s.replace(/(href|src)\s*=\s*("javascript:[^"]*"|'javascript:[^']*')/gi, "")
  return s
}

// Descrição rica (about_the_game): HTML da Steam com imagens e títulos,
// colapsada por padrão com "Mostrar mais" (igual Steam/Hydra).
export function GameDescription({ html, fallback }: { html?: string; fallback?: string }) {
  const { t } = useI18n()
  const [aberto, setAberto] = useState(false)
  const conteudo = (html || "").trim()
  // Hooks sempre antes de qualquer return condicional (regras do React).
  const limpo = useMemo(() => sanitizeHtml(conteudo), [conteudo])
  if (!conteudo) {
    if (!fallback) return null
    return (
      <div className="whitespace-pre-line rounded-2xl border border-white/[0.08] bg-white/[0.02] p-5 text-[13px] leading-relaxed text-white/70">
        {stripHtml(fallback)}
      </div>
    )
  }
  return (
    <div className="ui-card p-5">
      <div className="relative">
        <div
          className={`steam-desc text-[13px] leading-relaxed text-white/75 ${aberto ? "" : "max-h-[440px] overflow-hidden"}`}
          dangerouslySetInnerHTML={{ __html: limpo }}
        />
        {!aberto && <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-[#121216] to-transparent" />}
      </div>
      <button
        onClick={() => setAberto((v) => !v)}
        className="mt-3 w-full rounded-lg border border-white/10 py-2 text-[12px] font-medium text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
      >
        {aberto ? t("gamepage.mostrar_menos") : t("gamepage.mostrar_mais")}
      </button>
    </div>
  )
}

// Painel de suporte a controle. support = "full" | "partial" | "".
export function ControllerPanel({ support }: { support?: string }) {
  const { t } = useI18n()
  const s = (support || "").toLowerCase()
  if (s !== "full" && s !== "partial") return null
  return (
    <Panel title={t("controller.titulo")}>
      <div className="flex items-center gap-3">
        <div className="flex gap-2 text-white/70">
          {/* Xbox */}
          <svg width="26" height="26" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 0 0-6.9 2.76c1.8-.8 4.6.3 6.9 2.3 2.3-2 5.1-3.1 6.9-2.3A10 10 0 0 0 12 2zM4.2 5.6A10 10 0 0 0 2.4 15c.4-2.5 2.6-6 4.9-8.3-1-.6-2.1-1-3.1-1.1zm15.6 0c-1 .1-2.1.5-3.1 1.1 2.3 2.3 4.5 5.8 4.9 8.3a10 10 0 0 0-1.8-9.4zM12 8.2c-1.7 1.4-5.4 6.2-5.9 9.6A10 10 0 0 0 12 22a10 10 0 0 0 5.9-4.2c-.5-3.4-4.2-8.2-5.9-9.6z" /></svg>
          {/* PlayStation */}
          <svg width="26" height="26" viewBox="0 0 24 24" fill="currentColor"><path d="M9.5 3.2v16.3l3.4 1.1V7.6c0-.7.3-1.1.8-1 .6.2.7.8.7 1.5v4.6c2.1 1 3.8-.1 3.8-2.8 0-2.8-1-4-3.9-5C13.2 4.5 11.1 3.9 9.5 3.2zM14.6 17.9l5.5-2c.6-.2.7-.5.2-.7-.5-.2-1.5-.3-2.1-.1l-3.6 1.3v-2l.2-.1s1-.4 2.5-.5c1.5-.2 3.3 0 4.7.5 1.6.6 1.8 1.5.4 2l-7.8 2.8v-1.7zM3.4 17.7c-1.6-.5-1.9-1.4-1.2-1.9.7-.5 1.8-.9 1.8-.9l4.7-1.7v1.9l-3.4 1.2c-.6.2-.7.5-.2.7.5.2 1.5.3 2.1.1l1.5-.5v1.7c-.1 0-.2 0-.3.1-1.6.3-3.3.2-4.9-.3z" /></svg>
        </div>
        <span className="text-[13px] text-white/80">{t(s === "full" ? "controller.completo" : "controller.parcial")}</span>
      </div>
    </Panel>
  )
}

function requisitosCurtos(html: string) {
  const texto = stripHtml(html)
    .replace(/^(Minimum|Recommended|Mínimos|Recomendados):?\s*/gim, "")
    .replace(/\b(Requer um sistema operacional e processador de 64 bits|Requires a 64-bit processor and operating system)\b/gi, "")
  const linhas = texto
    .split(/\n+/)
    .map((l) => l.replace(/^[-•]\s*/, "").trim())
    .filter(Boolean)
    .filter((l) => !/^notas adicionais:?$/i.test(l))
  return linhas.slice(0, 8)
}

// Requisitos de sistema com abas Mínimos/Recomendados, limpos e compactos.
export function RequirementsPanel({ min, rec }: { min?: string; rec?: string }) {
  const { t } = useI18n()
  const [aba, setAba] = useState<"min" | "rec">(min ? "min" : "rec")
  if (!min && !rec) return null
  const linhas = requisitosCurtos((aba === "min" ? min : rec) || "")
  const Tab = ({ id, label, has }: { id: "min" | "rec"; label: string; has: boolean }) =>
    has ? (
      <button
        onClick={() => setAba(id)}
        className={`flex-1 border-b px-3 py-2 text-[12px] font-medium transition-colors ${
          aba === id ? "border-[color:var(--accent)] bg-white text-black" : "border-white/10 text-white/70 hover:bg-white/[0.05] hover:text-white"
        }`}
      >
        {label}
      </button>
    ) : null
  return (
    <Panel title={t("requisitos.titulo")}>
      <div className="-mx-5 -mt-1 mb-4 grid grid-cols-2 overflow-hidden border-y border-white/10">
        <Tab id="min" label={t("requisitos.minimos")} has={Boolean(min)} />
        <Tab id="rec" label={t("requisitos.recomendados")} has={Boolean(rec)} />
      </div>
      <ul className="space-y-1.5 text-[12px] leading-relaxed text-white/70">
        {linhas.map((l, i) => (
          <li key={i} className="flex gap-2">
            <span className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-white/45" />
            <span>{l}</span>
          </li>
        ))}
      </ul>
    </Panel>
  )
}

// Tabela de idiomas. supported_languages é HTML: "English<strong>*</strong>, ...".
// "*" (dentro de <strong>) marca áudio completo; senão só interface/legenda.
export function LanguagesPanel({ languages }: { languages?: string }) {
  const { t } = useI18n()
  const linhas = useMemo(() => {
    const raw = String(languages || "")
    if (!raw) return []
    // Corta em ", " no nível do texto, preservando as tags <strong>.
    const semNota = raw.replace(/<br\s*\/?>.*/is, "") // remove nota de rodapé
    return semNota.split(/,\s*/).map((frag) => {
      const audio = /\*/.test(frag)
      const nome = stripHtml(frag).replace(/\*/g, "").trim()
      return { nome, audio }
    }).filter((l) => l.nome)
  }, [languages])
  if (linhas.length === 0) return null
  const Check = ({ on }: { on: boolean }) =>
    on ? (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12" /></svg>
    ) : (
      <span className="text-white/20">—</span>
    )
  return (
    <Panel title={t("idioma.titulo")}>
      <table className="w-full text-[12px]">
        <thead>
          <tr className="text-left text-white/45">
            <th className="pb-2 font-medium">{t("idioma.idioma")}</th>
            <th className="pb-2 text-center font-medium">{t("idioma.interface")}</th>
            <th className="pb-2 text-center font-medium">{t("idioma.audio")}</th>
            <th className="pb-2 text-center font-medium">{t("idioma.legendas")}</th>
          </tr>
        </thead>
        <tbody>
          {linhas.map((l) => (
            <tr key={l.nome} className="border-t border-white/[0.05]">
              <td className="py-1.5 text-white/80">{l.nome}</td>
              <td className="py-1.5"><div className="flex justify-center"><Check on /></div></td>
              <td className="py-1.5"><div className="flex justify-center"><Check on={l.audio} /></div></td>
              <td className="py-1.5"><div className="flex justify-center"><Check on /></div></td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  )
}

type Comment = { steamid?: string; author: string; avatar?: string; text: string; positive: boolean; hours: number; hoursAtReview?: number; helpful: number; timestamp?: number }
type Stats = { owners?: string; ccu?: number; reviewDesc?: string; reviewPositivePct?: number | null; totalReviews?: number; comments?: Comment[] }

// Hook compartilhado por Stats, Reviews e Comentários (um só fetch por appid).
function useGameStats(appid: string) {
  const [data, setData] = useState<Stats | null | undefined>(undefined)
  useEffect(() => {
    let vivo = true
    setData(undefined)
    window.launcherAPI?.gameStats(appid).then((r) => {
      if (vivo) setData(r?.info || null)
    })
    return () => { vivo = false }
  }, [appid])
  return data
}

function StatLine({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 text-[13px]">
      <span className="flex items-center gap-2.5 font-semibold text-white/80">{icon}{label}</span>
      <span className="text-white/90">{value}</span>
    </div>
  )
}

// Estatísticas: jogadores ativos e avaliação.
export function StatsPanel({ appid }: { appid: string }) {
  const { t } = useI18n()
  const d = useGameStats(appid)
  if (!d || (!d.ccu && d.reviewPositivePct == null)) return null
  return (
    <Panel title={t("stats.titulo")}>
      <div className="flex flex-col gap-3 text-[13px]">
        {typeof d.ccu === "number" && d.ccu > 0 && (
          <StatLine
            icon={<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" /><circle cx="9" cy="7" r="4" /><path d="M22 21v-2a4 4 0 0 0-3-3.87" /><path d="M16 3.13a4 4 0 0 1 0 7.75" /></svg>}
            label={t("stats.jogadores_ativos")}
            value={fmtNum(d.ccu)}
          />
        )}
        {typeof d.reviewPositivePct === "number" && (
          <StatLine
            icon={<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="m12 17.27 6.18 3.73-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z" /></svg>}
            label={t("stats.avaliacao")}
            value={fmtNotaSteam(d.reviewPositivePct)}
          />
        )}
      </div>
    </Panel>
  )
}

// Avaliações estilo Hydra: cabeçalho com resumo (descrição + %/total), aba de
// ordenação (Recentes · Melhor pontuados · Mais votados) e lista de reviews
// com avatar identicon, tempo relativo e "Mostrar mais" incremental.
type OrdemReview = "recentes" | "melhores" | "votadas"

export function ReviewsPanel({ appid }: { appid: string }) {
  const { t } = useI18n()
  const d = useGameStats(appid)
  const [ordem, setOrdem] = useState<OrdemReview>("recentes")
  const [visiveis, setVisiveis] = useState(4)

  useEffect(() => { setVisiveis(4) }, [appid, ordem])

  const comments = d?.comments || []
  if (!d || (comments.length === 0 && !d.totalReviews)) return null

  const ordenadas = [...comments].sort((a, b) => {
    if (ordem === "recentes") return (b.timestamp || 0) - (a.timestamp || 0)
    if (ordem === "votadas") return (b.helpful || 0) - (a.helpful || 0)
    // "melhores": recomendadas primeiro, dentro delas mais votadas primeiro
    if (a.positive !== b.positive) return a.positive ? -1 : 1
    return (b.helpful || 0) - (a.helpful || 0)
  })
  const mostrando = ordenadas.slice(0, visiveis)
  const restam = ordenadas.length - visiveis

  return (
    <Panel title={t("reviews.titulo")} right={<span className="text-white/35">({d.totalReviews ? d.totalReviews.toLocaleString() : comments.length})</span>}>
      {comments.length > 0 && (
        <>
          {/* Ordenação */}
          <div className="mb-4 flex items-center gap-1 border-b border-white/[0.06] pb-2 text-[11.5px] font-medium">
            <SortBtn ativo={ordem === "recentes"} onClick={() => setOrdem("recentes")}>{t("reviews.recentes")}</SortBtn>
            <SortSep />
            <SortBtn ativo={ordem === "melhores"} onClick={() => setOrdem("melhores")}>{t("reviews.melhor_pontuadas")}</SortBtn>
            <SortSep />
            <SortBtn ativo={ordem === "votadas"} onClick={() => setOrdem("votadas")}>{t("reviews.mais_votadas")}</SortBtn>
          </div>

          <div className="flex flex-col">
            {mostrando.map((c, i) => (
              <ReviewCard key={`${c.steamid || i}-${c.timestamp || i}`} c={c} last={i === mostrando.length - 1} />
            ))}
          </div>

          {restam > 0 && (
            <button
              onClick={() => setVisiveis((v) => v + 6)}
              className="mt-4 w-full rounded-full border border-white/10 py-2.5 text-[12.5px] font-semibold text-white/70 transition-colors hover:border-white/20 hover:bg-white/[0.04] hover:text-white"
            >
              {t("reviews.mostrar_mais")} ({restam})
            </button>
          )}
        </>
      )}
    </Panel>
  )
}

function SortBtn({ ativo, onClick, children }: { ativo: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-2.5 py-1 transition-colors ${ativo ? "text-white" : "text-white/45 hover:text-white/75"}`}
    >
      {ativo ? "▾ " : ""}{children}
    </button>
  )
}
function SortSep() {
  return <span className="text-white/15">|</span>
}

// Cartão individual de review — avatar identicon + nome + tempo relativo,
// estrelinha derivada da recomendação, texto e rodapé com votos úteis.
function ReviewCard({ c, last }: { c: Comment; last: boolean }) {
  const { t } = useI18n()
  const cor = identiconColor(c.steamid || c.author)
  const inicial = (c.author?.replace(/^Steam\s*/, "") || "?")[0]?.toUpperCase() || "?"
  const quando = tempoRelativo(c.timestamp || 0, t)
  return (
    <div className={`py-4 ${last ? "" : "border-b border-white/[0.05]"}`}>
      <div className="mb-2 flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          {c.avatar ? (
            <img
              src={c.avatar}
              alt=""
              draggable={false}
              className="h-9 w-9 shrink-0 rounded-full object-cover"
              style={{ boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.08)" }}
              onError={(e) => {
                const el = e.currentTarget as HTMLImageElement
                el.replaceWith(Object.assign(document.createElement("span"), {
                  className: "flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-[13px] font-bold text-black/80",
                  style: `background:${cor}`,
                  textContent: inicial,
                }))
              }}
            />
          ) : (
            <span
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-[13px] font-bold text-black/80"
              style={{ background: cor }}
            >
              {inicial}
            </span>
          )}
          <div className="min-w-0">
            <div className="truncate text-[13px] font-semibold text-white/90">{c.author || "Steam"}</div>
            <div className="flex items-center gap-2 text-[11px] text-white/40">
              <span className={c.positive ? "text-[#4adf9a]" : "text-[#ff6b81]"}>
                {c.positive ? "★" : "☆"} {t(c.positive ? "comentarios.recomenda" : "comentarios.nao_recomenda")}
              </span>
              {(c.hoursAtReview || c.hours) > 0 && (
                <span>· {(c.hoursAtReview || c.hours)}h {t("comentarios.jogadas")}</span>
              )}
            </div>
          </div>
        </div>
        {quando && <span className="shrink-0 text-[11px] text-white/40">{quando}</span>}
      </div>
      <p className="whitespace-pre-line pl-12 text-[13px] leading-relaxed text-white/75">{c.text}</p>
      {c.helpful > 0 && (
        <p className="mt-2 pl-12 text-[11px] text-white/35">
          👍 {c.helpful} {t("comentarios.util")}
        </p>
      )}
    </div>
  )
}

// Identicon determinístico: hash simples → tom pastel HSL. Sem chamada de rede.
function identiconColor(seed: string) {
  let h = 0
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0
  return `hsl(${h % 360} 65% 62%)`
}

// "há 5 horas" / "há 3 dias" / "há 2 meses". Sem dep (Intl.RelativeTimeFormat).
function tempoRelativo(ts: number, t: (k: string) => string): string {
  if (!ts) return ""
  const diff = (Date.now() / 1000) - ts
  if (diff < 60) return t("tempo.agora")
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" })
  const [val, unit] = escalaTempo(diff)
  return rtf.format(-val, unit)
}
function escalaTempo(seg: number): [number, Intl.RelativeTimeFormatUnit] {
  if (seg < 3600) return [Math.round(seg / 60), "minute"]
  if (seg < 86400) return [Math.round(seg / 3600), "hour"]
  if (seg < 2592000) return [Math.round(seg / 86400), "day"]
  if (seg < 31536000) return [Math.round(seg / 2592000), "month"]
  return [Math.round(seg / 31536000), "year"]
}

// Mantido como stub p/ compatibilidade — CommentsPanel foi fundido em Reviews.
export function CommentsPanel(_: { appid: string }) { return null }

