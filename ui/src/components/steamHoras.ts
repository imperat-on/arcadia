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
const ouvintes = new Set<(h: Record<string, number>) => void>()

function avisar() {
  for (const f of ouvintes) {
    try {
      f({ ...cache })
    } catch {}
  }
}

/** Nova leitura, forçada: quem está na tela recebe o número fresquinho. */
export async function recarregarSteamHoras(): Promise<Record<string, number>> {
  emVoo = null
  const h = await carregarSteamHoras()
  avisar()
  return h
}

// A Steam grava o playtime quando o jogo FECHA. Ao voltar o foco para a janela
// (você acabou de sair do jogo), relemos — com trava de tempo para o `focus` não
// virar uma leitura por clique. Sem isso as horas só atualizavam reiniciando.
let ultimaLeitura = 0
const INTERVALO_FOCO = 30_000
if (typeof window !== "undefined" && typeof window.addEventListener === "function") {
  window.addEventListener("focus", () => {
    const agora = Date.now()
    if (agora - ultimaLeitura < INTERVALO_FOCO) return
    ultimaLeitura = agora
    void recarregarSteamHoras()
  })
}

/**
 * A chave das horas na Steam é o NÚMERO do appid. O jogo na biblioteca do Arcadia
 * carrega esse número dentro do `id` (`steam:990080`), e o campo `appid` costuma
 * nem existir fora das linhas da loja — procurar `game.appid` dava undefined, a
 * chave saía vazia e o selo ficava em 0min com as horas certas na mão.
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

/** Minutos da Steam para um id/appid (ou vários candidatos), ou 0. */
export function minutosDaSteam(
  steam: Record<string, number> | null | undefined,
  fontes: unknown,
): number {
  const mapa = steam || {}
  const lista = Array.isArray(fontes) ? fontes : [fontes]
  for (const f of lista) {
    if (f == null || f === "") continue
    const min = Number(mapa[chaveAppid(f)] || 0)
    if (min > 0) return min
  }
  return 0
}

export function useSteamHoras(): Record<string, number> {
  const [horas, setHoras] = useState<Record<string, number>>(cache)
  useEffect(() => {
    let vivo = true
    const ouvir = (h: Record<string, number>) => {
      if (vivo) setHoras(h)
    }
    ouvintes.add(ouvir)
    void carregarSteamHoras().then((h) => {
      if (vivo) setHoras({ ...h })
    })
    return () => {
      vivo = false
      ouvintes.delete(ouvir)
    }
  }, [])
  return horas
}

/**
 * Horas do jogo, com uma fonte só de verdade.
 *
 * O número da Steam é o TOTAL da conta e **já inclui** o tempo das sessões que o
 * Arcadia lançou (o jogo abre pela Steam), então somar os dois seria contagem dupla.
 * A escolha é o MAIOR dos dois, que também resolve dois casos reais:
 *   - jogo crackeado/emulado: a Steam não conta nada e vale o tempo do Arcadia;
 *   - jogo que a Steam tem com um resíduo (segundos/minutos de um teste) e o Arcadia
 *     mediu horas: vale o do Arcadia, em vez de aparecer "3min" no perfil.
 *
 * `fontes` aceita o id do jogo, o appid, ou os dois: a biblioteca usa `id`
 * (`steam:990080`) e a loja usa `appid` (`990080`).
 */
export function horasCombinadas(
  steam: Record<string, number> | null | undefined,
  fontes: unknown,
  minutosArcadia?: number,
): number {
  const daSteam = minutosDaSteam(steam, fontes)
  const arcadia = Number(minutosArcadia) || 0
  return Math.max(daSteam, arcadia)
}
