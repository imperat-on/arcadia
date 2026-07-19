// Loja Steam (estilo Acella): catálogo Hubcap + download via DepotDownloader
// (.NET) + registro do jogo na Steam (appmanifest.acf + SLSsteam config).
// Tudo é subprocess + parsing de texto — sem dependências novas de npm.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn, execFile } = require("child_process")

const DATA_DIR = path.join(os.homedir(), ".local", "share", "arcadia")
const BIN_DIR = path.join(DATA_DIR, "bin")
const DEPS_DIR = path.join(BIN_DIR, "deps", "depotdownloader")
const TMP_DIR = path.join(BIN_DIR, "tmp")
const CONFIG = path.join(DATA_DIR, "config.json")

const HUBCAP_BASE = "https://hubcapmanifest.com/api/v1"

// Log diagnóstico da loja (restart de Steam, etc) em logs/store.log.
function storeLog(msg) {
  try {
    fs.mkdirSync(path.join(BIN_DIR, "..", "logs"), { recursive: true })
    fs.appendFileSync(path.join(BIN_DIR, "..", "logs", "store.log"), `[${new Date().toISOString()}] ${msg}\n`)
  } catch {}
}

function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
  } catch {
    return {}
  }
}

async function gh(url, opts = {}) {
  const r = await fetch(url, { headers: { "User-Agent": "arcadia", ...(opts.headers || {}) }, ...opts })
  return r
}

// ---------- .NET / DepotDownloader ----------

function dotnetBin() {
  const local = path.join(BIN_DIR, "dotnet", "dotnet")
  if (fs.existsSync(local)) return local
  return "dotnet" // PATH do sistema
}

async function ensureDotnet(onProgress) {
  try {
    execFile("dotnet", ["--version"], (e, stdout) => {})
    const ok = await new Promise((res) => execFile("dotnet", ["--version"], (e) => res(!e)))
    if (ok) return { ok: true, path: "dotnet" }
  } catch {}
  // Instala .NET 9 localmente via script oficial.
  const dir = path.join(BIN_DIR, "dotnet")
  fs.mkdirSync(dir, { recursive: true })
  const script = path.join(TMP_DIR, "dotnet-install.sh")
  fs.mkdirSync(TMP_DIR, { recursive: true })
  const r = await fetch("https://dot.net/v1/dotnet-install.sh")
  if (!r.ok) return { ok: false, error: `dotnet-install.sh HTTP ${r.status}` }
  fs.writeFileSync(script, Buffer.from(await r.arrayBuffer()))
  fs.chmodSync(script, 0o755)
  onProgress?.("Instalando .NET 9 (pode demorar)…")
  const code = await new Promise((res) => {
    const c = spawn("bash", [script, "--channel", "9.0", "--install-dir", dir], { stdio: "ignore" })
    c.on("close", res)
    c.on("error", () => res(1))
  })
  const bin = path.join(dir, "dotnet")
  return code === 0 && fs.existsSync(bin) ? { ok: true, path: bin } : { ok: false, error: "falha ao instalar o .NET" }
}

function depsOk() {
  return fs.existsSync(path.join(DEPS_DIR, "DepotDownloader.dll"))
}

// ---------- Hubcap (catálogo + manifestos) ----------

function mapJogos(data) {
  return (data.games || [])
    .map((g) => ({
      appid: String(g.game_id || ""),
      title: g.game_name || "",
      cover: g.header_image || "",
      manifest: Boolean(g.manifest_available),
    }))
    .filter((g) => g.appid && g.title)
}

