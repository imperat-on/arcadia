"use strict"

// Preflight local de firmware/BIOS inspirado no Hydra. O módulo só lê
// metadados e arquivos locais: não baixa, não executa emuladores e não segue
// symlinks.
const fsDefault = require("node:fs")
const path = require("node:path")
const os = require("node:os")

const PS1_MIN = 256 * 1024
const PS1_MAX = 16 * 1024 * 1024
const PS2_MIN = 4 * 1024 * 1024
const PS2_MAX = 8 * 1024 * 1024
const PS1_SIGNATURE = Buffer.from("Sony Computer Entertainment", "latin1")
const PS2_RESET = Buffer.from("RESET", "latin1")
const PS2_ROMVER = Buffer.from("ROMVER", "latin1")
const MAX_READ = 512 * 1024
const PREFLIGHT_CODES = Object.freeze({
  EMULATOR_UNKNOWN: "EMULATOR_UNKNOWN",
  BIOS_NOT_CONFIGURED: "BIOS_NOT_CONFIGURED",
})

function normalizeAbsolute(value) {
  if (typeof value !== "string" || !value.trim() || value.includes("\u0000")) return ""
  const raw = value.trim()
  if (!path.isAbsolute(raw) || raw.split(path.sep).includes("..")) return ""
  return path.normalize(raw)
}

function hasSymlinkComponent(file, fsImpl) {
  const absolute = path.resolve(file)
  const root = path.parse(absolute).root
  let current = root
  const relative = path.relative(root, absolute)
  for (const part of relative ? relative.split(path.sep) : []) {
    current = path.join(current, part)
    try {
      if (fsImpl.lstatSync(current).isSymbolicLink()) return true
    } catch {
      return false
    }
  }
  return false
}

function safeDirectory(value, fsImpl) {
  const normalized = normalizeAbsolute(value)
  if (!normalized || hasSymlinkComponent(normalized, fsImpl)) return ""
  try {
    const stat = fsImpl.lstatSync(normalized)
    return stat.isDirectory() && !stat.isSymbolicLink() ? normalized : ""
  } catch {
    return ""
  }
}

