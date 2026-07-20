"use client"

import { useEffect, useMemo, useRef } from "react"
import { StoreTile } from "./StoreTile"
import type { JogoLinha } from "./types"

interface StoreCategoriaProps {
  /** Cada índice é uma página carregada; concatenadas viram a lista visível. */
  paginas: JogoLinha[][]
  carregando: boolean
  temMais: boolean
  onFocar: (j: JogoLinha) => void
  onAbrir: (j: JogoLinha) => void
  /** Chamado pelo IntersectionObserver quando o sentinel aparece na viewport. */
  onPedirMais: () => void
}

// Uma categoria aberta: grade densa dos mesmos ladrilhos da vitrine, com
// rolagem infinita. Os ladrilhos são idênticos aos do trilho de propósito —
// entrar numa categoria deve parecer o mesmo lugar, só que maior.
export function StoreCategoria({
  paginas,
  carregando,
  temMais,
  onFocar,
  onAbrir,
  onPedirMais,
}: StoreCategoriaProps) {
  const todos = useMemo(() => paginas.flat(), [paginas])

  // A trava anti-duplo-disparo vive no pai (StoreConsole); aqui só sinalizamos
  // a intenção. A margem começa a pedir antes de bater no fim, para a rolagem
  // não empacar enquanto a resposta chega.
  const sentinel = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    const el = sentinel.current
    if (!el) return
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) onPedirMais()
      },
      { rootMargin: "600px 0px 600px 0px" },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [onPedirMais])

  if (carregando && !todos.length) {
    return (
      <div className="grid px-12 pb-12" style={{ gridTemplateColumns: "repeat(auto-fill, 176px)", gap: 20 }}>
        {Array.from({ length: 21 }).map((_, i) => (
          <div
            key={`sk${i}`}
            className="animate-pulse rounded-lg bg-white/[0.04]"
            style={{ width: 176, aspectRatio: "3 / 4" }}
          />
        ))}
      </div>
    )
  }

  if (!todos.length) {
    return <p className="px-12 py-16 text-white/35">Nada por aqui.</p>
  }

  return (
    <>
      <div className="grid px-12 pb-6" style={{ gridTemplateColumns: "repeat(auto-fill, 176px)", gap: 20 }}>
        {todos.map((j, i) => (
          <StoreTile key={j.appid} jogo={j} indice={i} onFocar={onFocar} onAbrir={onAbrir} />
        ))}
      </div>
      {temMais && <div ref={sentinel} className="h-8" />}
    </>
  )
}
