// Download de binários dos runners (Legendary etc.) para ~/.config/arcadia/runners/.
// Mesma ideia do download-helper-binaries do Heroic, mas mínima: um binário por runner.

const fs = require("fs")
const path = require("path")
const os = require("os")

function getDefaultRunnersDir() {
  if (process.platform === "win32") {
    return path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local"), "arcadia", "runners")
  }
  return path.join(os.homedir(), ".config", "arcadia", "runners")
}

const RUNNERS_DIR = getDefaultRunnersDir()

// Repo do Legendary mudou de dono — a API antiga (derrod) redireciona; usar o
// id numérico que segue o redirect automaticamente.
const LEGENDARY_RELEASE = "https://api.github.com/repositories/249938026/releases/latest"

async function downloadLegendary() {
  fs.mkdirSync(RUNNERS_DIR, { recursive: true })
  const rel = await fetch(LEGENDARY_RELEASE, {
    headers: { "User-Agent": "arcadia" },
  }).then((r) => r.json())
  const asset = (rel.assets || []).find((a) => a.name === "legendary")
  if (!asset) throw new Error("asset 'legendary' não encontrado na release")
  const dest = path.join(RUNNERS_DIR, process.platform === "win32" ? "legendary.exe" : "legendary")
  const buf = Buffer.from(await fetch(asset.browser_download_url).then((r) => r.arrayBuffer()))
  fs.writeFileSync(dest, buf)
  if (process.platform !== "win32") {
    try { fs.chmodSync(dest, 0o755) } catch {}
  }
  return dest
}

// Garante o binário; devolve o caminho. Baixa só se não existir.
async function ensureLegendary() {
  const ext = process.platform === "win32" ? ".exe" : ""
  const dest = path.join(RUNNERS_DIR, `legendary${ext}`)
  if (fs.existsSync(dest)) return dest
  return downloadLegendary()
}

module.exports = { RUNNERS_DIR, ensureLegendary }
