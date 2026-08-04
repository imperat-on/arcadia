"use client"

// Página de detalhe de um jogo da LOJA (não instalado). Layout cinemático:
// hero fullbleed com wash gradiente, cover retrato flutuante, título em fonte
// display, chip de meta + ações sticky em vidro logo abaixo. Duas colunas de
// conteúdo com painéis reagrupados. Preto OLED em toda superfície.

import { useEffect, useRef, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"
import {
  GameMediaGallery,
  GameDescription,
  ProtonDBPanel,
  ControllerPanel,
  RequirementsPanel,
  LanguagesPanel,
  StatsPanel,
  ReviewsPanel,
  CommentsPanel,
} from "./GameDetailPanels"
import { AchievementsPanel } from "./AchievementsPanel"
import { FixesPanel } from "./FixesPanel"

type ItemLoja = {
  appid: string
  title: string
  cover?: string
  capa?: string
  heroi?: string
  manifest?: boolean
}
type Info = NonNullable<
  Awaited<ReturnType<NonNullable<Window["launcherAPI"]>["gameSysinfo"]>>["info"]
>

export function StoreGamePage({
  jogo,
  onClose,
  onBaixar,
  onAdicionar,
  onRemover,
  onConfig,
  onJogar,
  naBiblioteca,
  ocupado,
  embedded,
  game,
}: {
  jogo: ItemLoja
  game?: Game
  onClose: () => void
  onBaixar: () => void
  onAdicionar: () => void
  onRemover?: () => void
  onConfig?: () => void
  onJogar?: () => void
  naBiblioteca: boolean
  ocupado: boolean
  embedded?: boolean
}) {
  const { t } = useI18n()
  const [info, setInfo] = useState<Info | null>(null)
  const [busy, setBusy] = useState(true)
  const [slsAtivo, setSlsAtivo] = useState(false)
  const [fixesAtivo, setFixesAtivo] = useState(false)
  const voltarRef = useRef<HTMLButtonElement>(null)

  // ESC fecha. Antes só o botão fechava — sem teclado, quem chegava aqui pelo
  // duplo-clique do desktop tinha que ir com o mouse até o canto pra sair.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", h)
    return () => window.removeEventListener("keydown", h)
  }, [onClose])

  // Ao abrir, foca o botão voltar. Isso dá um ponto inicial para o D-pad do
  // gamepad (a página vira uma tela navegável por espaço), e ainda faz o
  // scrollIntoView do useGamepadNav parar no topo, e não no meio dos painéis.
  useEffect(() => {
    const id = requestAnimationFrame(() => voltarRef.current?.focus())
    return () => cancelAnimationFrame(id)
  }, [jogo.appid])
  const [rolado, setRolado] = useState(false)
  const [installPath, setInstallPath] = useState("")
  const [fixesAberto, setFixesAberto] = useState(false)
  // Um jogo pode ter torrent numa fonte JSON conectada mesmo sem SLSsteam ativo.
  // Quando isso for verdade, o botão Baixar aparece com só a opção Torrent.
  const [temTorrent, setTemTorrent] = useState(false)
  const temDownload = jogo.manifest !== false

  useEffect(() => {
    setBusy(true)
    setInfo(null)
    const g = {
      id: `steam:${jogo.appid}`,
      title: jogo.title,
      launcher: "steam",
      launch_cmd: [] as string[],
    }
    window.launcherAPI?.gameSysinfo(g as never).then((r) => {
      setInfo(r?.info || null)
      setBusy(false)
    })
  }, [jogo.appid])

  useEffect(() => {
    if (!naBiblioteca) {
      setInstallPath("")
      return
    }
    const g = game || {
      id: `steam:${jogo.appid}`,
      title: jogo.title,
      launcher: "steam",
      launch_cmd: [] as string[],
    }
    window.launcherAPI?.storeInstallDir(g as never).then((r) => setInstallPath(r?.path || ""))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jogo.appid, naBiblioteca, game])

  useEffect(() => {
    const carregar = () =>
      window.launcherAPI?.storeStatus?.().then((s) => {
        setSlsAtivo(Boolean(s?.slssteam))
        setFixesAtivo(Boolean(s?.luatools))
      })
    carregar()
    return window.launcherAPI?.onPluginsChanged?.(() => carregar())
  }, [])

  // Consulta as fontes JSON conectadas para saber se este jogo tem torrent.
  // 1 chamada por página aberta — barato o suficiente pra não valer cache.
  useEffect(() => {
    let vivo = true
    window.launcherAPI
      ?.sourcesSearch?.(jogo.title, 1)
      .then((r) => {
        if (!vivo) return
        // sourcesSearch retorna { ok, results: [...] } — não é array direto.
        setTemTorrent(Boolean(r?.ok && Array.isArray(r.results) && r.results.length > 0))
      })
      .catch(() => {
        if (vivo) setTemTorrent(false)
      })
    return () => {
      vivo = false
    }
  }, [jogo.title])

  const hero =
    jogo.heroi || `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/library_hero.jpg`
  const header =
    info?.header || `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/header.jpg`
  const portraitUrl = `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/library_600x900.jpg`
  // A capa flutuante é retrato (160×240 / 2:3); `jogo.cover` costuma ser o
  // header paisagem, então preferimos `jogo.capa` (library_600x900). Se até a
  // `capa` vier como header (ex.: DesktopLauncher passando cover como capa),
  // caimos na URL retrato padrão.
  const capa = jogo.capa && !jogo.capa.includes("/header.jpg") ? jogo.capa : portraitUrl
  const dev = info?.developers?.[0]
  const pub = info?.publishers?.[0]
  const instalado = game ? game.installed !== false : Boolean(onJogar)

  return (
    <div
      data-gamepad-cursor-surface
      className={`${embedded ? "relative h-full" : "fixed inset-0 z-[55]"} flex flex-col bg-black`}
      style={{ animation: "sg-in 0.22s var(--ease-out)" }}
      onScroll={(e) => setRolado((e.target as HTMLDivElement).scrollTop > 40)}
    >
      {/* Voltar — sempre visível, glass */}
      <button
        ref={voltarRef}
        onClick={onClose}
        className="absolute left-5 top-5 z-[30] flex h-10 w-10 items-center justify-center rounded-full text-white/90 backdrop-blur-md transition-colors hover:text-white focus:ring-2 focus:ring-[color:var(--accent)] focus:ring-offset-2 focus:ring-offset-black"
        style={{
          background: "rgba(0,0,0,0.55)",
          boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.10)",
        }}
        title={t("common.voltar")}
      >
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <polyline points="15 18 9 12 15 6" />
        </svg>
      </button>

      <div
        data-gamepad-scroll
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={(e) => setRolado((e.currentTarget as HTMLDivElement).scrollTop > 40)}
      >
        {/* ─── Hero cinemático ───────────────────────────────────────────── */}
        <div className="relative h-[62vh] min-h-[420px] w-full overflow-hidden bg-black">
          <img
            src={hero}
            alt=""
            draggable={false}
            className="absolute inset-0 h-full w-full object-cover"
            onError={(e) => {
              ;(e.currentTarget as HTMLImageElement).src = header
            }}
            style={{ filter: "saturate(1.05)" }}
          />
          {/* Wash: leve escurecida por cima + vinheta lateral + rebaixo pra preto embaixo */}
          <div
            className="absolute inset-0"
            style={{
              background:
                "linear-gradient(180deg, rgba(0,0,0,0.25) 0%, rgba(0,0,0,0.05) 40%, rgba(0,0,0,0.85) 88%, #000 100%)",
            }}
          />
          <div
            className="absolute inset-0"
            style={{
              background:
                "radial-gradient(120% 80% at 50% 100%, transparent 40%, rgba(0,0,0,0.55) 100%)",
            }}
          />

          {/* Bloco inferior: cover + título + meta */}
          <div className="absolute inset-x-0 bottom-0 z-[5]">
            <div className="mx-auto flex max-w-[1400px] items-end gap-6 px-8 pb-8">
              {/* Cover retrato flutuante */}
              <div
                className="relative hidden shrink-0 overflow-hidden rounded-xl md:block"
                style={{
                  width: 160,
                  height: 240,
                  boxShadow: "0 30px 60px rgba(0,0,0,0.7), inset 0 0 0 1px rgba(255,255,255,0.08)",
                }}
              >
                <img
                  src={capa}
                  alt=""
                  draggable={false}
                  className="h-full w-full object-cover"
                  onError={(e) => {
                    ;(e.currentTarget as HTMLImageElement).src = header
                  }}
                />
              </div>

              <div className="min-w-0 flex-1 pb-1">
                <h1 className="game-name text-[46px] font-semibold leading-[1.05] text-white drop-shadow-[0_4px_20px_rgba(0,0,0,0.6)]">
                  {jogo.title}
                </h1>
                <div className="mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-white/70">
                  {info?.release_date && <Chip>{info.release_date}</Chip>}
                  {dev && <Chip>{dev}</Chip>}
                  {pub && pub !== dev && (
                    <Chip subtle>
                      {t("gamepage.publicado_por")} {pub}
                    </Chip>
                  )}
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* ─── Barra de ações sticky (glass, aparece completa depois do hero) */}
        <div
          className={`sticky top-0 z-[20] border-b transition-colors ${rolado ? "border-white/[0.08]" : "border-transparent"}`}
          style={{
            background: rolado ? "rgba(4,4,6,0.72)" : "transparent",
            backdropFilter: rolado ? "blur(20px) saturate(1.3)" : "none",
            WebkitBackdropFilter: rolado ? "blur(20px) saturate(1.3)" : "none",
          }}
        >
          <div className="mx-auto flex max-w-[1400px] items-center gap-3 px-8 py-3">
            <span
              className={`game-name text-[15px] font-semibold text-white transition-opacity ${rolado ? "opacity-100" : "opacity-0"}`}
            >
              {jogo.title}
            </span>
            <span className="flex-1" />
            {!naBiblioteca && !temDownload && (
              <span className="hidden text-[12px] text-white/45 sm:inline">
                {t("store.nenhum_download")}
              </span>
            )}
            <div className="flex gap-2">
              {!naBiblioteca ? (
                <>
                  <GhostBtn
                    onClick={onAdicionar}
                    disabled={ocupado}
                    label={t("store.adicionar_biblioteca")}
                  >
                    <svg
                      width="14"
                      height="14"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.2"
                      strokeLinecap="round"
                    >
                      <line x1="12" y1="5" x2="12" y2="19" />
                      <line x1="5" y1="12" x2="19" y2="12" />
                    </svg>
                  </GhostBtn>
                  {(slsAtivo || temTorrent) && temDownload && (
                    <PrimaryBtn onClick={onBaixar} disabled={ocupado} label={t("store.baixar")}>
                      <svg
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.4"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="7 10 12 15 17 10" />
                        <line x1="12" y1="15" x2="12" y2="3" />
                      </svg>
                    </PrimaryBtn>
                  )}
                </>
              ) : instalado ? (
                <>
                  {onJogar && (
                    <PrimaryBtn onClick={onJogar} disabled={ocupado} label={t("gamepage.jogar")}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M8 5v14l11-7z" />
                      </svg>
                    </PrimaryBtn>
                  )}
                  {fixesAtivo && (
                    <GhostBtn onClick={() => setFixesAberto(true)} disabled={ocupado} label="Fixes">
                      <svg
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M12 2v20" />
                        <path d="M2 12h20" />
                        <path d="m7 7 10 10" />
                        <path d="m17 7-10 10" />
                      </svg>
                    </GhostBtn>
                  )}
                  {onConfig && (
                    <GhostBtn onClick={onConfig} disabled={ocupado} label={t("gamepage.gerenciar")}>
                      <svg
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.14.32.22.66.22 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
                      </svg>
                    </GhostBtn>
                  )}
                  {onRemover && (
                    <DangerBtn onClick={onRemover} disabled={ocupado} label={t("common.remover")} />
                  )}
                </>
              ) : (
                <>
                  {onBaixar && (slsAtivo || temTorrent) && (
                    <PrimaryBtn onClick={onBaixar} disabled={ocupado} label={t("store.baixar")}>
                      <svg
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.4"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="7 10 12 15 17 10" />
                        <line x1="12" y1="15" x2="12" y2="3" />
                      </svg>
                    </PrimaryBtn>
                  )}
                  {onRemover && (
                    <DangerBtn onClick={onRemover} disabled={ocupado} label={t("common.remover")} />
                  )}
                </>
              )}
            </div>
          </div>
        </div>

        {/* ─── Corpo: 2 colunas ───────────────────────────────────────────── */}
        <div className="mx-auto grid max-w-[1400px] grid-cols-1 gap-6 px-8 pb-16 pt-6 lg:grid-cols-[1fr_360px]">
          <div className="flex min-w-0 flex-col gap-6">
            <GameMediaGallery movies={info?.movies} screenshots={info?.screenshots} />
            <GameDescription html={info?.about} fallback={info?.short_description} />
            {busy && !info && (
              <p className="text-[13px] text-white/40">{t("gamepage.carregando")}</p>
            )}
            <ReviewsPanel appid={jogo.appid} />
            <CommentsPanel appid={jogo.appid} />
          </div>

          <div className="flex flex-col gap-4">
            <ProtonDBPanel appid={jogo.appid} />
            <StatsPanel appid={jogo.appid} />
            <AchievementsPanel appid={jogo.appid} />
            <RequirementsPanel min={info?.req_min} rec={info?.req_rec} />
            <LanguagesPanel languages={info?.languages} />
            <ControllerPanel support={info?.controller_support} />
          </div>
        </div>
      </div>

      {fixesAberto && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
          onClick={() => setFixesAberto(false)}
        >
          <div className="w-[520px] max-w-[92vw]" onClick={(e) => e.stopPropagation()}>
            <FixesPanel appid={jogo.appid} installPath={installPath} />
          </div>
        </div>
      )}

      <style>{`
        @keyframes sg-in { from { opacity:0; transform:translateY(6px);} to {opacity:1; transform:translateY(0);} }
      `}</style>
    </div>
  )
}

