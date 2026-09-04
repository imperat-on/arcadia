"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const os = require("node:os")

const SNAPSHOT_VERSION = 1
const MAX_LABEL_LENGTH = 120

function safePart(value, fallback = "item") {
  const clean = String(value || "")
    .replace(/[^a-z0-9._-]/gi, "_")
    .replace(/^\.+$/, "")
    .slice(0, 160)
  return clean || fallback
}

function absoluteDirectory(value) {
  if (typeof value !== "string" || !value.trim()) return ""
  const resolved = path.resolve(value)
  return resolved === path.parse(resolved).root ? "" : resolved
}

function inside(parent, child) {
  const base = path.resolve(parent) + path.sep
  return path.resolve(child).startsWith(base)
}

function createSnapshotService({ snapshotsDir, fsImpl = fsDefault, now = () => new Date() } = {}) {
  if (!snapshotsDir) throw new Error("snapshotsDir é obrigatório")
  const root = path.resolve(snapshotsDir)

  function gameDir(gameId) {
    return path.join(root, safePart(gameId, "unknown"))
  }

  function readManifest(file) {
    try {
      const manifest = JSON.parse(fsImpl.readFileSync(file, "utf8"))
      if (
        manifest?.version !== SNAPSHOT_VERSION ||
        typeof manifest.id !== "string" ||
        typeof manifest.gameId !== "string" ||
        typeof manifest.created_at !== "string"
      ) return null
      return {
        version: SNAPSHOT_VERSION,
        id: manifest.id,
        gameId: manifest.gameId,
        created_at: manifest.created_at,
        label: typeof manifest.label === "string" ? manifest.label : "",
        source_name: typeof manifest.source_name === "string" ? manifest.source_name : "",
      }
    } catch {
      return null
    }
  }

  function list(gameId) {
    const directory = gameDir(gameId)
    try {
      return fsImpl
        .readdirSync(directory, { withFileTypes: true })
        .map((entry) => {
          if (!entry.isDirectory()) return null
          return readManifest(path.join(directory, entry.name, "manifest.json"))
        })
        .filter((manifest) => manifest && manifest.gameId === String(gameId))
        .sort((a, b) => b.created_at.localeCompare(a.created_at))
    } catch {
      return []
    }
  }

  function create({ gameId, sourceDir, label = "" } = {}) {
    const source = absoluteDirectory(sourceDir)
    let sourceStat
    try { sourceStat = source ? fsImpl.lstatSync(source) : null } catch { sourceStat = null }
    if (!gameId || !sourceStat || !sourceStat.isDirectory() || sourceStat.isSymbolicLink()) {
      return { ok: false, error: "origem_invalida" }
    }
    const safeGame = safePart(gameId, "unknown")
    const id = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`
    const directory = path.join(root, safeGame, id)
    const temporary = `${directory}.tmp`
    try {
      fsImpl.mkdirSync(path.join(root, safeGame), { recursive: true })
      fsImpl.rmSync(temporary, { recursive: true, force: true })
      fsImpl.mkdirSync(temporary, { recursive: true })
      fsImpl.cpSync(source, path.join(temporary, "data"), {
        recursive: true,
        force: false,
        dereference: false,
        filter: (entry) => !fsImpl.lstatSync(entry).isSymbolicLink(),
      })
      const manifest = {
        version: SNAPSHOT_VERSION,
        id,
        gameId: String(gameId),
        created_at: now().toISOString(),
        label: String(label || "").trim().slice(0, MAX_LABEL_LENGTH),
        source_name: path.basename(source),
      }
      fsImpl.writeFileSync(path.join(temporary, "manifest.json"), JSON.stringify(manifest, null, 2))
      fsImpl.renameSync(temporary, directory)
      return { ok: true, snapshot: manifest }
    } catch (error) {
      try { fsImpl.rmSync(temporary, { recursive: true, force: true }) } catch {}
      return { ok: false, error: String(error.message || error) }
    }
  }

  function restore({ gameId, snapshotId, targetDir, backup = true } = {}) {
    const target = absoluteDirectory(targetDir)
    const id = safePart(snapshotId, "")
    const directory = path.join(gameDir(gameId), id)
    const data = path.join(directory, "data")
    const manifest = readManifest(path.join(directory, "manifest.json"))
    // SEGURANCA: o renderer nunca e fonte de verdade para targetDir.
    // targetDir absoluto arbitrario (ex: /etc, C:\Windows) permitiria
    // sobrescrever arquivos do sistema com dados de save. So restaurar
    // para diretorios sob o home do usuario.
    const home = os.homedir()
    const dentroDoHome = target && (target === home || target.startsWith(home + path.sep))
    if (!target || !dentroDoHome || !id || !inside(root, directory) || !manifest || manifest.gameId !== String(gameId)) {
      return { ok: false, error: "snapshot_invalido" }
    }
    if (!fsImpl.existsSync(data) || !fsImpl.statSync(data).isDirectory()) {
      return { ok: false, error: "snapshot_sem_dados" }
    }
    const temporary = `${target}.arcadia-restore-${crypto.randomUUID().slice(0, 8)}`
    let previousPath = ""
    const backupPath = backup ? `${target}.arcadia-backup-${Date.now()}` : ""
    try {
      if (fsImpl.lstatSync(data).isSymbolicLink()) return { ok: false, error: "snapshot_invalido" }
      fsImpl.rmSync(temporary, { recursive: true, force: true })
      fsImpl.cpSync(data, temporary, {
        recursive: true,
        force: false,
        dereference: false,
        filter: (entry) => !fsImpl.lstatSync(entry).isSymbolicLink(),
      })
      if (fsImpl.existsSync(target)) {
        previousPath = backupPath || `${target}.arcadia-rollback-${crypto.randomUUID().slice(0, 8)}`
        fsImpl.renameSync(target, previousPath)
      }
      fsImpl.renameSync(temporary, target)
      if (previousPath && !backup) fsImpl.rmSync(previousPath, { recursive: true, force: true })
      return { ok: true, backupPath, snapshot: manifest }
    } catch (error) {
      try { fsImpl.rmSync(temporary, { recursive: true, force: true }) } catch {}
      // If the final rename failed, put the previous save back before exposing
      // the error. A failed restore must never leave the user's target absent.
      if (previousPath && !fsImpl.existsSync(target) && fsImpl.existsSync(previousPath)) {
        try { fsImpl.renameSync(previousPath, target) } catch {}
      }
      return { ok: false, error: String(error.message || error), backupPath }
    }
  }

  function remove({ gameId, snapshotId } = {}) {
    const id = safePart(snapshotId, "")
    const directory = path.join(gameDir(gameId), id)
    const manifest = readManifest(path.join(directory, "manifest.json"))
    if (!id || !inside(root, directory) || !manifest || manifest.gameId !== String(gameId)) {
      return { ok: false, error: "snapshot_invalido" }
    }
    try {
      fsImpl.rmSync(directory, { recursive: true, force: true })
      return { ok: true }
    } catch (error) {
      return { ok: false, error: String(error.message || error) }
    }
  }

  return { create, list, restore, remove, safePart }
}

module.exports = { SNAPSHOT_VERSION, createSnapshotService, safePart }
