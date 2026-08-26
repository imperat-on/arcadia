"use client"

import { useEffect, useRef, useState } from "react"

// Cross-fade de duas camadas para o fundo do herói.
//
// Antes, a troca do jogo remontava o elemento de fundo (via `key={id}`): a
// imagem/vídeo velha sumia imediatamente e a nova nascia com `opacity: 0`,
// deixando o preto do container aparecer entre uma e outra — a "piscada".
//
// Aqui as duas camadas convivem por um instante: quando `id` muda, a camada
// "próxima" recebe o novo hero e sobe em opacidade por cima da atual. Só
// quando o fade termina a atual é substituída. A antiga fica visível o tempo
// todo, então o container preto nunca reaparece.

const FADE_MS = 500

function isVideo(url?: string | null): boolean {
  if (!url) return false
  const u = url.toLowerCase()
  return u.endsWith(".webm") || u.endsWith(".mp4") || u.endsWith(".mov")
}

interface HeroBackgroundProps {
  /** Modo de fundo preto (aba Notícias / Loja): apaga tudo. */
  preto?: boolean
  /** URL da imagem ou vídeo do herói do jogo selecionado. */
  hero?: string | null
  /** ID do jogo — usado para detectar troca (cada jogo tem seu hero). */
  id?: string | null
}

interface Camada {
  id: string | null
  hero: string | null
}

export function HeroBackground({ preto, hero, id }: HeroBackgroundProps) {
  const [atual, setAtual] = useState<Camada>({ id: id ?? null, hero: hero ?? null })
  const [proxima, setProxima] = useState<Camada | null>(null)
  const timer = useRef<number | undefined>(undefined)

  useEffect(() => {
    if ((id ?? null) === atual.id) return
    // Cancela um fade em curso: assume o alvo mais recente sem "engasgar".
    if (timer.current) window.clearTimeout(timer.current)
    setProxima({ id: id ?? null, hero: hero ?? null })
    timer.current = window.setTimeout(() => {
      setAtual({ id: id ?? null, hero: hero ?? null })
      setProxima(null)
      timer.current = undefined
    }, FADE_MS)
    return () => {
      if (timer.current) {
        window.clearTimeout(timer.current)
        timer.current = undefined
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id])

  // Preto (Notícias/Loja): tampa qualquer camada com um preto sólido.
  if (preto) {
    return <div className="absolute inset-0" style={{ background: "#000000" }} />
  }

  return (
    <>
      {/* Fallback: se nada foi montado ainda, um gradiente cobre o preto puro. */}
      <div
        className="absolute inset-0"
        style={{ background: "linear-gradient(135deg, #000000, #161619)" }}
      />
      {/* Camada atual: fica em opacidade cheia, sem fade (já está posicionada). */}
      {atual.hero && <CamadaBG key={`a-${atual.id}`} hero={atual.hero} />}
      {/* Camada próxima: entra por cima em fade, some quando o timer confirma. */}
      {proxima?.hero && <CamadaBG key={`p-${proxima.id}`} hero={proxima.hero} entrando />}
    </>
  )
}

function CamadaBG({ hero, entrando }: { hero: string; entrando?: boolean }) {
  const [montado, setMontado] = useState(false)
  useEffect(() => {
    // Um frame depois de montar, sobe de 0 → 1 e a transição CSS pega.
    const id = requestAnimationFrame(() => setMontado(true))
    return () => cancelAnimationFrame(id)
  }, [])
  const opacity = entrando ? (montado ? 1 : 0) : 1
  const transition = entrando ? `opacity ${FADE_MS}ms ease-out` : undefined
  if (isVideo(hero)) {
    return (
      <video
        className="retro-hero-background absolute inset-0 h-full w-full object-cover"
        src={hero}
        style={{ opacity, transition }}
        autoPlay
        loop
        muted
        playsInline
      />
    )
  }
  return (
    <div
      className="retro-hero-background absolute inset-0"
      style={{
        opacity,
        transition,
        backgroundImage: `url(${hero})`,
        backgroundSize: "cover",
        backgroundPosition: "top center",
      }}
    />
  )
}
