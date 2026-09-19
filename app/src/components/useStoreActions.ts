"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { useI18n } from "../i18n/I18nContext"
import type { Game } from "./ps5-launcher/types"

// Ações da loja — Baixar, Add, Remover, reiniciar a Steam — compartilhadas
// entre o modo desktop e o modo console. Antes moravam só no StoreView.tsx do
// desktop, e cada correção feita ali (guarda de pedido em voo, busy liberado
// em finally, cache do manifesto) teria de ser refeita na loja do console.
// Com o hook, uma correção vale para os dois.

export type DepotInfo = {
  depotId: string
  manifestId: string
  key: string
  size?: number
  name?: string
  os?: string
  language?: string
  dlcAppid?: string
  shared?: boolean
  optional?: boolean
}

export type ManifestInfo = {
  depots: DepotInfo[]
  token?: string
  dlcs?: string[]
  fonte?: string
  installdir?: string
}

export type JogoLoja = {
  appid: string
  title: string
  cover?: string
  capa?: string
  hero?: string
  heroi?: string
}

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
  onSemManifesto?: (jogo: JogoLoja, motivo: string, acao: "adicionar" | "baixar") => void
}

/** Mensagem de aviso: as que começam com "Aviso:"/"Warning:" (nos três idiomas). */
export function ehAviso(texto: string) {
  return /^(Aviso|Warning):/.test(String(texto || "").trim())
}

// Casamento de título da loja PC. Espelha o matcher do backend
// (electron/sources.js): compara PALAVRA inteira ("ark" não casa
// "dark"/"shark"), exige a mesma sequência/numeral (Far Cry 3 != Far Cry 2;
// Portal != Portal 2) e, quando o alvo é DLC/pack, exige o sufixo do DLC
// (todas as palavras do alvo). Acento é dobrado (NFD: "Ragnarök" ->
// "ragnarok"), nunca removido. null = não é o mesmo jogo; score maior =
// opção mais relevante (exato > prefixo > contém a frase > só palavras) e
// `extra` (palavras a mais) desempata.
export type MatchTituloLoja = { score: number; extra: number }

const STOPWORDS_TITULO = new Set([
  "the", "a", "an", "of", "and", "or", "to", "in", "on", "at", "for", "with",
  "from", "de", "da", "do", "das", "dos", "e", "y", "la", "el", "los", "las",
  "del", "le", "les", "des", "du",
])
const ROMANOS_TITULO: Record<string, number> = {
  i: 1, ii: 2, iii: 3, iv: 4, v: 5, vi: 6, vii: 7, viii: 8, ix: 9, x: 10,
  xi: 11, xii: 12, xiii: 13,
}
const NUMEROS_TITULO: Record<string, number> = {
  one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8,
  nine: 9, ten: 10,
}
const MARCA_VERSAO_TITULO = /^(?:v|ver|versao|version|build|patch|update|hotfix|rev|revision)\d*$/
const CONTEXTO_NUMERO_TITULO = /^(?:episode|ep|part|chapter|act|book|vol|volume|disc|disk)$/

