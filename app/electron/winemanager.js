// Wine/Proton version manager: lista/baixa GE-Proton e Wine-GE (releases do
// GloriousEggroll) e detecta Protons da Steam. Instalados vão para
// ~/.local/share/arcadia/wine/ (baixados) ou são lidos de
// ~/.steam/steam/steamapps/common/Proton* (Steam). Prefixos por jogo ficam em
// ~/.local/share/arcadia/prefixes/<appid>/.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn, execFile } = require("child_process")

const DATA_DIR = path.join(os.homedir(), ".local", "share", "arcadia")
const WINE_DIR = path.join(DATA_DIR, "wine")
const PREFIX_DIR = path.join(DATA_DIR, "prefixes")

const API = "https://api.github.com/repos"
const REPOS = {
  "ge-proton": `${API}/GloriousEggroll/proton-ge-custom/releases?per_page=30`,
  "wine-ge": `${API}/GloriousEggroll/wine-ge-custom/releases?per_page=30`,
}

// Pastas onde a Steam guarda os Protons oficiais (steamapps/common) e os
// customizados instalados pelo usuário (compatibilitytools.d — GE-Proton via
// ProtonUp-Qt ou manual).
const STEAM_COMMON = [
  path.join(os.homedir(), ".steam", "steam", "steamapps", "common"),
  path.join(os.homedir(), ".local", "share", "Steam", "steamapps", "common"),
  path.join(os.homedir(), ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "steamapps", "common"), // flatpak
  path.join(os.homedir(), ".steam", "steam", "compatibilitytools.d"),
  path.join(os.homedir(), ".local", "share", "Steam", "compatibilitytools.d"),
  path.join(os.homedir(), ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "compatibilitytools.d"), // flatpak
]

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

async function gh(url) {
  const r = await fetch(url, { headers: { "User-Agent": "arcadia" } })
  if (!r.ok) throw new Error(`GitHub ${r.status}`)
  return r.json()
}

// Versões instaladas = subpastas de wine/ com bin/wine dentro.
// Aceita duas estruturas: GE-Proton (files/bin/wine) e Wine-GE (bin/wine direto).
function installed() {
  try {
    return fs
      .readdirSync(WINE_DIR, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => {
        const dir = path.join(WINE_DIR, d.name)
        const wineFiles = path.join(dir, "files", "bin", "wine")
        const wineDirect = path.join(dir, "bin", "wine")
        const wine = fs.existsSync(wineFiles) ? wineFiles : fs.existsSync(wineDirect) ? wineDirect : ""
        return { id: d.name, name: d.name, path: dir, wine }
      })
      .filter((v) => v.wine)
  } catch {
    return []
  }
}

