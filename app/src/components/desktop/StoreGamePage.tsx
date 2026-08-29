"use client"

// Página de detalhe de um jogo da LOJA (não instalado). Layout cinemático:
// hero fullbleed com wash gradiente, cover retrato flutuante, título em fonte
// display, chip de meta + ações sticky em vidro logo abaixo. Duas colunas de
// conteúdo com painéis reagrupados. Preto OLED em toda superfície.

import { useEffect, useRef, useState } from "react"
import { useJogoRodando } from "../useJogoRodando"
import type { Game } from "../ps5-launcher/types"
import { useI18n } from "../../i18n/I18nContext"
import {
  GameMediaGallery,
  GameDescription,
  Panel,
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
import { CommunityPanel } from "../CommunityPanel"
import { RetroAchievementsGamePanel } from "./RetroAchievementsGamePanel"

type ItemLoja = {
  appid: string
  title: string
  cover?: string
  capa?: string
  heroi?: string
  manifest?: boolean
}

export type RetroStoreDetail = {
  systemId?: string
  platform?: string
  description?: string
  genres?: string[]
  releaseYear?: number | null
  developers?: string[]
  publishers?: string[]
  offerCount?: number
  availableCount?: number
  fileSize?: string
  sourceCount?: number
  links?: { label: string; onClick: () => void }[]
  screenshots?: string[]
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
  statusMessage,
  naBiblioteca,
  ocupado,
  embedded,
  game,
  slssteamAtivo,
  retro,
  rodando: rodandoExterno,
  abrindo: abrindoExterno,
  onStop,
  onCancel,
}: {
  jogo: ItemLoja
  game?: Game
  onClose: () => void
  onBaixar: () => void
  onAdicionar: () => void
  onRemover?: () => void
  onConfig?: () => void
  onJogar?: () => void
  statusMessage?: string
  naBiblioteca: boolean
  ocupado: boolean
  embedded?: boolean
  slssteamAtivo?: boolean
  retro?: RetroStoreDetail
  /** Estado/ações do launcher pai, quando a página está embutida. */
  rodando?: boolean
  abrindo?: boolean
  onStop?: () => void
  onCancel?: () => void
}) {
  const { t } = useI18n()
  const [info, setInfo] = useState<Info | null>(null)
  const [busy, setBusy] = useState(true)
  const isRetro = Boolean(retro)
  const [slsAtivo, setSlsAtivo] = useState(Boolean(slssteamAtivo))
  // O mesmo estado vale para a biblioteca, a loja e o modo console. O hook
  // também pede o replay do estado atual ao montar, então a página não perde
  // o botão PARAR quando é aberta depois que o jogo já iniciou.
  const jogoAtivo = useJogoRodando(game ? [game] : [])
  const mesmaSessao = Boolean(game && jogoAtivo.jogo?.id === game.id)
  const rodandoLocal = mesmaSessao && jogoAtivo.rodando
  const abrindoLocal = mesmaSessao && jogoAtivo.pendente
  const rodando = rodandoExterno ?? rodandoLocal
  const abrindo = abrindoExterno ?? abrindoLocal
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

  useEffect(() => {
    if (isRetro) {
      setInfo(null)
      setBusy(false)
      return
    }
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
  }, [jogo.appid, isRetro])

  useEffect(() => {
    if (isRetro || !naBiblioteca) {
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
  }, [jogo.appid, naBiblioteca, game, isRetro])

  useEffect(() => {
    if (typeof slssteamAtivo === "boolean") setSlsAtivo(slssteamAtivo)
  }, [slssteamAtivo])

  useEffect(() => {
    const carregar = () =>
      window.launcherAPI?.storeStatus?.().then((s) => {
        if (typeof slssteamAtivo !== "boolean") setSlsAtivo(Boolean(s?.slssteam))
        setFixesAtivo(Boolean(s?.luatools))
      })
    carregar()
    return window.launcherAPI?.onPluginsChanged?.(() => carregar())
  }, [])

  // Consulta as fontes JSON conectadas e confirma que há uma URI baixável.
  // O índice leve só traz título/ref; validar o jogo completo evita mostrar
  // "Baixar" para entradas sem magnet ou URL.
  useEffect(() => {
    let vivo = true
    setTemTorrent(false)
    if (isRetro) return () => { vivo = false }
    ;(async () => {
      try {
        const normalizar = (s: string) =>
          String(s || "")
            .replace(/\s+(?:on|na)\s+steam(?:\s*[-|:].*)?$/i, "")
            .toLowerCase()
            .replace(/[^a-z0-9]/g, "")
        const tituloBusca = String(jogo.title || "").replace(/\s+(?:on|na)\s+steam(?:\s*[-|:].*)?$/i, "").trim()
        const r = await window.launcherAPI?.sourcesSearch?.(tituloBusca, 50)
        const resultados = Array.isArray(r?.results) ? r.results : []
        const alvo = normalizar(tituloBusca)
        const tokensAlvo = tituloBusca
          .toLowerCase()
          .split(/\s+/)
          .map((token) => token.replace(/[^a-z0-9]/g, ""))
          .filter((token) => token.length >= 3)
        for (const candidato of resultados) {
          const titulo = normalizar(String(candidato.title || ""))
          const tokensCoincidentes = tokensAlvo.filter((token) => titulo.includes(token)).length
          const compativel =
            titulo &&
            (titulo.includes(alvo) ||
              (titulo.length >= 8 && alvo.includes(titulo)) ||
              (tokensAlvo.length >= 2 && tokensCoincidentes >= Math.min(2, tokensAlvo.length)))
          if (!compativel) continue
          const full = await window.launcherAPI?.sourcesGame?.(candidato.ref)
          const rawUris = full?.game?.uris
          const uris = Array.isArray(rawUris)
            ? rawUris
            : typeof rawUris === "string"
              ? [rawUris]
              : full?.game?.uri
                ? [full.game.uri]
                : []
          if (uris.some((uri) => /^(magnet:|https?:\/\/)/i.test(String(uri).trim()))) {
            if (vivo) setTemTorrent(true)
            return
          }
        }
      } catch {}
    })()
    return () => {
      vivo = false
    }
  }, [jogo.title, isRetro])

  const podeBaixar = isRetro
    ? Boolean(retro?.availableCount || retro?.offerCount || slsAtivo)
    : slsAtivo || temTorrent

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
  const dev = retro?.developers?.[0] || info?.developers?.[0]
  const pub = retro?.publishers?.[0] || info?.publishers?.[0]
  const release = retro?.releaseYear ? String(retro.releaseYear) : info?.release_date || String(game?.year || "—")
  // Retro library entries may exist before the ROM is downloaded. Only an
  // emulator-ready launch callback means the game is installed in this view.
  const instalado = isRetro ? Boolean(onJogar) : game ? game.installed !== false : Boolean(onJogar)
  const retroScreenshots = (retro?.screenshots || []).map((src) => ({ thumb: src, full: src }))

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
        <div className="store-game-hero relative h-[46vh] min-h-[280px] w-full overflow-hidden bg-black">
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
            <div className="mx-auto flex max-w-[1400px] items-end gap-6 px-6 pb-6">
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
                  {release !== "—" && <Chip>{release}</Chip>}
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
          <div className="mx-auto flex max-w-[1400px] items-center gap-3 px-6 py-3">
            <span
              className={`game-name text-[15px] font-semibold text-white transition-opacity ${rolado ? "opacity-100" : "opacity-0"}`}
            >
              {jogo.title}
            </span>
            <span className="flex-1" />
            {!naBiblioteca && !podeBaixar && (
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
                  {podeBaixar && (
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
                  {onJogar && (rodando ? (
                    <>
                      <PrimaryBtn onClick={() => {}} disabled label={t("gamepage.rodando")}>
                        <svg viewBox="0 0 24 24" fill="none" className="h-3.5 w-3.5 animate-spin">
                          <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" className="opacity-30" />
                          <path d="M4 12a8 8 0 0 1 8-8" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
                        </svg>
                      </PrimaryBtn>
                      <button
                        type="button"
                        data-game-action="stop"
                        onClick={onStop || (() => jogoAtivo.parar())}
                        className="flex items-center gap-2 rounded-full bg-[#ef4444] px-5 py-2 text-[12.5px] font-bold text-white transition-transform hover:scale-[1.03]"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
                          <rect x="5" y="5" width="14" height="14" rx="2" />
                        </svg>
                        {t("gamepage.parar")}
                      </button>
                    </>
                  ) : abrindo ? (
                    <PrimaryBtn
                      onClick={onCancel || (() => jogoAtivo.cancelar())}
                      action="cancel"
                      label={t("common.cancelar")}
                    >
                      <span className="text-base leading-none">×</span>
                    </PrimaryBtn>
                  ) : (
                    <PrimaryBtn onClick={onJogar} disabled={ocupado} label={t("gamepage.jogar")}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M8 5v14l11-7z" />
                      </svg>
                    </PrimaryBtn>
                  ))}
                  {isRetro && podeBaixar && (
                    <PrimaryBtn onClick={onBaixar} disabled={ocupado} label={t("store.baixar")}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                        <polyline points="7 10 12 15 17 10" />
                        <line x1="12" y1="15" x2="12" y2="3" />
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
                  {onBaixar && podeBaixar && (
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

        {statusMessage && (
          <div className="mx-auto max-w-[1400px] px-6 pt-4">
            <p role="status" className="rounded-lg border border-amber-200/15 bg-amber-200/[0.04] px-3 py-2 text-[12px] text-amber-100/75">
              {statusMessage}
            </p>
          </div>
        )}

        {/* ─── Corpo: 2 colunas ───────────────────────────────────────────── */}
        <div className="mx-auto grid max-w-[1400px] grid-cols-1 gap-6 px-6 pb-10 pt-6 lg:grid-cols-[1fr_360px]">
          <div className="flex min-w-0 flex-col gap-6">
            {(isRetro || info?.release_date || info?.publishers?.length) && (
              <div className="ui-card p-5 text-[13px]">
                {release !== "—" && <p className="text-white/80">{t("gamepage.lancado_em")} {release}</p>}
                {(dev || pub || retro?.platform) && (
                  <p className="mt-1 text-white/55">
                    {retro?.platform || (dev ? `${t("gamepage.publicado_por")} ${dev}` : pub)}
                  </p>
                )}
              </div>
            )}
            <GameMediaGallery
              movies={isRetro ? [] : info?.movies}
              screenshots={isRetro ? retroScreenshots : info?.screenshots}
            />
            <GameDescription
              html={isRetro ? undefined : info?.about}
              fallback={isRetro ? retro?.description : info?.short_description}
            />
            {busy && !info && !isRetro && (
              <p className="text-[13px] text-white/40">{t("gamepage.carregando")}</p>
            )}
            {!isRetro && <ReviewsPanel appid={jogo.appid} />}
            {!isRetro && <CommentsPanel appid={jogo.appid} />}
            {!isRetro && <CommunityPanel appid={jogo.appid} title={jogo.title} />}
          </div>

          <div className="flex flex-col gap-4">
            {isRetro ? (
              <>
                <RetroSummaryPanel detail={retro} />
                {retro?.systemId && <RetroAchievementsGamePanel title={jogo.title} systemId={retro.systemId} />}
                {retro?.links?.length ? (
                  <Panel title="Links rápidos">
                    <div className="flex flex-col gap-1">
                      {retro.links.map((link) => (
                        <button key={link.label} type="button" onClick={link.onClick} className="border-b border-white/[.06] px-1 py-2 text-left text-[12px] text-white/65 last:border-0 hover:text-white">
                          {link.label}
                        </button>
                      ))}
                    </div>
                  </Panel>
                ) : null}
              </>
            ) : (
              <>
                <ProtonDBPanel appid={jogo.appid} />
                <StatsPanel appid={jogo.appid} />
                <AchievementsPanel appid={jogo.appid} />
                <RequirementsPanel min={info?.req_min} rec={info?.req_rec} />
                <LanguagesPanel languages={info?.languages} />
                <ControllerPanel support={info?.controller_support} />
              </>
            )}
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

function RetroSummaryPanel({ detail }: { detail?: RetroStoreDetail }) {
  const rows = [
    ["Sistema", detail?.systemId],
    ["Plataforma", detail?.platform],
    ["Gêneros", detail?.genres?.join(", ")],
    ["Lançamento", detail?.releaseYear ? String(detail.releaseYear) : undefined],
    ["Desenvolvedora", detail?.developers?.join(", ")],
    ["Distribuidora", detail?.publishers?.join(", ")],
    ["Fontes", detail?.sourceCount != null ? String(detail.sourceCount) : undefined],
    ["Ofertas", detail?.offerCount != null ? String(detail.offerCount) : undefined],
    ["Tamanho", detail?.fileSize],
  ].filter((row): row is [string, string] => Boolean(row[1]))
  return (
    <Panel title="Informações">
      <dl className="grid grid-cols-[minmax(90px,auto)_1fr] gap-x-3 gap-y-3 text-[12px]">
        {rows.map(([label, value]) => <div key={label} className="contents"><dt className="uppercase tracking-[.06em] text-white/35">{label}</dt><dd className="truncate text-white/70">{value}</dd></div>)}
      </dl>
      {detail?.availableCount != null && <p className="mt-4 border-t border-white/[.06] pt-3 text-[11px] text-white/45">{detail.availableCount} opções disponíveis</p>}
    </Panel>
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
  action,
}: {
  children: React.ReactNode
  onClick: () => void
  disabled?: boolean
  label: string
  action?: "stop" | "cancel"
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      data-game-action={action}
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