export function foldTituloLoja(value: string) {
  return String(value || "")
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[\u00f8]/g, "o")
    .replace(/[\u00e6]/g, "ae")
    .replace(/[\u0153]/g, "oe")
    .replace(/[\u00df]/g, "ss")
    .replace(/[\u0142]/g, "l")
    .replace(/[\u0111]/g, "d")
    .replace(/['\u2019`\u00b4]/g, "")
}

function palavrasTituloLoja(value: string) {
  return foldTituloLoja(value)
    .split(/[^a-z0-9]+/)
    .filter(Boolean)
}

function tituloCanonicoLoja(value: string) {
  const palavras = palavrasTituloLoja(value)
  const seq = new Set<number>()
  const exigidas: string[] = []
  let versao = false
  for (let i = 0; i < palavras.length; i++) {
    const palavra = palavras[i]
    const anterior = palavras[i - 1] || ""
    const proxima = palavras[i + 1] || ""
    if (MARCA_VERSAO_TITULO.test(palavra) && (/\d$/.test(palavra) || /^\d/.test(proxima))) {
      versao = true
      continue
    }
    if (versao && /^\d/.test(palavra)) continue
    versao = false
    let numero = 0
    if (/^\d+$/.test(palavra)) {
      if (/^\d/.test(anterior) || /^\d/.test(proxima)) continue
      const n = Number(palavra)
      if (n >= 1 && n <= 99) numero = n
      else {
        exigidas.push(palavra)
        continue
      }
    } else if (ROMANOS_TITULO[palavra]) {
      if (palavra.length > 1 || palavra === "v" || palavra === "x" || CONTEXTO_NUMERO_TITULO.test(anterior))
        numero = ROMANOS_TITULO[palavra]
    } else if (NUMEROS_TITULO[palavra] && CONTEXTO_NUMERO_TITULO.test(anterior)) {
      numero = NUMEROS_TITULO[palavra]
    }
    if (numero) {
      seq.add(numero)
      exigidas.push(`#${numero}`)
      continue
    }
    if (palavra.length < 2 || STOPWORDS_TITULO.has(palavra)) continue
    exigidas.push(palavra)
  }
  return { palavras, seq, exigidas }
}

export function matchTituloLoja(alvo: string, candidato: string): MatchTituloLoja | null {
  const a = tituloCanonicoLoja(alvo)
  const c = tituloCanonicoLoja(candidato)
  if (!a.palavras.length || !c.palavras.length) return null
  if (a.seq.size !== c.seq.size) return null
  for (const n of a.seq) if (!c.seq.has(n)) return null
  const candidatas = new Set(c.exigidas)
  for (const exigida of a.exigidas) if (!candidatas.has(exigida)) return null
  const compacto = (s: string) => foldTituloLoja(s).replace(/[^a-z0-9]/g, "")
  const alvoCompacto = compacto(alvo)
  const candCompacto = compacto(candidato)
  const cru = String(alvo || "").trim().toLowerCase() === String(candidato || "").trim().toLowerCase()
  let score = 1
  if (cru) score = 5
  else if (candCompacto === alvoCompacto) score = 4
  else if (alvoCompacto && candCompacto.startsWith(alvoCompacto)) score = 3
  else if (alvoCompacto && candCompacto.includes(alvoCompacto)) score = 2
  return { score, extra: Math.max(0, c.palavras.length - a.palavras.length) }
}

export function useStoreActions(games: Game[] = [], opts: StoreActionsOpts = {}) {
  const { t: _t } = useI18n()
  const [jaAdicionados, setJaAdicionados] = useState<Set<string>>(new Set())
  const [removidosLocal, setRemovidosLocal] = useState<Set<string>>(new Set())
  const [escolhendo, setEscolhendo] = useState<EscolhaDisco | null>(null)
  const [metodo, setMetodo] = useState<EscolhaMetodo | null>(null)
  const [busy, setBusy] = useState("")
  const [toast, setToast] = useState("")
  const [slsAtivo, setSlsAtivo] = useState(false)
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
        setFixesAtivo(Boolean(s?.luatools))
      })
    status()
    const offLib = window.launcherAPI?.onLibraryChanged(() => status())
    const offPlugins = window.launcherAPI?.onPluginsChanged?.(() => status())
    return () => {
      offLib?.()
      offPlugins?.()
    }
  }, [])

  // Jogos que não devem oferecer Baixar/Add: já adicionados à Steam ou já
  // presentes na biblioteca do Arcadia. Ocultos (hidden) não contam: "Remover"
  // da loja só oculta o jogo indexado, e sem este filtro o botão continuava
  // "Na biblioteca" depois de removido.
  const bloqueados = new Set(
    [
      ...jaAdicionados,
      ...games.filter((g) => !g.hidden).map((g) => String(g.id).replace(/^steam:/, "")),
    ].filter((appid) => !removidosLocal.has(appid)),
  )

  // Buscar o manifesto passa por vários provedores e pode levar dezenas de
  // segundos. Guardamos por appid para que Add logo depois de Baixar no mesmo
  // jogo seja instantâneo em vez de repetir a busca inteira.
  const infoCache = useRef(new Map<string, ManifestInfo>())
  const obterInfo = useCallback(async (appid: string) => {
    const guardado = infoCache.current.get(appid)
    // Cache velho (versão anterior do backend) pode não ter os/language/dlcAppid.
    // Nesse caso refaz o fetch para pegar metadata enriquecida.
    const temMeta = (i?: ManifestInfo) => !!i?.depots?.some((d) => d.os || d.language || d.dlcAppid)
    if (guardado && temMeta(guardado)) return { ok: true, ...guardado }
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
      const limparTitulo = (s: string) =>
        String(s || "")
          .replace(/\s+(?:on|na)\s+steam(?:\s*[-|:].*)?$/i, "")
          .trim()
      const tituloBusca = limparTitulo(title)
      // 50: jogos populares têm MUITAS releases (RDR2 tem 30+ entre as
      // fontes) — o diálogo lista todas as que têm link baixável.
      const r = await window.launcherAPI?.sourcesSearch?.(tituloBusca, 50)
      if (!tituloBusca) return null
      // Só o mesmo jogo entra, e a opção mais relevante (título exato >
      // prefixo > frase > palavras) vai para o topo do diálogo.
      const cands = (r?.results || [])
        .flatMap((g) => {
          const match = matchTituloLoja(tituloBusca, g.title)
          return match ? [{ g, match }] : []
        })
        .sort((a, b) => b.match.score - a.match.score || a.match.extra - b.match.extra)
        .map((item) => item.g)
      // Junta TODAS as fontes baixáveis: magnet (torrent) ou URL http direta
      // (o backend resolve/recusa hoster HTML). O diálogo lista tudo e quem
      // escolhe é o usuário.
      const opcoes: OpcaoTorrent[] = []
      for (const cand of cands) {
        const full = await window.launcherAPI?.sourcesGame?.(cand.ref)
        const rawUris = full?.game?.uris
        const uris = Array.isArray(rawUris)
          ? rawUris
          : typeof rawUris === "string"
            ? [rawUris]
            : full?.game?.uri
              ? [full.game.uri]
              : []
        const magnet = uris.find((u) => /^magnet:/i.test(String(u).trim()))
        const http = uris.find((u) => /^https?:\/\//i.test(String(u).trim()))
        const uri = magnet || http
        if (uri)
          opcoes.push({
            ref: cand.ref,
            magnet: String(uri),
            fonte: cand.src,
            tituloFonte: cand.title,
            fileSize: cand.fileSize,
            http: !magnet,
          })
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
          if (semManifestoRef.current) semManifestoRef.current(jogo, motivo, "baixar")
          else setToast(motivo)
          return
        }
        const libs = ((await window.launcherAPI?.storeLibraries()) || []) as Biblioteca[]
        if (meu !== pedido.current) return
        if (!libs.length) {
          setToast(_t("store.no_steam_lib"))
          return
        }
        // O diálogo aparece sempre, mesmo com uma biblioteca só: um download de
        // vários GB não deve começar sem confirmação.
        setEscolhendo({ jogo, info: info as ManifestInfo, libs })
      } catch (e) {
        setToast(_t("store.falha_download", { erro: String(e) }))
      } finally {
        if (meu === pedido.current) setBusy("")
      }
    },
    [obterInfo],
  )

  // Entrada pública do botão Baixar: se o jogo existe numa fonte com magnet,
  // pergunta o método (Depot vs Torrent) antes de seguir. Sem fonte, cai
  // direto no fluxo Depot de sempre.
  // 3 cenários:
  //   1) só torrent (JSON conectado, SLSsteam OFF)      → abre diálogo, etapa "fonte" direta
  //   2) só depot  (SLSsteam ON, sem torrent na fonte)  → baixa via depot sem diálogo
  //   3) ambos                                          → abre diálogo com escolha de método
  const baixar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setBusy(jogo.appid)
      try {
        const opcoes = await acharTorrent(jogo.title)
        if (meu !== pedido.current) return
        if (opcoes) {
          // caso 1 e 3: diálogo cuida da etapa inicial via depotDisponivel
          setMetodo({ jogo, opcoes })
          return
        }
      } finally {
        if (meu === pedido.current) setBusy("")
      }
      // caso 2: sem torrent — só faz sentido se depot estiver ativo
      if (slsAtivo) await baixarDepot(jogo)
    },
    [acharTorrent, baixarDepot, slsAtivo],
  )

  // Confirma o download via torrent na pasta escolhida (padrão: mesma do
  // InstallDialog — config.default_install_path ou ~/Games/Arcadia).
  const confirmarTorrent = useCallback(async (jogo: JogoLoja, magnet: string, savePath: string) => {
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
      setToast(
        r?.ok
          ? `"${jogo.title}" entrou nos downloads via torrent.`
          : r?.error || "Falha ao iniciar o torrent",
      )
    } catch (e) {
      setToast(_t("store.falha_torrent", { erro: String(e) }))
    } finally {
      setBusy("")
    }
  }, [])

  const confirmarBaixar = useCallback(
    async (
      jogo: JogoLoja,
      info: ManifestInfo,
      steamDir?: string,
      depotsEscolhidos?: DepotInfo[],
    ) => {
      setEscolhendo(null)
      setBusy(jogo.appid)
      try {
        const r = await window.launcherAPI?.storeInstall({
          appid: jogo.appid,
          title: jogo.title,
          cover:
            jogo.capa ||
            jogo.cover ||
            `https://steamcdn-a.akamaihd.net/steam/apps/${jogo.appid}/library_600x900.jpg`,
          installdir: info.installdir || jogo.title.replace(/[^A-Za-z0-9]/g, ""),
          depots: depotsEscolhidos && depotsEscolhidos.length ? depotsEscolhidos : info.depots,
          token: info.token,
          dlcs: info.dlcs,
          steamDir,
        })
        const via = info.fonte ? ` (via ${info.fonte})` : ""
        if (r?.plugin) {
          setToast(_t("store.requer_plugin", { plugin: String(r.plugin) }))
        } else {
          setToast(
            r?.ok
              ? `"${jogo.title}" entrou na fila de downloads${via}.`
              : r?.error || "Falha ao enfileirar",
          )
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
  //
  // O botão se chama "adicionar à biblioteca" e NÃO pode terminar sem nada: sem
  // manifesto (ou com a injeção falhando), o jogo entra na biblioteca de todo
  // jeito e a pessoa recebe o motivo. Antes o caminho com o componente ativo
  // abortava no `return null` e o jogo não aparecia em lugar nenhum — nem na
  // biblioteca, nem na Steam.
  const adicionar = useCallback(
    async (jogo: JogoLoja) => {
      const meu = ++pedido.current
      setEscolhendo(null)
      setBusy(jogo.appid)
      const paraBiblioteca = () =>
        window.launcherAPI?.storeAddToLibrary({
          appid: jogo.appid,
          title: jogo.title,
          cover: jogo.capa || jogo.cover,
          hero: jogo.hero,
          heroi: jogo.heroi,
        })
      try {
        let r
        if (slsAtivo) {
          const info = await obterInfo(jogo.appid)
          if (info?.ok) {
            const injetado = await window.launcherAPI?.storeAddToSteam({
              appid: jogo.appid,
              title: jogo.title,
              token: info.token,
              dlcs: info.dlcs,
            })
            if (injetado?.ok) r = injetado
            else r = { ...(await paraBiblioteca()), injecao: "falhou", motivo: injetado?.error || "" }
          } else {
            // Sem manifesto — o caso normal de integração ligada SEM a chave do
            // Hubcap (sem chave não há provedor, e sem provedor não há manifesto).
            // O jogo entra só na biblioteca, com a mensagem de sucesso de sempre:
            // isto não é erro, e aviso aqui só poluía a tela.
            r = await paraBiblioteca()
          }
        } else {
          r = await paraBiblioteca()
        }
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
        setToast(
          faltaPlugin
            ? _t("store.requer_plugin", { plugin: String(r.plugin) })
            : r?.injecao === "sem_manifesto"
              ? _t("store.adicionado_sem_manifesto", { titulo: jogo.title, motivo: String(r.motivo || "") })
              : r?.injecao === "falhou"
                ? _t("store.adicionado_sem_injecao", { titulo: jogo.title, motivo: String(r.motivo || "") })
                : r?.ok
                  ? _t("store.adicionado_biblioteca", { titulo: jogo.title })
                  : r?.error || _t("store.falha_adicionar"),
        )
      } catch (e) {
        setToast(_t("store.falha_adicionar_detalhe", { erro: String(e) }))
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
