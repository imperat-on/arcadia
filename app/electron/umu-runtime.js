"use strict"

const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { getDataDir } = require("./runtime-paths")
const { fetchRede } = require("./httpfetch")

const UMU_VERSION = "1.4.4"
const UMU_ARCHIVE_URL =
  `https://github.com/Open-Wine-Components/umu-launcher/releases/download/${UMU_VERSION}/` +
  `umu-launcher-${UMU_VERSION}-zipapp.tar`
const UMU_ARCHIVE_SHA256 = "eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851"
const UMU_EXECUTABLE_SHA256 = "d0005a58602041229cc467dab03dc0c0b9e8cce09a8145b16b7683244cf17804"
const MAX_ARCHIVE_BYTES = 2 * 1024 * 1024

let installPromise = null

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex")
}

function managedUmuPath(dataDir = getDataDir()) {
  return path.join(dataDir, "bin", "umu", UMU_VERSION, "umu-run")
}

function isExecutable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK)
    return true
  } catch {
    return false
  }
}

function isManagedUmuValid(file = managedUmuPath()) {
  try {
    return isExecutable(file) && sha256(fs.readFileSync(file)) === UMU_EXECUTABLE_SHA256
  } catch {
    return false
  }
}

function externalCandidates(home = os.homedir(), envPath = process.env.PATH || "") {
  const candidates = []
  for (const dir of String(envPath).split(path.delimiter)) {
    if (dir) candidates.push(path.join(dir, "umu-run"))
  }
  candidates.push(
    path.join(home, ".local", "share", "lutris", "runtime", "umu", "umu-run"),
    path.join(home, ".config", "heroic", "tools", "runtimes", "umu", "umu-run"),
    path.join(home, ".config", "heroic", "tools", "runtimes", "umu", "umu_run.py"),
    "/usr/local/share/umu/umu-run",
    "/usr/share/umu/umu-run",
    "/opt/umu/umu-run",
  )
  return [...new Set(candidates)]
}

function findUmuLauncher(options = {}) {
  const managed = managedUmuPath(options.dataDir)
  if (isManagedUmuValid(managed)) return managed
  for (const candidate of externalCandidates(options.home, options.envPath)) {
    if (isExecutable(candidate)) return candidate
  }
  return ""
}

function parseTarSize(block) {
  const raw = block.toString("ascii").replace(/\0.*$/, "").trim()
  if (!/^[0-7]+$/.test(raw)) return 0
  return Number.parseInt(raw, 8)
}

function extractUmuFromTar(archive) {
  for (let offset = 0; offset + 512 <= archive.length; ) {
    const header = archive.subarray(offset, offset + 512)
    if (header.every((byte) => byte === 0)) break
    const name = header.subarray(0, 100).toString("utf8").replace(/\0.*$/, "")
    const size = parseTarSize(header.subarray(124, 136))
    const type = header[156]
    const start = offset + 512
    const end = start + size
    if (end > archive.length) throw new Error("pacote UMU truncado")
    if (name === "umu/umu-run" && (type === 0 || type === 48)) {
      return Buffer.from(archive.subarray(start, end))
    }
    offset = start + Math.ceil(size / 512) * 512
  }
  throw new Error("umu-run não encontrado no pacote oficial")
}

async function installManagedUmu({ dataDir = getDataDir(), fetchImpl = fetchRede } = {}) {
  const target = managedUmuPath(dataDir)
  if (isManagedUmuValid(target)) return { ok: true, path: target, managed: true }

  const response = await fetchImpl(UMU_ARCHIVE_URL, {
    headers: { "User-Agent": "Arcadia Launcher" },
    signal: AbortSignal.timeout(120000),
  })
  if (!response.ok) throw new Error(`download do UMU falhou (HTTP ${response.status})`)
  const archive = Buffer.from(await response.arrayBuffer())
  if (archive.length > MAX_ARCHIVE_BYTES) throw new Error("pacote UMU excedeu o tamanho esperado")
  if (sha256(archive) !== UMU_ARCHIVE_SHA256) throw new Error("SHA-256 do pacote UMU inválido")

  const executable = extractUmuFromTar(archive)
  if (sha256(executable) !== UMU_EXECUTABLE_SHA256) {
    throw new Error("SHA-256 do executável UMU inválido")
  }

  const dir = path.dirname(target)
  const temporary = path.join(dir, `.umu-run-${process.pid}-${Date.now()}.tmp`)
  fs.mkdirSync(dir, { recursive: true })
  try {
    fs.writeFileSync(temporary, executable, { mode: 0o755 })
    fs.chmodSync(temporary, 0o755)
    fs.renameSync(temporary, target)
  } finally {
    try {
      fs.unlinkSync(temporary)
    } catch {}
  }
  return { ok: true, path: target, managed: true }
}

async function ensureUmuLauncher(options = {}) {
  const managed = managedUmuPath(options.dataDir)
  if (isManagedUmuValid(managed)) return { ok: true, path: managed, managed: true }
  if (!installPromise) {
    installPromise = installManagedUmu(options).finally(() => {
      installPromise = null
    })
  }
  try {
    return await installPromise
  } catch (error) {
    const fallback = findUmuLauncher(options)
    if (fallback) {
      return { ok: true, path: fallback, managed: false, warning: String(error?.message || error) }
    }
    return { ok: false, error: `Não foi possível preparar o UMU: ${error?.message || error}` }
  }
}

module.exports = {
  UMU_VERSION,
  UMU_ARCHIVE_URL,
  UMU_ARCHIVE_SHA256,
  UMU_EXECUTABLE_SHA256,
  managedUmuPath,
  isManagedUmuValid,
  findUmuLauncher,
  extractUmuFromTar,
  installManagedUmu,
  ensureUmuLauncher,
}
