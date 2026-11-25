// Lado Node do subsistema torrent. Linux: worker Python (torrent_rpc/main.py)
// por JSON-lines no stdio. Windows nativo: engine aria2c (electron/aria2.js) —
// mesmo estado/UI. Persiste os downloads ativos em torrent_state.json para
// retomar após reiniciar o app (o engine verifica os arquivos e continua).
const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn } = require("child_process")
const { Readable } = require("stream")
const { pipeline } = require("stream/promises")
const { fetchRede, fetchManual } = require("./httpfetch")
const { getDataDir } = require("./runtime-paths")

const DATA_DIR = getDataDir()
const STATE = path.join(DATA_DIR, "torrent_state.json")
const WORKER = path.join(__dirname, "torrent_rpc", "main.py")

let child = null
let nextId = 1
const pendentes = new Map() // id -> { resolve, reject, timer }
let statusTimer = null
let vigiaAria2Timer = null // watchdog do daemon aria2 (RPC mudo = event loop preso)
let vigiaFalhas = 0
let vigiaUltimaRecuperacao = 0
let tickRpcInicio = 0 // timestamp do tick em andamento (guarda anti-empilhamento)
let onProgress = null
let _libtorrentOk = null // null = ainda não testado

const MAX_TORRENT_ID_LENGTH = 256
const MAX_DOWNLOAD_URI_LENGTH = 8192

// IDs and URIs arrive from renderer IPC, so validate them again in the main
// process.  Retro/source catalogs sanitize their own payloads too, but the
// renderer is not a trust boundary.  Keeping this normalization here also
// means both catalogs use the same ``tor:<stable-id>`` key in torrent_state.
function normalizeTorrentId(value) {
  const raw = typeof value === "string" ? value.trim() : String(value || "").trim()
  if (!raw) return ""
  const id = raw.startsWith("tor:") ? raw.slice(4) : raw
  if (
    !id ||
    id.length > MAX_TORRENT_ID_LENGTH - 4 ||
    !/^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(id) ||
    id.startsWith("tor:")
  )
    return ""
  return `tor:${id}`
}

function isPrivateHost(hostname) {
  const host = String(hostname || "")
    .toLowerCase()
    .replace(/^\[|\]$/g, "")
  if (!host || host === "localhost" || host.endsWith(".localhost") || host === "::1") return true
  if (host.includes(":")) return true // IPv6 literals, including link-local addresses.
  if (/^(10|127)\./.test(host) || /^192\.168\./.test(host)) return true
  const private172 = /^172\.(1[6-9]|2\d|3[0-1])\./.test(host)
  return private172
}

/**
 * Normalize a download URI accepted by torrent:start.
 *
 * Only magnets carrying a BitTorrent xt and HTTP(S) URLs are valid.  This is
 * deliberately kept in the main process: catalog normalization protects the
 * normal Retro path, while this check protects the existing Sources path and
 * arbitrary renderer IPC callers.  No file/javascript/custom protocol can
 * reach the worker or HTTP resolver.
 */
function normalizeDownloadUri(value) {
  if (typeof value !== "string") return ""
  const uri = value.trim()
  if (!uri || uri.length > MAX_DOWNLOAD_URI_LENGTH || /[\u0000-\u001f\u007f]/.test(uri)) return ""

  if (/^magnet:/i.test(uri)) {
    try {
      const parsed = new URL(uri)
      if (parsed.protocol.toLowerCase() !== "magnet:" || parsed.username || parsed.password)
        return ""
      const hasBtih = parsed.searchParams
        .getAll("xt")
        .some((xt) => /^urn:btih:[A-Za-z0-9]+$/i.test(xt))
      if (!hasBtih) return ""
      // The Python worker accepts the canonical lowercase magnet scheme.
      return `magnet:${uri.slice(uri.indexOf(":") + 1)}`
    } catch {
      return ""
    }
  }

  try {
    const parsed = new URL(uri)
    if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return ""
    if (parsed.username || parsed.password || parsed.hash || isPrivateHost(parsed.hostname))
      return ""
    return uri
  } catch {
    return ""
  }
}

