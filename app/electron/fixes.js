// Port Node dos fixes do luatools-moon (fixes.lua + crackfix.lua +
// launcherfix.lua + downloader.sh). Responde "existe crack/bypass/online fix
// pra este appid?", baixa o zip e aplica extraindo em install_path, mantendo
// manifests dos DLLs/launchers instalados para o un-fix e launch redirect.
//
// Sem dep externa: curl do sistema + 7zz/unzip do sistema. Cache em disco.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn } = require("child_process")
const { fetchRede } = require("./httpfetch")
const { getDataDir } = require("./runtime-paths")

const HOME = os.homedir()
const DATA_DIR = getDataDir()
const CACHE_DIR = path.join(DATA_DIR, "cache")
const TMP_DIR = path.join(DATA_DIR, "tmp")
const FIXES_INDEX_CACHE = path.join(CACHE_DIR, "fixes-index.json")
const RYUU_INDEX_CACHE = path.join(CACHE_DIR, "ryuu-index.json")
const RYUU_AUTH_FILE = path.join(DATA_DIR, "ryuu_auth.json")
// Formato antigo (texto cru tratado como Authorization: Bearer). O Ryuu não
// aceita Bearer; só X-Auth-Key ou Cookie: session=. Migração transparente:
// se o arquivo antigo existir e for texto, o normaliza como key na hora da
// leitura.
const RYUU_AUTH_FILE_LEGACY = path.join(DATA_DIR, "ryuu_auth.txt")

const REFRESH_TTL_MS = 6 * 60 * 60 * 1000

const INDEX_URL = "https://index.luatools.work/fixes-index.json"
const GENERIC_BASE = "https://files.luatools.work/GameBypasses"
const ONLINE_BASE = "https://files.luatools.work/OnlineFix1"
const RYUU_CATALOG_URL = "https://generator.ryuu.lol/files/fixes.json"
const RYUU_BASE = "https://generator.ryuu.lol/fixes/"

function ensureDirs() {
  fs.mkdirSync(CACHE_DIR, { recursive: true })
  fs.mkdirSync(TMP_DIR, { recursive: true })
}

function readFile(p) {
  try {
    return fs.readFileSync(p, "utf-8")
  } catch {
    return null
  }
}
function writeFile(p, data) {
  ensureDirs()
  fs.writeFileSync(p, data)
}
function fileMtimeMs(p) {
  try {
    return fs.statSync(p).mtimeMs
  } catch {
    return 0
  }
}
function isFresh(p) {
  return fileMtimeMs(p) > 0 && Date.now() - fileMtimeMs(p) < REFRESH_TTL_MS
}

async function fetchText(url, opts = {}) {
  const r = await fetchRede(url, {
    signal: AbortSignal.timeout(15000),
    headers: { "User-Agent": "arcadia" },
    ...opts,
  })
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  return r.text()
}

