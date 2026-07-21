"use client"

import { useEffect, useRef, useState } from "react"
import { useGamepadNav } from "./useGamepadNav"
import { semHtml } from "./texto"
import type { JogoLinha } from "./types"
import { useI18n } from "../../i18n/I18nContext"

type Detalhes = NonNullable<Awaited<ReturnType<NonNullable<typeof window.launcherAPI>["storeDetails"]>>["jogo"]>

interface StoreGamePageProps {
  jogo: JogoLinha | null
  bloqueado: boolean
  ocupado: boolean
  onBaixar: () => void
  onAdicionar: () => void
  onRemover: () => void
  onFechar: () => void
}

export function StoreGamePage({
  jogo,
  bloqueado,
  ocupado,
  onBaixar,
  onAdicionar,
  onRemover,
  onFechar,
}: StoreGamePageProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const aberto = Boolean(jogo)
  useGamepadNav(ref, aberto, onFechar)

  const [det, setDet] = useState<Detalhes | null>(null)
  const [carregando, setCarregando] = useState(false)
  const [erro, setErro] = useState("")

  useEffect(() => {
    if (!jogo) return
    setDet(null)
    setErro("")
    setCarregando(true)
    let cancelado = false
    window.launcherAPI
      ?.storeDetails(jogo.appid)
      .then((r) => {
        if (cancelado) return
        if (r?.ok && r.jogo) setDet(r.jogo)
        else setErro(r?.error || t("store.erro_carregar_ficha"))
      })
      .finally(() => !cancelado && setCarregando(false))
    return () => {
      cancelado = true
    }
  }, [jogo])

  if (!jogo) return null

  const trailer = det?.trailer
  const semManifesto = jogo.manifest === false

  return (
    <div ref={ref} className="gp-scope fixed inset-0 z-[70] overflow-y-auto bg-[#08090b] text-white">
      {/* Arte de fundo esmaecida, como na página da loja da Steam */}
      {det?.fundo && (
        <div
          className="pointer-events-none fixed inset-0 opacity-25"
          style={{ backgroundImage: `url(${det.fundo})`, backgroundSize: "cover", backgroundPosition: "center" }}
        />
      )}
      <div className="pointer-events-none fixed inset-0 bg-gradient-to-b from-black/60 via-black/80 to-[#08090b]" />

      <div className="relative mx-auto max-w-[1180px] px-12 py-10">
        <h1 className="text-4xl font-light tracking-wide">{det?.nome || jogo.title}</h1>
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-white/50">
          {det?.devs?.[0] && <span>{det.devs[0]}</span>}
          {det?.lancamento && <span>· {det.lancamento}</span>}
          {det?.generos?.length ? <span>· {det.generos.join(" · ")}</span> : null}
          {det?.metacritic ? <span className="text-[#4adf9a]">· Metacritic {det.metacritic}</span> : null}
          {det?.preco && <span className="text-white/75">· {det.preco}</span>}
        </div>

        {/* Trailer: MP4 do CDN da Steam, mudo e em laço, como a loja faz */}
        <div className="mt-6 aspect-video w-full overflow-hidden rounded-2xl bg-black ring-1 ring-white/10">
          {trailer ? (
            <video
              key={trailer.url}
              src={trailer.url}
              poster={trailer.poster}
              autoPlay
              loop
              muted
              playsInline
              className="h-full w-full object-cover"
            />
          ) : det?.screenshots?.[0] ? (
            <img src={det.screenshots[0]} alt="" className="h-full w-full object-cover" />
          ) : (
            <div className="flex h-full items-center justify-center text-white/25">
              {carregando ? t("store.carregando") : t("store.sem_trailer")}
            </div>
          )}
        </div>

        {/* Ações — as mesmas do modo desktop, vindas do hook compartilhado */}
        <div className="mt-6 flex flex-wrap gap-3">
          {bloqueado ? (
            <>
              <div className="flex items-center gap-2 rounded-xl border border-[color:var(--accent)]/40 px-6 py-3 text-sm font-semibold" style={{ color: "var(--accent)" }}>
                {t("store.na_biblioteca")}
              </div>
              <Botao rotulo={t("common.remover")} perigo onClick={onRemover} desabilitado={ocupado} />
            </>
          ) : semManifesto ? (
            <div
              title={t("store.sem_manifesto_tooltip")}
              className="rounded-xl border border-white/10 px-6 py-3 text-sm font-semibold text-white/35"
            >
              {t("store.sem_manifesto")}
            </div>
          ) : (
            <>
              <Botao rotulo={ocupado ? "…" : t("store.baixar")} primario onClick={onBaixar} desabilitado={ocupado} />
              <Botao rotulo={t("store.adicionar_steam")} onClick={onAdicionar} desabilitado={ocupado} />
            </>
          )}
          <Botao rotulo={t("common.voltar")} onClick={onFechar} />
        </div>

        {jogo.fontes?.length ? (
          <p className="mt-3 text-xs text-white/35">{t("store.manifesto_disponivel_em")} {jogo.fontes.join(", ")}</p>
        ) : null}

        {det?.descricao && (
          <p className="mt-8 max-w-3xl text-[15px] leading-relaxed text-white/70">{semHtml(det.descricao)}</p>
        )}

        {det?.screenshots?.length ? (
          <>
            <h2 className="mt-10 mb-3 text-[15px] font-medium text-white/70">{t("store.imagens")}</h2>
            <div className="flex gap-3 overflow-x-auto pb-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              {det.screenshots.map((s) => (
                <img
                  key={s}
                  src={s}
                  alt=""
                  loading="lazy"
                  className="h-[168px] shrink-0 rounded-lg object-cover ring-1 ring-white/10"
                />
              ))}
            </div>
          </>
        ) : null}

        {det?.reqMin && (
          <>
            <h2 className="mt-10 mb-3 text-[15px] font-medium text-white/70">{t("store.requisitos_minimos")}</h2>
            <p className="max-w-3xl whitespace-pre-line text-[13px] leading-relaxed text-white/45">
              {semHtml(det.reqMin)}
            </p>
          </>
        )}

        {erro && <p className="mt-8 text-sm text-[#ff6b81]">{t("store.ficha_indisponivel", { erro })}</p>}
      </div>
    </div>
  )
}

function Botao({
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
      className={`rounded-xl px-6 py-3 text-sm font-semibold outline-none transition-all disabled:opacity-50 ${
        primario
          ? "text-black enabled:hover:scale-[1.03]"
          : perigo
            ? "border border-[#ff6b81]/40 text-[#ff6b81] enabled:hover:bg-[#ff6b81]/10"
            : "border border-white/15 text-white/80 enabled:hover:bg-white/[0.08]"
      }`}
      style={primario ? { background: "var(--accent)" } : undefined}
    >
      {rotulo}
    </button>
  )
}
