"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { Game } from "./ps5-launcher/types"

// Ações da loja — Baixar, Add, Remover, reiniciar a Steam — compartilhadas
// entre o modo desktop e o modo console. Antes moravam só no StoreView.tsx do
// desktop, e cada correção feita ali (guarda de pedido em voo, busy liberado
// em finally, cache do manifesto) teria de ser refeita na loja do console.
// Com o hook, uma correção vale para os dois.

export type ManifestInfo = {
  depots: { depotId: string; manifestId: string; key: string }[]
  token?: string
  dlcs?: string[]
  fonte?: string
}

export type JogoLoja = { appid: string; title: string }

export type Biblioteca = { path: string; steamDir: string; free: number }

export type EscolhaDisco = {
  jogo: JogoLoja
  info: ManifestInfo
  libs: Biblioteca[]
}

export function useStoreActions(games: Game[] = []) {
  const [jaAdicionados, setJaAdicionados] = useState<Set<string>>(new Set())
  const [escolhendo, setEscolhendo] = useState<EscolhaDisco | null>(null)
  const [busy, setBusy] = useState("")
  const [toast, setToast] = useState("")

  // O toast some sozinho; sem isso ele ficaria na tela até a próxima ação.
  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(""), 5000)
    return () => clearTimeout(t)
  }, [toast])

  // Registro da SLSsteam. Relido a cada mudança de biblioteca para os cards
  // refletirem o estado real mesmo quando a alteração vem de outra tela ou de
  // um download que terminou.
  useEffect(() => {
    const status = () =>
      window.launcherAPI?.storeStatus().then((s) => {
        setJaAdicionados(new Set(s?.adicionados || []))
      })
    status()
    return window.launcherAPI?.onLibraryChanged(() => status())
  }, [])

  // Jogos que não devem oferecer Baixar/Add: já adicionados à Steam ou já
  // presentes na biblioteca do Arcadia.
  const bloqueados = new Set([
    ...jaAdicionados,
    ...games.map((g) => String(g.id).replace(/^steam:/, "")),
  ])

  // Buscar o manifesto passa por vários provedores e pode levar dezenas de
  // segundos. Guardamos por appid para que Add logo depois de Baixar no mesmo
  // jogo seja instantâneo em vez de repetir a busca inteira.
  const infoCache = useRef(new Map<string, ManifestInfo>())
  const obterInfo = useCallback(async (appid: string) => {
    const guardado = infoCache.current.get(appid)
    if (guardado) return { ok: true, ...guardado }
    const info = await window.launcherAPI?.storeInstallInfo(appid)
    if (info?.ok && info.depots?.length) infoCache.current.set(appid, info as ManifestInfo)
    return info
  }, [])

  // Cada ação recebe um número. Se o usuário fizer outra coisa no meio, a
  // anterior é abandonada: sem isto, um "Baixar" lento resolvia depois de o
  // usuário fechar o diálogo e clicar em Add, reabrindo o popup de disco por
  // cima da confirmação.
  const pedido = useRef(0)

  // `busy` desabilita os botões, então nunca pode ficar preso: toda saída —
  // inclusive pedido abandonado e exceção — libera no finally.
  const baixar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setBusy(jogo.appid)
      try {
        const info = await obterInfo(jogo.appid)
        if (meu !== pedido.current) return
        if (!info?.ok || !info.depots?.length) {
          setToast(info?.error || "Sem manifesto para este jogo.")
          return
        }
        const libs = ((await window.launcherAPI?.storeLibraries()) || []) as Biblioteca[]
        if (meu !== pedido.current) return
        if (!libs.length) {
          setToast("Nenhuma biblioteca Steam encontrada.")
          return
        }
        // O diálogo aparece sempre, mesmo com uma biblioteca só: um download de
        // vários GB não deve começar sem confirmação.
        setEscolhendo({ jogo, info: info as ManifestInfo, libs })
      } catch (e) {
        setToast(`Falha ao preparar o download: ${e}`)
      } finally {
        if (meu === pedido.current) setBusy("")
      }
    },
    [obterInfo],
  )

  const confirmarBaixar = useCallback(
    async (jogo: JogoLoja, info: ManifestInfo, steamDir?: string) => {
      setEscolhendo(null)
      setBusy(jogo.appid)
      try {
        const r = await window.launcherAPI?.storeInstall({
          appid: jogo.appid,
          title: jogo.title,
          cover: `https://cdn.akamai.steamstatic.com/steam/apps/${jogo.appid}/header.jpg`,
          installdir: jogo.title.replace(/[^A-Za-z0-9]/g, ""),
          depots: info.depots,
          token: info.token,
          dlcs: info.dlcs,
          steamDir,
        })
        const via = info.fonte ? ` (via ${info.fonte})` : ""
        setToast(r?.ok ? `"${jogo.title}" entrou na fila de downloads${via}.` : r?.error || "Falha ao enfileirar")
      } catch (e) {
        setToast(`Falha ao enfileirar: ${e}`)
      } finally {
        setBusy("")
      }
    },
    [],
  )

  // Registra o jogo na Steam sem baixar — a própria Steam baixa depois pela
  // CDN dela.
  const adicionar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setEscolhendo(null) // um diálogo aberto taparia a confirmação
      setBusy(jogo.appid)
      try {
        const info = await obterInfo(jogo.appid)
        if (meu !== pedido.current) return
        if (!info?.ok || !info.depots?.length) {
          setToast(info?.error || "Sem manifesto para este jogo.")
          return
        }
        const r = await window.launcherAPI?.storeAddToSteam({
          appid: jogo.appid,
          token: info.token,
          dlcs: info.dlcs,
        })
        if (meu !== pedido.current) return
        if (r?.ok) setJaAdicionados((prev) => new Set(prev).add(jogo.appid))
        setToast(
          r?.ok
            ? `"${jogo.title}" adicionado! Reinicie a Steam para baixar por lá.`
            : r?.error || "Falha ao adicionar",
        )
      } catch (e) {
        setToast(`Falha ao adicionar: ${e}`)
      } finally {
        if (meu === pedido.current) setBusy("")
      }
    },
    [obterInfo],
  )

  const remover = useCallback(async (jogo: JogoLoja) => {
    setBusy(jogo.appid)
    try {
      // Remove de tudo: pasta + appmanifest (downloads) + registro SLSsteam.
      const r = await window.launcherAPI?.storeRemoveDownloaded(jogo.appid)
      if (r?.ok) {
        setJaAdicionados((prev) => {
          const n = new Set(prev)
          n.delete(jogo.appid)
          return n
        })
      }
      setToast(r?.ok ? `"${jogo.title}" removido da Steam.` : r?.error || "Falha ao remover")
    } catch (e) {
      setToast(`Falha ao remover: ${e}`)
    } finally {
      setBusy("")
    }
  }, [])

  const reiniciarSteam = useCallback(async () => {
    const r = await window.launcherAPI?.slssteamLaunch()
    setToast(r?.ok ? "Reiniciando a Steam com a SLSsteam…" : r?.error || "Falha ao abrir a Steam")
  }, [])

  return {
    bloqueados,
    jaAdicionados,
    escolhendo,
    setEscolhendo,
    busy,
    toast,
    setToast,
    baixar,
    confirmarBaixar,
    adicionar,
    remover,
    reiniciarSteam,
  }
}
