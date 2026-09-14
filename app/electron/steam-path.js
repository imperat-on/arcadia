// Resolve a raiz da instalação da Steam, tolerante a onde ela foi instalada.
// Antes isso estava duplicado no steamstore.js e HARDCODED nos módulos de
// conquistas (~/.local/share/Steam), que quebrava em máquinas com a Steam
// nativa .deb (~/.steam/steam) ou Flatpak — o appcache/stats nunca era achado
// e o loadAllSchemas logava ENOENT a cada chamada.
const fs = require("fs")
const path = require("path")
const os = require("os")

// Onde uma Steam pode estar instalada, em ordem de confiança.
//
// No Windows, procurar só em "Program Files (x86)\Steam" e "Program Files\Steam"
// deixava de fora quem instalou no registro em outro lugar — D:\Steam, C:\Games\Steam,
// outro disco. Sintoma real: as conquistas vinham do servidor e o playtime não,
// porque a leitura do localconfig.vdf não achava a instalação.
const LETRAS_DISCO = "CDEFGHIJKLMNOPQRSTUVWXYZ"

/** Steam registrada no Windows (SteamPath/InstallPath). Silencioso fora do win32. */
function raizesDoRegistro() {
  if (process.platform !== "win32") return []
  const saida = []
  const consultas = [
    ["HKCU\\Software\\Valve\\Steam", "SteamPath"],
    ["HKLM\\SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"],
    ["HKLM\\SOFTWARE\\Valve\\Steam", "InstallPath"],
  ]
  let execSync = null
  try {
    execSync = require("child_process").execSync
  } catch {
    return []
  }
  for (const [chave, valor] of consultas) {
    try {
      const out = execSync(`reg query "${chave}" /v ${valor}`, {
        encoding: "utf8",
        timeout: 3000,
        windowsHide: true,
      })
      const m = String(out).match(/REG_SZ\s+(.+?)\s*$/m)
      if (m) saida.push(m[1].trim().replace(/[\\/]+$/, ""))
    } catch {}
  }
  return saida
}

/** Pastas candidatas a raiz da Steam, na plataforma atual. */
function candidatosSteam() {
  const cands = []
  if (process.env.STEAM_DIR) cands.push(process.env.STEAM_DIR)
  if (process.platform === "win32") {
    cands.push(...raizesDoRegistro())
    for (const pf of [process.env["ProgramFiles(x86)"], process.env.ProgramFiles]) {
      if (pf) cands.push(path.join(pf, "Steam"))
    }
    for (const letra of LETRAS_DISCO) {
      const raiz = `${letra}:\\`
      cands.push(path.join(raiz, "Steam"))
      cands.push(path.join(raiz, "Program Files (x86)", "Steam"))
      cands.push(path.join(raiz, "Program Files", "Steam"))
      cands.push(path.join(raiz, "Games", "Steam"))
      cands.push(path.join(raiz, "SteamLibrary"))
    }
  } else {
    const home = os.homedir()
    cands.push(path.join(home, ".steam", "steam"))
    cands.push(path.join(home, ".local", "share", "Steam"))
    cands.push(path.join(home, ".steam", "root"))
    cands.push(path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"))
    cands.push(path.join(home, ".var", "app", "com.valvesoftware.Steam", "data", "Steam"))
  }
  const vistos = new Set()
  return cands.filter((p) => p && !vistos.has(p) && vistos.add(p))
}

/** Uma pasta é raiz da Steam se tem o executável, a appcache ou a steamapps. */
function pareceRaizDaSteam(dir) {
  try {
    return (
      fs.existsSync(path.join(dir, "steam.exe")) ||
      fs.existsSync(path.join(dir, "appcache")) ||
      fs.existsSync(path.join(dir, "steamapps"))
    )
  } catch {
    return false
  }
}

function findSteamDir() {
  if (process.platform === "win32") {
    for (const c of candidatosSteam()) {
      if (pareceRaizDaSteam(c)) return c
    }
    return candidatosSteam()[0] || "C:\\Program Files (x86)\\Steam"
  }
  return findSteamDirLinux()
}

function findSteamDirLinux() {
  const candidatos = candidatosSteam()
  for (const c of candidatos) {
    if (fs.existsSync(path.join(c, "steamapps"))) return c
  }
  return candidatos[0]
}

module.exports = { findSteamDir, findSteamExe, candidatosSteam, raizesDoRegistro }

function findSteamExe() {
  if (process.platform !== "win32") return "steam"
  const dir = findSteamDir()
  const exe = path.join(dir, "steam.exe")
  if (fs.existsSync(exe)) return exe
  return "steam.exe"
}
