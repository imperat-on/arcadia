import { useEffect, useState } from "react"

// Horas da conta Steam vinculada, para as capas e o perfil.
//
// Uma leitura serve para todos os tiles: o cache é do processo (módulo) e a
// chamada é única, compartilhada entre quem pedir ao mesmo tempo. Uma chamada por
// capa faria dezenas de leituras do mesmo arquivo.

let cache: Record<string, number> = {}
let emVoo: Promise<void> | null = null

export function carregarSteamHoras(): Promise<void> {
  if (emVoo) return emVoo
  emVoo = (async () => {
    try {
      const r = await window.launcherAPI?.steamHorasTodas()
      if (r?.ok && r.horas) cache = { ...r.horas }
    } catch {}
  })()
  return emVoo
}

export function useSteamHoras(): Record<string, number> {
  const [horas, setHoras] = useState<Record<string, number>>(cache)
  useEffect(() => {
    let vivo = true
    void carregarSteamHoras().then(() => {
      if (vivo) setHoras({ ...cache })
    })
    return () => {
      vivo = false
    }
  }, [])
  return horas
}

/**
 * Horas do jogo, uma fonte só de verdade:
 *   - a conta Steam manda quando tem o número (é o TOTAL, e já inclui o tempo das
 *     sessões que o Arcadia lançou — o jogo abre pela Steam);
 *   - o tempo medido pelo Arcadia entra só quando a Steam não conta o jogo
 *     (crackeado/emulador). Somar os dois seria contagem dupla.
 */
export function horasCombinadas(
  steam: Record<string, number> | null | undefined,
  appid: unknown,
  minutosArcadia?: number,
): number {
  const daSteam = Number((steam || {})[String(appid)] || 0)
  return daSteam > 0 ? daSteam : Number(minutosArcadia) || 0
}
