"use strict"

const { execFileSync } = require("child_process")

function isProcessAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 1) return false
  if (process.platform === "win32") {
    try {
      const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], {
        encoding: "utf-8",
        timeout: 3000,
        stdio: ["ignore", "pipe", "pipe"],
      })
      // tasklist exits 0 even when the PID does not exist, returning
      // "INFO: No tasks are running which match the specified criteria."
      // Only a non-empty CSV line containing the PID means the process is alive.
      return out.trim().length > 0 && out.includes(String(pid))
    } catch {
      return false
    }
  }
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

function killProcessTree(pid, signal = "SIGTERM") {
  if (!Number.isInteger(pid) || pid <= 1) return false
  if (process.platform === "win32") {
    try {
      const force = signal === "SIGKILL" || signal === 9
      execFileSync("taskkill", ["/PID", String(pid), "/T", force ? "/F" : ""].filter(Boolean), {
        encoding: "utf-8",
        timeout: 5000,
        stdio: ["ignore", "pipe", "pipe"],
      })
      return true
    } catch {
      return false
    }
  }
  try {
    process.kill(-pid, signal)
    return true
  } catch {
    return false
  }
}

function isProcessRunning(pattern) {
  if (process.platform === "win32") {
    try {
      // Use tasklist (available on all Windows versions) instead of wmic (deprecated)
      const out = execFileSync("tasklist", [
        "/FI", `IMAGENAME eq ${pattern}`,
        "/FO", "CSV", "/NH"
      ], { encoding: "utf-8", timeout: 3000, stdio: ["ignore", "pipe", "pipe"] })
      return out.toLowerCase().includes(pattern.toLowerCase())
    } catch {
      return false
    }
  }
  try {
    execFileSync("pgrep", ["-f", pattern], { stdio: "ignore", timeout: 3000 })
    return true
  } catch {
    return false
  }
}

function isSteamRunning() {
  if (process.platform === "win32") {
    return isProcessRunning("steam.exe")
  }
  try {
    execFileSync("pgrep", ["-x", "steam"], { stdio: "ignore", timeout: 2000 })
    return true
  } catch {
    return false
  }
}

// No Windows, verifica se algum processo do jogo Steam ainda está rodando
// sob steamapps\common. A Steam client fica na bandeja e isSteamRunning
// retorna true mesmo depois do jogo fechar — o que impedia o playtime de
// ser creditado (a sessão nunca finalizava). Este check é mais específico.
function isSteamGameRunning() {
  if (process.platform !== "win32") return false
  try {
    const out = String(execFileSync("powershell", [
      "-NoProfile", "-NonInteractive", "-Command",
      "& { (Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '*steamapps\\common*' }).Count }"
    ], { timeout: 3000, encoding: "utf8" })).trim()
    return Number(out) > 0
  } catch {
    return false
  }
}

module.exports = { isProcessAlive, killProcessTree, isProcessRunning, isSteamRunning, isSteamGameRunning }
