"use strict"

// Engine BitTorrent/magnet para Windows nativo via aria2c.
//
// Por que existe: o worker local do subsistema torrent (torrent_rpc/main.py)
// depende de Python + libtorrent, que não existem no Windows — sem debrid
// configurado, magnet/torrent ficava impossível. O aria2c é um binário único
// (sem runtime) que fala BitTorrent (DHT/trackers) e expõe JSON-RPC local.
//
// Cadeia de suprimento: mesma política do DepotDownloader — URL fixada no host
// oficial de releases, ZIP validado (assinatura PK + teto de tamanho + SHA-256
// pinado) antes de extrair. O daemon escuta apenas em 127.0.0.1, numa porta
// aleatória, e exige um secret aleatório por sessão (JSON-RPC "token:").
// Nenhum argumento de usuário passa por shell.

const fs = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const { execFile: execFileDefault, spawn: spawnDefault } = require("node:child_process")
const { fetchRede: fetchDefault } = require("./httpfetch")

const ARIA2_RELEASE_URL =
  "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip"
// SHA-256 do zip acima, conferido na release oficial em 2026-09-11 (2.475.379 bytes).
const ARIA2_SHA256 = "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288"
const RELEASE_HOST_PREFIX = "https://github.com/aria2/aria2/releases/download/"
const MAX_ARCHIVE_BYTES = 32 * 1024 * 1024
const DOWNLOAD_TIMEOUT_MS = 120_000
const EXE_NAME = "aria2c.exe"

// Infohash do magnet (hex 40 ou base32 32 → hex) — usado p/ deduplicar com o
// daemon (aria2 recusa re-adicionar "InfoHash already registered").
function infohashFromMagnet(magnet) {
  const m = /xt=urn:btih:([a-zA-Z0-9]+)/i.exec(String(magnet || ""))
  if (!m) return ""
  const v = m[1]
  if (/^[0-9a-fA-F]{40}$/.test(v)) return v.toLowerCase()
  if (/^[A-Za-z2-7]{32}$/.test(v)) {
    const alf = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
    let bits = ""
    for (const ch of v.toUpperCase()) {
      const idx = alf.indexOf(ch)
      if (idx < 0) return ""
      bits += idx.toString(2).padStart(5, "0")
    }
    let hex = ""
    for (let i = 0; i + 8 <= bits.length; i += 8) {
      hex += parseInt(bits.slice(i, i + 8), 2).toString(16).padStart(2, "0")
    }
    return hex.length >= 40 ? hex.slice(0, 40) : ""
  }
  return ""
}

// Trackers públicos injetados em magnets. Motivo (caso real 2026-09-11): em
// redes que bloqueiam UDP, DHT e trackers UDP ficam mortos, e magnet "só
// xt+dn" (comum em repacks) trava em 0 peers para sempre. Os HTTP(S) abaixo
// responderam announce com peers nessa rede (verificado em 2026-09-11).
const TRACKERS_PADRAO = [
  "http://tracker.opentrackr.org:1337/announce",
  "https://tracker.zhuqiy.com:443/announce",
  "https://tracker.bt4g.com:443/announce",
  "http://t.nyaatracker.com:80/announce",
  "http://tracker.mywaifu.best:6969/announce",
  "http://nyaa.tracker.wf:7777/announce",
  // UDP: só úteis em redes sem bloqueio; falham em silêncio onde bloqueado.
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://open.stealth.si:80/announce",
]

// Anexa os trackers padrão que ainda não existem no magnet (dedupe contra a
// forma crua e a URL-encoded). Não altera o infohash — só adiciona tr=.
function injetarTrackers(magnet, trackers = TRACKERS_PADRAO) {
  const texto = String(magnet || "")
  if (!/^magnet:/i.test(texto)) return texto
  const faltando = trackers.filter(
    (t) => !texto.includes(t) && !texto.includes(encodeURIComponent(t)),
  )
  if (!faltando.length) return texto
  const sep = texto.includes("?") ? "&" : "?"
  return texto + faltando.map((t, i) => `${i === 0 ? sep : "&"}tr=${encodeURIComponent(t)}`).join("")
}

function erroTexto(error) {
  return String(error?.message || error || "erro desconhecido")
}

