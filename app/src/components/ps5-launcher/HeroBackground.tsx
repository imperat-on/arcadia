"use client"

import { useEffect, useRef, useState } from "react"

const FADE_MS = 500
function isVideo(url: string): boolean {
  return /\.(webm|mp4|mov)(?:[?#]|$)/i.test(url)
}
interface HeroBackgroundProps {
  preto?: boolean
  hero?: string | null
  id?: string | null
}

// Retain the decoded current image until its replacement is actually ready.
// A selection generation prevents late loads from replacing a newer selection.
export function HeroBackground({ preto, hero, id }: HeroBackgroundProps) {
  const [current, setCurrent] = useState<string | null>(null)
  const [incoming, setIncoming] = useState<{ source: string; ticket: number; ready: boolean } | null>(null)
  const generation = useRef(0)
  useEffect(() => {
    const ticket = ++generation.current
    setIncoming(null)
    const commit = () => {
      if (ticket !== generation.current || !hero) return
      setIncoming({ source: hero, ticket, ready: !isVideo(hero) })
    }
    if (!hero) setCurrent(null)
    else if (isVideo(hero)) commit()
    else {
      const image = new Image()
      image.onload = commit
      image.src = hero
    }
    return () => { ++generation.current }
  }, [hero, id])
  useEffect(() => {
    if (!incoming || !incoming.ready || incoming.source !== hero || preto) return
    const timer = setTimeout(() => {
      if (incoming.ticket !== generation.current) return
      setCurrent(incoming.source)
      setIncoming(null)
    }, window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : FADE_MS)
    return () => clearTimeout(timer)
  }, [incoming, hero, preto])

  const candidate = incoming && incoming.source === hero && incoming.source !== current ? incoming : null
  return <div className="absolute inset-0" aria-hidden="true" style={{ background: "#000" }}>
    {!preto && current && <Layer key={current} source={current} />}
    {!preto && candidate && <Layer key={candidate.source} source={candidate.source}
      entering={candidate.ready} waiting={!candidate.ready}
      onReady={() => setIncoming((layer) => layer?.ticket === candidate.ticket && !layer.ready ? { ...layer, ready: true } : layer)}
      onError={() => setIncoming((layer) => layer?.ticket === candidate.ticket ? null : layer)} />}
  </div>
}
function Layer({ source, entering, waiting, onReady, onError }: {
  source: string; entering?: boolean; waiting?: boolean; onReady?: () => void; onError?: () => void
}) {
  const video = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    if (!video.current) return
    const update = () => {
      if (document.hidden || !document.hasFocus() || window.matchMedia("(prefers-reduced-motion: reduce)").matches) video.current?.pause()
      else void video.current?.play().catch(() => {})
    }
    update()
    window.addEventListener("focus", update)
    window.addEventListener("blur", update)
    document.addEventListener("visibilitychange", update)
    return () => { window.removeEventListener("focus", update); window.removeEventListener("blur", update); document.removeEventListener("visibilitychange", update) }
  }, [source])
  const style = { opacity: waiting ? 0 : undefined }
  const className = `retro-hero-background absolute inset-0 h-full w-full object-cover${entering ? " arcadia-hero-enter" : ""}`
  return isVideo(source)
    ? <video ref={video} className={className} src={source} style={style} onLoadedData={onReady} onError={onError} preload="auto" loop muted playsInline />
    : <>
        <img className={className} src={source} alt="" style={style} />
        <img className={`arcadia-hero-ambient${entering ? " arcadia-hero-ambient-enter" : ""}`} src={source} alt="" draggable={false} />
      </>
}