async function search(query) {
  const cfg = readConfig()
  // 1) Hubcap (catálogo com indicação de manifesto disponível).
  if (cfg.hubcap_api_key) {
    try {
      const r = await gh(`${HUBCAP_BASE}/library?search=${encodeURIComponent(query)}`, {
        headers: { Authorization: `Bearer ${cfg.hubcap_api_key}` },
      })
      if (r.ok) return { ok: true, jogos: mapJogos(await r.json()), fonte: "hubcap" }
      if (r.status !== 429) return { ok: false, error: `Hubcap HTTP ${r.status}` }
      // 429: cai no fallback abaixo.
    } catch {}
  }
  // 2) Fallback: busca oficial da Steam (sem key, sem indicação de manifesto).
  const r = await gh(
    `https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(query)}&cc=br&l=portuguese`,
  )
  if (!r.ok) return { ok: false, error: `Hubcap 429 e Steam HTTP ${r.status}` }
  const data = await r.json()
  const jogos = (data.items || [])
    .map((g) => ({
      appid: String(g.id || ""),
      title: g.name || "",
      cover: "",
      manifest: true,
    }))
    .filter((g) => g.appid && g.title)
  return { ok: true, jogos, fonte: "steam" }
}

// Lançamentos/adicionados recentemente no catálogo (home da aba Lojas).
async function recent(limit = 24) {
  const cfg = readConfig()
  if (!cfg.hubcap_api_key) return { ok: false, error: "sem API key" }
  const r = await gh(`${HUBCAP_BASE}/library?limit=${limit}&sort_by=updated`, {
    headers: { Authorization: `Bearer ${cfg.hubcap_api_key}` },
  })
  if (!r.ok) return { ok: false, error: `Hubcap HTTP ${r.status}` }
  return { ok: true, jogos: mapJogos(await r.json()).filter((g) => g.manifest) }
}

// "Mais baixados/em alta" (o Hubcap não tem ranking): SteamSpy top 100 das
// últimas 2 semanas — sem API key.
async function popular() {
  const r = await gh("https://steamspy.com/api.php?request=top100in2weeks")
  if (!r.ok) return { ok: false, error: `SteamSpy HTTP ${r.status}` }
  const data = await r.json()
  const jogos = Object.values(data)
    .map((g) => ({
      appid: String(g.appid || ""),
      title: g.name || "",
      cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
      manifest: true,
    }))
    .filter((g) => g.appid && g.title)
    .slice(0, 24)
  return { ok: true, jogos }
}

// ---------- Fixes de jogos (estilo luatools: GameBypass/OnlineFix) ----------
// Índice: index.luatools.work/fixes-index.json (lista appids). Zips:
// files.luatools.work/GameBypasses/<appid>.zip e OnlineFix1/<appid>.zip.
const FIXES_INDEX = "https://index.luatools.work/fixes-index.json"
let fixesCache = null

async function fixesIndex() {
  if (fixesCache && Date.now() - fixesCache.ts < 6 * 3600_000) return fixesCache.data
  const r = await gh(FIXES_INDEX, { headers: { "User-Agent": "luatools" } })
  if (!r.ok) throw new Error(`índice de fixes HTTP ${r.status}`)
  const data = await r.json()
  fixesCache = { ts: Date.now(), data }
  return data
}

