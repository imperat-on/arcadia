"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")

const SYSTEMD_UNIT_RE = /^arcadia-game-[0-9]+-[0-9]+\.service$/

function createSystemdUnitName({ pid = process.pid, tokenId } = {}) {
  const ownerPid = Number(pid)
  const id = Number(tokenId)
  if (!Number.isSafeInteger(ownerPid) || ownerPid <= 1) return ""
  if (!Number.isSafeInteger(id) || id <= 0) return ""
  return `arcadia-game-${ownerPid}-${id}.service`
}

function isSystemdUnitName(value) {
  return SYSTEMD_UNIT_RE.test(String(value || ""))
}

function canUseSystemdSession({
  platform = process.platform,
  binExists = () => false,
  env = process.env,
  fsImpl = fsDefault,
} = {}) {
  if (platform !== "linux") return false
  if (!binExists("systemd-run") || !binExists("systemctl")) return false

  const runtimeDir = String(env?.XDG_RUNTIME_DIR || "").trim()
  const dbus = String(env?.DBUS_SESSION_BUS_ADDRESS || "").trim()
  if (dbus) return true
  if (!runtimeDir || !runtimeDir.startsWith("/")) return false
  try {
    return fsImpl.existsSync(path.join(runtimeDir, "bus"))
  } catch {
    return false
  }
}

function buildExternalGamescopeCommand(
  command,
  {
    width = 1920,
    height = 1080,
    fps = 0,
    keepAlive = false,
    systemdUnit = "",
    environmentKeys = [],
  } = {},
) {
  if (!Array.isArray(command) || command.length === 0) {
    throw new Error("comando Gamescope vazio")
  }

  const args = ["-W", String(width || 1920), "-H", String(height || 1080)]
  if (fps) args.push("-r", String(fps))
  if (keepAlive) args.push("--keep-alive")

  let primary = command.map((value) => String(value))
  if (systemdUnit) {
    if (!isSystemdUnitName(systemdUnit)) throw new Error("unidade systemd inválida")
    // The transient service is deliberately the Gamescope primary child.  A
    // normal `systemd-run --scope` client exits when the launcher's main PID
    // exits, which lets Gamescope's reaper kill a handed-off child.  A service
    // with RemainAfterExit plus --wait stays in place while its cgroup still
    // contains the handoff/game processes.
    const keys = Array.from(new Set(environmentKeys))
      .map((key) => String(key || ""))
      .filter((key) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(key))
    primary = [
      "systemd-run",
      "--user",
      ...keys.map((key) => `--setenv=${key}`),
      "--unit",
      systemdUnit,
      "--property=RemainAfterExit=yes",
      // Wine service helpers can ignore SIGTERM. Keep explicit Stop bounded
      // so an owned cgroup cannot pin an empty Gamescope session for 90s.
      "--property=TimeoutStopSec=5s",
      // Keep Proton/game stdout on Arcadia's launch log instead of only in
      // the user journal. RemainAfterExit still keeps this primary alive.
      "--wait",
      "--pipe",
      "--collect",
      "--",
      ...primary,
    ]
  }

  return {
    cmd: ["gamescope", ...args, "--", ...primary],
    processSession: systemdUnit
      ? { type: "systemd", unit: systemdUnit, cgroupRoot: "/sys/fs/cgroup" }
      : null,
  }
}

function systemdStopArgs(unit) {
  if (!isSystemdUnitName(unit)) return null
  return ["--user", "stop", "--no-block", unit]
}

function parseSystemdShow(output) {
  const result = {}
  for (const line of String(output || "").split(/\r?\n/)) {
    const separator = line.indexOf("=")
    if (separator <= 0) continue
    result[line.slice(0, separator)] = line.slice(separator + 1)
  }
  return result
}

function readCgroupPids(
  cgroupPath,
  { cgroupRoot = "/sys/fs/cgroup", fsImpl = fsDefault } = {},
) {
  const value = String(cgroupPath || "")
  if (!value.startsWith("/") || value.includes("\0") || value.split("/").includes("..")) return null

  const root = path.resolve(String(cgroupRoot || "/sys/fs/cgroup"))
  const target = path.resolve(root, `.${value}`)
  if (target !== root && !target.startsWith(root + path.sep)) return null

  try {
    const content = fsImpl.readFileSync(path.join(target, "cgroup.procs"), "utf8")
    return content
      .split(/\s+/)
      .filter((pid) => /^\d+$/.test(pid))
      .map((pid) => Number(pid))
      .filter((pid) => Number.isSafeInteger(pid) && pid > 1)
  } catch {
    return null
  }
}

module.exports = {
  buildExternalGamescopeCommand,
  canUseSystemdSession,
  createSystemdUnitName,
  isSystemdUnitName,
  parseSystemdShow,
  readCgroupPids,
  systemdStopArgs,
}
