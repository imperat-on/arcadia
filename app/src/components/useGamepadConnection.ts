"use client"

import { useEffect, useRef, useState } from "react"

// Rastreia controles conectados via eventos nativos (gamepadconnected /
// gamepaddisconnected). O Gamepad API do Chromium só relata mudanças por
// esses eventos — só polling em getGamepads() nunca dispara um "não está mais
// aqui" a tempo, e é o que faz falta para notificações de conexão/queda.

export type TipoControle = "xbox" | "playstation" | "switch" | "generico"

export interface ControleConectado {
  index: number
  id: string
  tipo: TipoControle
  connected: boolean
}

/** Detecta a família do controle pela string livre do gamepad.id (varia por
 * navegador/driver, então casa por palavras-chave em vez de VID/PID exatos). */
export function detectarTipoControle(id: string): TipoControle {
  const s = id.toLowerCase()
  if (/dualsense|dualshock|playstation|ps[345]|sony/.test(s)) return "playstation"
  if (/switch|joy-?con|nintendo|pro controller/.test(s)) return "switch"
  if (/xbox|xinput|microsoft/.test(s)) return "xbox"
  return "generico"
}

function listar(): ControleConectado[] {
  if (typeof navigator === "undefined" || !navigator.getGamepads) return []
  const out: ControleConectado[] = []
  for (const gp of navigator.getGamepads()) {
    if (!gp) continue
    out.push({ index: gp.index, id: gp.id, tipo: detectarTipoControle(gp.id), connected: gp.connected })
  }
  return out
}

/**
 * Lista de controles conectados, atualizada nos eventos de conexão/queda, e
 * callbacks opcionais para notificar quem quiser reagir (toast, etc.).
 *
 * getGamepads() só popula depois de QUALQUER input no controle em alguns
 * browsers — por isso o efeito também escuta o primeiro frame de animação
 * após montar, para pegar controles já conectados antes do launcher abrir.
 */
export function useGamepadConnection(
  onConnected?: (c: ControleConectado) => void,
  onDisconnected?: (c: ControleConectado) => void,
) {
  const [controles, setControles] = useState<ControleConectado[]>(() => listar())
  const onConnectedRef = useRef(onConnected)
  onConnectedRef.current = onConnected
  const onDisconnectedRef = useRef(onDisconnected)
  onDisconnectedRef.current = onDisconnected

  useEffect(() => {
    if (typeof window === "undefined") return
    const conectar = (e: GamepadEvent) => {
      const c: ControleConectado = {
        index: e.gamepad.index,
        id: e.gamepad.id,
        tipo: detectarTipoControle(e.gamepad.id),
        connected: true,
      }
      setControles(listar())
      onConnectedRef.current?.(c)
    }
    const desconectar = (e: GamepadEvent) => {
      const c: ControleConectado = {
        index: e.gamepad.index,
        id: e.gamepad.id,
        tipo: detectarTipoControle(e.gamepad.id),
        connected: false,
      }
      setControles(listar())
      onDisconnectedRef.current?.(c)
    }
    window.addEventListener("gamepadconnected", conectar)
    window.addEventListener("gamepaddisconnected", desconectar)
    // Controle já plugado antes do mount (ex.: reabrir o launcher) não dispara
    // "gamepadconnected" — sincroniza a lista uma vez após o primeiro frame.
    const raf = requestAnimationFrame(() => setControles(listar()))
    return () => {
      window.removeEventListener("gamepadconnected", conectar)
      window.removeEventListener("gamepaddisconnected", desconectar)
      cancelAnimationFrame(raf)
    }
  }, [])

  const primeiro = controles[0] ?? null

  return { controles, primeiro }
}
