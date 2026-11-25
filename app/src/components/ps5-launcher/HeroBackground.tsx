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
  const [incoming, setIncoming] = useState<string | null>(null)
  const generation = useRef(0)
  useEffect(() => {
    const ticket = ++generation.current
    let timer: ReturnType<typeof setTimeout> | undefined
    setIncoming(null)
    const commit = () => {
      if (ticket !== generation.current) return
      setIncoming(hero || null)
      timer = setTimeout(() => {
        if (ticket !== generation.current) return
        setCurrent(hero || null)
        setIncoming(null)
      }, window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : FADE_MS)
    }
    if (!hero || isVideo(hero)) commit()
    else {
      const image = new Image()
      image.onload = commit
      image.onerror = () => { if (ticket === generation.current) { setCurrent(null); setIncoming(null) } }
      image.src = hero
    }
    return () => { ++generation.current; if (timer) clearTimeout(timer) }
  }, [hero, id])
  return <div className="absolute inset-0" aria-hidden="true" style={{ background: "#000" }}>
    {!preto && current && <Layer source={current} />}
    {!preto && incoming && <Layer key={incoming} source={incoming} entering />}
  </div>
}
function Layer({ source, entering = false }: { source: string; entering?: boolean }) {
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
  const style = { animation: entering ? `console-art-fade ${FADE_MS}ms ease both` : undefined }
  return isVideo(source)
    ? <video ref={video} className="retro-hero-background absolute inset-0 h-full w-full object-cover" src={source} style={style} loop muted playsInline />
    : <img className="retro-hero-background absolute inset-0 h-full w-full object-cover" src={source} alt="" style={style} />
}
