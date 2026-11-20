"use client"

import { useEffect, useRef, useState } from "react"

import { useI18n } from "../../i18n/I18nContext"
import { useHlsTrailer } from "../../hooks/useHlsTrailer"

export type TrailerMovie = {
  id: number
  name?: string
  thumb: string
  mp4?: string
  webm?: string
  hls?: string
}

/**
 * Player único dos trailers da ficha do jogo (biblioteca e loja).
 *
 * A Steam aposentou o MP4: `movies[].mp4` vem vazio e só `hls_h264` existe.
 * Este componente toca o HLS direto, sem baixar o trailer antes. Quando a API
 * ainda traz um MP4/WebM progressivo, ele é usado nativamente.
 */
export function TrailerPlayer({ movie }: { movie: TrailerMovie }) {
  const { t } = useI18n()
  const videoRef = useRef<HTMLVideoElement>(null)
  const direto = movie.mp4 || movie.webm || ""
  const hls = direto ? null : movie.hls || null
  const { estado, erro, recarregar } = useHlsTrailer(videoRef, hls)

  const [pronto, setPronto] = useState(false)
  const [erroDireto, setErroDireto] = useState(false)
  const [tentativaDireta, setTentativaDireta] = useState(0)

  useEffect(() => {
    setPronto(false)
    setErroDireto(false)
  }, [movie.id, direto, hls])

  const falhou = erro || erroDireto
  const carregando = !falhou && (direto ? !pronto : estado === "carregando")

  const tentarDeNovo = () => {
    setErroDireto(false)
    setPronto(false)
    if (direto) setTentativaDireta((n) => n + 1)
    else recarregar()
  }

  return (
    <div className="relative h-full w-full bg-black">
      <video
        // Só o caminho nativo remonta: o HLS é religado pelo próprio hook.
        key={direto ? `${movie.id}:${tentativaDireta}` : movie.id}
        ref={videoRef}
        src={direto || undefined}
        poster={movie.thumb}
        controls
        autoPlay
        muted
        playsInline
        preload="auto"
        className="h-full w-full object-contain"
        onCanPlay={() => setPronto(true)}
        onError={() => {
          if (direto) setErroDireto(true)
        }}
      />

      {carregando && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/20">
          <span className="rounded-lg bg-black/60 px-3 py-1.5 text-sm text-[color:var(--text-2)]">
            {t("trailer.carregando_video")}
          </span>
        </div>
      )}

      {falhou && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-black/80 px-6 text-center">
          <span className="text-sm text-[color:var(--text-2)]">{t("trailer.falha_carregar")}</span>
          <button
            type="button"
            onClick={tentarDeNovo}
            className="rounded-lg px-4 py-2 text-sm font-semibold text-white"
            style={{ background: "rgba(255,255,255,0.12)" }}
          >
            {t("trailer.tentar_novamente")}
          </button>
        </div>
      )}
    </div>
  )
}
