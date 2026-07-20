"use client"

import { useRef, type ReactNode } from "react"

interface ImgCascataProps {
  /** URLs a tentar em ordem. */
  fontes: string[]
  alt?: string
  /** Exibido quando todas as fontes falham. */
  fallback?: ReactNode
  /** Classes adicionais além do preenchimento absoluto padrão. */
  className?: string
  loading?: "eager" | "lazy"
  /** Disparado uma vez quando TODAS as fontes falham. */
  onEsgotar?: () => void
}

/**
 * Imagem com fallback em cadeia: se a primeira URL falhar, tenta a próxima
 * via `onError`. Quando esgota as fontes, mostra o fallback (se houver).
 * O container preenche o pai; use dentro de um elemento com dimensão definida.
 */
export function ImgCascata({
  fontes,
  alt = "",
  fallback,
  className = "",
  loading = "lazy",
  onEsgotar,
}: ImgCascataProps) {
  const ref = useRef<HTMLImageElement | null>(null)
  const idx = useRef(0)
  const wrapper = useRef<HTMLDivElement | null>(null)
  if (!fontes.length) {
    return fallback ? (
      <div ref={wrapper} className="relative h-full w-full">
        {fallback}
      </div>
    ) : null
  }
  return (
    <div ref={wrapper} className="relative h-full w-full">
      <img
        ref={ref}
        src={fontes[0]}
        alt={alt}
        draggable={false}
        loading={loading}
        className={`absolute inset-0 h-full w-full object-cover ${className}`}
        onError={() => {
          idx.current += 1
          if (ref.current && idx.current < fontes.length) {
            ref.current.src = fontes[idx.current]
          } else {
            if (fallback && wrapper.current) ref.current?.remove()
            onEsgotar?.()
          }
        }}
      />
      {fallback && (
        <div className="absolute inset-0 -z-10 flex h-full w-full items-center justify-center">
          {fallback}
        </div>
      )}
    </div>
  )
}
