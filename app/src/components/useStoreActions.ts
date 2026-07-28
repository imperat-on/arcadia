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

export type JogoLoja = { appid: string; title: string; cover?: string; capa?: string; hero?: string; heroi?: string }

export type Biblioteca = { path: string; steamDir: string; free: number }

export type EscolhaDisco = {
  jogo: JogoLoja
  info: ManifestInfo
  libs: Biblioteca[]
}

export type OpcaoTorrent = {
  ref: string
  magnet: string // nome legado: pode ser magnet ou URL http direta
  fonte: string
  tituloFonte: string
  fileSize: string
  http?: boolean
}

export type EscolhaMetodo = {
  jogo: JogoLoja
  opcoes: OpcaoTorrent[]
}

export interface StoreActionsOpts {
  /**
   * Chamado quando nenhum provedor tem manifesto para o jogo.
   * Sem ele, a falta de manifesto vira apenas um toast — que é o que as duas
   * lojas querem. O modo console usa o gancho para oferecer a instalação pela
   * Steam como saída, em vez de deixar o jogo sem caminho nenhum.
   */
  onSemManifesto?: (jogo: JogoLoja, motivo: string) => void
}

export function useStoreActions(games: Game[] = [], opts: StoreActionsOpts = {}) {
  const [jaAdicionados, setJaAdicionados] = useState<Set<string>>(new Set())
  const [removidosLocal, setRemovidosLocal] = useState<Set<string>>(new Set())
  const [escolhendo, setEscolhendo] = useState<EscolhaDisco | null>(null)
  const [metodo, setMetodo] = useState<EscolhaMetodo | null>(null)
  const [busy, setBusy] = useState("")
  const [toast, setToast] = useState("")
  const [slsAtivo, setSlsAtivo] = useState(false)
  const [cheevoAtivo, setCheevoAtivo] = useState(false)
  const [fixesAtivo, setFixesAtivo] = useState(false)

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
        setSlsAtivo(Boolean(s?.slssteam))
        setCheevoAtivo(Boolean(s?.slscheevo))
        setFixesAtivo(Boolean(s?.luatools))
      })
    status()
    const offLib = window.launcherAPI?.onLibraryChanged(() => status())
    const offPlugins = window.launcherAPI?.onPluginsChanged?.(() => status())
    return () => { offLib?.(); offPlugins?.() }
  }, [])

  // Jogos que não devem oferecer Baixar/Add: já adicionados à Steam ou já
  // presentes na biblioteca do Arcadia. Ocultos (hidden) não contam: "Remover"
  // da loja só oculta o jogo indexado, e sem este filtro o botão continuava
  // "Na biblioteca" depois de removido.
  const bloqueados = new Set([
    ...jaAdicionados,
    ...games.filter((g) => !g.hidden).map((g) => String(g.id).replace(/^steam:/, "")),
  ].filter((appid) => !removidosLocal.has(appid)))

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

  // Em ref para o callback não entrar nas dependências de `baixar` — quem
  // passa uma função inline recriaria o useCallback a cada render.
  const semManifestoRef = useRef(opts.onSemManifesto)
  semManifestoRef.current = opts.onSemManifesto

  // Procura o jogo nas fontes JSON (aba Fontes) com magnet disponível.
  // Casa por título normalizado: o título da fonte é longo ("ELDEN RING:
  // Deluxe Edition, v1.12 + 9 DLCs...") e o da loja é curto — basta um
  // conter o outro. Devolve TODAS as opções com magnet: quem escolhe a
  // fonte é o usuário no diálogo. Sem fonte adicionada, o índice está
  // vazio e volta null.
  const acharTorrent = useCallback(async (title: string) => {
    try {
      // 50: jogos populares têm MUITAS releases (RDR2 tem 30+ entre as
      // fontes) — o diálogo lista todas as que têm link baixável.
      const r = await window.launcherAPI?.sourcesSearch?.(title, 50)
      const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "")
      const alvo = norm(title)
      if (!alvo) return null
      const cands = (r?.results || []).filter((g) => {
        const t = norm(g.title)
        return t.includes(alvo) || (t.length >= 8 && alvo.includes(t))
      })
      // Junta TODAS as fontes baixáveis: magnet (torrent) ou URL http direta
      // (o backend resolve/recusa hoster HTML). O diálogo lista tudo e quem
      // escolhe é o usuário.
      const opcoes: OpcaoTorrent[] = []
      for (const cand of cands) {
        const full = await window.launcherAPI?.sourcesGame?.(cand.ref)
        const uris = full?.game?.uris || (full?.game?.uri ? [full.game.uri] : [])
        const magnet = uris.find((u) => String(u).startsWith("magnet:"))
        const http = uris.find((u) => /^https?:\/\//.test(String(u)))
        const uri = magnet || http
        if (uri) opcoes.push({ ref: cand.ref, magnet: String(uri), fonte: cand.src, tituloFonte: cand.title, fileSize: cand.fileSize, http: !magnet })
      }
      return opcoes.length ? opcoes : null
    } catch {
      return null
    }
  }, [])

  // `busy` desabilita os botões, então nunca pode ficar preso: toda saída —
  // inclusive pedido abandonado e exceção — libera no finally.
  const baixarDepot = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setBusy(jogo.appid)
      try {
        const info = await obterInfo(jogo.appid)
        if (meu !== pedido.current) return
        if (!info?.ok || !info.depots?.length) {
          const motivo = info?.error || "Sem manifesto para este jogo."
          if (semManifestoRef.current) semManifestoRef.current(jogo, motivo)
          else setToast(motivo)
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

  // Entrada pública do botão Baixar: se o jogo existe numa fonte com magnet,
  // pergunta o método (Depot vs Torrent) antes de seguir. Sem fonte, cai
  // direto no fluxo Depot de sempre.
  const baixar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setBusy(jogo.appid)
      try {
        const opcoes = await acharTorrent(jogo.title)
        if (meu !== pedido.current) return
        if (opcoes) {
          setMetodo({ jogo, opcoes })
          return
        }
      } finally {
        if (meu === pedido.current) setBusy("")
      }
      await baixarDepot(jogo)
    },
    [acharTorrent, baixarDepot],
  )

  // Confirma o download via torrent na pasta escolhida (padrão: mesma do
  // InstallDialog — config.default_install_path ou ~/Games/Arcadia).
  const confirmarTorrent = useCallback(
    async (jogo: JogoLoja, magnet: string, savePath: string) => {
      setMetodo(null)
      setBusy(jogo.appid)
      try {
        const r = await window.launcherAPI?.torrentStart({
          gameId: jogo.appid,
          url: magnet,
          savePath,
          title: jogo.title,
          cover: `https://cdn.cloudflare.steamstatic.com/steam/apps/${jogo.appid}/library_600x900.jpg`,
        })
        setToast(r?.ok ? `"${jogo.title}" entrou nos downloads via torrent.` : r?.error || "Falha ao iniciar o torrent")
      } catch (e) {
        setToast(`Falha ao iniciar o torrent: ${e}`)
      } finally {
        setBusy("")
      }
    },
    [],
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
        if (r?.plugin) {
          setToast(`Requer o plugin ${r.plugin} (aba Plugins).`)
        } else {
          setToast(r?.ok ? `"${jogo.title}" entrou na fila de downloads${via}.` : r?.error || "Falha ao enfileirar")
        }
      } catch (e) {
        setToast(`Falha ao enfileirar: ${e}`)
      } finally {
        setBusy("")
      }
    },
    [],
  )

  // Add muda conforme a integração local: ativa = registra na Steam usando o
  // manifesto; desativada = cria só o stub na biblioteca do Arcadia.
  const adicionar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setEscolhendo(null)
      setBusy(jogo.appid)
      try {
        const r = slsAtivo
          ? await (async () => {
              const info = await obterInfo(jogo.appid)
              if (!info?.ok) {
                const motivo = info?.error || "Sem manifesto para este jogo."
                if (semManifestoRef.current) semManifestoRef.current(jogo, motivo)
                else setToast(motivo)
                return null
              }
              return window.launcherAPI?.storeAddToSteam({
                appid: jogo.appid,
                title: jogo.title,
                token: info.token,
                dlcs: info.dlcs,
              })
            })()
          : await window.launcherAPI?.storeAddToLibrary({
              appid: jogo.appid,
              title: jogo.title,
              cover: jogo.capa || jogo.cover,
              hero: jogo.hero,
              heroi: jogo.heroi,
            })
        if (meu !== pedido.current || !r) return
        if (r?.ok) {
          setJaAdicionados((prev) => new Set(prev).add(jogo.appid))
          setRemovidosLocal((prev) => {
            const n = new Set(prev)
            n.delete(jogo.appid)
            return n
          })
        }
        const faltaPlugin = "plugin" in r && r.plugin
        setToast(faltaPlugin ? "Configure uma integração local em Plugins para habilitar estas ações." : r?.ok ? `"${jogo.title}" adicionado à biblioteca.` : r?.error || "Falha ao adicionar")
      } catch (e) {
        setToast(`Falha ao adicionar: ${e}`)
      } finally {
        if (meu === pedido.current) setBusy("")
      }
    },
    [obterInfo, slsAtivo],
  )

  const remover = useCallback(async (jogo: JogoLoja) => {
    // Mesma guarda de `pedido` de baixar/adicionar: sem ela, um Add/Baixar
    // lento em voo resolvia depois do Remover e revertia o estado do botão.
    const meu = ++pedido.current
    setBusy(jogo.appid)
    try {
      const r = await window.launcherAPI?.storeRemoveFromLibrary(jogo.appid)
      if (meu !== pedido.current) return
      if (r?.ok) {
        setJaAdicionados((prev) => {
          const n = new Set(prev)
          n.delete(jogo.appid)
          return n
        })
        setRemovidosLocal((prev) => new Set(prev).add(jogo.appid))
      }
      setToast(r?.ok ? `"${jogo.title}" removido da biblioteca.` : r?.error || "Falha ao remover")
    } catch (e) {
      setToast(`Falha ao remover: ${e}`)
    } finally {
      if (meu === pedido.current) setBusy("")
    }
  }, [])

  const reiniciarSteam = useCallback(async () => {
    const r = await window.launcherAPI?.slssteamLaunch()
    setToast(r?.ok ? "Reiniciando a Steam com a SLSsteam…" : r?.error || "Falha ao abrir a Steam")
  }, [])

  return {
    bloqueados,
    jaAdicionados,
    slsAtivo,
    cheevoAtivo,
    fixesAtivo,
    escolhendo,
    setEscolhendo,
    metodo,
    setMetodo,
    busy,
    toast,
    setToast,
    baixar,
    baixarDepot,
    confirmarTorrent,
    confirmarBaixar,
    adicionar,
    remover,
    reiniciarSteam,
  }
}