async function checkFixes(appid) {
  try {
    const idx = await fixesIndex()
    const id = Number(appid)
    const has = (arr) => (arr || []).some((v) => Number(v) === id)
    return {
      ok: true,
      generic: has(idx.genericFixes),
      online: has(idx.onlineFixes),
    }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Baixa e extrai o fix na pasta de instalação do jogo.
async function applyFix(appid, type, installPath) {
  try {
    const base = type === "online" ? "OnlineFix1" : "GameBypasses"
    const url = `https://files.luatools.work/${base}/${appid}.zip`
    const r = await gh(url, { headers: { "User-Agent": "luatools" } })
    if (!r.ok) return { ok: false, error: `fix HTTP ${r.status}` }
    const zipPath = path.join(TMP_DIR, `fix_${appid}.zip`)
    fs.mkdirSync(TMP_DIR, { recursive: true })
    fs.writeFileSync(zipPath, Buffer.from(await r.arrayBuffer()))
    if (!fs.existsSync(installPath)) return { ok: false, error: `pasta não existe: ${installPath}` }
    await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, installPath], res))
    fs.rmSync(zipPath, { force: true })
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Pasta de instalação do jogo (para onde o fix vai).
function gameInstallDir(g) {
  const home = os.homedir()
  const appid = String(g?.id || "").replace(/^steam:/, "")
  if (g?.launcher === "custom" && g.exe) return path.dirname(g.exe)
  if (g?.launcher === "steam" || String(g?.id || "").startsWith("steam:")) {
    for (const dir of [
      path.join(home, ".steam", "steam", "steamapps"),
      path.join(home, ".local", "share", "Steam", "steamapps"),
      path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "steamapps"),
    ]) {
      const acf = path.join(dir, `appmanifest_${appid}.acf`)
      if (fs.existsSync(acf)) {
        const m = /"installdir"\s+"([^"]+)"/.exec(fs.readFileSync(acf, "utf-8"))
        if (m) return path.join(dir, "common", m[1])
      }
    }
  }
  if (g?.launcher === "epic") {
    try {
      const inst = JSON.parse(fs.readFileSync(path.join(home, ".config", "legendary", "installed.json"), "utf-8"))
      const app = String(g.id).replace(/^epic:/, "")
      if (inst[app]?.install_path) return inst[app].install_path
    } catch {}
  }
  return ""
}

// Provedores de manifesto (estilo luatools): tentados EM ORDEM até um
// devolver depots. Todos devolvem um zip no formato SteamTools (.lua+manifest).
const PROVEDORES = [
  { nome: "Morrenus", url: (appid, cfg) => `${HUBCAP_BASE}/manifest/${appid}?api_key=${cfg.hubcap_api_key || ""}`, headers: (cfg) => ({ Authorization: `Bearer ${cfg.hubcap_api_key}` }), precisaKey: true },
  { nome: "Ryuu", url: (appid) => `http://167.235.229.108/${appid}`, headers: () => ({}) },
  { nome: "TwentyTwo Cloud", url: (appid) => `https://api.twentytwocloud.com/download?appid=${appid}`, headers: () => ({}) },
  { nome: "Sushi", url: (appid) => `https://raw.githubusercontent.com/sushi-dev55-alt/sushitools-games-repo-alt/refs/heads/main/${appid}.zip`, headers: () => ({}) },
]

