// Runner Legendary (Epic Games) — wrapper do binário CLI, mesma interface que
// os demais runners vão seguir (gog, nile): status / login / library / callRunner.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn, execFileSync } = require("child_process")
const { RUNNERS_DIR, ensureLegendary } = require("./download")

const BIN = path.join(RUNNERS_DIR, "legendary")
const LEGENDARY_CFG = path.join(os.homedir(), ".config", "legendary")

// Spawn com stream: chama onOutput(chunk, "stdout"|"stderr") ao vivo e resolve
// com { code, stdout, stderr } no fim. Base para downloads/instalações depois.
function callRunner(args, { onOutput, timeout = 120000 } = {}) {
  return new Promise((resolve, reject) => {
    let stdout = ""
    let stderr = ""
    const child = spawn(BIN, args, { env: { ...process.env } })
    const kill = setTimeout(() => {
      child.kill("SIGKILL")
      reject(new Error(`legendary ${args[0]}: timeout`))
    }, timeout)
    child.stdout.on("data", (d) => {
      stdout += d
      onOutput?.(String(d), "stdout")
    })
    child.stderr.on("data", (d) => {
      stderr += d
      onOutput?.(String(d), "stderr")
    })
    child.on("error", (e) => {
      clearTimeout(kill)
      reject(e)
    })
    child.on("close", (code) => {
      clearTimeout(kill)
      resolve({ code, stdout, stderr })
    })
  })
}

function status() {
  const installed = fs.existsSync(BIN)
  let logged = false
  let user = ""
  try {
    // O Legendary guarda a sessão em ~/.config/legendary/user.json
    const u = JSON.parse(
      fs.readFileSync(path.join(LEGENDARY_CFG, "user.json"), "utf-8"),
    )
    logged = Boolean(u && (u.refresh_token || u.access_token))
    user = u.displayName || u.account_id || ""
  } catch {
    /* sem sessão */
  }
  return { installed, logged, user }
}

// Login interativo: o `legendary auth` precisa de TTY (abre URL, usuário cola
// o código). Abre num terminal, mesmo padrão do SLScheevo.
function login() {
  const terms = ["kitty", "kgx", "gnome-terminal", "konsole", "alacritty", "xterm"]
  const term = terms.find((t) => {
    try {
      execFileSync("which", [t], { stdio: "ignore" })
      return true
    } catch {
      return false
    }
  })
  if (!term) return { ok: false, error: "nenhum terminal encontrado" }
  const child = spawn(term, ["-e", BIN, "auth"], {
    cwd: RUNNERS_DIR,
    detached: true,
    stdio: "ignore",
  })
  child.unref()
  return { ok: true }
}

// Pega a melhor imagem de um tipo (capa vertical, hero, logo) do metadata.
function imgOf(game, ...types) {
  const imgs = game?.metadata?.keyImages || []
  for (const t of types) {
    const hit = imgs.find((i) => i.type === t && i.url)
    if (hit) return hit.url
  }
  return ""
}

// Biblioteca Epic completa (instalados ou não), normalizada p/ nosso Game.
async function library() {
  const gamesRes = await callRunner(["list-games", "--json"], { timeout: 60000 })
  if (gamesRes.code !== 0) throw new Error(gamesRes.stderr || "list-games falhou")
  let installed = []
  try {
    const instRes = await callRunner(["list-installed", "--json"], { timeout: 60000 })
    if (instRes.code === 0) installed = JSON.parse(instRes.stdout)
  } catch {
    /* sem instalados */
  }
  const instSet = new Set(installed.map((g) => g.app_name))
  const games = JSON.parse(gamesRes.stdout)
  return games
    .filter((g) => g.app_name && g.app_title)
    .map((g) => ({
      id: `epic:${g.app_name}`,
      title: g.app_title,
      launcher: "epic",
      launch_cmd: [BIN, "launch", g.app_name],
      installed: instSet.has(g.app_name),
      cover: imgOf(g, "DieselGameBoxTall", "OfferImageTall"),
      hero: imgOf(g, "DieselGameBox", "OfferImageWide", "VaultClosed"),
      logo: imgOf(g, "DieselGameBoxLogo"),
    }))
}

module.exports = { callRunner, status, login, library, ensureLegendary }
