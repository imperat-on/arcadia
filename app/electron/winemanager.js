// Wine/Proton manager: detecta Protons da Steam (oficiais e customizados em
// compatibilitytools.d). Sem download/gerenciamento próprio — a Steam cuida
// disso. Prefixos por jogo ficam em ~/.local/share/arcadia/prefixes/<appid>/.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn } = require("child_process")
const { getDataDir } = require("./runtime-paths")

const DATA_DIR = getDataDir()
const PREFIX_DIR = path.join(DATA_DIR, "prefixes")

// Pastas onde a Steam guarda os Protons oficiais (steamapps/common) e os
// customizados instalados pelo usuário (compatibilitytools.d — GE-Proton via
// ProtonUp-Qt ou manual). Além dos locais padrão, inclui libraries.vdf de
// bibliotecas Steam extras (steamstore.steamLibraries()).
function steamCommonDirs() {
  const base = [
    path.join(os.homedir(), ".steam", "steam", "steamapps", "common"),
    path.join(os.homedir(), ".local", "share", "Steam", "steamapps", "common"),
    path.join(
      os.homedir(),
      ".var",
      "app",
      "com.valvesoftware.Steam",
      ".local",
      "share",
      "Steam",
      "steamapps",
      "common",
    ), // flatpak
    path.join(os.homedir(), ".steam", "steam", "compatibilitytools.d"),
    path.join(os.homedir(), ".local", "share", "Steam", "compatibilitytools.d"),
    path.join(
      os.homedir(),
      ".var",
      "app",
      "com.valvesoftware.Steam",
      ".local",
      "share",
      "Steam",
      "compatibilitytools.d",
    ), // flatpak
  ]
  try {
    // require lazy: steamstore não requer winemanager (grep confirmou), mas
    // lazy evita qualquer ciclo futuro no top-level.
    const { steamLibraries } = require("./steamstore")
    for (const lib of steamLibraries() || []) {
      // lib.path é ".../steamapps" → common = ".../steamapps/common"
      const common = path.join(lib.path, "common")
      if (!base.includes(common)) base.push(common)
      // libraries.vdf também podem hospedar compatibilitytools.d (raro mas ok)
      const compat = path.join(path.dirname(lib.path), "compatibilitytools.d")
      if (!base.includes(compat)) base.push(compat)
    }
  } catch {}
  return base
}

function safeId(appid) {
  return String(appid).replace(/[^a-z0-9._-]/gi, "_")
}

// Base dos prefixos: respeita "Pasta padrão para novos prefixos Wine"
// (default_wine_prefix_path no config.json; padrão ~/Games/Arcadia/Prefixes).
// Prefixos já existentes no local antigo (PREFIX_DIR) continuam sendo usados.
function prefixBase(appid) {
  let base = path.join(os.homedir(), "Games", "Arcadia", "Prefixes")
  try {
    const cfg = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "config.json"), "utf-8"))
    if (cfg.default_wine_prefix_path) base = cfg.default_wine_prefix_path
  } catch {}
  const antigo = path.join(PREFIX_DIR, safeId(appid))
  if (base !== PREFIX_DIR && fs.existsSync(antigo)) return PREFIX_DIR
  return base
}

// Protons da Steam detectados: oficiais em steamapps/common/Proton* e
// customizados em compatibilitytools.d/<qualquer-nome> (têm compatibilitytool.vdf).
function steamProtons() {
  const seen = new Set()
  const out = []
  for (const common of steamCommonDirs()) {
    const isCompatDir = /compatibilitytools\.d$/.test(common)
    try {
      const dirs = fs.readdirSync(common, { withFileTypes: true })
      for (const d of dirs) {
        if (!d.isDirectory()) continue
        // Em steamapps/common só olhamos pastas Proton*; em compatibilitytools.d
        // qualquer subpasta vale (GE-Proton, Proton-GE, etc).
        if (!isCompatDir && !/^proton/i.test(d.name)) continue
        if (seen.has(d.name)) continue // mesmo Proton em múltiplas instalações da Steam
        const dir = path.join(common, d.name)
        const proton = path.join(dir, "proton")
        const filesBin = path.join(dir, "files", "bin", "wine")
        const vdf = path.join(dir, "compatibilitytool.vdf")
        if (fs.existsSync(proton) || fs.existsSync(filesBin) || fs.existsSync(vdf)) {
          seen.add(d.name)
          out.push({
            id: `steam:${d.name}`,
            name: `${d.name} (Steam)`,
            path: dir,
            wine: fs.existsSync(filesBin) ? filesBin : proton,
            kind: "steam",
          })
        }
      }
    } catch {
      /* pasta não existe — segue */
    }
  }
  return out
}