// Baixa o zip do appid (provedor com fallback) e extrai depots/keys/token do .lua.
async function getManifest(appid) {
  const cfg = readConfig()
  const zipPath = path.join(TMP_DIR, `manifest_${appid}.zip`)
  fs.mkdirSync(TMP_DIR, { recursive: true })

  let fonte = ""
  let baixou = false
  const erros = []
  for (const p of PROVEDORES) {
    if (p.precisaKey && !cfg.hubcap_api_key) continue
    try {
      const r = await gh(p.url(appid, cfg), { headers: { "User-Agent": "arcadia", ...p.headers(cfg) } })
      if (!r.ok) {
        erros.push(`${p.nome}: HTTP ${r.status}`)
        continue
      }
      const buf = Buffer.from(await r.arrayBuffer())
      // Zip válido começa com PK — alguns provedores devolvem HTML/erro com 200.
      if (buf.length < 4 || buf[0] !== 0x50 || buf[1] !== 0x4b) {
        erros.push(`${p.nome}: resposta não é zip`)
        continue
      }
      fs.writeFileSync(zipPath, buf)
      fonte = p.nome
      baixou = true
      break
    } catch (e) {
      erros.push(`${p.nome}: ${e}`)
    }
  }
  if (!baixou) return { ok: false, error: erros.join(" · ") || "nenhum provedor disponível" }

  const outDir = path.join(TMP_DIR, `manifest_${appid}`)
  fs.rmSync(outDir, { recursive: true, force: true })
  fs.mkdirSync(outDir, { recursive: true })
  await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, outDir], res))

  // .lua: addappid(id, ...ignored..., "depotkey"), setManifestid(depot, "id"), addtoken("...")
  const depots = []
  let token = ""
  const dlcs = []
  for (const f of fs.readdirSync(outDir)) {
    if (!f.endsWith(".lua")) continue
    const lua = fs.readFileSync(path.join(outDir, f), "utf-8")
    const keyRe = /addappid\(\s*(\d+)\s*,\s*\d+\s*,\s*"([0-9a-f]+)"\s*\)/g
    let m
    const keys = {}
    while ((m = keyRe.exec(lua))) keys[m[1]] = m[2]
    const manRe = /setManifestid\(\s*(\d+)\s*,\s*"(\d+)"\s*(?:,\s*(\d+))?/g
    while ((m = manRe.exec(lua))) {
      depots.push({ depotId: m[1], manifestId: m[2], key: keys[m[1]] || "", size: Number(m[3] || 0) })
    }
    const tokRe = /addtoken\(\s*"([^"]+)"\s*\)/
    const t = tokRe.exec(lua)
    if (t) token = t[1]
    const dlcRe = /addappid\(\s*(\d+)\s*\)/g
    while ((m = dlcRe.exec(lua))) {
      if (m[1] !== String(appid) && !dlcs.includes(m[1])) dlcs.push(m[1])
    }
  }
  if (!depots.length) return { ok: false, error: `${fonte}: zip sem depots` }
  return { ok: true, appid: String(appid), depots, token, dlcs, outDir, fonte }
}

// ---------- Download via DepotDownloader ----------

// Monta o spawn do download de um appid. Retorna { cmd, args, env }.
async function prepareDownload({ appid, installdir, depots, steamDir }) {
  if (!depsOk()) return { ok: false, error: "DepotDownloader não encontrado em bin/deps" }
  const keysFile = path.join(TMP_DIR, `keys_${appid}.vdf`)
  const linhas = depots.filter((d) => d.key).map((d) => `${d.depotId};${d.key}`)
  fs.writeFileSync(keysFile, linhas.join("\n"))
  const dest = path.join(steamDir, "steamapps", "common", installdir)
  fs.mkdirSync(dest, { recursive: true })
  const args = [path.join(DEPS_DIR, "DepotDownloader.dll"), "-app", String(appid)]
  for (const d of depots) {
    args.push("-depot", String(d.depotId), "-manifest", String(d.manifestId))
    const manFile = path.join(TMP_DIR, `manifest_${appid}`, `${d.depotId}_${d.manifestId}.manifest`)
    if (fs.existsSync(manFile)) args.push("-manifestfile", manFile)
  }
  args.push("-depotkeys", keysFile, "-max-downloads", "20", "-dir", dest, "-validate")
  return { ok: true, cmd: dotnetBin(), args, dest }
}

// ---------- Registro na Steam (acf + SLSsteam) ----------

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

function acfEscape(s) {
  return String(s).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
}

// Escreve steamapps/appmanifest_<appid>.acf mínimo (port do Acella).
// A marca "ArcadiaDownload" distingue downloads NOSSOS de jogos owned — é ela
// que habilita o Remover seguro (nunca apaga acf de jogo comprado).
function writeAcf({ appid, title, installdir, steamDir }) {
  const apps = path.join(steamDir, "steamapps")
  const agora = Math.floor(Date.now() / 1000)
  const conteudo = [
    `"AppState"`,
    `{`,
    `\t"appid"\t\t"${acfEscape(appid)}"`,
    `\t"Universe"\t\t"1"`,
    `\t"name"\t\t"${acfEscape(title)}"`,
    `\t"StateFlags"\t\t"4"`,
    `\t"installdir"\t\t"${acfEscape(installdir)}"`,
    `\t"LastUpdated"\t\t"${agora}"`,
    `\t"LastPlayed"\t\t"0"`,
    `\t"SizeOnDisk"\t\t"1"`,
    `\t"buildid"\t\t"1"`,
    `\t"ArcadiaDownload"\t\t"1"`,
    `}`,
    ``,
  ].join("\n")
  fs.writeFileSync(path.join(apps, `appmanifest_${appid}.acf`), conteudo)
}