// Percent-encode RFC-3986 (equivalente a crackfix.url_encode do Lua).
function urlEncode(s) {
  return String(s || "").replace(
    /[^A-Za-z0-9\-._~]/g,
    (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`,
  )
}

// ─── Index de fixes (luatools.work) ─────────────────────────────────────────
async function loadFixesIndex() {
  if (isFresh(FIXES_INDEX_CACHE)) {
    const raw = readFile(FIXES_INDEX_CACHE)
    if (raw) {
      try {
        return JSON.parse(raw)
      } catch {
        /* cai no fetch */
      }
    }
  }
  try {
    const body = await fetchText(INDEX_URL)
    writeFile(FIXES_INDEX_CACHE, body)
    return JSON.parse(body)
  } catch {
    const raw = readFile(FIXES_INDEX_CACHE)
    if (raw) {
      try {
        return JSON.parse(raw)
      } catch {
        return null
      }
    }
    return null
  }
}

async function checkGenericFix(appid) {
  const data = await loadFixesIndex()
  if (!data) return { available: false, status: 0 }
  const n = Number(appid)
  const list = Array.isArray(data.genericFixes) ? data.genericFixes : []
  if (list.some((v) => Number(v) === n)) {
    return { available: true, status: 200, url: `${GENERIC_BASE}/${n}.zip` }
  }
  return { available: false, status: 404 }
}

async function checkOnlineFix(appid) {
  const data = await loadFixesIndex()
  if (!data) return { available: false, status: 0 }
  const n = Number(appid)
  const list = Array.isArray(data.onlineFixes) ? data.onlineFixes : []
  if (list.some((v) => Number(v) === n)) {
    return { available: true, status: 200, url: `${ONLINE_BASE}/${n}.zip` }
  }
  return { available: false, status: 404 }
}

// ─── Ryuu crack/bypass catalogue ────────────────────────────────────────────
async function loadRyuuIndex() {
  if (isFresh(RYUU_INDEX_CACHE)) {
    const raw = readFile(RYUU_INDEX_CACHE)
    if (raw) {
      try {
        return JSON.parse(raw)
      } catch {
        /* refetch */
      }
    }
  }
  try {
    const body = await fetchText(RYUU_CATALOG_URL)
    writeFile(RYUU_INDEX_CACHE, body)
    return JSON.parse(body)
  } catch {
    const raw = readFile(RYUU_INDEX_CACHE)
    if (raw) {
      try {
        return JSON.parse(raw)
      } catch {
        return null
      }
    }
    return null
  }
}

function isHypervisor(entry) {
  if (!entry || typeof entry !== "object") return false
  const badge = String(entry.badge || "").toLowerCase()
  const file = String(entry.file || entry.filename || "").toLowerCase()
  if (badge === "hypervisor" || file.includes("hypervisor")) return true
  for (const b of entry.badges || []) {
    if (String(b).toLowerCase() === "hypervisor") return true
  }
  return false
}

function entryBadge(entry) {
  const b = String(entry.badge || "").toLowerCase()
  if (b) return b
  let first = ""
  for (const x of entry.badges || []) {
    const s = String(x).toLowerCase()
    if (!first) first = s
    if (s === "bypass") return s
  }
  return first
}

function pickEntry(entries) {
  if (!Array.isArray(entries)) return null
  let first = null
  for (const e of entries) {
    if (!e || typeof e !== "object") continue
    const file = e.file || e.filename || e.href
    if (!file) continue
    if (isHypervisor(e)) continue
    if (entryBadge(e) === "bypass") return e
    if (!first) first = e
  }
  return first
}

function sanitizeUrl(href) {
  const s = String(href || "")
  if (!s) return null
  return s.replace(
    /[\s"<>\\^`{|}]/g,
    (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`,
  )
}

async function checkCrackFix(appid) {
  const n = Number(appid)
  if (!n) return { available: false, status: 0 }
  const index = await loadRyuuIndex()
  if (!index) return { available: false, status: 0 }

  let entries = null
  let gameName = null
  if (index && typeof index === "object" && index.fixes && typeof index.fixes === "object") {
    entries = index.fixes[String(n)]
  } else if (Array.isArray(index)) {
    for (const g of index) {
      if (g && Number(g.appid) === n) {
        entries = g.fixes
        gameName = g.name
        break
      }
    }
  }

  const entry = pickEntry(entries)
  if (!entry) return { available: false, status: 404, gameName }
  const file = entry.file || entry.filename
  const url = file ? RYUU_BASE + urlEncode(file) : sanitizeUrl(entry.href)
  return {
    available: true,
    status: 200,
    url,
    file,
    badge: entryBadge(entry),
    requiresAuth: typeof url === "string" && url.startsWith(RYUU_BASE),
    gameName,
  }
}

// ─── Aplicação (download + extract + manifests) ─────────────────────────────
function tmpState(appid) {
  return path.join(TMP_DIR, `fix_${appid}_state.json`)
}
function tmpZip(appid) {
  return path.join(TMP_DIR, `fix_${appid}.zip`)
}
function tmpHeaders(appid) {
  return path.join(TMP_DIR, `fix_${appid}_headers.txt`)
}

function readState(appid) {
  const raw = readFile(tmpState(appid))
  if (!raw) return { status: "done" }
  try {
    return JSON.parse(raw)
  } catch {
    return { status: "downloading" }
  }
}

function writeState(appid, s) {
  writeFile(tmpState(appid), JSON.stringify(s))
}

function cleanupTmp(appid) {
  for (const p of [tmpState(appid), tmpZip(appid), tmpHeaders(appid)]) {
    try {
      fs.unlinkSync(p)
    } catch {
      /* ok */
    }
  }
}

function find7z() {
  if (process.platform === "win32") {
    const candidates = [
      path.join(__dirname, "..", "bin", "7z.exe"),
      "C:\\Program Files\\7-Zip\\7z.exe",
      "C:\\Program Files (x86)\\7-Zip\\7z.exe",
    ]
    for (const c of candidates) if (fs.existsSync(c)) return c
    return null
  }
  const candidates = [
    path.join(__dirname, "..", "bin", "7zz"),
    "/usr/bin/7zz",
    "/usr/bin/7z",
    "/usr/local/bin/7zz",
    "/usr/local/bin/7z",
  ]
  for (const c of candidates) if (fs.existsSync(c)) return c
  return null
}

// Ponto de extensão: mantém o downloader.sh do luatools (já provado). Sem ele
// instalado, fazemos o fluxo em Node (fetch → extract via 7zz/unzip).
async function applyFix({ appid, url, type, installPath }) {
  const a = String(appid)
  // SEGURANCA F3: defense-in-depth — url e installPath sao validados no IPC
  // handler mas este modulo pode ser chamado de outros contextos.
  const FIX_BASES = [
    "https://files.luatools.work/GameBypasses/",
    "https://files.luatools.work/OnlineFix1/",
    "https://generator.ryuu.lol/fixes/",
  ]
  if (typeof url !== "string" || !FIX_BASES.some((b) => url.startsWith(b))) {
    return { ok: false, error: "url rejeitada: fonte desconhecida" }
  }
  if (typeof installPath !== "string" || !path.isAbsolute(installPath) || installPath.includes("..")) {
    return { ok: false, error: "installPath rejeitado" }
  }
  const dest = tmpZip(a)
  ensureDirs()

  const isRyuu = typeof url === "string" && url.startsWith(RYUU_BASE)
  let headerFile = ""
  if (isRyuu) {
    const cred = readRyuuAuth()
    const line = ryuuAuthHeaderLine(cred)
    if (!line) {
      return {
        ok: false,
        errorCode: "authentication",
        error: "Ryuu authentication is required. Add your session cookie or X-Auth-Key first.",
      }
    }
    headerFile = tmpHeaders(a)
    fs.writeFileSync(headerFile, line, { mode: 0o600 })
  }

  writeState(a, { status: "downloading", bytesRead: 0, totalBytes: 0 })

  // Windows: no bash script, return error (fixes not supported on Windows yet)
  if (process.platform === "win32") {
    return { ok: false, error: "Game fixes are not yet supported on Windows" }
  }

  // Spawna o downloader em bg e devolve imediatamente; UI faz poll em status.
  const sevenz = find7z()
  const downloaderPath = path.join(__dirname, "fix_downloader.sh")
  const env = {
    ...process.env,
    MAX_TIME: "0",
    SPEED_LIMIT: "1024",
    SPEED_TIME: "45",
    EXTRACT_NESTED: "1",
    SEVENZ: sevenz || "",
    FIX_TYPE: type || "generic",
    LD_LIBRARY_PATH: "",
    LD_PRELOAD: "",
    LD_AUDIT: "",
    STEAM_RUNTIME_LIBRARY_PATH: "",
    STEAM_ZENITY: "",
  }
  const log = path.join(DATA_DIR, "fix.log")
  const logFd = fs.openSync(log, "a")
  const child = spawn(
    downloaderPath,
    [url, dest, installPath, tmpState(a), "discord(dot)gg/luatools", headerFile],
    { env, detached: true, stdio: ["ignore", logFd, logFd] },
  )
  child.unref()
  fs.closeSync(logFd)
  return { ok: true }
}

function getStatus(appid) {
  const s = readState(appid)
  if (s.status === "extracted") {
    s.status = "done"
    cleanupTmp(appid)
  } else if (s.status === "failed") {
    const out = { ...s }
    cleanupTmp(appid)
    return out
  }
  return s
}

function cancelApply(appid) {
  cleanupTmp(appid)
  return { ok: true }
}

// ─── Manifestos dos fixes aplicados ─────────────────────────────────────────
function manifestPath(installPath, kind) {
  return path.join(installPath, kind === "dlls" ? ".slssteam_fix_dlls" : ".slssteam_fix_launchers")
}

function readManifest(installPath, kind) {
  const raw = readFile(manifestPath(installPath, kind))
  if (!raw) return []
  return raw
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
}

function isFixed(installPath) {
  return fs.existsSync(manifestPath(installPath, "dlls"))
}

// Launcher fix: se manifest existe, aponta o Play pro launcher do crack.
// Formato Proton: "<abs path>" %command%
function buildLauncherRedirect(installPath) {
  const launchers = readManifest(installPath, "launchers")
  if (launchers.length === 0) return null
  const launcher = launchers[0]
  const abs = path.isAbsolute(launcher) ? launcher : path.join(installPath, launcher)
  if (!fs.existsSync(abs)) return null
  return `"${abs}" %command%`
}

function unfix(installPath) {
  if (!installPath) return { ok: false, error: "no_install_path" }
  try {
    fs.unlinkSync(manifestPath(installPath, "dlls"))
  } catch {
    /* ok */
  }
  try {
    fs.unlinkSync(manifestPath(installPath, "launchers"))
  } catch {
    /* ok */
  }
  return { ok: true }
}

// ─── Auth Ryuu ──────────────────────────────────────────────────────────────
// Ryuu aceita session cookie OU X-Auth-Key. NÃO aceita Authorization: Bearer
// (o Arcadia mandava isso antes e o download falhava com 401 mesmo com key
// válida). Espelha o luatools-moon (plugin/backend/ryuu_auth.lua).
function normalizeRyuuAuth(raw) {
  const input = String(raw || "").trim()
  if (!input) return null
  if (/[\r\n]/.test(input) || input.length > 8192) return null
  // "X-Auth-Key: xxx" ou "Cookie: session=xxx" (formato colado do DevTools).
  const hdr = input.match(/^([A-Za-z0-9-]+)\s*:\s*(.+)$/)
  if (hdr) {
    const name = hdr[1].toLowerCase()
    const value = hdr[2].trim()
    if (!value) return null
    if (name === "x-auth-key") return { kind: "key", value }
    if (name === "cookie") {
      const m = value.match(/(?:^|;\s*)session=([^;]+)/)
      return m ? { kind: "session", value: m[1].trim() } : null
    }
    return null
  }
  // "session=xxx" solto.
  const s = input.match(/^session=(.+)$/)
  if (s) return { kind: "session", value: s[1].trim() }
  // Valor cru: X-Auth-Key.
  return { kind: "key", value: input }
}

function ryuuAuthHeaderLine(cred) {
  if (!cred || typeof cred !== "object") return null
  const v = String(cred.value || "").trim()
  if (!v || /[\r\n]/.test(v)) return null
  if (cred.kind === "session") return `Cookie: session=${v}\n`
  if (cred.kind === "key") return `X-Auth-Key: ${v}\n`
  return null
}

function readRyuuAuth() {
  const raw = readFile(RYUU_AUTH_FILE)
  if (raw) {
    try {
      const parsed = JSON.parse(raw)
      if (ryuuAuthHeaderLine(parsed)) return parsed
    } catch {
      /* cai no legado */
    }
  }
  // Migração do formato antigo (texto cru salvo como Bearer): trata como
  // X-Auth-Key. Escreve por cima no formato novo e apaga o legado.
  const legado = readFile(RYUU_AUTH_FILE_LEGACY)
  if (legado && legado.trim()) {
    const cred = { kind: "key", value: legado.trim() }
    try {
      writeRyuuAuth(cred)
      fs.unlinkSync(RYUU_AUTH_FILE_LEGACY)
    } catch {
      /* ok */
    }
    return cred
  }
  return null
}

function writeRyuuAuth(cred) {
  ensureDirs()
  fs.writeFileSync(RYUU_AUTH_FILE, JSON.stringify(cred), { mode: 0o600 })
}

function setRyuuAuth(key) {
  const cred = normalizeRyuuAuth(key)
  if (!cred) return { ok: false, error: "invalid_credential" }
  writeRyuuAuth(cred)
  return { ok: true, kind: cred.kind }
}
function getRyuuAuthStatus() {
  const cred = readRyuuAuth()
  return { configured: Boolean(cred), kind: cred?.kind || null }
}
function clearRyuuAuth() {
  try {
    fs.unlinkSync(RYUU_AUTH_FILE)
  } catch {
    /* ok */
  }
  try {
    fs.unlinkSync(RYUU_AUTH_FILE_LEGACY)
  } catch {
    /* ok */
  }
  return { ok: true }
}

module.exports = {
  checkGenericFix,
  checkOnlineFix,
  checkCrackFix,
  applyFix,
  getStatus,
  cancelApply,
  isFixed,
  buildLauncherRedirect,
  unfix,
  setRyuuAuth,
  getRyuuAuthStatus,
  clearRyuuAuth,
}
