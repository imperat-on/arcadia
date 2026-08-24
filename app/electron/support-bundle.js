"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const os = require("node:os")

const BUNDLE_VERSION = 1
const MAX_LOG_FILES = 24
const MAX_LOG_BYTES = 512 * 1024

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

function redactText(value, { dataDir = "", homeDir = os.homedir() } = {}) {
  let text = String(value || "")
  for (const [source, replacement] of [
    [dataDir, "<DATA_DIR>"],
    [homeDir, "<HOME>"],
  ]) {
    if (source) text = text.replace(new RegExp(escapeRegExp(source), "g"), replacement)
  }
  return text
    .replace(/((?:access|refresh)[_-]?token|token|password|passwd|api[_-]?key|x-api-key|authorization|cookie|secret)\s*[:=]\s*["']?[^\s,"'}]+["']?/gi, "$1=<REDACTED>")
    .replace(/Bearer\s+[A-Za-z0-9._~+/=-]+/gi, "Bearer <REDACTED>")
}

function safeName(value, fallback = "file") {
  const name = String(value || "").replace(/[^a-z0-9._-]/gi, "_").slice(0, 120)
  return name || fallback
}

function createSupportBundle({ dataDir, fsImpl = fsDefault, homeDir = os.homedir(), now = () => new Date() } = {}) {
  if (!dataDir) throw new Error("dataDir é obrigatório")

  function readLogs() {
    const directory = path.join(dataDir, "logs")
    try {
      return fsImpl
        .readdirSync(directory, { withFileTypes: true })
        .filter((entry) => entry.isFile() && entry.name.endsWith(".log"))
        .sort((a, b) => a.name.localeCompare(b.name))
        .slice(0, MAX_LOG_FILES)
        .map((entry) => {
          const file = path.join(directory, entry.name)
          try {
            const stat = fsImpl.statSync(file)
            const length = Math.min(stat.size, MAX_LOG_BYTES)
            const fd = fsImpl.openSync(file, "r")
            const buffer = Buffer.alloc(length)
            fsImpl.readSync(fd, buffer, 0, length, Math.max(0, stat.size - length))
            fsImpl.closeSync(fd)
            return { name: safeName(entry.name), text: redactText(buffer.toString("utf8"), { dataDir, homeDir }) }
          } catch {
            return null
          }
        })
        .filter(Boolean)
    } catch {
      return []
    }
  }

  function create({ outputDir, report = {} } = {}) {
    if (typeof outputDir !== "string" || !outputDir.trim()) return { ok: false, error: "destino_invalido" }
    const destination = path.resolve(outputDir)
    const id = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`
    const bundleDir = path.join(destination, `arcadia-support-${id}`)
    const temporary = `${bundleDir}.tmp`
    try {
      fsImpl.mkdirSync(destination, { recursive: true })
      fsImpl.rmSync(temporary, { recursive: true, force: true })
      fsImpl.mkdirSync(path.join(temporary, "logs"), { recursive: true })
      const logs = readLogs()
      const files = ["diagnostics.json"]
      fsImpl.writeFileSync(
        path.join(temporary, "diagnostics.json"),
        JSON.stringify(JSON.parse(redactText(JSON.stringify(report), { dataDir, homeDir })), null, 2),
      )
      for (const log of logs) {
        fsImpl.writeFileSync(path.join(temporary, "logs", log.name), log.text)
        files.push(`logs/${log.name}`)
      }
      const manifest = {
        version: BUNDLE_VERSION,
        created_at: now().toISOString(),
        files,
        redacted: true,
      }
      fsImpl.writeFileSync(path.join(temporary, "manifest.json"), JSON.stringify(manifest, null, 2))
      fsImpl.renameSync(temporary, bundleDir)
      return { ok: true, path: bundleDir, files }
    } catch (error) {
      try { fsImpl.rmSync(temporary, { recursive: true, force: true }) } catch {}
      return { ok: false, error: String(error.message || error) }
    }
  }

  return { create, readLogs }
}

module.exports = { BUNDLE_VERSION, MAX_LOG_BYTES, createSupportBundle, redactText, safeName }