function arquivoRegular(fsImpl, file) {
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isFile() && !stat.isSymbolicLink() && stat.size > 0
  } catch {
    return false
  }
}

// O zip tem uma pasta raiz; procura o aria2c.exe em qualquer profundidade.
function acharExe(fsImpl, dir, depth = 0) {
  if (depth > 3) return null
  let entradas
  try {
    entradas = fsImpl.readdirSync(dir, { withFileTypes: true })
  } catch {
    return null
  }
  for (const e of entradas) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) {
      const achado = acharExe(fsImpl, p, depth + 1)
      if (achado) return achado
    } else if (String(e.name).toLowerCase() === EXE_NAME) {
      return p
    }
  }
  return null
}

// Gerencia o binário: baixa/valida/extrai o aria2c.exe para depsDir.
function createAria2Manager({
  depsDir,
  tmpDir,
  fsImpl = fs,
  fetchImpl = fetchDefault,
  execFileImpl = execFileDefault,
  archiveUrl = ARIA2_RELEASE_URL,
  archiveSha256 = ARIA2_SHA256,
  now = () => Date.now(),
  pid = process.pid,
} = {}) {
  if (!depsDir || !tmpDir) throw new Error("depsDir e tmpDir são obrigatórios")

  const exePath = path.join(depsDir, EXE_NAME)
  let inFlight = null

  function installed() {
    return arquivoRegular(fsImpl, exePath)
  }

  async function download() {
    if (installed()) return { ok: true, path: exePath }

    let archivePath = ""
    let stageDir = ""
    try {
      const url = String(archiveUrl || "")
      if (!url.startsWith(RELEASE_HOST_PREFIX)) {
        return { ok: false, error: "aria2: URL de release não permitida" }
      }

      fsImpl.mkdirSync(tmpDir, { recursive: true })
      const response = await fetchImpl(url, {
        headers: { "User-Agent": "arcadia", Accept: "application/octet-stream" },
        signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS),
      })
      if (!response?.ok) {
        return { ok: false, error: `aria2: download HTTP ${response?.status || "?"}` }
      }

      const bytes = Buffer.from(await response.arrayBuffer())
      if (bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) {
        return { ok: false, error: "aria2: resposta não é um zip válido" }
      }
      if (bytes.length > MAX_ARCHIVE_BYTES) {
        return { ok: false, error: "aria2: arquivo de release grande demais" }
      }
      if (archiveSha256) {
        const digest = crypto.createHash("sha256").update(bytes).digest("hex")
        if (digest !== String(archiveSha256).toLowerCase()) {
          return { ok: false, error: "aria2: hash da release não confere" }
        }
      }

      const suffix = `${pid}-${now()}-${Math.random().toString(16).slice(2)}`
      archivePath = path.join(tmpDir, `.aria2-${suffix}.zip`)
      stageDir = path.join(tmpDir, `.aria2-stage-${suffix}`)
      fsImpl.writeFileSync(archivePath, bytes, { mode: 0o600 })
      fsImpl.mkdirSync(stageDir, { recursive: true })
      await new Promise((resolve, reject) => {
        execFileImpl(
          "powershell",
          [
            "-NoProfile",
            "-Command",
            `Expand-Archive -Path "${archivePath}" -DestinationPath "${stageDir}" -Force`,
          ],
          (error, _stdout, stderr) => {
            if (!error) return resolve()
            reject(new Error(String(stderr || "").trim() || erroTexto(error)))
          },
        )
      })

      const fonte = acharExe(fsImpl, stageDir)
      if (!fonte) return { ok: false, error: `aria2: ${EXE_NAME} não encontrado no pacote` }

      fsImpl.mkdirSync(path.dirname(depsDir), { recursive: true })
      fsImpl.rmSync(depsDir, { recursive: true, force: true })
      fsImpl.mkdirSync(depsDir, { recursive: true })
      fsImpl.copyFileSync(fonte, exePath)
      if (!installed()) return { ok: false, error: "aria2: instalação incompleta" }
      return { ok: true, path: exePath }
    } catch (error) {
      return { ok: false, error: `aria2: ${erroTexto(error)}` }
    } finally {
      if (archivePath) {
        try {
          fsImpl.rmSync(archivePath, { force: true })
        } catch {}
      }
      if (stageDir) {
        try {
          fsImpl.rmSync(stageDir, { recursive: true, force: true })
        } catch {}
      }
    }
  }

  function ensure() {
    if (installed()) return Promise.resolve({ ok: true, path: exePath })
    if (inFlight) return inFlight
    inFlight = download().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  return { installed, ensure, exePath }
}