function Chip({ children, subtle }: { children: React.ReactNode; subtle?: boolean }) {
  return (
    <span
      className={`inline-flex items-center rounded-full px-3 py-1 text-[12px] ${subtle ? "text-white/55" : "text-white/85"}`}
      style={{
        background: subtle ? "rgba(255,255,255,0.04)" : "rgba(255,255,255,0.07)",
        boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.06)",
      }}
    >
      {children}
    </span>
  )
}

function PrimaryBtn({
  children,
  onClick,
  disabled,
  label,
}: {
  children: React.ReactNode
  onClick: () => void
  disabled?: boolean
  label: string
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex items-center gap-2 rounded-full bg-white px-5 py-2 text-[12.5px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
    >
      {children}
      {label}
    </button>
  )
}

function GhostBtn({
  children,
  onClick,
  disabled,
  label,
}: {
  children: React.ReactNode
  onClick: () => void
  disabled?: boolean
  label: string
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex items-center gap-2 rounded-full border border-white/15 bg-white/[0.03] px-4 py-2 text-[12.5px] font-semibold text-white/85 transition-colors enabled:hover:border-white/25 enabled:hover:bg-white/[0.07] enabled:hover:text-white disabled:opacity-50"
    >
      {children}
      {label}
    </button>
  )
}

function DangerBtn({
  onClick,
  disabled,
  label,
}: {
  onClick: () => void
  disabled?: boolean
  label: string
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="rounded-full border border-[#ff6b81]/40 px-4 py-2 text-[12.5px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
    >
      {label}
    </button>
  )
}
