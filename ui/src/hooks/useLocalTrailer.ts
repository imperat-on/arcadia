"use client"

import { useEffect, useState } from "react"

/** Converte os bytes base64 do IPC num blob URL reproduzível. */
export function blobDeTrailer(dados: { data?: string; mime?: string }): string | null {
  if (!dados.data) return null
  const binario = atob(dados.data)
  const bytes = new Uint8Array(binario.length)
  for (let i = 0; i < binario.length; i++) bytes[i] = binario.charCodeAt(i)
  return URL.createObjectURL(new Blob([bytes], { type: dados.mime || "video/mp4" }))
}

/**
 * Carrega o trailer baixado de um jogo como blob URL.
 *
 * O `<video>` do WebKitGTK recusa `file://` a partir de `tauri://` (erro 4),
 * então o console e o fundo do Big Picture não podem usar o caminho que
 * `trailerPath` devolve. O blob é revogado quando o jogo muda.
 */
export function useLocalTrailer(id: string | null | undefined, ativo = true) {
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    setUrl(null)
    if (!id || !ativo) return
    let vivo = true
    let criado: string | null = null

    window.launcherAPI
      ?.trailerLocalData?.(id)
      .then((dados) => {
        if (!vivo) return
        const blob = blobDeTrailer(dados || {})
        if (!blob) return
        criado = blob
        setUrl(blob)
      })
      .catch(() => {})

    return () => {
      vivo = false
      if (criado) URL.revokeObjectURL(criado)
    }
  }, [id, ativo])

  return url
}