// Acfs de downloads feitos pelo Arcadia (com a marca), em todas as bibliotecas.
function arcadiaDownloaded() {
  const out = []
  for (const lib of steamLibraries()) {
    try {
      for (const f of fs.readdirSync(lib.path)) {
        const m = /^appmanifest_(\d+)\.acf$/.exec(f)
        if (!m) continue
        const txt = fs.readFileSync(path.join(lib.path, f), "utf-8")
        if (txt.includes('"ArcadiaDownload"')) {
          const dir = /"installdir"\s+"([^"]+)"/.exec(txt)
          out.push({ appid: m[1], steamapps: lib.path, installdir: dir?.[1] || "" })
        }
      }
    } catch {}
  }
  return out
}

// Remove um jogo BAIXADO pela loja: apaga pasta, appmanifest marcado e
// registro na SLSsteam. Nunca toca em acf sem a marca (jogos owned).
function removeDownloaded(appid) {
  const id = String(appid)
  const achados = arcadiaDownloaded().filter((a) => a.appid === id)
  for (const a of achados) {
    try { fs.rmSync(path.join(a.steamapps, `appmanifest_${id}.acf`), { force: true }) } catch {}
    if (a.installdir) {
      const dir = path.join(a.steamapps, "common", a.installdir)
      if (dir.startsWith(path.join(a.steamapps, "common") + path.sep) && fs.existsSync(dir)) {
        try { fs.rmSync(dir, { recursive: true, force: true }) } catch {}
      }
    }
  }
  removeFromSteam(id)
  return { ok: true, removidos: achados.length }
}

// Registra appid/token/DLCs no ~/.config/SLSsteam/config.yaml (edição simples).
function registerSlssteam({ appid, token, dlcs }) {
  const cfgPath = path.join(os.homedir(), ".config", "SLSsteam", "config.yaml")
  if (!fs.existsSync(cfgPath)) return { ok: false, error: "config.yaml da SLSsteam não encontrado" }
  let y = fs.readFileSync(cfgPath, "utf-8")
  const appidStr = String(appid)
  if (!y.includes(appidStr)) {
    // AdditionalApps: lista de appids extras exibidos como owned.
    if (/^AdditionalApps:/m.test(y)) {
      y = y.replace(/^AdditionalApps:\s*$/m, `AdditionalApps:\n  - ${appidStr}`)
      y = y.replace(/^(AdditionalApps:\s*\[)([^\]]*)\]/m, (_m, a, b) => `${a}${b ? b + ", " : ""}${appidStr}]`)
    } else {
      y += `\nAdditionalApps:\n  - ${appidStr}\n`
    }
  }
  if (token && !y.includes(token)) {
    if (/^AppTokens:/m.test(y)) {
      y = y.replace(/^AppTokens:\s*$/m, `AppTokens:\n  ${appidStr}: ${token}`)
      y = y.replace(/^AppTokens:\s*\{\}\s*$/m, `AppTokens:\n  ${appidStr}: ${token}`)
    } else {
      y += `\nAppTokens:\n  ${appidStr}: ${token}\n`
    }
  }
  for (const dlc of dlcs || []) {
    if (!y.includes(dlc)) {
      // DlcData é mapa de mapas: <appid>: { <dlcId>: "nome" } — escrever como
      // lista (- dlc) ou chave duplicada CRASHA a Steam no boot (yaml-cpp).
      if (new RegExp(`^  ${appidStr}:`, "m").test(y)) {
        y = y.replace(new RegExp(`^(  ${appidStr}:\\s*\\n)`, "m"), `$1    ${dlc}: "DLC"\n`)
      } else if (/^DlcData:/m.test(y)) {
        y = y.replace(/^DlcData:\s*$/m, `DlcData:\n  ${appidStr}:\n    ${dlc}: "DLC"`)
      } else {
        y += `\nDlcData:\n  ${appidStr}:\n    ${dlc}: "DLC"\n`
      }
    }
  }
  fs.writeFileSync(cfgPath, y)
  return { ok: true }
}

