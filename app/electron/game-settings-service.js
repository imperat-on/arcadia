"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")

/**
 * Persiste configurações por jogo sem acoplar o domínio ao Electron.
 *
 * `getPath` é uma função porque os dados ficam no escopo da conta ativa;
 * resolver o caminho a cada operação evita reutilizar o cache de outra conta.
 * A escrita usa o mesmo contrato de segurança dos outros stores: temporário,
 * rename atômico e rejeição de symlink no destino.
 */
function createGameSettingsService({ getPath, fsImpl = fsDefault } = {}) {
  if (typeof getPath !== "function") throw new Error("getPath é obrigatório")

  let cache = { filePath: "", mtimeMs: -1, data: {} }

  function currentPath() {
    try {
      return String(getPath() || "")
    } catch {
      return ""
    }
  }

  function isSymlink(filePath) {
    try {
      return fsImpl.lstatSync(filePath).isSymbolicLink()
    } catch {
      return false
    }
  }

  function plainObject(value) {
    return value && typeof value === "object" && !Array.isArray(value) ? value : {}
  }

  function readAll() {
    const filePath = currentPath()
    if (!filePath || isSymlink(filePath)) return {}
    try {
      const mtimeMs = fsImpl.statSync(filePath).mtimeMs
      if (filePath !== cache.filePath || mtimeMs !== cache.mtimeMs) {
        const parsed = JSON.parse(fsImpl.readFileSync(filePath, "utf-8"))
        cache = { filePath, mtimeMs, data: plainObject(parsed) }
      }
      return cache.data
    } catch {
      return {}
    }
  }

  function persist(filePath, data) {
    if (!filePath || isSymlink(filePath)) return false
    const temporary = `${filePath}.tmp-${process.pid}-${Date.now()}`
    try {
      fsImpl.mkdirSync(path.dirname(filePath), { recursive: true, mode: 0o700 })
      // A pre-existing temporary symlink must never be followed by writeFile.
      if (isSymlink(temporary)) return false
      fsImpl.writeFileSync(temporary, JSON.stringify(data, null, 2), {
        encoding: "utf8",
        mode: 0o600,
        flag: "wx",
      })
      if (typeof fsImpl.chmodSync === "function") fsImpl.chmodSync(temporary, 0o600)
      if (isSymlink(temporary)) return false
      fsImpl.renameSync(temporary, filePath)
      if (isSymlink(filePath)) return false
      cache = { filePath, mtimeMs: fsImpl.statSync(filePath).mtimeMs, data }
      return true
    } catch {
      return false
    } finally {
      try {
        fsImpl.rmSync(temporary, { force: true })
      } catch {
        // The rename succeeded or the filesystem already removed the temp.
      }
    }
  }

  function get(id) {
    if (!id) return {}
    return readAll()[id] || {}
  }

  function set(id, patch) {
    if (!id) return {}
    const all = readAll()
    all[id] = { ...(all[id] || {}), ...(patch || {}) }
    persist(currentPath(), all)
    return all[id]
  }

  function remove(id) {
    if (!id) return
    const all = readAll()
    if (!all[id]) return
    delete all[id]
    persist(currentPath(), all)
  }

  return { readAll, get, set, remove }
}

module.exports = { createGameSettingsService }
