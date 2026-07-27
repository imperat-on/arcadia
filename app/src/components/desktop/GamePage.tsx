"use client"

import { useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { fmtBytes, fmtMiB } from "../tamanho"
import { userLocale } from "../../i18n/locale"
import { useI18n } from "../../i18n/I18nContext"
import {
  AchievementsPanel, GameMediaGallery, GameDescription, ProtonDBPanel,
  ControllerPanel, LanguagesPanel, StatsPanel, ReviewsPanel, CommentsPanel,
  stripHtml,
} from "./GameDetailPanels"
import { FixesPanel } from "./FixesPanel"

interface Sysinfo {
  download_size?: number
  disk_size?: number
  version?: string
  req_min?: string
  req_rec?: string
  screenshots?: { thumb: string; full: string }[]
  movies?: { id: number; name: string; thumb: string; mp4: string; webm: string }[]
  release_date?: string
  publishers?: string[]
  developers?: string[]
  header?: string
  background?: string
  about?: string
  short_description?: string
  controller_support?: string
  languages?: string
}

// Página do jogo (estilo Heroic): abre ao clicar no card. Hero + metadados à
// esquerda, dados de instalação/requisitos à direita.
export function GamePage({
  game: g,
  onClose,
  onJogar,
  onInstalar,
  onImportar,
  onConfig,
  embedded,
}: {
  game: Game
  onClose: () => void
  onJogar: () => void
  onInstalar: () => void
  onImportar: () => void
  onConfig: () => void
  embedded?: boolean
}) {
  const { t } = useI18n()
  const instalado = g.installed !== false
  const epic = g.launcher === "epic"
  const steamAppid = g.launcher === "steam" ? String(g.id).replace(/^steam:/, "") : ""
  const [aba, setAba] = useState<"dados" | "requisitos">("dados")
  const [sys, setSys] = useState<Sysinfo | null>(null)
  const [sysBusy, setSysBusy] = useState(true)
  const [installPath, setInstallPath] = useState("")

  // Dados reais: tamanhos (legendary, Epic) e requisitos (Steam appdetails).
  useEffect(() => {
    setSysBusy(true)
    window.launcherAPI?.gameSysinfo(g).then((r) => {
      setSys(r?.info || {})
      setSysBusy(false)
    })
  }, [g.id])

  // Path de instalação (pra FixesPanel saber onde extrair).
  useEffect(() => {
    if (!instalado) { setInstallPath(""); return }
    window.launcherAPI?.storeInstallDir(g).then((r) => setInstallPath(r?.path || ""))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [g.id, instalado])

  const gibBytes = fmtBytes
  const gib = fmtMiB
  const tamDownload = sys?.download_size ? gibBytes(sys.download_size) : gib(g.size)
  const tamInstalado = sys?.disk_size ? gibBytes(sys.disk_size) : instalado ? gib(g.size) : "—"
  const ultimaVez = g.last_played
    ? new Date(g.last_played).toLocaleDateString(userLocale(), { day: "2-digit", month: "2-digit", year: "numeric" })
    : t("common.nunca")

  return (
    <div className={`${embedded ? "relative h-full" : "fixed inset-0 z-[55]"} flex flex-col bg-black`} style={{ animation: "gp-in 0.18s ease-out" }}>
      {/* Fundo: hero desfocado */}
      {g.hero || g.cover ? (
        <img src={g.hero || g.cover} alt="" className="pointer-events-none absolute inset-0 h-full w-full object-cover opacity-25 blur-md" draggable={false} />
      ) : null}
      <div className="pointer-events-none absolute inset-0 bg-black/60" />

      {/* Botão voltar */}
      <button
        onClick={onClose}
        className="absolute left-4 top-4 z-10 flex h-10 w-10 items-center justify-center rounded-full bg-white/[0.08] text-white/80 backdrop-blur-sm transition-colors hover:bg-white/[0.16] hover:text-white"
        title={t("common.voltar")}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="15 18 9 12 15 6" />
        </svg>
      </button>

      <div className="relative z-[1] mx-auto grid h-full w-full max-w-[1400px] flex-1 grid-cols-2 gap-5 overflow-hidden p-5 pt-16">
        {/* Coluna esquerda: arte + info */}
        <div className="flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-[#0d0d10]/90">
          <div className="relative h-[42%] shrink-0 overflow-hidden bg-black">
            {g.hero || g.cover ? (
              <img src={g.hero || g.cover} alt="" className="h-full w-full object-cover" draggable={false} />
            ) : null}
            <div className="absolute inset-0 bg-gradient-to-b from-black/20 via-transparent to-[#0d0d10]" />
            {g.logo ? (
              <img src={g.logo} alt="" className="absolute left-5 top-5 max-h-[64px] max-w-[55%] object-contain object-left drop-shadow-[0_2px_6px_rgba(0,0,0,0.9)]" draggable={false} />
            ) : null}
          </div>

          <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
            <h1 className="game-name truncate text-2xl font-light text-white">{g.title}</h1>
            {(g.developer || g.publisher) && (
              <p className="mt-0.5 text-[13px] italic text-white/50">{g.developer || g.publisher}</p>
            )}
            <p className="mt-3 min-h-0 flex-1 overflow-y-auto text-[13px] leading-relaxed text-white/65">
              {g.description || t("gamepage.sem_descricao")}
            </p>

            <div className="mt-4 shrink-0">
              <p className="flex items-center gap-2 text-[12px] text-white/45">
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <circle cx="12" cy="12" r="9" /><polyline points="12 7 12 12 15 14" />
                </svg>
                {t("gamepage.ultimo_jogo")} <span className="text-white/70">{ultimaVez}</span>
              </p>
              {!instalado && <p className="mt-1.5 text-[13px] italic" style={{ color: "var(--accent)" }}>{t("gamepage.nao_instalado")}</p>}

              <div className="mt-3 flex gap-2.5">
                {instalado ? (
                  <>
                    <button
                      onClick={onJogar}
                      className="flex items-center gap-2 rounded-lg px-6 py-2.5 text-[13px] font-bold tracking-wide text-black transition-transform hover:scale-[1.03]"
                      style={{ background: "var(--accent)" }}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
                      {t("gamepage.jogar")}
                    </button>
                    <button
                      onClick={onConfig}
                      className="flex items-center gap-2 rounded-lg border border-white/20 px-6 py-2.5 text-[13px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.14.32.22.66.22 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
                      {t("gamepage.gerenciar")}
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      onClick={onInstalar}
                      className="flex items-center gap-2 rounded-lg px-6 py-2.5 text-[13px] font-bold tracking-wide text-black transition-transform hover:scale-[1.03]"
                      style={{ background: "var(--accent)" }}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
                      </svg>
                      {t("gamepage.instalar")}
                    </button>
                    {epic && (
                      <button
                        onClick={onImportar}
                        className="rounded-lg border border-white/20 px-6 py-2.5 text-[13px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
                      >
                        {t("gamepage.importar_jogo")}
                      </button>
                    )}
                    <button
                      onClick={onConfig}
                      className="flex items-center gap-2 rounded-lg border border-white/20 px-6 py-2.5 text-[13px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.14.32.22.66.22 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
                      {t("gamepage.gerenciar")}
                    </button>
                  </>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Coluna direita: dados + painéis estilo Hydra */}
        <div className="flex min-h-0 flex-col overflow-y-auto rounded-2xl border border-white/[0.08] bg-[#0d0d10]/90 p-5">
          <div className="mx-auto mb-5 flex gap-1 rounded-full border border-white/10 bg-white/[0.03] p-1">
            {([
              ["dados", t("gamepage.dados_instalacao")],
              ["requisitos", t("gamepage.requisitos")],
            ] as const).map(([id, label]) => (
              <button
                key={id}
                onClick={() => setAba(id)}
                className={`rounded-full px-4 py-1.5 text-[11px] font-semibold tracking-wider transition-colors ${
                  aba === id ? "bg-white/[0.1] text-white" : "text-white/45 hover:text-white/75"
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          {aba === "dados" ? (
            <div className="flex flex-col gap-4">
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="flex items-center gap-2.5 text-[13px] font-medium text-white/85">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
                  </svg>
                  {t("gamepage.tamanho_download")}
                </span>
                <span className="text-[13px] text-white/70">{sysBusy && !sys ? t("gamepage.carregando") : tamDownload}</span>
              </div>
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="flex items-center gap-2.5 text-[13px] font-medium text-white/85">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <rect x="2" y="3" width="20" height="7" rx="2" /><rect x="2" y="14" width="20" height="7" rx="2" />
                  </svg>
                  {t("gamepage.tamanho_instalado")}
                </span>
                <span className="text-[13px] text-white/70">{sysBusy && !sys ? t("gamepage.carregando") : tamInstalado}</span>
              </div>
              {sys?.version && (
                <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                  <span className="text-[13px] font-medium text-white/85">{t("gamepage.versao")}</span>
                  <span className="text-[13px] text-white/70">{sys.version}</span>
                </div>
              )}
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="text-[13px] font-medium text-white/85">{t("gamepage.loja")}</span>
                <span className="text-[13px] capitalize text-white/70">{g.launcher}</span>
              </div>
            </div>
          ) : (
            <div className="min-h-0 flex-1 overflow-y-auto text-[13px]">
              {sysBusy && !sys ? (
                <p className="text-white/45">{t("gamepage.buscando_requisitos")}</p>
              ) : sys?.req_min || sys?.req_rec ? (
                <div className="flex flex-col gap-4">
                  {sys.req_min ? (
                    <div>
                      <h4 className="mb-1.5 text-[12px] font-semibold uppercase tracking-wider" style={{ color: "var(--accent)" }}>{t("gamepage.minimos")}</h4>
                      <p className="whitespace-pre-line leading-relaxed text-white/70">{stripHtml(sys.req_min)}</p>
                    </div>
                  ) : null}
                  {sys.req_rec ? (
                    <div>
                      <h4 className="mb-1.5 text-[12px] font-semibold uppercase tracking-wider" style={{ color: "var(--accent)" }}>{t("gamepage.recomendados")}</h4>
                      <p className="whitespace-pre-line leading-relaxed text-white/70">{stripHtml(sys.req_rec)}</p>
                    </div>
                  ) : null}
                  {g.launcher !== "steam" && (
                    <p className="text-[11px] text-white/30">{t("gamepage.fonte_steam")}</p>
                  )}
                </div>
              ) : (
                <div className="flex flex-col gap-3">
                  {([
                    [t("gamepage.genero"), g.genre],
                    [t("gamepage.ano"), g.year],
                    [t("gameoverview.detalhes.jogadores"), g.players],
                    [t("gameoverview.detalhes.metacritic"), g.metacritic != null ? `${g.metacritic}/100` : undefined],
                  ] as const).map(([k, v]) => (
                    <div key={String(k)} className="flex items-center justify-between border-b border-white/[0.06] pb-2.5">
                      <span className="font-medium text-white/85">{k}:</span>
                      <span className="text-white/70">{v != null && v !== "" ? String(v) : t("common.fallback")}</span>
                    </div>
                  ))}
                  <p className="mt-2 text-[12px] text-white/35">{t("gamepage.requisitos_indisponiveis")}</p>
                </div>
              )}
            </div>
          )}

          {/* Galeria + painéis (só jogos Steam com appid) */}
          {steamAppid && (
            <div className="mt-5 flex flex-col gap-5">
              {(sys?.movies?.length || sys?.screenshots?.length) ? (
                <GameMediaGallery movies={sys?.movies} screenshots={sys?.screenshots} />
              ) : null}
              <GameDescription html={sys?.about} fallback={sys?.short_description} />
              <ProtonDBPanel appid={steamAppid} />
              <StatsPanel appid={steamAppid} />
              <ControllerPanel support={sys?.controller_support} />
              <LanguagesPanel languages={sys?.languages} />
              <AchievementsPanel appid={steamAppid} />
              {instalado && <FixesPanel appid={steamAppid} installPath={installPath} />}
              {/* Avaliações + comentários abaixo da descrição. */}
              <ReviewsPanel appid={steamAppid} />
              <CommentsPanel appid={steamAppid} />
            </div>
          )}
        </div>
      </div>

      <style>{`
        @keyframes gp-in {
          from { opacity: 0; transform: scale(1.01); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>
  )
}
