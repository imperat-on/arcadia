"use strict"

const fsDefault = require("node:fs")
const osDefault = require("node:os")
const path = require("node:path")

const DIAGNOSTICS_VERSION = 1

function createDiagnosticsService({
  dataDir,
  fsImpl = fsDefault,
  osImpl = osDefault,
  now = () => new Date(),
  appVersion = "",
  getQueue = () => [],
  getLibrary = () => [],
} = {}) {
  if (!dataDir) throw new Error("dataDir é obrigatório")

  function fileInfo(name) {
    const file = path.join(dataDir, name)
    try {
      const stat = fsImpl.statSync(file)
      return { present: true, bytes: stat.size }
    } catch {
      return { present: false, bytes: 0 }
    }
  }

  function collect() {
    const queue = Array.isArray(getQueue()) ? getQueue() : []
    let library = []
    try {
      const value = getLibrary()
      library = Array.isArray(value) ? value : value?.games || []
    } catch {}
    const snapshotsDir = path.join(dataDir, "snapshots")
    let snapshotCount = 0
    try {
      snapshotCount = fsImpl
        .readdirSync(snapshotsDir, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .reduce((total, gameDir) => {
          try {
            return total + fsImpl.readdirSync(path.join(snapshotsDir, gameDir.name), { withFileTypes: true }).filter((entry) => entry.isDirectory()).length
          } catch {
            return total
          }
        }, 0)
    } catch {}
    let writable = false
    try {
      fsImpl.mkdirSync(dataDir, { recursive: true })
      fsImpl.accessSync(dataDir, fsDefault.constants.W_OK)
      writable = true
    } catch {}
    const byStatus = queue.reduce((result, item) => {
      const status = String(item?.status || "unknown")
      result[status] = (result[status] || 0) + 1
      return result
    }, {})
    const byLauncher = library.reduce((result, game) => {
      const launcher = String(game?.launcher || "unknown")
      result[launcher] = (result[launcher] || 0) + 1
      return result
    }, {})
    return {
      version: DIAGNOSTICS_VERSION,
      generated_at: now().toISOString(),
      app: { version: String(appVersion || "unknown") },
      runtime: {
        platform: osImpl.platform(),
        release: osImpl.release(),
        arch: osImpl.arch(),
        node: process.versions.node,
        electron: process.versions.electron || "",
      },
      storage: {
        writable,
        data_dir_configured: Boolean(process.env.ARCADIA_DATA_DIR),
        library: fileInfo("library.json"),
        downloads: fileInfo("downloads.json"),
        snapshots: snapshotCount,
      },
      library: { total: library.length, by_launcher: byLauncher },
      downloads: { total: queue.length, by_status: byStatus },
    }
  }

  return { collect }
}

module.exports = { DIAGNOSTICS_VERSION, createDiagnosticsService }
