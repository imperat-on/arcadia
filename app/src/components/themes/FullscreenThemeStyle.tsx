"use client"

import { useEffect, useRef } from "react"
import { useFullscreenTheme } from "./FullscreenThemeContext"

const STYLE_ID = "arcadia-fullscreen-theme"

export function FullscreenThemeStyle() {
  const { state, confirmReady } = useFullscreenTheme()
  const styleRef = useRef<HTMLStyleElement | null>(null)
  const confirmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Cria/remove style tag
  useEffect(() => {
    let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null
    if (!el) {
      el = document.createElement("style")
      el.id = STYLE_ID
      document.head.appendChild(el)
    }
    styleRef.current = el

    return () => {
      const existing = document.getElementById(STYLE_ID)
      if (existing) existing.remove()
      styleRef.current = null
    }
  }, [])

  // Data attributes na raiz
  useEffect(() => {
    const root = document.documentElement
    root.setAttribute("data-arcadia-mode", "console")
    root.setAttribute("data-fullscreen-theme", state.activeId)
    root.setAttribute("data-theme-api", "1")
    root.setAttribute("data-theme-ready", state.ready ? "true" : "false")
    if (state.pendingId) {
      root.setAttribute("data-theme-pending", state.pendingId)
    } else {
      root.removeAttribute("data-theme-pending")
    }

    return () => {
      root.removeAttribute("data-arcadia-mode")
      root.removeAttribute("data-fullscreen-theme")
      root.removeAttribute("data-theme-api")
      root.removeAttribute("data-theme-ready")
      root.removeAttribute("data-theme-pending")
    }
  }, [state.activeId, state.ready, state.pendingId])

  // Aplica CSS do payload quando disponível
  useEffect(() => {
    const el = styleRef.current
    if (!el) return

    // Usa o CSS do payload (já normalizado pelo main)
    const css = state.payload?.css || ""
    el.textContent = css

    // Se há um pendingId e o payload foi carregado, agenda confirmação
    // após o CSS ser aplicado e o render estabilizar
    if (state.pendingId && state.payload && !state.ready) {
      if (confirmTimerRef.current) clearTimeout(confirmTimerRef.current)
      confirmTimerRef.current = setTimeout(async () => {
        await confirmReady()
      }, 300) // Aguarda 2 frames + margem
    }

    return () => {
      if (confirmTimerRef.current) {
        clearTimeout(confirmTimerRef.current)
        confirmTimerRef.current = null
      }
    }
  }, [state.payload, state.pendingId, state.ready, confirmReady])

  return null
}
