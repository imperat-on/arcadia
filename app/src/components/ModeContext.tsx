"use client"

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react"

export type LauncherMode = "console" | "desktop"

type ModeContextValue = {
  mode: LauncherMode
  isConsole: boolean
  setMode: (mode: LauncherMode) => Promise<void>
}

const ModeContext = createContext<ModeContextValue | null>(null)

function initialMode(): LauncherMode {
  return window.launcherMode === "console" ? "console" : "desktop"
}

export function ModeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<LauncherMode>(initialMode)
  const modeRef = useRef(mode)

  useEffect(() => {
    modeRef.current = mode
  }, [mode])

  const setMode = useCallback(async (next: LauncherMode) => {
    if (next === modeRef.current) return
    const result = await window.launcherAPI?.setLauncherMode(next)
    if (result?.ok === false) throw new Error(result.error || "Falha ao trocar o modo")
    modeRef.current = next
    setModeState(next)
  }, [])

  return <ModeContext.Provider value={{ mode, isConsole: mode === "console", setMode }}>{children}</ModeContext.Provider>
}

export function useMode() {
  const context = useContext(ModeContext)
  if (!context) throw new Error("useMode precisa estar dentro de ModeProvider")
  return context
}