async function fetchPublicHttp(url, options = {}, maxRedirects = 5) {
  let current = normalizeDownloadUri(url)
  let requestOptions = { ...options }
  if (!current || !/^https?:\/\//i.test(current)) throw new Error("URI de download inválida")
  for (let redirects = 0; redirects <= maxRedirects; redirects++) {
    // fetchManual (undici): o net.fetch do Chromium não suporta
    // redirect:"manual" e falhava com "Redirect was cancelled" em qualquer 3xx.
    const response = await fetchManual(current, { ...requestOptions, redirect: "manual" })
    if (response.status < 300 || response.status >= 400) return response
    const location = response.headers?.get?.("location")
    try {
      await response.body?.cancel?.()
    } catch {}
    if (!location) throw new Error("redirecionamento sem destino")
    let next
    try {
      next = normalizeDownloadUri(new URL(location, current).toString())
    } catch {
      next = ""
    }
    if (!next || !/^https?:\/\//i.test(next)) throw new Error("redirecionamento inseguro")
    try {
      if (new URL(next).host !== new URL(current).host && requestOptions.headers) {
        const headers = { ...requestOptions.headers }
        for (const key of Object.keys(headers)) {
          if (/^(?:authorization|cookie|proxy-authorization)$/i.test(key)) delete headers[key]
        }
        requestOptions = { ...requestOptions, headers }
      }
    } catch {}
    current = next
  }
  throw new Error("redirecionamento em excesso")
}

function emitProgress() {
  try {
    onProgress?.(readState())
  } catch {
    // A renderer can disappear while an IPC event is being emitted.
  }
}

function upsertState(item) {
  const lista = readState().filter((current) => current.gameId !== item.gameId)
  lista.push(item)
  writeState(lista)
  emitProgress()
  return item
}

function patchState(gameId, patch) {
  const lista = readState()
  const item = lista.find((current) => current.gameId === gameId)
  if (!item) return null
  Object.assign(item, patch)
  writeState(lista)
  emitProgress()
  return item
}

function markStateError(gameId, error) {
  return patchState(gameId, {
    erro: String(error || "falha ao iniciar download"),
    // An errored item is retained for Downloads, but is not presented as an
    // actively transferring item; resume clears the error and retries it.
    pausado: true,
    completo: false,
  })
}

function readState() {
  try {
    const value = JSON.parse(fs.readFileSync(STATE, "utf-8"))
    return Array.isArray(value) ? value.filter((item) => item && typeof item === "object") : []
  } catch {
    return []
  }
}

function writeState(list) {
  try {
    const tmp = `${STATE}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(list, null, 2))
    fs.renameSync(tmp, STATE)
  } catch {}
}

function matarChild() {
  statusTimer && clearInterval(statusTimer)
  statusTimer = null
  vigiaAria2Timer && clearInterval(vigiaAria2Timer)
  vigiaAria2Timer = null
  for (const [, p] of pendentes) {
    clearTimeout(p.timer)
    p.reject(new Error("worker torrent morreu"))
  }
  pendentes.clear()
  child = null
}

function ensureWorker() {
  if (child) return true
  try {
    child = spawn("python3", [WORKER], { stdio: ["pipe", "pipe", "inherit"] })
  } catch {
    return false
  }
  let buf = ""
  child.stdout.setEncoding("utf-8")
  child.stdout.on("data", (chunk) => {
    buf += chunk
    let nl
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim()
      buf = buf.slice(nl + 1)
      if (!line) continue
      let msg
      try {
        msg = JSON.parse(line)
      } catch {
        continue
      }
      if (msg.event === "ready") continue
      const p = pendentes.get(msg.id)
      if (p) {
        pendentes.delete(msg.id)
        clearTimeout(p.timer)
        if (msg.error) p.reject(Object.assign(new Error(msg.error.code), { code: msg.error.code }))
        else p.resolve(msg.result)
      }
    }
  })
  child.on("error", matarChild)
  child.on("close", matarChild)
  return true
}

function rpc(method, params = {}, timeoutMs = 120000) {
  if (!ensureWorker()) return Promise.reject(new Error("falha ao iniciar worker torrent"))
  const id = nextId++
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pendentes.delete(id)
      reject(new Error("rpc_timeout"))
    }, timeoutMs)
    pendentes.set(id, { resolve, reject, timer })
    child.stdin.write(JSON.stringify({ id, method, params }) + "\n")
  })
}

// Detecta libtorrent uma única vez (é a dependência do worker).
function libtorrentDisponivel() {
  if (_libtorrentOk !== null) return Promise.resolve(_libtorrentOk)
  return new Promise((res) => {
    const p = spawn("python3", ["-c", "import libtorrent"])
    p.on("close", (code) => {
      _libtorrentOk = code === 0
      res(_libtorrentOk)
    })
    p.on("error", () => {
      _libtorrentOk = false
      res(false)
    })
  })
}

// Poll de status enquanto houver downloads ativos (evento para a UI).
// Watchdog do daemon aria2 ------------------------------------------------
// Caso real (2026-09-11): sob carga o RPC do aria2 1.37 fica MUDO em rajadas
// (30s–2,5min, reproduzido em controle) enquanto o download CONTINUA — e um
// processo morto não era ressuscitado (a UI congelava). Regras:
//   - processo morto com item ativo -> recicla JÁ (não espera sonda);
//   - RPC mudo mas progredindo em disco (mtime do .aria2 avança) -> deixa
//     quieto: é rajada normal e reciclar mataria um download saudável;
//   - RPC mudo SEM progresso por ~50s -> recicla (trava real): mata, sobe de
//     novo e retoma os ativos do disco. Cooldown de 120s entre reciclagens.
const ultimoVivo = new Map() // gameId -> último snapshot vivo do engine (anti "0 do nada")
let vigiaUltimaAtividade = 0

function mtimeArquivo(p) {
  try {
    return fs.statSync(p).mtimeMs
  } catch {
    return 0
  }
}

// O .aria2 de controle fica em <savePath>/<name>.aria2 e é reescrito pelo
// daemon conforme o download avança — sinal de progresso mesmo com RPC mudo.
function controlFileDe(item) {
  const nome = item.folderName || ""
  if (nome && item.savePath) return path.join(item.savePath, `${nome}.aria2`)
  // Sem nome conhecido: usa o .aria2 mais recente do savePath.
  try {
    let melhor = 0
    let caminho = ""
    for (const f of fs.readdirSync(item.savePath)) {
      if (!f.endsWith(".aria2")) continue
      const m = mtimeArquivo(path.join(item.savePath, f))
      if (m > melhor) {
        melhor = m
        caminho = path.join(item.savePath, f)
      }
    }
    return caminho
  } catch {
    return ""
  }
}

async function vigiarAria2() {
  if (!_aria2) return
  const ativos = readState().filter(
    (i) => i.engine === "aria2" && !i.completo && !i.pausado && !i.erro && !i.cacheando,
  )
  if (!ativos.length) {
    vigiaFalhas = 0
    return
  }
  const reciclar = (motivo) => {
    if (Date.now() - vigiaUltimaRecuperacao < 120_000) return
    vigiaUltimaRecuperacao = Date.now()
    vigiaFalhas = 0
    console.warn(`[torrent] reciclando daemon aria2 (${motivo}; ${ativos.length} item(ns))`)
    try {
      _aria2.stop()
    } catch {}
    ariaJobs.clear()
    ultimoVivo.clear()
    tickRpcInicio = 0 // destranca um tick preso no daemon morto
    setTimeout(() => {
      retomar().catch(() => {})
    }, 1500)
  }
  if (!_aria2.isAlive()) {
    reciclar("processo morto")
    return
  }
  let vivo = false
  try {
    vivo = await Promise.race([
      _aria2.probe(4000),
      new Promise((res) => setTimeout(() => res(false), 6000)),
    ])
  } catch {
    vivo = false
  }
  if (vivo) {
    vigiaFalhas = 0
    vigiaUltimaAtividade = Date.now()
    return
  }
  vigiaFalhas++
  // Mudo mas baixando? Rajada normal do aria2 — não interromper.
  const progrediu = ativos.some((i) => {
    const c = controlFileDe(i)
    return c && mtimeArquivo(c) > vigiaUltimaAtividade
  })
  if (progrediu) {
    vigiaFalhas = 0
    vigiaUltimaAtividade = Date.now()
    return
  }
  if (vigiaFalhas >= 9) reciclar("RPC mudo sem progresso")
}

function armarPolling() {
  if (statusTimer) return
  // Vigia do daemon aria2 (lazy: só age quando há item aria2 ativo).
  if (!vigiaAria2Timer) {
    vigiaAria2Timer = setInterval(() => {
      vigiarAria2().catch(() => {})
    }, 10_000)
  }
  statusTimer = setInterval(async () => {
    // Tick anterior ainda preso num RPC lento? Não empilha outro (com um
    // daemon mudo, cada tick abria mais uma conexão pendurada). O teto de 2
    // minutos destranca mesmo se a promessa presa nunca resolver.
    if (tickRpcInicio && Date.now() - tickRpcInicio < 120_000) return
    tickRpcInicio = Date.now()
    try {
      const lista = readState()
      // Só consulta o worker Python se houver torrent vivo (HTTP é lido direto).
      let todos = {}
      if (
        lista.some(
          (i) =>
            i.engine !== "http" &&
            i.engine !== "debrid" &&
            i.engine !== "aria2" &&
            !i.completo &&
            !i.pausado,
        )
      ) {
        try {
          todos = (await rpc("status", {}, 30000)) || {}
        } catch {}
      }
      const ativos = []
      for (const item of lista) {
        // Cancelado há instantes: o snapshot pode ser anterior ao cancel —
        // ignorar aqui é o que impede o item "ressuscitar" no writeState.
        if (marcadosCancelados.has(item.gameId)) continue
        // Preserve failed starts in the queue so Downloads can explain the
        // failure and offer cancel/retry instead of silently dropping them.
        if (item.erro) {
          ativos.push(item)
          continue
        }
        // Esperando o debrid cachear: mostra parado (0 MB) até o link sair.
        if (item.cacheando) {
          ativos.push(item)
          continue
        }
        if (item.engine === "http") {
          const h = httpDls.get(item.gameId)
          if (item.pausado || !h) {
            httpSpeed.delete(item.gameId)
            ativos.push({ ...item, downloadSpeed: 0 })
            continue
          }
          const completo = h.total > 0 && h.bytes >= h.total
          // Velocidade REAL (B/s) por amostra de tempo — o 1º tick sai 0 (sem
          // referência) e os seguintes mostram o ritmo atual.
          const agora = Date.now()
          const bps = httpBps(httpSpeed.get(item.gameId), h.bytes, agora)
          if (completo) httpSpeed.delete(item.gameId)
          else httpSpeed.set(item.gameId, { bytes: h.bytes, ts: agora })
          ativos.push({
            ...item,
            progress: h.total > 0 ? h.bytes / h.total : 0,
            bytesDownloaded: h.bytes,
            fileSize: h.total,
            downloadSpeed: bps,
            completo,
          })
          continue
        }
        if (item.pausado) {
          ativos.push(item)
          continue
        }
        let s = todos[item.gameId]
        if (item.engine === "aria2") {
          const gid = ariaJobs.get(item.gameId)
          const aria2 = getAria2()
          if (!aria2 || !gid) {
            // Daemon reiniciado/sem gid: mantém o item visível no estado.
            ativos.push(item)
            continue
          }
          try {
            s = await aria2.tell(gid)
            // Magnet: quando o metadata termina, o download real assume um gid
            // novo (followedBy). Atualiza o mapeamento para pause/cancel futuros.
            if (s.gid && s.gid !== gid) ariaJobs.set(item.gameId, s.gid)
            // Guarda o último snapshot vivo: se o RPC emudecer numa rajada, a
            // UI continua mostrando os últimos números reais em vez de zeros.
            ultimoVivo.set(item.gameId, {
              progress: s.progress,
              bytesDownloaded: s.bytesDownloaded,
              downloadSpeed: s.downloadSpeed,
              numPeers: s.numPeers,
              numSeeds: s.numSeeds,
              fileSize: s.fileSize,
              folderName: s.folderName,
              pausado: s.pausado,
              state: s.state,
            })
          } catch {
            // Falha de RPC não pode DESCARTAR o estado do download; mostra o
            // último snapshot conhecido (nunca inventa zero no lugar de dado).
            ativos.push({ ...item, ...(ultimoVivo.get(item.gameId) || {}) })
            continue
          }
        }
        if (!s) {
          // Sem handle de motor vivo (worker ausente/daemon reiniciado):
          // item COMPLETO continua registrado — a UI mostra "concluído" até
          // o usuário dispensar (ver comentário do writeState abaixo). Não
          // descartar concluídos aqui: no Windows o registro sumia no 1º tick.
          if (item.completo) {
            ativos.push(item)
            continue
          }
          continue // cancelado/sem handle
        }
        const completo = s.progress >= 1
        ativos.push({ ...item, ...s, completo })
      }
      // Jogos completos ficam registrados (para a UI mostrar "concluído")
      // até o usuário dispensar; os demais voltam ao estado vivo.
      writeState(
        ativos.map(
          ({
            gameId,
            url,
            savePath,
            fileIndices,
            pausado,
            completo,
            title,
            engine,
            fileName,
            cover,
            fileSize,
            cacheando,
            erro,
            // progresso/bytes: sem eles, item PAUSADO perde a posição a cada
            // tick (a projeção antiga zerava a barra: "— / X GiB · 0%").
            progress,
            bytesDownloaded,
            // nome da pasta do torrent: o watchdog usa p/ achar o .aria2 de
            // controle (sinal de progresso com o RPC mudo).
            folderName,
          }) => ({
            gameId,
            url,
            savePath,
            fileIndices,
            pausado,
            completo,
            title,
            engine,
            fileName,
            cover,
            fileSize,
            cacheando,
            erro,
            progress,
            bytesDownloaded,
            folderName,
          }),
        ),
      )
      if (onProgress) onProgress(ativos)
      // Guard de cancelamento: libera o gameId quando o item já saiu do estado.
      for (const id of [...marcadosCancelados]) {
        if (!ativos.some((a) => a.gameId === id)) marcadosCancelados.delete(id)
      }
      if (!ativos.some((a) => !a.completo && !a.pausado && !a.erro)) {
        clearInterval(statusTimer)
        statusTimer = null
        if (vigiaAria2Timer) {
          clearInterval(vigiaAria2Timer)
          vigiaAria2Timer = null
        }
      }
    } catch {} finally {
      tickRpcInicio = 0
    }
  }, 1000)
}

// --- Motor HTTP (fontes sem magnet: pixeldrain/datanodes/vikingfile etc.) ---
// Download direto por stream para <arquivo>.part, com resume por Range ao
// pausar/reabrir o app. Hosters que respondem HTML (página de espera/captcha,
// ex.: gofile, 1fichier) são recusados — esses precisariam de resolvedor.
const httpDls = new Map() // gameId -> { ctrl, bytes, total, fileName }
const httpSpeed = new Map() // gameId -> { bytes, ts }: amostra p/ velocidade B/s

// Velocidade HTTP em bytes/segundo: delta de bytes ÷ tempo real decorrido.
// (O antigo delta "por tick" não servia: `_b` não é persistido pelo estado —
// a projeção do tick tem whitelist — e o intervalo do tick não é 1s. Efeito
// do bug: a "velocidade" exibida virava o TOTAL baixado, "419 MiB/s".)
function httpBps(prev, bytes, agora) {
  if (!prev || !(agora > prev.ts) || bytes < prev.bytes) return 0
  return Math.round(((bytes - prev.bytes) * 1000) / (agora - prev.ts))
}
const debridJobs = new Map() // gameId -> AbortController (espera de cache)

// Algum debrid configurado? (magnet via debrid só com token presente)
function temDebridConfigurado() {
  try {
    const cfg = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "config.json"), "utf-8"))
    return ["realdebrid_token", "torbox_token", "alldebrid_token", "premiumize_token"].some((k) =>
      String(cfg[k] || "").trim(),
    )
  } catch {
    return false
  }
}

// Política 2026-09-12: releases (magnet/torrent) NÃO baixam sem debrid — o
// caminho P2P foi desativado por decisão de produto (menos superfície de
// falha). Mensagem única para start() e resume().
const ERRO_DEBRID = "Sem debrid conectado — downloads por torrent exigem debrid."

// --- Engine aria2 (Windows): magnet sem debrid (dormente; bloqueado acima) --
// O worker Python (libtorrent) é Linux-only; no Windows nativo o aria2c
// assume magnet/torrent (binário único, JSON-RPC local em 127.0.0.1).
// Import preguiçoso: nada disto carrega no Linux.
let _aria2 = null
const ariaJobs = new Map() // gameId -> gid
// gameIds cancelados: o tick de polling pode ter um snapshot ANTERIOR ao
// cancel e "ressuscitar" o item — o guard faz o tick ignorá-lo até sumir.
const marcadosCancelados = new Set()
function getAria2() {
  if (_aria2) return _aria2
  try {
    const { createAria2Manager, createAria2Engine } = require("./aria2")
    const manager = createAria2Manager({
      depsDir: path.join(DATA_DIR, "bin", "deps", "aria2"),
      tmpDir: path.join(DATA_DIR, "bin", "tmp"),
    })
    _aria2 = createAria2Engine({
      ensureExe: () => manager.ensure(),
      workDir: path.join(DATA_DIR, "bin", "aria2"),
    })
  } catch {
    _aria2 = null
  }
  return _aria2
}

// Hoster -> URL direta. Resolvedores conhecidos (gofile/pixeldrain/rootz)
// moram em hosters.js; desconhecidos seguem como estão (datanodes etc. caem
// na checagem de HTML mais abaixo).
const { resolverHoster, resolverMagnet } = require("./hosters")

async function resolverHttp(url, direto = false) {
  // direto=true: a URL JÁ é o arquivo (veio do debrid/CDN). Sem isto o link
  // do CDN do TorBox era mandado de volta pro createwebdownload deles —
  // "site not supported" — e o download morria depois de cachear.
  if (direto) return { url }
  try {
    const r = await resolverHoster(url)
    if (r?.url) return r
  } catch {
    // Debrid rejeitou (hoster fora do catálogo deles): NÃO é erro fatal.
    // Cai na URL crua abaixo — se for link direto de verdade, baixa; se for
    // página HTML de hoster, a checagem de content-type explica ao usuário.
  }
  return { url }
}

function nomeArquivoHttp(url, cd) {
  const m = /filename\*?=(?:UTF-8''|")?([^";]+)/i.exec(String(cd || ""))
  let nome = m ? decodeURIComponent(m[1].replace(/"/g, "")) : ""
  if (!nome) {
    try {
      nome = decodeURIComponent(new URL(url).pathname.split("/").pop() || "")
    } catch {}
  }
  nome = nome.replace(/[\\/:*?"<>|]/g, "_").replace(/\.\./g, "_").replace(/^\./, "_").trim()
  return nome || "download.bin"
}

async function startHttp({ gameId, url, savePath, title, cover, direto }) {
  const fail = (error) => {
    const message = String(error || "falha ao iniciar download HTTP")
    markStateError(gameId, message)
    return { ok: false, queued: true, error: message }
  }
  const res = await resolverHttp(url, direto)
  if (res.erro) return fail(res.erro)
  const direta = normalizeDownloadUri(res.url)
  if (!direta || !/^https?:\/\//i.test(direta)) return fail("URI de download inválida")
  const headersExtra = res.headers || {}
  // Resume: o .part existente vira Range: bytes=N-.
  const it = readState().find((i) => i.gameId === gameId)
  const fileName = it?.fileName || ""
  const part = fileName ? path.join(savePath, fileName + ".part") : ""
  const jaBaixado = part && fs.existsSync(part) ? fs.statSync(part).size : 0
  const headers = { "User-Agent": "arcadia", ...headersExtra }
  // Sempre com Range: a resposta 206 revela suporte a ranges (download
  // paralelo por segmentos); 200 = servidor sem range → stream único.
  headers.Range = `bytes=${jaBaixado}-`

  let r
  try {
    r = await fetchPublicHttp(direta, { headers })
  } catch (e) {
    return fail(e.message || e)
  }
  if (!r.ok && r.status !== 206) return fail(`HTTP ${r.status}`)
  const ctype = String(r.headers.get("content-type") || "")
  if (ctype.includes("text/html")) {
    r.body?.cancel().catch(() => {})
    return fail("hoster não suportado (respondeu página HTML — precisa de resolvedor)")
  }

  const nomeFinal = fileName || nomeArquivoHttp(direta, r.headers.get("content-disposition"))
  const partFinal = path.join(savePath, nomeFinal + ".part")
  const rangeTotal = /\/(\d+)\s*$/.exec(String(r.headers.get("content-range") || ""))
  const total = rangeTotal
    ? Number(rangeTotal[1])
    : jaBaixado + (Number(r.headers.get("content-length")) || 0)

  // Download paralelo: servidor aceita range (206) e arquivo é grande.
  // CDNs de debrid limitam POR CONEXÃO — 8 segmentos ≈ 8x a velocidade.
  if (r.status === 206 && total >= 64 * 1024 * 1024) {
    await r.body?.cancel().catch(() => {})
    return startHttpMulti({
      gameId,
      direta,
      headersExtra,
      savePath,
      nomeFinal,
      total,
      title,
      cover,
      it,
    })
  }

  const ctrl = new AbortController()
  const h = { ctrl, bytes: jaBaixado, total }
  httpDls.set(gameId, h)

  // Grava o estado (engine http + nome do arquivo) antes de começar a baixar.
  // upsertState also emits immediately, so a mounted Downloads view does not
  // have to wait for the one-second polling tick.
  upsertState({
    ...(it || {}),
    gameId,
    url,
    savePath,
    title: title || it?.title || nomeFinal,
    engine: "http",
    fileName: nomeFinal,
    pausado: false,
    completo: false,
    erro: "",
    cacheando: false,
    cover: cover || it?.cover || "",
    fileSize: total || 0,
  })
  armarPolling()

  // Stream -> disco. Abort no pause: o .part fica para o resume.
  const contador = new (require("stream").Transform)({
    transform(chunk, _e, cb) {
      h.bytes += chunk.length
      cb(null, chunk)
    },
  })
  pipeline(Readable.fromWeb(r.body), contador, fs.createWriteStream(partFinal, { flags: "a" }), {
    signal: ctrl.signal,
  })
    .then(() => {
      httpDls.delete(gameId)
      if (h.bytes >= h.total && h.total > 0) {
        try {
          fs.renameSync(partFinal, path.join(savePath, nomeFinal))
        } catch {}
        const l = readState()
        const item = l.find((i) => i.gameId === gameId)
        if (item) {
          item.completo = true
          item.pausado = false
          item.erro = ""
          item.fileSize = h.total || item.fileSize
          writeState(l)
          emitProgress()
        }
      }
    })
    .catch((error) => {
      httpDls.delete(gameId) // abort/erro de rede: o .part garante o resume
      if (h.reason === "cancel") return
      if (h.reason === "pause") {
        if (!httpDls.has(gameId)) patchState(gameId, { pausado: true, erro: "" })
        return
      }
      markStateError(gameId, error?.message || "download interrompido")
    })
  return { ok: true }
}

// Download paralelo por segmentos (estilo aria2): N partes com Range
// independente, cada uma em <nome>.part<i> — resume natural pelo tamanho
// do arquivo no disco. Ao final, concatena tudo no arquivo definitivo.
const SEG_ALVO = 8
const SEG_MIN_BYTES = 32 * 1024 * 1024 // 32MB por segmento

async function startHttpMulti({
  gameId,
  direta,
  headersExtra,
  savePath,
  nomeFinal,
  total,
  title,
  cover,
  it,
}) {
  const n = Math.max(2, Math.min(SEG_ALVO, Math.floor(total / SEG_MIN_BYTES)))
  const tam = Math.ceil(total / n)
  const partes = Array.from({ length: n }, (_, i) => ({
    ini: i * tam,
    fim: Math.min(total, (i + 1) * tam) - 1,
    arq: path.join(savePath, `${nomeFinal}.part${i}`),
  }))
  partes.forEach((p) => {
    p.alvo = p.fim - p.ini + 1
  })

  const ctrl = new AbortController()
  const h = { ctrl, total, bytes: 0, multi: true }
  httpDls.set(gameId, h)
  // bytes vêm do disco (funciona mesmo depois de reabrir o app).
  h.timer = setInterval(() => {
    try {
      h.bytes = partes.reduce((s, p) => s + (fs.existsSync(p.arq) ? fs.statSync(p.arq).size : 0), 0)
    } catch {}
  }, 1000)

  // Estado (engine http) — idêntico ao modo stream único.
  upsertState({
    ...(it || {}),
    gameId,
    url: direta,
    savePath,
    title: title || it?.title || nomeFinal,
    engine: "http",
    fileName: nomeFinal,
    pausado: false,
    completo: false,
    erro: "",
    cacheando: false,
    cover: cover || it?.cover || "",
    fileSize: total || 0,
  })
  armarPolling()

  const limpar = () => {
    clearInterval(h.timer)
    httpDls.delete(gameId)
  }
  const marcarCompleto = () => {
    const l = readState()
    const item = l.find((i) => i.gameId === gameId)
    if (item) {
      item.completo = true
      item.pausado = false
      item.erro = ""
      item.fileSize = total
      writeState(l)
      emitProgress()
    }
  }

  const baixarParte = async (p) => {
    const ja = fs.existsSync(p.arq) ? fs.statSync(p.arq).size : 0
    if (ja >= p.alvo) return
    const rr = await fetchPublicHttp(direta, {
      headers: { "User-Agent": "arcadia", ...headersExtra, Range: `bytes=${p.ini + ja}-${p.fim}` },
      signal: ctrl.signal,
    })
    if (rr.status !== 206 && !rr.ok) throw new Error(`HTTP ${rr.status}`)
    await pipeline(Readable.fromWeb(rr.body), fs.createWriteStream(p.arq, { flags: "a" }), {
      signal: ctrl.signal,
    })
  }

  ;(async () => {
    try {
      await Promise.all(partes.map(baixarParte))
      // Concatena os segmentos no arquivo final.
      const destino = path.join(savePath, nomeFinal)
      const out = fs.createWriteStream(destino)
      for (const p of partes) {
        await pipeline(fs.createReadStream(p.arq), out, { end: false })
        fs.unlinkSync(p.arq)
      }
      out.end()
      await new Promise((res) => out.on("finish", res))
      marcarCompleto()
    } catch (error) {
      // Abortos intencionais de pausar/cancelar não são falhas de rede.
      if (h.reason === "pause") {
        if (!httpDls.has(gameId)) patchState(gameId, { pausado: true, erro: "" })
      } else if (h.reason !== "cancel") {
        markStateError(gameId, error?.message || "download interrompido")
      }
    } finally {
      limpar()
    }
  })()
  return { ok: true }
}

// Pasta padrão dos downloads torrent: config torrent_download_path, senão
// DATA_DIR/downloads/torrent. Separada dos downloads Steam (DepotDownloader)
// e Epic (legendary) de propósito — os três subsistemas nunca pisam na mesma
// pasta nem no mesmo id (daqui saem ids "tor:...").
function defaultSavePath() {
  try {
    const cfg = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "config.json"), "utf-8"))
    const p = String(cfg.torrent_download_path || "").trim()
    if (p) return p
  } catch {}
  return path.join(DATA_DIR, "downloads", "torrent")
}

async function start({ gameId, url, savePath, fileIndices, title, cover } = {}) {
  if (!gameId || !url) return { ok: false, error: "missing_args" }

  const stableId = normalizeTorrentId(gameId)
  const safeUrl = normalizeDownloadUri(url)
  if (!stableId) return { ok: false, error: "invalid_game_id" }
  if (!safeUrl) return { ok: false, error: "invalid_download_uri" }
  gameId = stableId
  url = safeUrl
  savePath = typeof savePath === "string" && savePath.trim() ? savePath : defaultSavePath()

  try {
    fs.mkdirSync(savePath, { recursive: true })
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }

  const previous = readState().find((item) => item.gameId === gameId)
  const sameUrl = previous?.url === url
  // Re-adicionar este gameId cancela o guard de "cancelado" (senão o tick
  // continuaria ignorando um item legitimamente recomeçado).
  marcadosCancelados.delete(gameId)
  const base = {
    ...(sameUrl ? previous : {}),
    gameId,
    url,
    savePath,
    fileIndices:
      fileIndices !== undefined
        ? Array.isArray(fileIndices)
          ? fileIndices
          : null
        : (previous?.fileIndices ?? null),
    title: title || previous?.title || "",
    cover: cover || previous?.cover || "",
    pausado: false,
    completo: false,
    erro: "",
  }
  // A changed URI is a new payload for the same stable game key. Do not reuse
  // an HTTP filename/engine from the previous source option.
  if (!sameUrl) {
    delete base.engine
    delete base.fileName
    delete base.fileSize
    delete base.cacheando
  }

  // URL http(s): register the item before resolving/fetching. A slow hoster or
  // an unavailable network must not make the Retro click disappear from
  // Downloads; startHttp updates this placeholder with its real filename.
  if (/^https?:\/\//i.test(url)) {
    upsertState({ ...base, engine: "http", cacheando: false })
    armarPolling()
    return startHttp({ gameId, url, savePath, title, cover })
  }

  // Política 2026-09-12: sem debrid, release por torrent não inicia — nem
  // debrid (não tem), nem aria2/P2P. O clique fica visível com o motivo.
  if (!temDebridConfigurado()) {
    upsertState({ ...base, erro: ERRO_DEBRID })
    armarPolling()
    return { ok: false, queued: true, error: ERRO_DEBRID }
  }

  // Magnet: com QUALQUER debrid configurado, o torrent baixa no servidor do
  // debrid. Enquanto cacheia, o item fica no Downloads como "cacheando" (0
  // MB) — SEM fallback para P2P: quem paga debrid quer o debrid. Quando o
  // link direto sai, o download HTTP começa sozinho.
  if (temDebridConfigurado()) {
    const ctrl = new AbortController()
    debridJobs.set(gameId, ctrl)
    upsertState({
      ...base,
      engine: "debrid",
      cacheando: true,
      pausado: false,
      completo: false,
    })
    armarPolling()
    // Job de fundo: espera o debrid cachear e então inicia o download HTTP.
    ;(async () => {
      try {
        const r = await resolverMagnet(String(url), { signal: ctrl.signal })
        if (!r?.url) return // abortado (cancel)
        if (!readState().some((i) => i.gameId === gameId)) return // cancelado
        const r2 = await startHttp({ gameId, url: r.url, savePath, title, cover, direto: true })
        if (!r2?.ok) throw new Error(r2?.error || "falha ao iniciar download")
      } catch (e) {
        console.warn("arcadia: debrid magnet falhou:", String(e.message || e))
        // Marca erro no item para não ficar "cacheando" para sempre.
        markStateError(gameId, e.message || e)
      } finally {
        debridJobs.delete(gameId)
      }
    })()
    return { ok: true }
  }

  // Windows nativo: sem worker Python/libtorrent — o magnet vai pelo aria2c.
  // Mantém a mesma forma de estado (progresso/velocidade/fileName) da UI.
  if (process.platform === "win32") {
    const aria2 = getAria2()
    const failAria = (error) => {
      markStateError(gameId, error)
      armarPolling()
      return { ok: false, queued: true, error }
    }
    const err = "engine de torrent indisponível no Windows"
    if (!aria2) return failAria(err)
    try {
      const pronto = await aria2.ensure()
      if (!pronto.ok) throw new Error(pronto.error || "falha ao preparar o aria2")
      const gid = await aria2.addMagnet(String(url), { dir: savePath, fileIndices })
      ariaJobs.set(gameId, gid)
      upsertState({
        ...base,
        engine: "aria2",
        cacheando: false,
        pausado: false,
        completo: false,
        erro: "",
      })
      armarPolling()
      return { ok: true }
    } catch (e) {
      return failAria(String(e.message || e))
    }
  }

  // Persist the placeholder before dependency/worker checks. This makes an
  // unsuccessful click observable in Downloads and keeps retry/cancel tied to
  // the same stable ID instead of losing the user's request.
  const torrentItem = { ...base }
  delete torrentItem.engine
  delete torrentItem.cacheando
  upsertState(torrentItem)
  if (!(await libtorrentDisponivel())) {
    const error = "libtorrent não instalado (sudo pacman -S libtorrent-rasterbar)"
    markStateError(gameId, error)
    armarPolling()
    return { ok: false, queued: true, error }
  }
  try {
    await rpc("action", {
      action: "start",
      game_id: gameId,
      url,
      save_path: savePath,
      file_indices: fileIndices ?? null,
    })
    patchState(gameId, { erro: "", pausado: false, completo: false })
    armarPolling()
    return { ok: true }
  } catch (e) {
    const error = String(e.code || e.message || e)
    markStateError(gameId, error)
    armarPolling()
    return { ok: false, queued: true, error }
  }
}

function normId(gameId) {
  return normalizeTorrentId(gameId)
}

async function pause(gameId) {
  gameId = normId(gameId)
  if (!gameId) return { ok: false, error: "invalid_game_id" }
  try {
    const current = readState().find((item) => item.gameId === gameId)
    const dj = debridJobs.get(gameId)
    const h = httpDls.get(gameId)
    const gidAria = ariaJobs.get(gameId)
    if (current?.erro && !dj && !h && !gidAria) {
      patchState(gameId, { pausado: true })
      return { ok: true }
    }
    if (dj) {
      dj.abort()
      debridJobs.delete(gameId)
    } else if (h) {
      h.reason = "pause"
      h.ctrl.abort()
      httpDls.delete(gameId)
    } else if (gidAria) {
      await getAria2()?.pause(gidAria)
    } else if (current?.engine === "aria2") {
      // Sem daemon vivo (reinício do app): nada a pausar no engine.
    } else {
      await rpc("action", { action: "pause", game_id: String(gameId) })
    }
    const lista = readState()
    const it = lista.find((i) => i.gameId === String(gameId))
    if (it) {
      it.pausado = true
      writeState(lista)
      emitProgress()
    }
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e.code || e.message || e) }
  }
}

async function resume(gameId) {
  gameId = normId(gameId)
  if (!gameId) return { ok: false, error: "invalid_game_id" }
  const it = readState().find((i) => i.gameId === gameId)
  if (!it) return { ok: false, error: "download não encontrado" }
  const gidAria = ariaJobs.get(gameId)
  // Política 2026-09-12: item P2P não retoma sem debrid (nem o unpause).
  if (it.engine === "aria2" && !temDebridConfigurado()) {
    return { ok: false, error: ERRO_DEBRID }
  }
  if (it.engine === "aria2" && gidAria) {
    try {
      await getAria2()?.unpause(gidAria)
      patchState(gameId, { pausado: false })
      armarPolling()
      return { ok: true }
    } catch {
      // Daemon caiu: cai no start(), que readiciona o magnet (o aria2
      // re-verifica o que já está no disco e continua de onde parou).
      ariaJobs.delete(gameId)
    }
  }
  return start({ ...it, gameId: it.gameId })
}

async function cancel(gameId) {
  gameId = normId(gameId)
  if (!gameId) return { ok: false, error: "invalid_game_id" }
  // Um tick em voo pode ter lido o estado ANTES deste cancel — o guard evita
  // que ele reescreva o item (ressuscitando o download na UI).
  marcadosCancelados.add(gameId)
  try {
    const current = readState().find((item) => item.gameId === gameId)
    const dj = debridJobs.get(gameId)
    const h = httpDls.get(gameId)
    const gidAria = ariaJobs.get(gameId)
    if (
      current &&
      !dj &&
      !h &&
      !gidAria &&
      (current.erro ||
        current.engine === "http" ||
        current.engine === "debrid" ||
        current.engine === "aria2")
    ) {
      writeState(readState().filter((item) => item.gameId !== gameId))
      emitProgress()
      return { ok: true }
    }
    if (dj) {
      dj.abort()
      debridJobs.delete(gameId)
    } else if (h) {
      h.reason = "cancel"
      h.ctrl.abort()
      httpDls.delete(gameId)
    } else if (gidAria) {
      await getAria2()?.remove(gidAria)
      ariaJobs.delete(gameId)
      ultimoVivo.delete(gameId)
    } else {
      await rpc("action", { action: "cancel", game_id: String(gameId) }).catch(() => {})
    }
    const it = readState().find((i) => i.gameId === gameId)
    // Cancelar HTTP apaga os .part (stream único e segmentos).
    if (it?.engine === "http" && it.fileName) {
      try {
        fs.rmSync(path.join(it.savePath, it.fileName + ".part"), { force: true })
        for (let i = 0; i < SEG_ALVO; i++) {
          fs.rmSync(path.join(it.savePath, `${it.fileName}.part${i}`), { force: true })
        }
      } catch {}
    }
    writeState(readState().filter((i) => i.gameId !== String(gameId)))
    emitProgress()
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e.code || e.message || e) }
  }
}

async function files(magnet, timeoutMs) {
  const safeMagnet = normalizeDownloadUri(magnet)
  if (!safeMagnet || !/^magnet:/i.test(safeMagnet)) {
    return { ok: false, error: "invalid_download_uri" }
  }
  if (!(await libtorrentDisponivel())) {
    return {
      ok: false,
      error:
        process.platform === "win32"
          ? "lista de arquivos indisponível no Windows (engine aria2)"
          : "libtorrent não instalado (sudo pacman -S libtorrent-rasterbar)",
    }
  }
  try {
    return {
      ok: true,
      ...(await rpc("torrent_files", { magnet: safeMagnet, timeout_ms: timeoutMs }, 130000)),
    }
  } catch (e) {
    return { ok: false, error: String(e.code || e.message || e) }
  }
}

async function setLimit(bytesPerSecond) {
  try {
    await rpc("action", {
      action: "set_download_limit",
      max_download_speed_bytes_per_second: bytesPerSecond,
    })
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e.code || e.message || e) }
  }
}

function list() {
  return readState()
}

// Retoma downloads que estavam ativos quando o app fechou. Chamado no boot.
async function retomar() {
  const pendentesDl = readState().filter((i) => !i.completo && !i.pausado)
  if (!pendentesDl.length) return
  // Só itens que dependem do worker Python exigem libtorrent. Magnet no win32
  // usa aria2 e NÃO pode ser bloqueado pelo gate do libtorrent.
  const usaWorker = pendentesDl.some((i) => {
    if (i.engine === "http" || i.engine === "debrid" || i.engine === "aria2") return false
    if (process.platform === "win32" && /^magnet:/i.test(String(i.url || ""))) return false
    return true
  })
  if (usaWorker && !(await libtorrentDisponivel())) return
  for (const it of pendentesDl) {
    try {
      if (it.engine === "http") {
        await startHttp(it)
      } else if (it.engine === "debrid" || it.engine === "aria2") {
        // Espera de cache/interrupção pelo fechamento do app: recomeça.
        await start(it)
      } else if (process.platform === "win32" && /^magnet:/i.test(String(it.url || ""))) {
        // Item legado (sem engine) de magnet no Windows: vai pelo aria2.
        await start(it)
      } else {
        const stableId = normalizeTorrentId(it.gameId)
        const safeUrl = normalizeDownloadUri(it.url)
        if (!stableId || !safeUrl) {
          markStateError(it.gameId, "URI de download inválida")
          continue
        }
        await rpc("action", {
          action: "start",
          game_id: stableId,
          url: safeUrl,
          save_path: it.savePath,
          file_indices: it.fileIndices ?? null,
        })
      }
    } catch {}
  }
  armarPolling()
}

module.exports = {
  start,
  pause,
  resume,
  cancel,
  files,
  setLimit,
  list,
  retomar,
  temDebridConfigurado,
  httpBps,
  normalizeDownloadUri,
  normalizeTorrentId,
  // Encerramento do app: derruba o daemon aria2 (Windows) se estiver vivo.
  shutdown: () => {
    try {
      getAria2()?.stop()
    } catch {}
  },
  onProgress: (cb) => {
    onProgress = cb
  },
}
