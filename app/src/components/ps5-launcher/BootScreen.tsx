"use client"

import { useEffect, useRef, useState } from "react"

// Tela de boot: toca um vídeo de abertura (estilo PS5) em tela cheia.
// O vídeo é lido do processo principal (asset do pacote ou cópia em
// ~/.local/share/arcadia/boot.mp4) e vira um blob URL. O `file://` usado antes
// era bloqueado quando a página roda em http:// (dev/preview) e dependia de um
// arquivo externo ao pacote. `saindo` dispara o fade de saída.
// Sem vídeo nenhum (sem bytes, sem fallback) o boot NÃO fica preso: onError
// pula direto para a home pelo mesmo caminho de sempre.
export function BootScreen({
  src,
  saindo,
  onEnded,
  onError,
}: {
  /** Fallback opcional (testes / contextos file://). Ausente = pular o boot. */
  src?: string
  saindo: boolean
  onEnded: () => void
  onError: () => void
}) {
  const ref = useRef<HTMLVideoElement>(null)
  const avisado = useRef(false)
  const [videoSrc, setVideoSrc] = useState<string | null>(null)
  const falhaRef = useRef(onError)
  const fimRef = useRef(onEnded)
  falhaRef.current = onError
  fimRef.current = onEnded

  const avisarFalha = () => {
    if (avisado.current) return
    avisado.current = true
    falhaRef.current()
  }

  useEffect(() => {
    let vivo = true
    let criado: string | null = null
    const usar = (bytes?: Uint8Array | null) => {
      if (!vivo) return
      if (bytes && bytes.byteLength) {
        criado = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "video/mp4" }))
        setVideoSrc(criado)
      } else if (src) {
        // Sem bytes: cai no caminho file:// passado (só funciona no Electron,
        // onde a própria página roda em file://).
        setVideoSrc(src)
      } else {
        // Nada para tocar: pula o boot — nunca prende a tela preta.
        vivo = false
        avisarFalha()
      }
    }
    const api = window.launcherAPI
    if (!api?.bootVideo) {
      usar(null)
      return () => {
        vivo = false
        if (criado) URL.revokeObjectURL(criado)
      }
    }
    api
      .bootVideo()
      .then((resultado) => usar(resultado?.ok ? resultado.data : null))
      .catch(() => usar(null))
    return () => {
      vivo = false
      if (criado) URL.revokeObjectURL(criado)
    }
  }, [src])

  // O evento de erro do <video> pode escapar antes dos manejadores do React
  // (media events não borbulham e o navegador pode falhar o src no mesmo ciclo
  // em que o elemento entra na árvore). Listeners nativos garantem que erro
  // ou fim sempre avisem — o boot nunca fica preso.
  useEffect(() => {
    const video = ref.current
    if (!video || !videoSrc) return
    const falha = () => avisarFalha()
    const fim = () => fimRef.current()
    video.addEventListener("error", falha)
    video.addEventListener("ended", fim)
    return () => {
      video.removeEventListener("error", falha)
      video.removeEventListener("ended", fim)
    }
  }, [videoSrc])

  return (
    <div className={`retro-boot-screen fixed inset-0 z-[80] bg-black ${saindo ? "boot-out" : ""}`}>
      <video
        ref={ref}
        src={videoSrc ?? undefined}
        autoPlay
        muted
        playsInline
        onEnded={onEnded}
        onError={onError}
        className="h-full w-full object-cover"
      />
    </div>
  )
}
