"use strict"

const fsDefault = require("node:fs")
const path = require("node:path")

function safeName(value) {
  return String(value || "jogo").replace(/[^a-z0-9._-]/gi, "_")
}

function createLaunchLog({ logDir, fsImpl = fsDefault, now = () => new Date() } = {}) {
  if (!logDir) throw new Error("logDir é obrigatório")

  function open(gameId, command) {
    let fd = null
    try {
      fsImpl.mkdirSync(logDir, { recursive: true })
      const logPath = path.join(logDir, `${safeName(gameId)}.log`)
      try {
        if (fsImpl.statSync(logPath).size > 5 * 1024 * 1024) {
          fsImpl.renameSync(logPath, `${logPath}.old`)
        }
      } catch {
        // Primeiro lançamento ou log removido entre stat/rename.
      }
      fd = fsImpl.openSync(logPath, "a")
      fsImpl.writeSync(fd, `\n\n=== ${now().toISOString()} launch: ${JSON.stringify(command)} ===\n`)
      return {
        path: logPath,
        stdio: ["ignore", fd, fd],
        close() {
          if (fd == null) return
          try {
            fsImpl.closeSync(fd)
          } catch {}
          fd = null
        },
      }
    } catch (error) {
      if (fd != null) {
        try {
          fsImpl.closeSync(fd)
        } catch {}
      }
      return { path: "", stdio: "ignore", close() {}, error }
    }
  }

  return { open, safeName }
}

module.exports = { createLaunchLog, safeName }
