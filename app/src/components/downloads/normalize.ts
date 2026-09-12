// View-model unificado de downloads.
//
// O app tem DOIS subsistemas com telas separadas: a fila do dm (instalações
// Legendary/Epic e DepotDownloader/Steam) e o subsistema torrent (magnet via
// aria2/libtorrent, HTTP direto e cache no debrid). Cada um tem status, campos
// e ações próprios — e isso vazava para a UI como dois cards diferentes, dois
// contadores que se contradiziam e torrent invisível no console.
//
// Este módulo normaliza os dois para UMA lista (`DownloadVM`) usada pelo card
// único e pelas duas telas. Regras:
//   - Puro: sem imports de runtime, sem window. É o que permite testar com
//     `node --test` (o card e o hook fazem a parte impura).
//   - Nenhum handler novo: as ações continuam sendo os mesmos IPC de sempre,
//     despachados por `kind` (dm* para fila, torrent* para o P2P).
//   - Sem data nos itens: a ordem de chegada é a ordem recebida da fonte; o
//     agrupamento é por status (estável dentro do mesmo status).

import type { DmItem, TorrentItem } from "../../global"

export type DownloadKind = "legendary" | "steam" | "torrent" | "http" | "debrid"
export type DownloadStatus =
  | "active"
  | "queued"
  | "paused"
  | "caching"
  | "done"
  | "error"
  | "canceled"
export type DownloadActionType = "pause" | "resume" | "cancel" | "retry" | "dismiss"

/** Item normalizado — o contrato do card único (desktop e console). */
export interface DownloadVM {
  id: string
  kind: DownloadKind
  status: DownloadStatus
  title: string
  cover: string
  percent: number
  /** bytes/s; 0 quando o motor não reporta (fila parada, cache no debrid etc.) */
  speedBps: number
  eta: string
  /** -1 = não se aplica (não é torrent) */
  peers: number
  seeds: number
  error: string
  // Métricas cruas: o card formata por kind (dm reporta MiB, torrent bytes).
  dmDoneMiB: number
  dmTotalMiB: number
  bytesDone: number
  bytesTotal: number
  folderName: string
}

/** Ações disponíveis por (kind, status) — mesma matriz para as duas fontes. */
export function actionsFor(kind: DownloadKind, status: DownloadStatus): DownloadActionType[] {
  switch (status) {
    case "active":
      return ["pause", "cancel"]
    case "queued":
      return ["cancel"]
    case "paused":
      return ["resume", "cancel"]
    case "caching":
      // Debrid baixando pro servidor dele: não há o que pausar localmente.
      return ["cancel"]
    case "error":
      return ["retry", "dismiss"]
    case "canceled":
      return ["dismiss"]
    case "done":
      // "dismiss" vira o "Remover" das duas fontes.
      return ["dismiss"]
  }
  void kind
}

/** Chave i18n do selo de origem ("Steam", "Torrent", "Debrid"...). */
export function kindKey(kind: DownloadKind): string {
  return `downloads.origem.${kind === "legendary" ? "epic" : kind}`
}

/** Chave i18n do estado — reusa as chaves já existentes onde há equivalência. */
export function statusKey(status: DownloadStatus): string {
  if (status === "caching") return "torrent.status.cacheando"
  return `downloads.status.${status === "active" ? "baixando" : status}`
}

function clampPct(n: number): number {
  if (!Number.isFinite(n)) return 0
  return Math.max(0, Math.min(100, Math.round(n)))
}

function dmKind(appid: string): DownloadKind {
  return /^steam:/i.test(String(appid)) ? "steam" : "legendary"
}

export function normalizeDmItem(it: DmItem): DownloadVM {
  const status: DownloadStatus =
    it.status === "downloading" ? "active" : (it.status as DownloadStatus) || "queued"
  return {
    id: String(it.appid),
    kind: dmKind(it.appid),
    status,
    title: it.title || it.appName || "",
    cover: it.cover || "",
    percent: clampPct(Number(it.percent) || 0),
    // A fila mede velocidade em MiB/s (Legendary/Depot) — normaliza p/ bytes/s.
    speedBps: Math.max(0, (Number(it.speed) || 0) * 1024 ** 2),
    eta: it.eta || "",
    peers: -1,
    seeds: -1,
    error: it.error || "",
    dmDoneMiB: Number(it.done) || 0,
    dmTotalMiB: Number(it.total) || 0,
    bytesDone: 0,
    bytesTotal: 0,
    folderName: "",
  }
}

export function normalizeTorrentItem(it: TorrentItem): DownloadVM {
  const kind: DownloadKind =
    it.engine === "http" ? "http" : it.engine === "debrid" || it.cacheando ? "debrid" : "torrent"
  const status: DownloadStatus = it.erro
    ? "error"
    : it.completo
      ? "done"
      : it.pausado
        ? "paused"
        : it.cacheando
          ? "caching"
          : "active"
  // Concluído guarda o tamanho final no estado (o polling para e os campos
  // vivos somem) — sem isto o card mostrava "— / — · 0%".
  const bytesDone = it.completo ? Number(it.fileSize) || 0 : Number(it.bytesDownloaded) || 0
  return {
    id: String(it.gameId),
    kind,
    status,
    title: it.title || it.folderName || it.gameId,
    cover: it.cover || "",
    percent: it.completo ? 100 : clampPct((Number(it.progress) || 0) * 100),
    speedBps: Math.max(0, Number(it.downloadSpeed) || 0),
    eta: "",
    peers: Math.max(0, Number(it.numPeers) || 0),
    seeds: Math.max(0, Number(it.numSeeds) || 0),
    error: it.erro || "",
    dmDoneMiB: 0,
    dmTotalMiB: 0,
    bytesDone,
    bytesTotal: Number(it.fileSize) || 0,
    folderName: it.folderName || "",
  }
}

/** Ordem interna dos estados na seção de ativos (estável p/ o resto). */
const RANK: Record<DownloadStatus, number> = {
  active: 0,
  caching: 1,
  queued: 2,
  paused: 3,
  error: 4,
  done: 5,
  canceled: 6,
}

export interface DownloadsFeed {
  ativos: DownloadVM[]
  concluidos: DownloadVM[]
  falhas: DownloadVM[]
  total: number
  ativosCount: number
  falhasCount: number
}

/** Junta as duas fontes num feed só, já agrupado. */
export function buildFeed(
  dm: DmItem[] | undefined | null,
  tor: TorrentItem[] | undefined | null,
): DownloadsFeed {
  const vms: DownloadVM[] = [
    ...(Array.isArray(dm) ? dm.filter(Boolean).map(normalizeDmItem) : []),
    ...(Array.isArray(tor) ? tor.filter(Boolean).map(normalizeTorrentItem) : []),
  ]
  const ativos: DownloadVM[] = []
  const concluidos: DownloadVM[] = []
  const falhas: DownloadVM[] = []
  for (const vm of vms) {
    if (vm.status === "done") concluidos.push(vm)
    else if (vm.status === "error" || vm.status === "canceled") falhas.push(vm)
    else ativos.push(vm)
  }
  // Sort estável: empate mantém a ordem de chegada (dm antes de torrent).
  ativos.sort((a, b) => RANK[a.status] - RANK[b.status])
  return {
    ativos,
    concluidos,
    falhas,
    total: vms.length,
    ativosCount: ativos.length,
    falhasCount: falhas.length,
  }
}