// Reinicia a Steam com a SLSsteam carregada (jogos aparecem como owned).
// Prefere o wrapper do slsteam-moon (~/.local/share/SLSsteam/path/steam),
// que injeta LD_AUDIT do jeito certo; fallback: steam puro + LD_AUDIT.
function launchSteamWithSls(cfg = readConfig()) {
  const home = os.homedir()
  const wrapper = path.join(home, ".local", "share", "SLSsteam", "path", "steam")
  const inject = [
    path.join(home, ".local", "share", "SLSsteam", "library-inject.so"),
    path.join(home, ".local", "share", "SLSsteam", "SLSsteam.so"),
  ]
  if (cfg.slssteam_path) inject.push(cfg.slssteam_path)
  const validos = inject.filter((p) => fs.existsSync(p))
  const usarWrapper = fs.existsSync(wrapper)
  if (!usarWrapper && validos.length < 2) {
    return { ok: false, error: "SLSsteam não instalada (rode o setup em Configurações)" }
  }
  execFile("pkill", ["-x", "steam"], () => {
    // Espera a Steam morrer DE VERDADE antes de relançar — relançar cedo
    // demais batia no lock da instância antiga e a nova morria (parecia
    // "shutdown" e precisava de um segundo clique).
    const env = { ...process.env }
    delete env.LD_LIBRARY_PATH
    delete env.LD_PRELOAD
    delete env.LD_AUDIT
    delete env.STEAM_RUNTIME_LIBRARY_PATH
    let cmd = "steam"
    if (usarWrapper) {
      cmd = wrapper
    } else {
      env.LD_AUDIT = validos.join(":")
    }
    let tentativas = 0
    const t0 = Date.now()
    const esperar = setInterval(() => {
      execFile("pgrep", ["-x", "steam"], (e) => {
        const morreu = Boolean(e)
        const estourou = ++tentativas > 120
        if (morreu || estourou) {
          clearInterval(esperar)
          storeLog(`restart: steam ${morreu ? "morreu" : "timeout 120s"} em ${Date.now() - t0}ms — relançando via ${cmd}`)
          // Graça de 2s p/ o lock do cliente liberar antes de relançar.
          setTimeout(() => {
            const child = spawn(cmd, [], { detached: true, stdio: "ignore", env })
            child.unref()
          }, 2000)
        }
      })
    }, 1000)
  })
  return { ok: true }
}

