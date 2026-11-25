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

// Gamescope parses these values as C++ integers.  Settings are persisted as
// JSON and can also come from older or hand-edited files, so do not interpolate
// arbitrary values into argv: reject non-integers and values outside the range
// that Gamescope can represent usefully.
const MAX_DIMENSION = 16_384
const MAX_REFRESH = 1_000
const MAX_FRAMERATE_LIMIT = 1_000

function safeInteger(value, { min = 0, max = Number.MAX_SAFE_INTEGER, fallback = 0 } = {}) {
  // Accept the numeric strings produced by older settings files, but do not
  // coerce booleans, null, objects, or floating-point text into valid flags.
  let numeric = value
  if (typeof value === "string") {
    const text = value.trim()
    if (!/^[+-]?\d+$/.test(text)) return fallback
    numeric = Number(text)
  }
  if (!Number.isSafeInteger(numeric) || numeric < min || numeric > max) return fallback
  return numeric
}

function validWindowMode(value) {
  return value === "fullscreen" || value === "borderless" || value === "windowed"
    ? value
    : "windowed"
}

function buildExternalGamescopeCommand(
  command,
  {
    width = 1920,
    height = 1080,
    // Legacy `fps` is the Gamescope nested refresh (`-r`).  Keep it separate
    // from `framerateLimit`, which controls --framerate-limit.
    fps = 0,
    hdr = false,
    windowMode = "windowed",
    framerateLimit = 0,
    keepAlive = false,
    systemdUnit = "",
    environmentKeys = [],
  } = {},
) {
  if (!Array.isArray(command) || command.length === 0) {
    throw new Error("comando Gamescope vazio")
  }

  const outputWidth = safeInteger(width, { min: 1, max: MAX_DIMENSION, fallback: 1920 })
  const outputHeight = safeInteger(height, { min: 1, max: MAX_DIMENSION, fallback: 1080 })
  const nestedRefresh = safeInteger(fps, { min: 1, max: MAX_REFRESH, fallback: 0 })
  const limit = safeInteger(framerateLimit, {
    min: 1,
    max: MAX_FRAMERATE_LIMIT,
    fallback: 0,
  })
  const args = ["-W", String(outputWidth), "-H", String(outputHeight)]
  if (nestedRefresh > 0) args.push("-r", String(nestedRefresh))
  if (limit > 0) args.push("--framerate-limit", String(limit))
  // Only the literal boolean enables HDR.  A stale string such as "false"
  // must not unexpectedly switch a display into HDR mode.
  if (hdr === true) args.push("--hdr-enabled")
  const mode = validWindowMode(windowMode)
  if (mode === "fullscreen") args.push("-f")
  else if (mode === "borderless") args.push("-b")
  // Do not force a persistent pointer lock here.  Gamescope 3.16.25's
  // Wayland backend can lose that lock when the host compositor changes focus
  // and does not re-arm it when focus returns.  That leaves external games
  // with a visible cursor but no usable mouse/keyboard input.  The normal
  // nested-mode pointer handling still follows the game and avoids that bug.
  if (keepAlive === true) args.push("--keep-alive")

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
