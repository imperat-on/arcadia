"use client"

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react"
import type { FullscreenThemePayload } from "../../themes/fullscreen/types"

export type FullscreenThemeState = {
  activeId: string
  pendingId: string | null
  payload: FullscreenThemePayload | null
  ready: boolean
  error: string | null
}

type FullscreenThemeContextValue = {
  state: FullscreenThemeState
  activate: (id: string) => Promise<boolean>
  confirmReady: () => Promise<boolean>
  rollback: () => Promise<void>
  recover: () => Promise<void>
}

const FullscreenThemeContext = createContext<FullscreenThemeContextValue | null>(null)

export function FullscreenThemeProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<FullscreenThemeState>({
    activeId: "arcadia.default",
    pendingId: null,
    payload: null,
    ready: true,
    error: null,
  })
  const stateRef = useRef(state)
  useEffect(() => { stateRef.current = state }, [state])

  // Carrega o tema ativo ao montar
  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const id = await window.launcherAPI?.fullscreenThemesGetActiveId?.()
        if (cancelled || typeof id !== "string") return
        const payload = await window.launcherAPI?.fullscreenThemesGetPayload?.(id)
        if (cancelled) return
        setState({ activeId: id, pendingId: null, payload: payload || null, ready: true, error: null })
      } catch {
        if (!cancelled) {
          setState({ activeId: "arcadia.default", pendingId: null, payload: null, ready: true, error: null })
        }
      }
    }
    load()
    return () => { cancelled = true }
  }, [])

  // Escuta mudanças de tema vindas do main
  useEffect(() => {
    const off = window.launcherAPI?.onFullscreenThemesChanged?.((data: { reason?: string; activeId?: string; pendingId?: string | null }) => {
      if (data?.reason === "confirmed" && typeof data.activeId === "string") {
        // Tema confirmado: atualiza activeId e limpa pending
        setState((prev) => ({ ...prev, activeId: data.activeId!, pendingId: null, ready: true, error: null }))
      } else if (data?.reason === "rollback") {
        // Rollback: volta ao tema anterior
        setState((prev) => ({ ...prev, activeId: data.activeId || "arcadia.default", pendingId: null, ready: true, error: null }))
      } else if (data?.reason === "recover") {
        setState((prev) => ({ ...prev, activeId: data.activeId || "arcadia.default", pendingId: null, ready: true, error: null }))
      }
    })
    return () => { off?.() }
  }, [])

  // Ativa um tema: solicita ativação, carrega payload, aplica CSS
  const activate = useCallback(async (id: string): Promise<boolean> => {
    try {
      // 1. Solicita ativação (define pendingId no main)
      const result = await window.launcherAPI?.fullscreenThemesActivate?.(id)
      if (!result?.ok) {
        setState((prev) => ({ ...prev, error: result?.error || "erro_desconhecido" }))
        return false
      }

      // 2. Carrega payload do tema
      const payload = await window.launcherAPI?.fullscreenThemesGetPayload?.(id)
      if (!payload) {
        await window.launcherAPI?.fullscreenThemesRollbackPending?.()
        setState((prev) => ({ ...prev, error: "payload_nao_carregado" }))
        return false
      }

      // 3. Atualiza estado com pendingId e payload
      setState((prev) => ({ ...prev, pendingId: id, payload, ready: false, error: null }))
      return true
    } catch (err) {
      await window.launcherAPI?.fullscreenThemesRollbackPending?.()
      setState((prev) => ({ ...prev, error: String(err) }))
      return false
    }
  }, [])

  // Confirma que o tema está saudável (chamado após CSS aplicado e render estável)
  const confirmReady = useCallback(async (): Promise<boolean> => {
    const pendingId = stateRef.current.pendingId
    if (!pendingId) return false

    try {
      const result = await window.launcherAPI?.fullscreenThemesConfirmReady?.(pendingId)
      if (result?.ok) {
        setState((prev) => ({ ...prev, activeId: pendingId, pendingId: null, ready: true, error: null }))
        return true
      }
      // Confirmação falhou: rollback
      await window.launcherAPI?.fullscreenThemesRollbackPending?.()
      const id = await window.launcherAPI?.fullscreenThemesGetActiveId?.()
      setState({ activeId: typeof id === "string" ? id : "arcadia.default", pendingId: null, payload: null, ready: true, error: result?.error || "confirmacao_falhou" })
      return false
    } catch {
      await window.launcherAPI?.fullscreenThemesRollbackPending?.()
      setState((prev) => ({ ...prev, pendingId: null, ready: true, error: "confirmacao_erro" }))
      return false
    }
  }, [])

  const rollback = useCallback(async () => {
    await window.launcherAPI?.fullscreenThemesRollbackPending?.()
    const id = await window.launcherAPI?.fullscreenThemesGetActiveId?.()
    setState({ activeId: typeof id === "string" ? id : "arcadia.default", pendingId: null, payload: null, ready: true, error: null })
  }, [])

  const recover = useCallback(async () => {
    const result = await window.launcherAPI?.fullscreenThemesRecover?.()
    setState({ activeId: result?.id || "arcadia.default", pendingId: null, payload: null, ready: true, error: null })
  }, [])

  return (
    <FullscreenThemeContext.Provider value={{ state, activate, confirmReady, rollback, recover }}>
      {children}
    </FullscreenThemeContext.Provider>
  )
}

export function useFullscreenTheme() {
  const context = useContext(FullscreenThemeContext)
  if (!context) throw new Error("useFullscreenTheme precisa estar dentro de FullscreenThemeProvider")
  return context
}
