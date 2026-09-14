import { useEffect, useState } from "react"

// Horas da conta Steam vinculada, para as capas e o perfil.
//
// Uma leitura serve para todos os tiles: o cache é do processo (módulo) e a
// chamada é única para quem pedir ao mesmo tempo. Uma chamada por capa faria
// dezenas de leituras do mesmo arquivo.
//
// IMPORTANTE: falha NÃO é cacheada. Se a primeira tentativa acontecer antes do IPC
// estar pronto (ou voltar vazia), a próxima montagem tenta de novo — guardar a
// promessa falhada deixava as capas em "0min" para sempre.

let cache: Record<string, number> = {}
let emVoo: Promise<Record<string, number>> | null = null

/**
 * A chave das horas na Steam é o NÚMERO do appid. O id do jogo na biblioteca do
 * Arcadia vem como `steam:990080` (e há ids de outras lojas, tipo `epic:...`), então
 * comparar a string crua nunca casa — era por isso que as capas mostravam 0min com
 * as horas certas chegando (44 jogos lidos).
 */
export function chaveAppid(appid: unknown): string {
  const s = String(appid ?? "").trim()
  const m = /(\d{2,})/.exec(s)
  return m ? m[1] : s
}

export async function carregarSteamHoras(): Promise<Record<string, number>> {
  if (emVoo) return emVoo
  emVoo = (async () => {
    try {
      const r = await window.launcherAPI?.steamHorasTodas()
      if (r?.ok && r.horas && Object.keys(r.horas).length) {
        cache = { ...r.horas }
        return cache
      }
    } catch {}
    emVoo = null // sem cache de falha: tenta de novo na próxima vez
    return cache
  })()
  return emVoo
}

/** Minutos da Steam para um appid/id de jogo, ou 0. */
export function minutosDaSteam(
  steam: Record<string, number> | null | undefined,
  appid: unknown,
): number {
  const mapa = steam || {}
  const exato = Number(mapa[chaveAppid(appid)] || 0)
  if (exato > 0) return exato
  return 0
}

export function useSteamHoras(): Record<string, number> {
  const [horas, setHoras] = useState<Record<string, number>>(cache)
  useEffect(() => {
    let vivo = true
    void carregarSteamHoras().then((h) => {
      if (vivo) setHoras({ ...h })
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
  const daSteam = minutosDaSteam(steam, appid)
  return daSteam > 0 ? daSteam : Number(minutosArcadia) || 0
}