function safeFile(value, fsImpl) {
  if (hasSymlinkComponent(value, fsImpl)) return false
  try {
    const stat = fsImpl.lstatSync(value)
    return stat.isFile() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function readIniValue(file, sectionName, keyName, root, fsImpl) {
  if (!safeFile(file, fsImpl)) return ""
  try {
    const lines = fsImpl.readFileSync(file, "utf8").split(/\r?\n/)
    let section = ""
    for (const line of lines) {
      const match = /^\s*\[([^\]]+)\]\s*$/.exec(line)
      if (match) {
        section = match[1].trim().toLowerCase()
        continue
      }
      if (section !== sectionName.toLowerCase()) continue
      // Escape special regex characters in keyName to prevent ReDoS
      const escaped = keyName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const value = new RegExp(`^\\s*${escaped}\\s*=(.*)$`, "i").exec(line)?.[1]?.trim()
      if (!value) continue
      const absolute = normalizeAbsolute(value)
      return absolute || path.normalize(path.join(root, value))
    }
  } catch {}
  return ""
}

function executableDir(executablePath) {
  const normalized = normalizeAbsolute(executablePath)
  return normalized ? path.dirname(normalized) : ""
}

function uniqueExistingDirectories(values, fsImpl) {
  const seen = new Set()
  const out = []
  for (const value of values) {
    const directory = safeDirectory(value, fsImpl)
    if (!directory || seen.has(directory)) continue
    seen.add(directory)
    out.push(directory)
  }
  return out
}

function biosCandidates(emulatorId, executablePath, manualPath, home, fsImpl) {
  const manual = normalizeAbsolute(manualPath)
  const executable = executableDir(executablePath)
  const candidates = manual ? [manual] : []
  if (emulatorId === "duckstation") {
    const configFiles = [
      path.join(home, ".local", "share", "duckstation", "settings.ini"),
      path.join(home, ".var", "app", "org.duckstation.DuckStation", "data", "duckstation", "settings.ini"),
    ]
    for (const file of configFiles) {
      const root = path.dirname(file)
      const configured = readIniValue(file, "bios", "SearchDirectory", root, fsImpl)
      if (configured) candidates.push(configured)
      candidates.push(path.join(root, "bios"))
    }
  }
  if (emulatorId === "pcsx2") {
    const configFiles = [
      path.join(home, ".config", "PCSX2", "inis", "PCSX2.ini"),
      path.join(home, ".var", "app", "net.pcsx2.PCSX2", "config", "PCSX2", "inis", "PCSX2.ini"),
    ]
    if (executable) configFiles.push(path.join(executable, "inis", "PCSX2.ini"))
    for (const file of configFiles) {
      const root = path.dirname(path.dirname(file))
      const configured = readIniValue(file, "folders", "Bios", root, fsImpl)
      if (configured) candidates.push(configured)
      candidates.push(path.join(root, "bios"))
    }
  }
  if (executable) candidates.push(path.join(executable, "bios"))
  return uniqueExistingDirectories(candidates, fsImpl)
}

function plausibleBios(directory, emulatorId, fsImpl) {
  const limits = emulatorId === "duckstation" ? [PS1_MIN, PS1_MAX] : [PS2_MIN, PS2_MAX]
  let entries
  try {
    entries = fsImpl.readdirSync(directory, { withFileTypes: true })
  } catch {
    return false
  }
  for (const entry of entries || []) {
    if (!entry?.isFile?.() || entry.isSymbolicLink?.()) continue
    const file = path.join(directory, entry.name)
    if (!safeFile(file, fsImpl)) continue
    let size
    try {
      size = Number(fsImpl.lstatSync(file).size)
    } catch {
      continue
    }
    if (!Number.isFinite(size) || size < limits[0] || size > limits[1]) continue
    let handle
    try {
      handle = fsImpl.openSync(file, "r")
      const bytes = Math.min(MAX_READ, emulatorId === "duckstation" ? 64 * 1024 : 512 * 1024)
      const buffer = Buffer.alloc(bytes)
      const read = fsImpl.readSync(handle, buffer, 0, bytes, 0)
      const head = buffer.subarray(0, read)
      if (emulatorId === "duckstation" && head.includes(PS1_SIGNATURE)) return true
      if (emulatorId === "pcsx2" && head.includes(PS2_RESET) && head.includes(PS2_ROMVER)) return true
    } catch {} finally {
      try {
        if (handle !== undefined) fsImpl.closeSync(handle)
      } catch {}
    }
  }
  return false
}

function firmwareCandidates(executablePath, home, fsImpl) {
  const executable = executableDir(executablePath)
  return uniqueExistingDirectories([
    path.join(home, ".config", "rpcs3", "dev_flash", "sys", "external"),
    path.join(home, ".var", "app", "net.rpcs3.RPCS3", "config", "rpcs3", "dev_flash", "sys", "external"),
    executable ? path.join(executable, "dev_flash", "sys", "external") : "",
  ], fsImpl)
}

function hasFirmware(directory, fsImpl) {
  try {
    return fsImpl.readdirSync(directory, { withFileTypes: true }).some(
      (entry) => entry?.isFile?.() && !entry.isSymbolicLink?.(),
    )
  } catch {
    return false
  }
}

function getEmulatorStatus({ emulatorId, executablePath = "", biosPath = "", home = os.homedir(), fsImpl = fsDefault } = {}) {
  const id = typeof emulatorId === "string" ? emulatorId.trim().toLowerCase() : ""
  if (!id) return { ok: false, error: PREFLIGHT_CODES.EMULATOR_UNKNOWN, code: PREFLIGHT_CODES.EMULATOR_UNKNOWN }
  if (id === "duckstation" || id === "pcsx2") {
    const candidates = biosCandidates(id, executablePath, biosPath, home, fsImpl)
    const detectedPath = candidates.find((candidate) => plausibleBios(candidate, id, fsImpl)) || null
    return { ok: true, emulatorId: id, kind: "bios", required: true, installed: Boolean(detectedPath), detectedPath }
  }
  if (id === "rpcs3") {
    const candidates = firmwareCandidates(executablePath, home, fsImpl)
    const detectedPath = candidates.find((candidate) => hasFirmware(candidate, fsImpl)) || null
    return { ok: true, emulatorId: id, kind: "firmware", required: false, installed: Boolean(detectedPath), detectedPath }
  }
  return { ok: true, emulatorId: id, kind: null, required: false, installed: true, detectedPath: null }
}

function preflightEmulator(options = {}) {
  const status = getEmulatorStatus(options)
  if (!status.ok) return status
  if (status.required && !status.installed) {
    return {
      ok: false,
      error: PREFLIGHT_CODES.BIOS_NOT_CONFIGURED,
      code: PREFLIGHT_CODES.BIOS_NOT_CONFIGURED,
      legacyError: "bios_nao_configurado",
      emulatorId: status.emulatorId,
      kind: status.kind,
      detectedPath: null,
    }
  }
  return status
}

module.exports = {
  PS1_MIN,
  PS1_MAX,
  PS2_MIN,
  PS2_MAX,
  PREFLIGHT_CODES,
  normalizeAbsolute,
  getEmulatorStatus,
  preflightEmulator,
}