// Instala a SLSsteam (slsteam-moon) do zero: baixa o release mais recente do
// GitHub, extrai e roda o setup.sh install. Retorna { ok, error? }.
async function installSlssteam(onProgress) {
  const home = os.homedir()
  const slsDir = path.join(home, ".local", "share", "SLSsteam")
  try {
    onProgress?.("Buscando release mais recente…")
    const rel = await gh("https://api.github.com/repos/swwayps/slsteam-moon/releases/latest").then((r) => r.json())
    const asset = (rel.assets || []).find((a) => /^slsteam-moon-linux-.*-lumen\.zip$/.test(a.name || ""))
    if (!asset) return { ok: false, error: "asset slsteam-moon-linux-*-lumen.zip não encontrado" }

    onProgress?.("Baixando slsteam-moon…")
    const zipPath = path.join(TMP_DIR, "slsteam-moon.zip")
    fs.mkdirSync(TMP_DIR, { recursive: true })
    const buf = await fetch(asset.browser_download_url).then((r) => {
      if (!r.ok) throw new Error(`download HTTP ${r.status}`)
      return r.arrayBuffer()
    })
    fs.writeFileSync(zipPath, Buffer.from(buf))

    const outDir = path.join(TMP_DIR, "slsteam-moon")
    fs.rmSync(outDir, { recursive: true, force: true })
    fs.mkdirSync(outDir, { recursive: true })
    await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, outDir], res))

    // setup.sh fica na raiz da pasta extraída (slsteam-moon-<ver>-lumen/).
    const raiz = fs.readdirSync(outDir).map((d) => path.join(outDir, d)).find((p) => fs.existsSync(path.join(p, "setup.sh")))
    if (!raiz) return { ok: false, error: "setup.sh não encontrado no pacote" }

    onProgress?.("Instalando (setup.sh install)…")
    const code = await new Promise((res) => {
      const c = spawn("bash", [path.join(raiz, "setup.sh"), "install"], { cwd: raiz, stdio: "ignore" })
      c.on("close", res)
      c.on("error", () => res(1))
      setTimeout(() => res(1), 300000)
    })
    const instalado = fs.existsSync(path.join(slsDir, "SLSsteam.so"))
    return code === 0 && instalado ? { ok: true } : { ok: false, error: `setup.sh saiu com código ${code}` }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Adiciona o jogo à Steam SEM baixar (estilo LuaTools): copia o .lua do
// manifesto para config/stplug-in (a SLSsteam aplica keys/tokens nativamente)
// e registra o appid em AdditionalApps. O download fica por conta da Steam.
function addToSteam(appid) {
  const home = os.homedir()
  const outDir = path.join(TMP_DIR, `manifest_${appid}`)
  const stplug = path.join(home, ".config", "SLSsteam", "config", "stplug-in")
  try {
    fs.mkdirSync(stplug, { recursive: true })
    let luas = 0
    if (fs.existsSync(outDir)) {
      for (const f of fs.readdirSync(outDir)) {
        if (f.endsWith(".lua")) {
          fs.copyFileSync(path.join(outDir, f), path.join(stplug, f))
          luas++
        }
      }
    }
    if (!luas) return { ok: false, error: "nenhum .lua no manifesto — baixe o manifesto antes (botão Baixar)" }
    return { ok: true, luas }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Bibliotecas Steam (multi-drive): lê steamapps/libraryfolders.vdf.
// Cada biblioteca é uma pasta "SteamLibrary" com steamapps/ dentro.
function steamLibraries() {
  const home = os.homedir()
  const raizes = [
    path.join(home, ".steam", "steam", "steamapps"),
    path.join(home, ".local", "share", "Steam", "steamapps"),
    path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "steamapps"),
  ]
  const libs = []
  for (const raiz of raizes) {
    const vdf = path.join(raiz, "libraryfolders.vdf")
    if (!fs.existsSync(vdf)) continue
    const txt = fs.readFileSync(vdf, "utf-8")
    // Formato: "0" { "path" "/x/SteamLibrary" ... } — pega todos os "path".
    for (const m of txt.matchAll(/"path"\s+"([^"]+)"/g)) {
      const p = m[1].replace(/\\\\/g, "/")
      const apps = path.join(p, "steamapps")
      if (!libs.some((l) => l.path === apps)) libs.push({ path: apps, steamDir: p })
    }
  }
  // Garante ao menos a principal.
  if (!libs.length) {
    const principal = findSteamDir()
    libs.push({ path: path.join(principal, "steamapps"), steamDir: principal })
  }
  // Espaço livre (df) por biblioteca.
  return libs.map((l) => {
    let free = 0
    try {
      const { execFileSync } = require("child_process")
      const out = execFileSync("df", ["-BG", "--output=avail", l.path], { encoding: "utf-8" })
      free = parseInt(out.trim().split("\n").pop(), 10) || 0
    } catch {}
    return { ...l, free }
  })
}