// Protons da Steam detectados: oficiais em steamapps/common/Proton* e
// customizados em compatibilitytools.d/<qualquer-nome> (têm compatibilitytool.vdf).
function steamProtons() {
  const seen = new Set()
  const out = []
  for (const common of STEAM_COMMON) {
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

// Lista de releases disponíveis no GitHub para um tipo (ge-proton | wine-ge).
async function available(kind) {
  const rels = await gh(REPOS[kind])
  if (!Array.isArray(rels)) return []
  return rels
    .map((rel) => {
      const assets = rel.assets || []
      // Prefere o asset x86_64 (desktop); ignora aarch64 e arquivos de assinatura.
      const asset =
        assets.find((a) => /x86_64\.tar\.(gz|xz)$/.test(a.name || "")) ||
        assets.find((a) => /\.tar\.(gz|xz)$/.test(a.name || "") && !/aarch64|sha512|sha256|\.sig/i.test(a.name || ""))
      if (!asset) return null
      const id = asset.name.replace(/\.tar\.(gz|xz)$/, "")
      return {
        id,
        name: rel.name || rel.tag_name || id,
        url: asset.browser_download_url,
        size: Math.round((asset.size || 0) / 1024 / 1024),
        releaseDate: rel.published_at || rel.created_at || "",
        kind,
      }
    })
    .filter(Boolean)
    // Wine-GE: ignora builds específicas de LoL (têm patches exclusivos).
    .filter((v) => (kind === "wine-ge" ? !/LoL/i.test(v.id) : true))
}

// Catálogo completo: instalados (Arcadia) + Protons da Steam + disponíveis.
// Os "available" vêm da API do GitHub (60 req/h sem token!) — por isso ficam
// cacheados em disco por 6h; se a API falha (403/rate limit), usa o cache.
const CATALOG_CACHE = path.join(DATA_DIR, "wine_catalog_cache.json")
const CATALOG_TTL = 6 * 60 * 60 * 1000

async function catalog() {
  const inst = installed().map((v) => ({
    ...v,
    // Classificação pela estrutura real: GE-Proton tem files/bin/wine;
    // Wine-GE (Lutris) tem bin/wine direto, sem pasta files/.
    kind: /\/files\/bin\/wine$/.test(v.wine) ? "ge-proton" : "wine-ge",
  }))
  const steam = steamProtons()

  let cache = null
  try {
    cache = JSON.parse(fs.readFileSync(CATALOG_CACHE, "utf-8"))
  } catch {}

  if (cache?.available && Date.now() - (cache.ts || 0) < CATALOG_TTL) {
    return { installed: [...inst, ...steam], available: cache.available.filter((a) => !inst.some((i) => i.id === a.id)) }
  }

  try {
    const [ge, wg] = await Promise.all([available("ge-proton"), available("wine-ge")])
    const avail = [...ge, ...wg]
    try {
      fs.writeFileSync(CATALOG_CACHE, JSON.stringify({ ts: Date.now(), available: avail }, null, 2))
    } catch {}
    return { installed: [...inst, ...steam], available: avail.filter((a) => !inst.some((i) => i.id === a.id)) }
  } catch (e) {
    // API fora (rate limit/rede): serve o cache mesmo velho.
    if (cache?.available) {
      return { installed: [...inst, ...steam], available: cache.available.filter((a) => !inst.some((i) => i.id === a.id)), stale: true }
    }
    throw e
  }
}

// Baixa e extrai uma versão (.tar.gz ou .tar.xz) em wine/<id>/, reportando progresso.
async function install(id, kind, onProgress) {
  const list = await available(kind || "ge-proton")
  const av = list.find((a) => a.id === id)
  if (!av) throw new Error("versão não encontrada")
  fs.mkdirSync(WINE_DIR, { recursive: true })

  // Detecta a extensão real do asset (GE-Proton usa .tar.gz, Wine-GE usa .tar.xz).
  const ext = /\.tar\.xz$/.test(av.url) ? ".tar.xz" : ".tar.gz"
  const tarFlag = ext === ".tar.xz" ? "-xJf" : "-xzf"
  const tgz = path.join(WINE_DIR, id + ext)

  const res = await fetch(av.url)
  if (!res.ok || !res.body) throw new Error(`download falhou: HTTP ${res.status}`)
  const total = Number(res.headers.get("content-length") || 0)
  const file = fs.createWriteStream(tgz)
  const reader = res.body.getReader()
  let done = 0
  for (;;) {
    const { done: fin, value } = await reader.read()
    if (fin) break
    file.write(value)
    done += value.length
    onProgress?.({ id, done, total })
  }
  await new Promise((r) => file.end(r))

  await new Promise((res2, rej) =>
    execFile("tar", [tarFlag, tgz, "-C", WINE_DIR], (e) => (e ? rej(e) : res2())),
  )
  fs.rmSync(tgz, { force: true })

  // O tar às vezes extrai com um nome de pasta diferente do id (ex: Wine-GE
  // baixa "wine-lutris-GE-Proton8-25" mas extrai "lutris-GE-Proton8-25").
  // Renomeamos a pasta extraída para o id esperado.
  const destino = path.join(WINE_DIR, id)
  if (!fs.existsSync(destino)) {
    const candidatas = fs
      .readdirSync(WINE_DIR, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name)
      // A pasta extraída é a que "casa" com o id ignorando o prefixo "wine-".
      .filter((n) => n === id || n === id.replace(/^wine-/, "") || id.endsWith(n) || n.endsWith(id.replace(/^wine-/, "")))
      .filter((n) => n !== id)
    if (candidatas.length) {
      fs.renameSync(path.join(WINE_DIR, candidatas[0]), destino)
    }
  }
  return { ok: true }
}

function remove(id) {
  const dir = path.join(WINE_DIR, id)
  if (!dir.startsWith(WINE_DIR)) return { ok: false }
  fs.rmSync(dir, { recursive: true, force: true })
  return { ok: true }
}

// Melhor binário de wine disponível: GE-Proton baixado > wine do sistema.
function bestWine() {
  const inst = installed()
  if (inst.length) return inst[inst.length - 1].wine
  return "wine" // PATH do sistema (pode não existir — o caller trata o erro)
}

function prefixOf(appid) {
  return path.join(prefixBase(appid), safeId(appid))
}

// Bootstrap do prefixo (wineboot) se ainda não existe drive_c.
// opts.prefix sobrescreve o prefixo padrão (caminho customizado do jogo).
async function verifyWinePrefix(appid, opts = {}) {
  const prefix = opts.prefix || prefixOf(appid)
  if (fs.existsSync(path.join(prefix, "drive_c"))) return { ok: true, prefix }
  fs.mkdirSync(prefix, { recursive: true })
  await new Promise((res) => {
    const child = spawn(opts.wine || bestWine(), ["wineboot", "-u"], {
      env: { ...process.env, WINEPREFIX: prefix },
      detached: true,
      stdio: "ignore",
    })
    child.on("close", res)
    child.on("error", res)
    setTimeout(res, 60000) // não trava o fluxo se o wineboot demorar
  })
  return { ok: true, prefix }
}

// Ferramentas do prefixo: winecfg / regedit / explorer / winetricks / wineboot.
// opts: { wine: caminho do binário escolhido, prefix: prefixo customizado }.
async function prefixTool(appid, tool, opts = {}) {
  const { prefix } = await verifyWinePrefix(appid, opts)
  const wine = opts.wine || bestWine()
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
  const wine = opts.wine || bestWine()
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
    for (const [arch, dest] of [["x86_64-windows", sys32], ["i386-windows", wow64]]) {
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
  installed,
  available,
  catalog,
  install,
  remove,
  verifyWinePrefix,
  prefixTool,
  runExe,
  installGraphicsLibs,
  prefixOf,
  steamProtons,
  WINE_DIR,
  PREFIX_DIR,
}
