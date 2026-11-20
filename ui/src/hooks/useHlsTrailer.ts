"use client"

import { useCallback, useEffect, useState } from "react"

export type EstadoTrailer = "inativo" | "carregando" | "pronto" | "erro"

/**
 * Toca um manifesto HLS da Steam no `<video>` via hls.js/MSE.
 *
 * ## Por que hls.js e não o caminho antigo
 *
 * O backend baixava o manifesto inteiro com ffmpeg (37–61 MB por trailer),
 * virava base64 e mandava tudo pelo IPC antes de o player montar. O primeiro
 * frame só aparecia no fim do download — e cada abertura repetia o trajeto.
 *
 * O WebKitGTK deste app tem MediaSource desde a 2.24, e o WebKit desliga o HLS
 * nativo de propósito para os sites usarem MSE. Então o caminho certo é o
 * mesmo do HydraLauncher/Chromium: hls.js busca o manifesto e alimenta o
 * `<video>` por MSE. Medido nesta máquina: primeiro frame em ~0,6 s.
 *
 * O hls.js é carregado sob demanda: quem abre uma página sem trailer não paga
 * o custo do parser.
 */
export function useHlsTrailer(
  video: React.RefObject<HTMLVideoElement>,
  url: string | null | undefined,
  ativo = true,
) {
  const [estado, setEstado] = useState<EstadoTrailer>(url ? "carregando" : "inativo")
  const [erro, setErro] = useState(false)
  const [tentativa, setTentativa] = useState(0)

  const recarregar = useCallback(() => setTentativa((n) => n + 1), [])

  useEffect(() => {
    const elemento = video.current
    setErro(false)
    if (!elemento || !url || !ativo) {
      setEstado("inativo")
      return
    }

    setEstado("carregando")
    let vivo = true
    let hls: import("hls.js").default | null = null

    const pronto = () => {
      if (vivo) setEstado("pronto")
    }
    elemento.addEventListener("canplay", pronto)
    elemento.addEventListener("playing", pronto)

    const iniciar = async () => {
      const { default: Hls } = await import("hls.js")
      if (!vivo) return

      if (Hls.isSupported()) {
        hls = new Hls({
          // O card tem ~820 px: não adianta baixar o nível 1080p inteiro.
          capLevelToPlayerSize: true,
          // Janela curta de propósito: trailer é preview, não um filme.
          maxBufferLength: 30,
          backBufferLength: 30,
        })

        let recuperouMidia = false
        let recuperouRede = false

        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          if (vivo) void elemento.play().catch(() => {})
        })

        hls.on(Hls.Events.ERROR, (_evento, dados) => {
          if (!dados.fatal || !vivo) return
          // Uma tentativa de recuperação por tipo. Se falhar de novo, mostra o
          // erro em vez de fingir que está carregando.
          if (dados.type === Hls.ErrorTypes.MEDIA_ERROR && !recuperouMidia) {
            recuperouMidia = true
            hls?.recoverMediaError()
            return
          }
          if (dados.type === Hls.ErrorTypes.NETWORK_ERROR && !recuperouRede) {
            recuperouRede = true
            hls?.startLoad()
            return
          }
          setErro(true)
          setEstado("erro")
        })

        hls.loadSource(url)
        hls.attachMedia(elemento)
        return
      }

      // Sem MSE, só resta o HLS nativo (Safari). WebKitGTK 2.38+ o desativa,
      // mas mantém o caminho para não quebrar outro webview.
      if (elemento.canPlayType("application/vnd.apple.mpegurl")) {
        elemento.src = url
        void elemento.play().catch(() => {})
      } else {
        setErro(true)
        setEstado("erro")
      }
    }

    void iniciar().catch(() => {
      if (!vivo) return
      setErro(true)
      setEstado("erro")
    })

    return () => {
      vivo = false
      elemento.removeEventListener("canplay", pronto)
      elemento.removeEventListener("playing", pronto)
      hls?.destroy()
      hls = null
    }
  }, [video, url, ativo, tentativa])

  return { estado, erro, recarregar }
}
