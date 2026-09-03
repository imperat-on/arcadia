// Resolve a raiz da instalação da Steam, tolerante a onde ela foi instalada.
// Antes isso estava duplicado no steamstore.js e HARDCODED nos módulos de
// conquistas (~/.local/share/Steam), que quebrava em máquinas com a Steam
// nativa .deb (~/.steam/steam) ou Flatpak — o appcache/stats nunca era achado
// e o loadAllSchemas logava ENOENT a cada chamada.
const fs = require("fs")
const path = require("path")
const os = require("os")

function findSteamDir() {
  if (process.platform === "win32") {
    return findSteamDirWindows()
  }
  return findSteamDirLinux()
}

function findSteamDirWindows() {
  const programFiles = [
    process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)",
    process.env["ProgramFiles"] || "C:\\Program Files",
  ]
  for (const pf of programFiles) {
    const steam = path.join(pf, "Steam")
    if (fs.existsSync(path.join(steam, "steamapps"))) return steam
  }
  // Check libraryfolders.vdf for alternate install locations
  for (const pf of programFiles) {
    const vdf = path.join(pf, "Steam", "steamapps", "libraryfolders.vdf")
    try {
      const content = fs.readFileSync(vdf, "utf-8")
      for (const m of content.matchAll(/"path"\s+"([^"]+)"/g)) {
        const libPath = m[1].replace(/\\\\/g, "/").replace(/\//g, "\\")
        if (fs.existsSync(path.join(libPath, "steamapps"))) return path.dirname(path.join(libPath, "steamapps"))
      }
    } catch {}
  }
  return path.join(programFiles[0], "Steam")
}

function findSteamDirLinux() {
  const home = os.homedir()
  const candidatos = [
    path.join(home, ".steam", "steam"),
    path.join(home, ".local", "share", "Steam"),
    path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"),
  ]
  for (const c of candidatos) {
    if (fs.existsSync(path.join(c, "steamapps"))) return c
  }
  return candidatos[0]
}

module.exports = { findSteamDir, findSteamExe }

function findSteamExe() {
  if (process.platform !== "win32") return "steam"
  const dir = findSteamDir()
  const exe = path.join(dir, "steam.exe")
  if (fs.existsSync(exe)) return exe
  return "steam.exe"
}
