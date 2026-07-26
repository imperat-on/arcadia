"use client"

// Página de detalhe de um jogo da LOJA (não instalado), estilo Hydra: hero,
// galeria trailer+screenshots, descrição, painel ProtonDB e Conquistas à
// direita, botões Baixar / Adicionar à biblioteca. Os dados vêm do
// gameSysinfo (appdetails Steam) já cacheado.

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import {
  GameMediaGallery, GameDescription, ProtonDBPanel, AchievementsPanel,
  ControllerPanel, RequirementsPanel, LanguagesPanel, StatsPanel, ReviewsPanel, CommentsPanel,
} from "./GameDetailPanels"

type ItemLoja = { appid: string; title: string; cover?: string; capa?: string; heroi?: string; manifest?: boolean }
type Info = NonNullable<Awaited<ReturnType<NonNullable<Window["launcherAPI"]>["gameSysinfo"]>>["info"]>

export function StoreGamePage({
  jogo,
  onClose,
  onBaixar,
  onAdicionar,
  onRemover,
  onConfig,
  naBiblioteca,
  ocupado,
  embedded,
}: {
  jogo: ItemLoja
  onClose: () => void
  onBaixar: () => void
  onAdicionar: () => void
  onRemover?: () => void
  onConfig?: () => void
  naBiblioteca: boolean
  ocupado: boolean
  embedded?: boolean
}) {
  const { t } = useI18n()
  const [info, setInfo] = useState<Info | null>(null)
  const [busy, setBusy] = useState(true)
  const [slsAtivo, setSlsAtivo] = useState(false)
  const temDownload = jogo.manifest !== false

  useEffect(() => {
    setBusy(true)
    setInfo(null)
    const g = { id: `steam:${jogo.appid}`, title: jogo.title, launcher: "steam", launch_cmd: [] as string[] }
    window.launcherAPI?.gameSysinfo(g as never).then((r) => {
      setInfo(r?.info || null)
      setBusy(false)
    })
  }, [jogo.appid])

  useEffect(() => {
    window.launcherAPI?.storeStatus?.().then((s) => setSlsAtivo(Boolean(s?.slssteam)))
  }, [])

  const hero = jogo.heroi || `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/library_hero.jpg`
  const header = info?.header || `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/header.jpg`

  return (
    <div className={`${embedded ? "relative h-full" : "fixed inset-0 z-[55]"} flex flex-col bg-[#0b0b0e]`} style={{ animation: "gp-in 0.18s ease-out" }}>
      <button
        onClick={onClose}
        className="absolute left-4 top-4 z-20 flex h-10 w-10 items-center justify-center rounded-full bg-black/50 text-white/80 backdrop-blur-sm transition-colors hover:bg-black/70 hover:text-white"
        title={t("common.voltar")}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="15 18 9 12 15 6" />
        </svg>
      </button>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* Hero grande */}
        <div className="relative h-[46vh] min-h-[280px] w-full overflow-hidden bg-black">
          <img src={hero} alt="" className="h-full w-full object-cover" draggable={false}
            onError={(e) => { (e.currentTarget as HTMLImageElement).src = header }} />
          <div className="absolute inset-0 bg-gradient-to-t from-[#0b0b0e] via-transparent to-transparent" />
          <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-4 p-6">
            <h1 className="text-3xl font-semibold text-white drop-shadow-lg">{jogo.title}</h1>
          </div>
        </div>

        {/* Barra de ação estilo Hydra */}
        <div className="mx-auto flex max-w-[1400px] items-center justify-between gap-4 px-6 py-4">
          <span className="text-[13px] text-white/55">
            {temDownload ? "" : t("store.nenhum_download")}
          </span>
          <div className="flex gap-2.5">
            {naBiblioteca ? (
              <>
                <span className="flex items-center gap-1.5 rounded-lg border border-[color:var(--accent)]/40 px-5 py-2.5 text-[13px] font-semibold" style={{ color: "var(--accent)" }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12" /></svg>
                  {t("store.na_biblioteca")}
                </span>
                {onConfig && (
                  <button
                    onClick={onConfig}
                    disabled={ocupado}
                    className="flex items-center gap-2 rounded-lg border border-white/20 px-5 py-2.5 text-[13px] font-semibold text-white/85 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.14.32.22.66.22 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
                    {t("gamepage.gerenciar")}
                  </button>
                )}
                {onRemover && (
                  <button
                    onClick={onRemover}
                    disabled={ocupado}
                    className="rounded-lg border border-[#ff6b81]/40 px-5 py-2.5 text-[13px] font-semibold text-[#ff6b81] transition-colors enabled:hover:bg-[#ff6b81]/10 disabled:opacity-50"
                  >
                    {t("common.remover")}
                  </button>
                )}
              </>
            ) : (
              <>
                <button
                  onClick={onAdicionar}
                  disabled={ocupado}
                  className="flex items-center gap-2 rounded-lg border border-white/20 px-5 py-2.5 text-[13px] font-semibold text-white/85 transition-colors enabled:hover:bg-white/[0.06] enabled:hover:text-white disabled:opacity-50"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
                  {t("store.adicionar_biblioteca")}
                </button>
                {slsAtivo && temDownload && (
                  <button
                    onClick={onBaixar}
                    disabled={ocupado}
                    className="flex items-center gap-2 rounded-lg px-6 py-2.5 text-[13px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
                    style={{ background: "var(--accent)" }}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" /></svg>
                    {t("store.baixar")}
                  </button>
                )}
              </>
            )}
          </div>
        </div>

        {/* Corpo: 2 colunas (conteúdo | painéis) */}
        <div className="mx-auto grid max-w-[1400px] grid-cols-[1fr_360px] gap-6 px-6 pb-10">
          <div className="flex min-w-0 flex-col gap-5">
            {(info?.release_date || info?.publishers?.length) ? (
              <div className="ui-card p-5 text-[13px]">
                {info?.release_date && (
                  <p className="text-white/80">{t("gamepage.lancado_em")} {info.release_date}</p>
                )}
                {info?.publishers?.length ? (
                  <p className="mt-1 text-white/55">{t("gamepage.publicado_por")} {info.publishers.join(", ")}</p>
                ) : null}
              </div>
            ) : null}

            <GameMediaGallery movies={info?.movies} screenshots={info?.screenshots} />

            <GameDescription html={info?.about} fallback={info?.short_description} />
            {busy && !info && <p className="text-[13px] text-white/40">{t("gamepage.carregando")}</p>}

            {/* Avaliações + comentários abaixo da descrição (não na lateral). */}
            <ReviewsPanel appid={jogo.appid} />
            <CommentsPanel appid={jogo.appid} />
          </div>

          <div className="flex flex-col gap-5">
            <ProtonDBPanel appid={jogo.appid} />
            <StatsPanel appid={jogo.appid} />
            <AchievementsPanel appid={jogo.appid} />
            <RequirementsPanel min={info?.req_min} rec={info?.req_rec} />
            <LanguagesPanel languages={info?.languages} />
            <ControllerPanel support={info?.controller_support} />
          </div>
        </div>
      </div>

      <style>{`@keyframes gp-in { from { opacity:0; transform:scale(1.01);} to {opacity:1; transform:scale(1);} }`}</style>
    </div>
  )
}