// Engine: um daemon aria2c por sessão + JSON-RPC local. O chamador (torrent.js)
// mapeia gameId -> gid.
function createAria2Engine({
  ensureExe,
  workDir = "",
  spawnImpl = spawnDefault,
  fetchImpl = fetchDefault,
  now = () => Date.now(),
} = {}) {
  if (typeof ensureExe !== "function") throw new Error("ensureExe é obrigatório")

  let child = null
  let port = 0
  let readyPromise = null
  let stderrTail = ""
  const secret = crypto.randomBytes(16).toString("hex")
  let idSeq = 1

  function isAlive() {
    return Boolean(child && child.exitCode === null && !child.killed)
  }

  async function rpc(method, params = [], timeoutMs = 20_000) {
    // Timeout com corrida PRÓPRIA: o fetch do Chromium (net.fetch) pode
    // ignorar o AbortSignal quando o daemon aceita a conexão e nunca responde
    // (caso real 2026-09-11: aria2 com o event loop preso — download seguia a
    // 40MB/s e a UI congelava). Sem a corrida, a chamada pendura para SEMPRE
    // e o polling inteiro congela junto.
    let timer = null
    const res = await Promise.race([
      fetchImpl(`http://127.0.0.1:${port}/jsonrpc`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: idSeq++,
          method,
          params: [`token:${secret}`, ...params],
        }),
        signal: AbortSignal.timeout(timeoutMs),
      }),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("aria2_rpc_timeout")), timeoutMs + 1000)
      }),
    ])
    clearTimeout(timer)
    const j = await res.json()
    if (j?.error) {
      throw Object.assign(new Error(j.error.message || "aria2_rpc_error"), {
        code: j.error.code,
      })
    }
    return j?.result
  }

  async function waitReady(timeoutMs = 10_000) {
    const fim = now() + timeoutMs
    while (now() < fim) {
      if (!isAlive()) return false
      try {
        await rpc("aria2.getVersion", [], 2500)
        return true
      } catch {
        await new Promise((r) => setTimeout(r, 250))
      }
    }
    return false
  }

  function startDaemon(exePath, listenPort) {
    const args = [
      "--enable-rpc=true",
      "--rpc-listen-all=false",
      `--rpc-listen-port=${listenPort}`,
      `--rpc-secret=${secret}`,
      "--quiet=true",
      "--console-log-level=warn",
      "--summary-interval=0",
      "--file-allocation=none",
      "--continue=true",
      "--allow-overwrite=true",
      "--auto-file-renaming=false",
      "--seed-time=0",
      // Verifica no disco o que já existe ao (re)adicionar — paridade com o
      // libtorrent no resume (sem isto, dados parciais seriam rebaixados).
      "--check-integrity=true",
      // Não lê config global (~/.aria2/aria2.conf) — comportamento fixo.
      "--no-conf=true",
      // Bootstrap DHT adicional: os entry points padrão do aria2 (porta 6881)
      // podem estar inacessíveis na rede do usuário; este respondeu em
      // 2026-09-11 (complementa os defaults, não substitui).
      "--dht-entry-point=dht.libtorrent.org:25401",
    ]
    if (workDir) {
      try {
        fs.mkdirSync(workDir, { recursive: true })
      } catch {}
      // Log do daemon para pós-mortem de travamentos (reescrito por sessão).
      const logPath = path.join(workDir, "aria2.log")
      try {
        fs.rmSync(logPath, { force: true })
      } catch {}
      args.push(
        `--dht-file-path=${path.join(workDir, "dht4.dat")}`,
        `--dht-file-path6=${path.join(workDir, "dht6.dat")}`,
        `--log=${logPath}`,
        "--log-level=warn",
      )
    }
    child = spawnImpl(exePath, args, { stdio: ["ignore", "ignore", "pipe"], windowsHide: true })
    port = listenPort
    stderrTail = ""
    child.stderr?.setEncoding?.("utf-8")
    child.stderr?.on?.("data", (c) => {
      stderrTail = (stderrTail + String(c)).slice(-800)
    })
    const invalidar = () => {
      child = null
      readyPromise = null
    }
    child.on("exit", invalidar)
    child.on("error", invalidar)
  }

  async function ensure() {
    if (isAlive() && readyPromise) {
      const ok = await readyPromise
      if (ok) return { ok: true }
    }
    try {
      if (child) child.kill()
    } catch {}
    child = null
    readyPromise = null

    const bin = await ensureExe()
    if (!bin?.ok) return { ok: false, error: bin?.error || "aria2 indisponível" }

    // Até 3 portas aleatórias antes de desistir (colisão/conflito local).
    for (let tentativa = 0; tentativa < 3; tentativa++) {
      const listenPort = 32_000 + Math.floor(Math.random() * 24_000)
      startDaemon(bin.path, listenPort)
      readyPromise = waitReady()
      const ok = await readyPromise
      if (ok) return { ok: true }
      try {
        child?.kill()
      } catch {}
      child = null
      readyPromise = null
    }
    return {
      ok: false,
      error: `aria2 não subiu${stderrTail ? `: ${stderrTail.trim().slice(-200)}` : ""}`,
    }
  }

  // Todos os registros do daemon com este infohash (active/waiting/stopped).
  async function findAllByInfoHash(ih) {
    if (!ih) return []
    const keys = ["gid", "infoHash", "status", "dir"]
    const listas = [
      await rpc("aria2.tellActive", [keys], 10_000).catch(() => []),
      await rpc("aria2.tellWaiting", [0, 100, keys], 10_000).catch(() => []),
      await rpc("aria2.tellStopped", [0, 100, keys], 10_000).catch(() => []),
    ]
    const out = []
    for (const lista of listas) {
      for (const d of lista || []) {
        if (String(d.infoHash || "").toLowerCase() === ih) out.push(d)
      }
    }
    return out
  }

  const mesmoDir = (a, b) => {
    try {
      return path.resolve(String(a || "")).toLowerCase() === path.resolve(String(b || "")).toLowerCase()
    } catch {
      return false
    }
  }

  async function addMagnet(magnet, { dir, fileIndices } = {}) {
    // Magnet sem tracker (ou rede que bloqueia UDP) fica refém do DHT, que
    // pode estar morto — garante a lista pública antes de entregar ao daemon.
    const uri = injetarTrackers(magnet)
    const opts = {
      dir,
      "seed-time": "0",
      "summary-interval": "0",
      "max-connection-per-server": "16",
      split: "16",
    }
    if (Array.isArray(fileIndices) && fileIndices.length) {
      // aria2 usa numeração 1-based; 1,3,5 = arquivos selecionados.
      opts["select-file"] = fileIndices
        .map((n) => Number(n) + 1)
        .filter((n) => Number.isInteger(n) && n > 0)
        .join(",")
    }

    // Mesmo infohash já no daemon (retry/re-download). O aria2 recusa
    // re-adicionar ("already registered", erro ASSÍNCRONO no gid novo), e
    // registros velhos ficam em stopped (metadata) e/ou active (payload).
    // Política: reusa SÓ se for o mesmo dir e ainda estiver rodando;
    // qualquer outro caso → remove os registros e re-adiciona limpo
    // (com check-integrity o re-add retoma/valida o que já está no disco).
    const ih = infohashFromMagnet(magnet)
    const existentes = await findAllByInfoHash(ih)
    let reusar = null
    for (const ex of existentes) {
      const st = String(ex.status || "")
      if ((st === "active" || st === "waiting" || st === "paused") && mesmoDir(ex.dir, dir)) {
        reusar = ex
        continue
      }
      if (st === "active" || st === "waiting" || st === "paused") {
        await rpc("aria2.forceRemove", [ex.gid], 10_000).catch(() => {})
      }
      await rpc("aria2.removeDownloadResult", [ex.gid], 10_000).catch(() => {})
    }
    if (reusar) {
      if (reusar.status === "paused") {
        await rpc("aria2.unpause", [reusar.gid], 10_000).catch(() => {})
      }
      return reusar.gid
    }

    return rpc("aria2.addUri", [[uri], opts], 30_000)
  }

  const TELL_KEYS = [
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "downloadSpeed",
    "uploadSpeed",
    "connections",
    "numSeeders",
    "errorCode",
    "errorMessage",
    "files",
    "bittorrent",
    "followedBy",
  ]

  function mapTell(s) {
    const total = Number(s.totalLength || 0)
    const done = Number(s.completedLength || 0)
    const completo = s.status === "complete"
    const progress = completo ? 1 : total > 0 ? Math.min(1, done / total) : 0
    const primeiro = Array.isArray(s.files) && s.files.length ? s.files[0] : null
    return {
      progress,
      bytesDownloaded: done,
      fileSize: total,
      downloadSpeed: Number(s.downloadSpeed || 0),
      uploadSpeed: Number(s.uploadSpeed || 0),
      numPeers: Number(s.connections || 0),
      numSeeds: Number(s.numSeeders || 0),
      state: String(s.status || ""),
      // Reflete o estado REAL do engine no item — o polling persiste `pausado`
      // a partir daqui (evita o tick sobrescrever o flag com snapshot velho).
      pausado: s.status === "paused",
      folderName: String(s.bittorrent?.name || ""),
      fileName: primeiro?.path ? path.basename(primeiro.path) : "",
      erro:
        s.status === "error"
          ? String(s.errorMessage || `aria2 erro ${s.errorCode || ""}`).trim()
          : "",
    }
  }

  async function tell(gid) {
    let atual = gid
    let s = await rpc("aria2.tellStatus", [atual, TELL_KEYS], 15_000)
    // Magnet: o metadata é um download separado ([METADATA]...) que "segue"
    // para o download real (followedBy). Segue a cadeia antes de reportar.
    for (let i = 0; i < 3 && s?.followedBy?.length; i++) {
      atual = s.followedBy[0]
      s = await rpc("aria2.tellStatus", [atual, TELL_KEYS], 15_000)
    }
    const mapped = mapTell(s)
    mapped.gid = atual
    // Fase de metadata (ainda é o .torrent, não o payload): reporta 0% e NUNCA
    // "completo" — senão o polling marcaria concluído com 10KB de metadata.
    if (s.status !== "error" && s.status !== "removed" && mapped.fileName.startsWith("[METADATA]")) {
      return {
        ...mapped,
        progress: 0,
        bytesDownloaded: 0,
        fileSize: 0,
        downloadSpeed: 0,
        completo: false,
        state: "active",
      }
    }
    return mapped
  }

  async function pause(gid) {
    await rpc("aria2.forcePause", [gid], 10_000)
  }

  async function unpause(gid) {
    await rpc("aria2.unpause", [gid], 10_000)
  }

  async function remove(gid) {
    try {
      await rpc("aria2.forceRemove", [gid], 10_000)
    } catch {}
    try {
      await rpc("aria2.removeDownloadResult", [gid], 10_000)
    } catch {}
  }

  // Sonda de saúde do daemon (RPC barato, com teto próprio): true = falando.
  async function probe(timeoutMs = 4000) {
    try {
      await rpc("aria2.getVersion", [], timeoutMs)
      return true
    } catch {
      return false
    }
  }

  function stop() {
    const alvo = child
    try {
      alvo?.kill()
    } catch {}
    child = null
    readyPromise = null
    // Um daemon travado em I/O de kernel (WaitReason=Executive) demora para
    // encerrar depois do TerminateProcess — registra para diagnóstico.
    if (alvo) {
      const t = setTimeout(() => {
        try {
          if (alvo.exitCode === null) {
            console.warn("[torrent] daemon aria2 ainda vivo 5s após o kill (preso em I/O de kernel?)")
          }
        } catch {}
      }, 5000)
      t?.unref?.()
    }
  }

  return { ensure, addMagnet, tell, pause, unpause, remove, stop, isAlive, probe }
}

module.exports = {
  createAria2Manager,
  createAria2Engine,
  TRACKERS_PADRAO,
  injetarTrackers,
  ARIA2_RELEASE_URL,
  ARIA2_SHA256,
  EXE_NAME,
}