function prefixOf(appid) {
  return path.join(prefixBase(appid), safeId(appid))
}

// Bootstrap do prefixo (wineboot) se ainda não existe drive_c.
// opts: { wine: binário obrigatório, prefix: prefixo customizado }.
async function verifyWinePrefix(appid, opts = {}) {
  if (!opts.wine) return { ok: false, error: "wine binary não fornecido" }
  const prefix = opts.prefix || prefixOf(appid)
  if (fs.existsSync(path.join(prefix, "drive_c"))) return { ok: true, prefix }
  fs.mkdirSync(prefix, { recursive: true })
  await new Promise((res) => {
    const child = spawn(opts.wine, ["wineboot", "-u"], {
      env: { ...process.env, WINEPREFIX: prefix },
      detached: true,
      stdio: "ignore",
    })
    child.on("close", res)
    child.on("error", res)
    setTimeout(res, 60000) // não trava o fluxo se o wineboot demorar
  })
  try {
    installGraphicsLibs(prefix, opts.wine, { dxvk: true, vkd3d: true })
  } catch {}
  return { ok: true, prefix }
}

// Ferramentas do prefixo: winecfg / regedit / explorer / winetricks / wineboot.
// opts: { wine: binário escolhido, prefix: prefixo customizado }.
async function prefixTool(appid, tool, opts = {}) {
  const { prefix } = await verifyWinePrefix(appid, opts)
  const wine = opts.wine
  const env = { ...process.env, WINEPREFIX: prefix }
  let cmd, args
  if (tool === "winetricks") {
    cmd = "winetricks"
    args = []
    env.WINE = wine
  } else if (["winecfg", "regedit", "explorer"].includes(tool)) {
    cmd = wine
    args = [tool]
  } else if (tool === "wineboot") {
    cmd = wine
    args = ["wineboot", "-u"]
  } else {
    return { ok: false, error: "ferramenta desconhecida" }
  }
  try {
    const child = spawn(cmd, args, { env, detached: true, stdio: "ignore" })
    child.unref()
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Executa um .exe/.msi/.bat dentro do prefixo do jogo.
async function runExe(appid, exePath, opts = {}) {
  const { prefix } = await verifyWinePrefix(appid, opts)
  const wine = opts.wine
  try {
    const child = spawn(wine, [exePath], {
      env: { ...process.env, WINEPREFIX: prefix },
      detached: true,
      stdio: "ignore",
    })
    child.unref()
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Instala DXVK / DXVK-NVAPI / VKD3D num prefixo, copiando as DLLs do build de
// Wine escolhido (layout GE: files/lib/wine/{dxvk,nvapi,vkd3d-proton}/<arch>/).
// Rodar o binário wine direto NÃO ativa DXVK sozinho — sem isso o jogo cai no
// wined3d (e DXVK_HUD/MangoHud Vulkan não funcionam).
function installGraphicsLibs(prefix, winePath, { dxvk = true, nvapi = false, vkd3d = false } = {}) {
  const sys32 = path.join(prefix, "drive_c", "windows", "system32")
  const wow64 = path.join(prefix, "drive_c", "windows", "syswow64")
  if (!fs.existsSync(sys32)) return { ok: false, error: "prefixo sem system32" }
  // .../files/(lib|lib64)/wine -> base "files"; wine direto: .../bin/wine -> raiz.
  const m = winePath.match(/^(.*)\/lib(?:64)?\/wine\/.*$/) || winePath.match(/^(.*)\/bin\/wine$/)
  if (!m) return { ok: false, error: "layout de wine desconhecido" }
  const base = m[1]
  const fontes = []
  if (dxvk) fontes.push("dxvk")
  if (nvapi) fontes.push("nvapi")
  if (vkd3d) fontes.push("vkd3d-proton")
  let copiados = 0
  for (const lib of fontes) {
    for (const [arch, dest] of [
      ["x86_64-windows", sys32],
      ["i386-windows", wow64],
    ]) {
      const src = path.join(base, "lib", "wine", lib, arch)
      if (!fs.existsSync(src) || !fs.existsSync(dest)) continue
      for (const f of fs.readdirSync(src)) {
        if (!f.toLowerCase().endsWith(".dll")) continue
        try {
          fs.copyFileSync(path.join(src, f), path.join(dest, f))
          copiados++
        } catch {}
      }
    }
  }
  return { ok: true, copiados }
}

module.exports = {
  verifyWinePrefix,
  prefixTool,
  runExe,
  installGraphicsLibs,
  prefixOf,
  steamProtons,
  PREFIX_DIR,
}