// Remove um jogo adicionado via "Add": apaga o .lua do stplug-in e tira o
// appid de AdditionalApps/AppTokens/DlcData no config.yaml da SLSsteam.
function removeFromSteam(appid) {
  const home = os.homedir()
  const id = String(appid)
  const stplug = path.join(home, ".config", "SLSsteam", "config", "stplug-in")
  try {
    if (fs.existsSync(stplug)) {
      for (const f of fs.readdirSync(stplug)) {
        if (f === `${id}.lua` || f.startsWith(`${id}_`)) fs.rmSync(path.join(stplug, f), { force: true })
      }
    }
  } catch {}
  const cfgPath = path.join(home, ".config", "SLSsteam", "config.yaml")
  if (!fs.existsSync(cfgPath)) return { ok: true }
  let y = fs.readFileSync(cfgPath, "utf-8")
  // Linha "  - <appid>" em AdditionalApps.
  y = y.replace(new RegExp(`^\\s*-\\s*${id}\\s*(#.*)?$\\n?`, "m"), "")
  // Token em AppTokens — SÓ dentro da seção AppTokens (antes o regex apagava
  // também a chave-pai do appid no DlcData, órfãos que crasham a Steam).
  const partes = y.split(/^(?=AppTokens:)/m)
  if (partes.length > 1) {
    const resto = partes[1].split(/^(?=\S)/m)
    resto[0] = resto[0].replace(new RegExp(`^\\s*${id}:.*$\\n?`, "m"), "")
    y = partes[0] + resto.join("")
  }
  fs.writeFileSync(cfgPath, y)
  return { ok: true }
}

async function status() {
  const dotnetSys = await new Promise((res) => execFile("dotnet", ["--version"], (e, stdout) => res(e ? "" : String(stdout).trim())))
  // AppIds já registrados (AdditionalApps do config.yaml + .lua no stplug-in)
  // — cobre apps adicionados por qualquer ferramenta, a qualquer época.
  const adicionados = new Set()
  try {
    const y = fs.readFileSync(path.join(os.homedir(), ".config", "SLSsteam", "config.yaml"), "utf-8")
    const bloco = y.split(/^AdditionalApps:/m)[1] || ""
    for (const m of bloco.matchAll(/^\s*-\s*(\d+)/gm)) adicionados.add(m[1])
  } catch {}
  try {
    const stplug = path.join(os.homedir(), ".config", "SLSsteam", "config", "stplug-in")
    for (const f of fs.readdirSync(stplug)) {
      const m = /^(\d+)(?:_.*)?\.lua$/.exec(f)
      if (m) adicionados.add(m[1])
    }
  } catch {}
  // Downloads feitos pelo Arcadia (acf marcado) também contam como "adicionados".
  for (const a of arcadiaDownloaded()) adicionados.add(a.appid)
  return {
    dotnet: fs.existsSync(path.join(BIN_DIR, "dotnet", "dotnet")) || dotnetSys || undefined,
    depotdownloader: depsOk(),
    hubcapKey: Boolean(readConfig().hubcap_api_key),
    slssteam: fs.existsSync(path.join(os.homedir(), ".local", "share", "SLSsteam", "SLSsteam.so")),
    steamDir: findSteamDir(),
    adicionados: [...adicionados],
  }
}

module.exports = {
  search,
  recent,
  popular,
  checkFixes,
  applyFix,
  gameInstallDir,
  getManifest,
  prepareDownload,
  ensureDotnet,
  writeAcf,
  registerSlssteam,
  launchSteamWithSls,
  installSlssteam,
  addToSteam,
  removeFromSteam,
  removeDownloaded,
  arcadiaDownloaded,
  steamLibraries,
  findSteamDir,
  status,
  DEPS_DIR,
}
