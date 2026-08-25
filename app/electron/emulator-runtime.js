"use strict"

// Inspeção read-only do /proc para impedir lançamentos concorrentes do mesmo
// emulador. Nenhum processo é criado e nenhum comando passa por shell.
const fsDefault = require("node:fs")
const path = require("node:path")

const RUNNING_CODES = Object.freeze({
  EMULATOR_ALREADY_RUNNING: "EMULATOR_ALREADY_RUNNING",
})

const PROCESS_NAMES = Object.freeze({
  pcsx2: ["pcsx2", "pcsx2-qt"],
  rpcs3: ["rpcs3"],
  // `dolphin` é o file manager do KDE; o processo do emulador é dolphin-emu.
  dolphin: ["dolphin-emu"],
  ppsspp: ["ppsspp", "ppssppheadless"],
  duckstation: ["duckstation", "duckstation-qt"],
  retroarch: ["retroarch"],
  melonds: ["melonds"],
  desmume: ["desmume"],
})

function normalizeId(value) {
  return typeof value === "string" ? value.trim().toLowerCase() : ""
}

function normalizeNames(emulatorId, executablePath) {
  const names = new Set((PROCESS_NAMES[emulatorId] || []).map((name) => name.toLowerCase()))
  if (typeof executablePath === "string" && executablePath.trim()) {
    names.add(path.basename(executablePath.trim()).toLowerCase())
  }
  return names
}

function readProcessName(procDir, fsImpl) {
  try {
    const comm = fsImpl.readFileSync(path.join(procDir, "comm"), "utf8").trim().toLowerCase()
    if (comm) return comm
  } catch {}
  try {
    return path.basename(fsImpl.readlinkSync(path.join(procDir, "exe"))).toLowerCase()
  } catch {
    return ""
  }
}

function getRunningEmulatorStatus({
  emulatorId,
  executablePath = "",
  procRoot = "/proc",
  fsImpl = fsDefault,
} = {}) {
  const id = normalizeId(emulatorId)
  const names = normalizeNames(id, executablePath)
  if (!id || !names.size) return { ok: true, emulatorId: id, running: false, pid: null }
  let entries
  try {
    entries = fsImpl.readdirSync(procRoot, { withFileTypes: true })
  } catch {
    return { ok: true, emulatorId: id, running: false, pid: null }
  }
  const pids = Array.from(entries || [])
    .filter((entry) => /^\d+$/.test(typeof entry === "string" ? entry : entry?.name || ""))
    .map((entry) => (typeof entry === "string" ? entry : entry.name))
    .sort((a, b) => Number(a) - Number(b))
  for (const pid of pids) {
    const name = readProcessName(path.join(procRoot, pid), fsImpl)
    if (names.has(name)) return { ok: true, emulatorId: id, running: true, pid: Number(pid) }
  }
  return { ok: true, emulatorId: id, running: false, pid: null }
}

function preflightRunningEmulator(options = {}) {
  const status = getRunningEmulatorStatus(options)
  if (!status.ok || !status.running) return status
  return {
    ok: false,
    error: RUNNING_CODES.EMULATOR_ALREADY_RUNNING,
    code: RUNNING_CODES.EMULATOR_ALREADY_RUNNING,
    emulatorId: status.emulatorId,
    pid: status.pid,
  }
}

module.exports = {
  PROCESS_NAMES,
  RUNNING_CODES,
  getRunningEmulatorStatus,
  preflightRunningEmulator,
}
