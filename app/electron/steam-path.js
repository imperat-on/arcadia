// Resolve a raiz da instalação da Steam, tolerante a onde ela foi instalada.
// Antes isso estava duplicado no steamstore.js e HARDCODED nos módulos de
// conquistas (~/.local/share/Steam), que quebrava em máquinas com a Steam
// nativa .deb (~/.steam/steam) ou Flatpak — o appcache/stats nunca era achado
// e o loadAllSchemas logava ENOENT a cada chamada.
const fs = require("fs")
const path = require("path")
const os = require("os")

function findSteamDir() {
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

module.exports = { findSteamDir }
