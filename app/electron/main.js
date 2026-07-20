const { app, BrowserWindow, ipcMain, dialog, shell } = require("electron")
const { getNews } = require("./news")
const { startAchievementWatcher } = require("./achievements")
const path = require("path")
const fs = require("fs")
const os = require("os")
const { spawn, execFile } = require("child_process")
const {
  readOverrides,
  setOverride,
  applyOverrides,
  artToDelete,
} = require("./overrides")
const {
  sgdbSearch,
  sgdbArt,
  wallhavenBusca,
  psnStoreSearch,
  psnMelhorResultado,
  psnStoreArt,
  steamArt,
  steamTextos,
  igdbGames,
  igdbArtDe,
  igdbTextosDe,
  xboxSearch,
  xboxProduto,
  xboxArtDe,
  xboxTextoDe,
  downloadTo,
  SGDB_ENDPOINT,
} = require("./metadata")

const HOME = os.homedir()
const DATA_DIR = path.join(HOME, ".local/share/arcadia")
const LIB = path.join(DATA_DIR, "library.json")
const INDEX = path.join(DATA_DIR, "index.py")
const CONFIG = path.join(DATA_DIR, "config.json")
const META_CACHE = path.join(DATA_DIR, "meta_cache.json")
const OVERRIDES = path.join(DATA_DIR, "overrides.json")
const ART_DIR = path.join(DATA_DIR, "art") // artes escolhidas pelo usuário
const NEWS_CACHE = path.join(DATA_DIR, "news_cache.json") // notícias cacheadas (TTL)
const TRAILERS_DIR = path.join(DATA_DIR, "trailers") // trailers baixados do YouTube
const BIN_DIR = path.join(DATA_DIR, "bin")
// Preferimos a cópia em bin/ (versão fixada), mas ela só existe se alguém já a
// tiver baixado. Em máquina limpa não há nada ali: sem o fallback para o yt-dlp
// do sistema, o execFile dava ENOENT e o usuário só via "trailer não encontrado".
// O resultado precisa ser um caminho ABSOLUTO: os handlers de IPC checam
// fs.existsSync(YTDLP) antes de agir, e um nome solto ("yt-dlp") nunca existe
// como arquivo relativo ao cwd — todo trailer virava "yt-dlp ausente".
function acharYtdlp() {
  const local = path.join(BIN_DIR, "yt-dlp")
  if (fs.existsSync(local)) return local
  // O PATH do processo pode estar enxuto (gamescope/sessão sem shell de login),
  // então varremos também os diretórios usuais além do que o PATH informar.
  const dirs = [
    ...(process.env.PATH || "").split(":").filter(Boolean),
    "/usr/bin", "/usr/local/bin", "/bin",
    path.join(os.homedir(), ".local", "bin"),
  ]
  for (const d of dirs) {
    const p = path.join(d, "yt-dlp")
    try {
      fs.accessSync(p, fs.constants.X_OK)
      return p
    } catch {}
  }
  return ""
}
const YTDLP = acharYtdlp()

// Padrão que casa o PROCESSO de um jogo rodando (Steam/Proton/Heroic/Lutris).
// Usado pelo vigia "game:running" e pelo "game:close". pgrep nunca casa
// consigo mesmo.
const PADRAO_JOGO = "steamapps/common/|steamapps/compatdata/|Heroic/Prefixes|lutris/runners"

// Logs de lançamento ("Habilitar logs detalhados", aba AVANÇADO).
const LOG_DIR = path.join(DATA_DIR, "logs")
// Script pós-jogo pendente (aba AVANÇADO): roda quando o jogo fechar.
let postGameScript = ""
// Jogo lançado por nós: { pid (líder do grupo), alvo }. O grupo de processos
// é o que fecha/vigia de forma universal (custom, umu, legendary, lutris).
let jogoAtivo = null
// yt-dlp precisa achar o Deno para resolver o desafio JS do YouTube (necessário
// em vídeos com restrição de idade). Aceitamos tanto a cópia em bin/ quanto a do
// sistema, e garantimos os diretórios padrão: no gamescope o PATH herdado pode
// vir enxuto, sem nem /usr/bin — foi o que já quebrou a busca de trailers.
const YTDLP_ENV = {
  ...process.env,
  PATH: [BIN_DIR, process.env.PATH || "", "/usr/bin", "/usr/local/bin", "/bin"]
    .filter(Boolean)
    .join(":"),
}
// Pasta do ffmpeg (necessário p/ juntar vídeo+áudio dos vídeos só-DASH). Passamos
// explícito porque o PATH do app pode não incluir /usr/bin (ex.: no gamescope).
const FFMPEG_DIR =
  ["/usr/bin", "/usr/local/bin", "/bin"].find((d) => fs.existsSync(path.join(d, "ffmpeg"))) || ""
const FF_ARGS = FFMPEG_DIR ? ["--ffmpeg-location", FFMPEG_DIR] : []
const SLS_CONFIG = path.join(HOME, ".config/SLSsteam/config.yaml")

// Diário do subsistema de trailers. Sem isto, toda falha (binário ausente,
// rede, extractor do YouTube quebrado) chegava na tela como o mesmo
// "Nenhum vídeo encontrado" — impossível de diagnosticar à distância.
const TRAILER_LOG = path.join(LOG_DIR, "trailers.log")
function logTrailer(msg) {
  try {
    fs.mkdirSync(LOG_DIR, { recursive: true })
    fs.appendFileSync(TRAILER_LOG, `${new Date().toISOString()} ${msg}\n`)
  } catch {}
}

// Trailers em andamento (evita baixar o mesmo jogo duas vezes ao mesmo tempo).
const trailerJobs = new Map()

function safeName(id) {
  return String(id).replace(/[^a-z0-9._-]/gi, "_")
}

// Remove TODOS os arquivos de um jogo (inclusive parciais .part/.fNNN de
// tentativas anteriores, que causam "HTTP 416 range not satisfiable").
function limparTrailer(safe) {
  try {
    for (const f of fs.readdirSync(TRAILERS_DIR)) {
      if (f === safe || f.startsWith(safe + ".")) {
        try {
          fs.unlinkSync(path.join(TRAILERS_DIR, f))
        } catch {
          /* já sumiu */
        }
      }
    }
  } catch {
    /* pasta ainda não existe */
  }
}

// Cookies do YouTube (arquivo cookies.txt do usuário) para vídeos com restrição
// de idade. Vazio = sem cookies (a maioria dos vídeos não precisa).
function cookieArgs() {
  try {
    const p = String(readConfig().youtube_cookies || "").trim()
    if (p && fs.existsSync(p)) return ["--cookies", p]
  } catch {
    /* sem config */
  }
  return []
}

// Caminho local do trailer já baixado (mp4/webm), ou "" se não existe.
function trailerLocal(id) {
  const base = path.join(TRAILERS_DIR, safeName(id))
  for (const ext of [".mp4", ".webm", ".mkv"]) {
    if (fs.existsSync(base + ext)) return base + ext
  }
  return ""
}

// Baixa o trailer do YouTube via yt-dlp. Resolve com o caminho local.
function baixarTrailer(id, titulo) {
  const existe = trailerLocal(id)
  if (existe) return Promise.resolve({ ok: true, path: existe })
  if (trailerJobs.has(id)) return trailerJobs.get(id)

  const job = new Promise((resolve) => {
    fs.mkdirSync(TRAILERS_DIR, { recursive: true })
    const safe = safeName(id)
    limparTrailer(safe) // tira parciais que causariam HTTP 416
    const args = [
      `ytsearch5:${titulo} trailer`,
      "--no-playlist",
      "--no-warnings",
      "--no-continue",
      "--no-part",
      "--match-filter",
      "duration > 20 & duration < 360", // pega trailer curto, não gameplay de 1h
      "-f",
      "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
      "--remux-video",
      "mp4",
      ...FF_ARGS,
      ...cookieArgs(),
      "-o",
      path.join(TRAILERS_DIR, `${safe}.%(ext)s`),
    ]
    execFile(YTDLP, args, { timeout: 180000, env: YTDLP_ENV }, (err) => {
      // yt-dlp pode sair !=0 (limite/reject); o que vale é o arquivo existir.
      const p = trailerLocal(id)
      if (p) return resolve({ ok: true, path: p })
      // ENOENT aqui é o binário ausente, não "sem resultado" — distinguir os
      // dois evita mandar o usuário caçar um problema de rede que não existe.
      if (err && err.code === "ENOENT") {
        return resolve({ ok: false, error: "yt-dlp não instalado (instale o pacote yt-dlp)" })
      }
      resolve({ ok: false, error: "trailer não encontrado" })
    })
  }).finally(() => trailerJobs.delete(id))

  trailerJobs.set(id, job)
  return job
}

// URL de stream direto (mp4 progressivo) para pré-visualizar sem baixar. O
// embed do YouTube recusa origem file:// (erro 153); um <video> nativo não.
function streamTrailer(url) {
  return new Promise((resolve) => {
    execFile(
      YTDLP,
      // 22/18 são progressivos (áudio+vídeo num arquivo só) que quase todo vídeo
      // tem — dá prévia mesmo nos que só têm faixas DASH separadas.
      ["-g", "-f", "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best", "--no-warnings", ...cookieArgs(), url],
      { timeout: 40000, maxBuffer: 1024 * 1024 * 4, env: YTDLP_ENV },
      (err, stdout, stderr) => {
        const link = String(stdout || "").split("\n").find((l) => l.startsWith("http"))
        if (link) return resolve({ ok: true, url: link })
        const age = /confirm your age|inappropriate/i.test(String(stderr || ""))
        resolve({ ok: false, error: age ? "age" : "sem stream" })
      },
    )
  })
}

// Busca (sem baixar) os vídeos do YouTube para o usuário escolher o certo.
function buscarTrailers(query) {
  return new Promise((resolve) => {
    const args = [
      `ytsearch12:${query} trailer`,
      "--flat-playlist",
      "--dump-json",
      "--no-warnings",
    ]
    execFile(YTDLP, args, { timeout: 40000, maxBuffer: 1024 * 1024 * 8, env: YTDLP_ENV }, (err, stdout, stderr) => {
      const out = []
      for (const line of String(stdout || "").split("\n")) {
        if (!line.trim()) continue
        try {
          const d = JSON.parse(line)
          const thumbs = d.thumbnails || []
          out.push({
            id: d.id,
            url: d.url || `https://www.youtube.com/watch?v=${d.id}`,
            title: d.title || "",
            duration: d.duration || 0,
            channel: d.channel || d.uploader || "",
            thumbnail: d.thumbnail || (thumbs.length ? thumbs[thumbs.length - 1].url : ""),
          })
        } catch {
          /* linha não-JSON: ignora */
        }
      }
      // Sem resultado E com falha do yt-dlp são coisas MUITO diferentes (rede,
      // binário quebrado, YouTube mudando o extractor), mas a tela mostrava
      // "Nenhum vídeo encontrado" para as duas. Devolvemos o motivo real.
      if (!out.length && err) {
        const msg = String(stderr || "").split("\n").filter((l) => /error/i.test(l))[0]
          || (err.code === "ENOENT" ? "yt-dlp não encontrado" : `yt-dlp falhou (${err.code ?? err.message})`)
        logTrailer(`busca "${query}" falhou: ${msg}`)
        return resolve({ results: [], error: msg })
      }
      logTrailer(`busca "${query}": ${out.length} resultado(s)`)
      resolve({ results: out })
    })
  })
}

// Baixa um vídeo ESPECÍFICO do YouTube como trailer do jogo (escolha manual).
// Emite progresso (%) por 'trailer:dlprogress' para a janela mostrar a barra.
function baixarTrailerUrl(id, url) {
  return new Promise((resolve) => {
    fs.mkdirSync(TRAILERS_DIR, { recursive: true })
    const safe = safeName(id)
    // Apaga o trailer anterior E parciais (o usuário corrige um errado; e
    // parciais de tentativas anteriores causam HTTP 416).
    limparTrailer(safe)
    const args = [
      url,
      "--no-playlist",
      "--no-warnings",
      "--no-continue",
      "--no-part",
      "--newline", // uma linha por atualização de progresso (fácil de parsear)
      "-f",
      "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
      "--remux-video",
      "mp4",
      ...FF_ARGS,
      ...cookieArgs(),
      "-o",
      path.join(TRAILERS_DIR, `${safe}.%(ext)s`),
    ]
    const emit = (data) => {
      if (win) win.webContents.send("trailer:dlprogress", { id, ...data })
    }
    let errBuf = ""
    const child = spawn(YTDLP, args, { env: YTDLP_ENV })
    const onData = (buf) => {
      const s = buf.toString()
      const m = s.match(/\[download\]\s+([0-9.]+)%/)
      if (m) emit({ percent: parseFloat(m[1]), stage: "download" })
      if (/\[VideoRemuxer\]|Merging/.test(s)) emit({ percent: 100, stage: "processando" })
    }
    child.stdout.on("data", onData)
    child.stderr.on("data", (b) => {
      errBuf += b.toString()
      onData(b)
    })
    child.on("close", () => {
      const p = trailerLocal(id)
      emit({ percent: 100, stage: "done" })
      if (p) return resolve({ ok: true, path: p })
      if (/confirm your age|inappropriate/i.test(errBuf)) {
        return resolve({ ok: false, error: "age" })
      }
      // Mostra o motivo real (ex.: ffmpeg ausente, vídeo indisponível).
      const linha = errBuf.split("\n").reverse().find((l) => /error|ffmpeg/i.test(l)) || ""
      resolve({ ok: false, error: linha.trim() || "falha ao baixar" })
    })
    child.on("error", (e) => resolve({ ok: false, error: String(e.message || e) }))
  })
}

function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
  } catch (e) {
    return {}
  }
}

// Busca/cache de sysinfo (tamanhos Epic via legendary + requisitos Steam).
// Usada pelo IPC game:sysinfo e pelo prefetch em background.
async function buildSysinfo(g) {
  const id = String(g?.id || "")
  const info = {}
  const legendary = g?.launch_cmd?.[0] || ""
  if (g?.launcher === "epic" || /legendary$/.test(legendary)) {
    const appName = id.replace(/^epic:/, "")
    const out = await new Promise((res) => {
      execFile(legendary, ["info", "--json", appName], { timeout: 60000, maxBuffer: 32 * 1024 * 1024 }, (e, stdout) => res(e ? "" : String(stdout)))
    })
    try {
      const j = JSON.parse(out)
      if (j?.manifest) {
        info.download_size = j.manifest.download_size
        info.disk_size = j.manifest.disk_size
        info.version = j.game?.version
      }
    } catch {}
  }

  let appid = g?.launcher === "steam" ? id.replace(/^steam:/, "") : ""
  if (!appid && g?.title) {
    try {
      const s = await fetchJson(
        `https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(g.title)}&cc=br&l=portuguese`,
      )
      appid = s?.items?.[0]?.id || ""
    } catch {}
  }
  if (appid) {
    try {
      const d = await fetchJson(
        `https://store.steampowered.com/api/appdetails?appids=${appid}&cc=br&l=portuguese`,
      )
      const reqs = d?.[appid]?.data?.pc_requirements
      if (reqs && !Array.isArray(reqs)) {
        info.req_min = reqs.minimum || ""
        info.req_rec = reqs.recommended || ""
      }
    } catch {}
  }
  return info
}

async function getSysinfo(g) {
  const id = String(g?.id || "")
  const cache = readJsonFile(SYSINFO_CACHE, {})
  if (cache[id]) return cache[id]
  const info = await buildSysinfo(g)
  cache[id] = info
  try {
    fs.writeFileSync(SYSINFO_CACHE, JSON.stringify(cache, null, 2))
  } catch {}
  return info
}

// Prefetch em background: vai enchendo o cache de todos os jogos serialmente,
// para a página abrir instantânea. Começa alguns segundos após o launch.
function startSysinfoPrefetch() {
  setTimeout(async () => {
    for (const g of readLibrary()) {
      try {
        await getSysinfo(g)
      } catch {}
      await new Promise((r) => setTimeout(r, 400)) // não atropela Steam/legendary
    }
  }, 8000)
}

// UMU (vem com o Heroic): jeito certo de rodar builds Proton fora da Steam —
// o wine direto do Proton quebra (libs do runtime não resolvem).
const UMU = path.join(os.homedir(), ".config", "heroic", "tools", "runtimes", "umu", "umu_run.py")

// Monta o comando de um jogo adicionado manualmente ("custom:<slug>").
// Windows: wine/Proton escolhido + exe. Linux: [exe].
// Retorna { cmd, env } — env traz PROTONPATH/STEAM_COMPAT_* quando é Proton.
function customLaunchCmd(id) {
  const g = readJsonFile(CUSTOM_GAMES, []).find((x) => x.id === id)
  if (!g) return null
  if (g.platform === "linux") return { cmd: [g.exe], env: {} }
  const wm = require("./winemanager")
  const s = getGameSettings(id)
  const prefixo = s.prefixPath || defaultPrefix(id)
  let v = null
  if (s.wineVersion) {
    v = [...wm.installed(), ...wm.steamProtons()].find((w) => w.id === s.wineVersion)
  }

  // Proton (vindo do compatibilitytools.d da Steam): NUNCA pelo wine direto.
  // Vai de UMU (igual ao Heroic) ou, sem UMU, pelo script `proton run`.
  if (v?.kind === "steam") {
    if (fs.existsSync(UMU)) {
      return { cmd: ["python3", UMU, g.exe], env: { PROTONPATH: v.path, WINEPREFIX: prefixo, GAMEID: "arcadia" } }
    }
    const proton = path.join(v.path, "proton")
    if (fs.existsSync(proton)) {
      return {
        cmd: [proton, "run", g.exe],
        env: {
          STEAM_COMPAT_DATA_PATH: prefixo,
          STEAM_COMPAT_CLIENT_INSTALL_PATH: path.join(os.homedir(), ".steam", "steam"),
          STEAM_COMPAT_APP_ID: "0",
          WINEPREFIX: prefixo,
        },
      }
    }
  }

  // Wine comum (GE-Proton do Arcadia, Wine-GE, sistema): wine direto + DXVK
  // manual no prefixo (rodar wine direto não ativa DXVK sozinho). Só instala
  // se o prefixo já existe — na 1ª execução o wine cria o prefixo antes.
  const wine = v?.wine && fs.existsSync(v.wine) ? v.wine : wm.bestWine()
  if (fs.existsSync(path.join(prefixo, "drive_c", "windows", "system32"))) {
    try {
      wm.installGraphicsLibs(prefixo, wine, {
        dxvk: s.autoDXVK !== false,
        nvapi: Boolean(s.autoNVAPI),
        vkd3d: Boolean(s.autoVKD3D),
      })
    } catch {}
  }
  return { cmd: [wine, g.exe], env: {} }
}

// --- Configurações por jogo (diálogo estilo Heroic) -------------------------
// Salvas em game_settings.json: { "<gameId>": { wineVersion, prefixPath, ... } }
const GAME_SETTINGS = path.join(DATA_DIR, "game_settings.json")
const SYSINFO_CACHE = path.join(DATA_DIR, "sysinfo_cache.json")
// Jogos adicionados manualmente ("Adicionar jogo"): entram na biblioteca.
const CUSTOM_GAMES = path.join(DATA_DIR, "custom_games.json")

function readJsonFile(p, fallback) {
  try {
    return JSON.parse(fs.readFileSync(p, "utf-8"))
  } catch {
    return fallback
  }
}

async function fetchJson(url) {
  const r = await fetch(url, { headers: { "User-Agent": "arcadia" } })
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  return r.json()
}

function readAllGameSettings() {
  try {
    return JSON.parse(fs.readFileSync(GAME_SETTINGS, "utf-8"))
  } catch {
    return {}
  }
}

function getGameSettings(id) {
  if (!id) return {}
  return readAllGameSettings()[id] || {}
}

function setGameSettings(id, patch) {
  if (!id) return {}
  const all = readAllGameSettings()
  all[id] = { ...(all[id] || {}), ...(patch || {}) }
  try {
    fs.writeFileSync(GAME_SETTINGS, JSON.stringify(all, null, 2))
  } catch {
    /* disco cheio/permissão: segue sem salvar */
  }
  return all[id]
}

// Prefixo padrão do jogo (respeita a pasta configurada em Config. Gerais).
function defaultPrefix(id) {
  return require("./winemanager").prefixOf(id)
}

// Limpeza pós-desinstalação (diálogo estilo Heroic): remove o prefixo do jogo
// (padrão ou o customizado salvo nas configurações) e/ou as configurações+log.
function limparAposDesinstalar(id, { removePrefix, removeSettings } = {}) {
  const s = getGameSettings(id)
  if (removePrefix) {
    const wm = require("./winemanager")
    const padrao = defaultPrefix(id)
    const candidatos = [padrao, s.prefixPath].filter(Boolean)
    for (const p of candidatos) {
      // Segurança: só apaga o prefixo padrão DESTE jogo (seja qual for a base
      // configurada), algo sob o PREFIX_DIR legado, ou o prefixo customizado
      // salvo para este jogo.
      const dentro = p === padrao || p.startsWith(wm.PREFIX_DIR + path.sep) || p === s.prefixPath
      if (dentro && fs.existsSync(p)) {
        try {
          fs.rmSync(p, { recursive: true, force: true })
        } catch {}
      }
    }
  }
  if (removeSettings) {
    const all = readAllGameSettings()
    if (all[id]) {
      delete all[id]
      try {
        fs.writeFileSync(GAME_SETTINGS, JSON.stringify(all, null, 2))
      } catch {}
    }
    try {
      fs.rmSync(path.join(LOG_DIR, `${String(id).replace(/[^a-z0-9._-]/gi, "_")}.log`), { force: true })
    } catch {}
  }
}

// Divide uma linha de argumentos respeitando aspas: a "b c" d -> [a, b c, d].
function splitArgs(str) {
  return (String(str || "").match(/"[^"]*"|'[^']*'|\S+/g) || []).map((t) => t.replace(/^["']|["']$/g, ""))
}

// Binário existe? Caminho absoluto ou procura no PATH. Evita launch silencioso
// quando o wrapper configurado (gamescope/gamemoderun/etc) não está instalado.
function binExists(cmd) {
  if (!cmd) return false
  if (cmd.includes("/")) return fs.existsSync(cmd)
  return String(process.env.PATH || "")
    .split(":")
    .some((dir) => fs.existsSync(path.join(dir, cmd)))
}

// Monta env/args de lançamento a partir das configurações do jogo.
// Retorna { cmd, env, warnings } já com gamescope (se ligado) e variáveis.
function applyGameSettings(cmd, s, gameId) {
  const warnings = []
  const env = { ...process.env }
  if (s.esync) env.WINEESYNC = "1"
  if (s.fsync) env.WINEFSYNC = "1"
  if (s.wineWayland) env.PROTON_ENABLE_WAYLAND = "1"
  if (s.wow64) env.PROTON_USE_WOW64 = "1"
  if (s.fsrHack) env.WINE_FULLSCREEN_FSR = "1"
  if (s.autoNVAPI) env.DXVK_ENABLE_NVAPI = "1"
  if (s.dxvkHud) env.DXVK_HUD = s.dxvkHud
  if (s.mangohud) env.MANGOHUD = "1"
  if (s.prefixPath) env.WINEPREFIX = s.prefixPath
  // Variáveis de ambiente extras (aba AVANÇADO).
  for (const v of s.envVars || []) {
    if (v && /^[A-Za-z_][A-Za-z0-9_]*$/.test(v.name || "")) env[v.name] = v.value ?? ""
  }
  let finalCmd = cmd
  // Legendary (Epic): a versão do Wine escolhida vira --wine e o prefixo
  // customizado vira --wine-prefix — sem isso o jogo sempre usava o wine do
  // sistema, ignorando a escolha do diálogo.
  if (/legendary$/.test(cmd[0]) && cmd[1] === "launch") {
    // Prefixo POR JOGO (padrão: pasta configurada/epic_<id>) — sem isso todos
    // os jogos Epic dividiam o ~/.wine do sistema.
    const prefixo = s.prefixPath || (gameId ? defaultPrefix(gameId) : "")
    if (prefixo) {
      env.WINEPREFIX = prefixo
      finalCmd = [...finalCmd, "--wine-prefix", prefixo]
    }
    if (s.wineVersion) {
      const wm = require("./winemanager")
      const v = [...wm.installed(), ...wm.steamProtons()].find((w) => w.id === s.wineVersion)
      if (v?.wine && fs.existsSync(v.wine)) {
        finalCmd = [...finalCmd, "--wine", v.wine]
        // Instala DXVK/NVAPI/VKD3D no prefixo efetivo — rodar o wine direto
        // não ativa DXVK sozinho (o jogo cairia no wined3d). Só se o prefixo
        // já existe (na 1ª execução o legendary cria o prefixo antes).
        if (prefixo && fs.existsSync(path.join(prefixo, "drive_c", "windows", "system32"))) {
          const r = wm.installGraphicsLibs(prefixo, v.wine, {
            dxvk: s.autoDXVK !== false,
            nvapi: Boolean(s.autoNVAPI),
            vkd3d: Boolean(s.autoVKD3D),
          })
          if (!r.ok) warnings.push(`DXVK: ${r.error}`)
        }
      } else {
        warnings.push(`versão do Wine "${s.wineVersion}" não encontrada — usando a do sistema`)
      }
    }
  }
  // Argumentos do jogo: entram depois do comando (não se aplica a Steam).
  if (s.gameArgs && cmd[0] !== "steam") finalCmd = [...finalCmd, ...splitArgs(s.gameArgs)]
  // Gamescope embrulha o comando (não se aplica a jogos Steam — a Steam tem
  // sua própria integração com gamescope). --disable-gamemode evita o abort
  // do gamescopereaper no gamemode_request_end (bug libgamemodeauto/dbus);
  // quem quer GameMode usa o checkbox (gamemoderun), que funciona.
  if (s.gamescope && cmd[0] !== "steam") {
    if (binExists("gamescope")) {
      const args = ["--disable-gamemode", "-W", String(s.gsWidth || 1920), "-H", String(s.gsHeight || 1080)]
      if (s.gsFps) args.push("-r", String(s.gsFps))
      finalCmd = ["gamescope", ...args, "--", ...finalCmd]
    } else {
      warnings.push("gamescope não está instalado — iniciando sem ele")
    }
  }
  // GameMode (Feral): embrulha tudo com gamemoderun (a Steam tem o dela).
  if (s.gamemode && cmd[0] !== "steam") {
    if (binExists("gamemoderun")) {
      finalCmd = ["gamemoderun", ...finalCmd]
    } else {
      warnings.push("gamemoderun não está instalado — iniciando sem ele")
    }
  }
  // MangoHud: embrulha com o binário `mangohud` (LD_PRELOAD correto p/ GL e
  // Vulkan). Só MANGOHUD=1 não basta em jogos OpenGL (ex.: Godot).
  if (s.mangohud && cmd[0] !== "steam") {
    if (binExists("mangohud")) {
      finalCmd = ["mangohud", ...finalCmd]
    } else {
      warnings.push("mangohud não está instalado — iniciando sem ele")
    }
  }
  // Wrappers customizados (aba AVANÇADO): os mais externos por último.
  if (cmd[0] !== "steam") {
    for (const w of s.wrappers || []) {
      if (!w || !w.cmd) continue
      if (binExists(w.cmd)) {
        finalCmd = [w.cmd, ...splitArgs(w.args), ...finalCmd]
      } else {
        warnings.push(`wrapper "${w.cmd}" não encontrado — ignorado`)
      }
    }
  }
  return { cmd: finalCmd, env, warnings }
}

// Merge raso (preserva chaves não enviadas; perfil é mesclado à parte).
function writeConfig(partial) {
  try {
    const cur = readConfig()
    const next = { ...cur, ...(partial || {}) }
    if (partial && partial.profile) {
      next.profile = { ...(cur.profile || {}), ...partial.profile }
    }
    if (partial && partial.sources) {
      next.sources = { ...(cur.sources || {}), ...partial.sources }
    }
    // Escrita atômica: grava num temporário e renomeia. Escrevendo direto por
    // cima, uma queda no meio deixa o config.json truncado — e com ele vão as
    // chaves de API, o perfil e todos os ajustes. O rename é atômico dentro do
    // mesmo sistema de arquivos, então ou fica o antigo, ou fica o novo.
    const tmp = `${CONFIG}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(next, null, 2), "utf-8")
    fs.renameSync(tmp, CONFIG)
    return { ok: true, config: next }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Mercado e idioma da loja da Microsoft. pt-BR por padrão, para as descrições
// virem em português; dá para trocar no config.json.
function xboxLocale(cfg) {
  return [String(cfg.xbox_market || "BR"), String(cfg.xbox_locale || "pt-br")]
}

// Lê library.json, aplica as edições do usuário e converte caminhos de arte
// locais em file:// para o <img>. Jogos adicionados manualmente entram aqui.
function readLibrary() {
  try {
    const games = JSON.parse(fs.readFileSync(LIB, "utf-8"))
    games.push(...readJsonFile(CUSTOM_GAMES, []))
    applyOverrides(games, readOverrides(OVERRIDES))
    for (const g of games) {
      for (const k of ["cover", "hero", "logo"]) {
        if (typeof g[k] === "string" && g[k].startsWith("/")) {
          g[k] = "file://" + g[k]
        }
      }
    }
    return games
  } catch (e) {
    return []
  }
}

function runIndexer() {
  return new Promise((res) => {
    try {
      execFile("python3", [INDEX], () => res())
    } catch {
      res()
    }
  })
}

// Conta os AppIds injetados pelo SLSsteam (bloco AdditionalApps).
function slssteamCount() {
  try {
    const custom = String(readConfig().slssteam_path || "").trim()
    const text = fs.readFileSync(custom || SLS_CONFIG, "utf-8")
    const lines = text.split("\n")
    let inBlock = false
    let count = 0
    for (const line of lines) {
      if (/^AdditionalApps\s*:/.test(line)) {
        inBlock = true
        continue
      }
      if (inBlock) {
        if (line && !/^\s/.test(line) && line.includes(":") &&
            !line.trimStart().startsWith("#")) {
          break
        }
        if (/^\s*-\s*\d+/.test(line)) count++
      }
    }
    return count
  } catch {
    return 0
  }
}

function heroicConnected() {
  for (const f of ["gog", "legendary", "nile"]) {
    try {
      const j = JSON.parse(
        fs.readFileSync(
          path.join(HOME, ".config/heroic/store_cache", `${f}_library.json`),
          "utf-8",
        ),
      )
      const n = Array.isArray(j) ? j.length : Object.keys(j).length
      if (n > 0) return true
    } catch {
      /* ignore */
    }
  }
  return false
}

let win
function createWindow() {
  const cfgIni = readConfig()
  win = new BrowserWindow({
    width: 1600,
    height: 900,
    backgroundColor: "#000000",
    autoHideMenuBar: true,
    icon: path.join(__dirname, "..", "public", "logo-512.png"),
    fullscreen: process.env.PS5_FULLSCREEN === "1",
    // "Usar janela sem moldura" (Config. Gerais) — requer reiniciar o app.
    frame: !cfgIni.frameless_window,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      // App local/pessoal: permite <img src="file://..."> das capas da Steam.
      webSecurity: false,
      // Deixa o trailer tocar sozinho COM som ao focar o jogo (estilo PS5).
      autoplayPolicy: "no-user-gesture-required",
    },
  })
  win.loadFile(path.join(__dirname, "..", "dist", "index.html"))
  // Aplica a escala salva assim que a página carrega.
  win.webContents.on("did-finish-load", () => {
    const z = Number(readConfig().ui_scale) || 1
    win.webContents.setZoomFactor(Math.min(2, Math.max(0.7, z)))
    // Modo console (tela cheia): cursor OCULTO por padrão, mas aparece ao
    // mexer o mouse e some após ~2s parado (navegação continua por gamepad).
    if (win.isFullScreen()) {
      win.webContents.executeJavaScript(`
        (() => {
          let timer
          const mostrar = () => {
            document.documentElement.style.cursor = 'default'
            clearTimeout(timer)
            timer = setTimeout(() => {
              document.documentElement.style.cursor = 'none'
            }, 2000)
          }
          document.documentElement.style.cursor = 'none'
          window.addEventListener('mousemove', mostrar, { passive: true })
        })()
      `).catch(() => {})
    }
    // Temas customizados: injeta todos os .css da pasta configurada.
    try {
      const dir = String(readConfig().custom_css_path || "").trim()
      if (dir && fs.existsSync(dir)) {
        const css = fs.readdirSync(dir)
          .filter((f) => f.endsWith(".css"))
          .map((f) => fs.readFileSync(path.join(dir, f), "utf-8"))
          .join("\n")
        if (css) win.webContents.insertCSS(css)
      }
    } catch {}
  })
  // Vigia de conquistas (toast estilo PS5 ao desbloquear).
  startAchievementWatcher((payload) => {
    if (win && !win.isDestroyed()) win.webContents.send("achievement:unlocked", payload)
  })
  // Foco real da janela (no gamescope o Chromium acha que está focado mesmo
  // com o jogo por cima) — o renderer trava gamepad/trailer com isso.
  win.on("blur", () => win?.webContents.send("app:focus", false))
  win.on("focus", () => win?.webContents.send("app:focus", true))

  // Modo gamescope: o Electron roda no X aninhado e NÃO recebe blur/focus
  // quando o jogo abre no desktop. Em vez de janela ativa (Wayland não tem
  // API pra isso), detecta o PROCESSO do jogo: qualquer executável rodando
  // de steamapps/common, compatdata (Proton) ou prefixos Heroic/Lutris.
  // pgrep nunca casa consigo mesmo; poll de 2s.
  if (process.env.ARCADIA_GAMESCOPE === "1") {
    let focado = true
    setInterval(() => {
      execFile("pgrep", ["-f", PADRAO_JOGO], (err) => {
        const jogoRodando = !err // exit 0 = achou processo
        const ativo = !jogoRodando
        if (ativo !== focado) {
          focado = ativo
          if (win && !win.isDestroyed()) win.webContents.send("app:focus", ativo)
        }
      })
    }, 2000)
  }

  // Vigia de jogo rodando (todos os modos): avisa o renderer nas transições
  // abriu/fechou. O card "jogando" do modo desktop se ancora nisso.
  // Primário: grupo de processos do jogo que NÓS lançamos (jogoAtivo) — cobre
  // custom/umu/legendary/lutris. Fallback: padrão clássico (jogos Steam, que
  // são filhos do cliente Steam, não nossos).
  let jogoRodando = false
  const marcar = (rodando) => {
    if (rodando === jogoRodando) return
    jogoRodando = rodando
    if (win && !win.isDestroyed()) win.webContents.send("game:running", rodando)
    // Jogo fechou: roda o script pós-jogo configurado (se houver).
    if (!rodando && postGameScript) {
      const script = postGameScript
      postGameScript = ""
      try {
        const p = spawn(script, [], { detached: true, stdio: "ignore" })
        p.unref()
      } catch {}
    }
  }
  setInterval(() => {
    if (jogoAtivo) {
      // Sinal 0: só testa se o grupo de processos ainda existe.
      try {
        process.kill(-jogoAtivo.pid, 0)
        marcar(true)
      } catch {
        jogoAtivo = null
        marcar(false)
      }
      return
    }
    execFile("pgrep", ["-f", PADRAO_JOGO], (err) => marcar(!err))
  }, 3000)
}

app.whenReady().then(() => {
  startSysinfoPrefetch()
  ipcMain.handle("library:get", () => readLibrary())

  ipcMain.handle("game:launch", async (_e, payload) => {
    // Aceita { cmd, gameId } (novo) ou o array cmd direto (legado).
    let rawCmd = Array.isArray(payload) ? payload : payload?.cmd
    const gameId = Array.isArray(payload) ? undefined : payload?.gameId
    // Jogo adicionado manualmente: monta o comando na hora (wine + exe).
    let envExtra = {}
    if (typeof gameId === "string" && gameId.startsWith("custom:")) {
      const built = customLaunchCmd(gameId)
      rawCmd = built?.cmd
      envExtra = built?.env || {}
    }
    if (!Array.isArray(rawCmd) || rawCmd.length === 0) return { ok: false }
    try {
      // Aplica as configurações do jogo (env vars, prefixo, gamescope).
      const s = getGameSettings(gameId)
      const { cmd, env: envBase, warnings } = applyGameSettings(rawCmd, s, gameId)
      const env = { ...envBase, ...envExtra }
      for (const w of warnings) console.warn("arcadia:", w)
      // Jogo custom: SEMPRE roda no prefixo dele (padrão ou customizado) —
      // sem isso caía no ~/.wine do sistema e o jogo não abria.
      if (typeof gameId === "string" && gameId.startsWith("custom:")) {
        env.WINEPREFIX = env.WINEPREFIX || s.prefixPath || defaultPrefix(gameId)
      }

      // Logs detalhados (aba AVANÇADO): stdout/stderr do jogo em logs/<id>.log.
      let stdio = "ignore"
      if (s.verboseLogs) {
        try {
          fs.mkdirSync(LOG_DIR, { recursive: true })
          const fd = fs.openSync(path.join(LOG_DIR, `${String(gameId || "jogo").replace(/[^a-z0-9._-]/gi, "_")}.log`), "a")
          stdio = ["ignore", fd, fd]
        } catch {}
      }

      // Script pré-jogo (aba AVANÇADO): espera terminar (máx. 60s) antes de lançar.
      if (s.scriptPre) {
        await new Promise((res) => {
          const p = spawn(s.scriptPre, [], { stdio: "ignore" })
          p.on("close", res)
          p.on("error", res)
          setTimeout(res, 60000)
        })
      }
      // Script pós-jogo: o vigia de processo roda quando o jogo fechar.
      postGameScript = s.scriptPost || ""

      // "Minimizar Arcadia ao iniciar um jogo" (Config. Gerais).
      if (readConfig().minimize_on_game_launch && win && !win.isDestroyed()) {
        setTimeout(() => win?.minimize(), 2000)
      }

      const soltar = (c) => {
        const child = spawn(c[0], c.slice(1), { detached: true, stdio, env })
        child.unref()
        // Registra o grupo de processos do jogo (o spawn detached vira líder).
        jogoAtivo = { pid: child.pid, alvo: c[c.length - 1] }
      }
      // Steam: se estiver em Big Picture, sai dele ANTES de abrir o jogo —
      // senão o steam://rungameid herda o modo BPM em vez da Steam normal.
      // MAS só manda o exitbigpicture se a Steam JÁ estiver rodando: com ela
      // fechada, esse URI inicia a Steam EM Big Picture (efeito colateral).
      if (cmd[0] === "steam" && typeof cmd[1] === "string" && cmd[1].startsWith("steam://")) {
        const run = () => soltar(cmd)
        execFile("pgrep", ["-x", "steam"], (err) => {
          if (!err) {
            // Steam rodando: sai do BPM e lança.
            try {
              const bp = spawn("steam", ["steam://exitbigpicture"], { detached: true, stdio: "ignore", env })
              bp.unref()
            } catch {}
            setTimeout(run, 900)
            return
          }
          // Steam FECHADA: abre o cliente puro (modo desktop), espera subir,
          // garante saída do BPM (ela pode restaurar a sessão anterior em BPM
          // — principalmente no gamescope) e só então lança o jogo.
          try {
            const st = spawn("steam", [], { detached: true, stdio: "ignore", env })
            st.unref()
          } catch {}
          let tentativas = 0
          const esperar = setInterval(() => {
            execFile("pgrep", ["-x", "steam"], (e2) => {
              if (!e2) {
                clearInterval(esperar)
                setTimeout(() => {
                  try {
                    const bp = spawn("steam", ["steam://exitbigpicture"], { detached: true, stdio: "ignore", env })
                    bp.unref()
                  } catch {}
                  setTimeout(run, 1200)
                }, 3000) // cliente subiu: espera a UI estabilizar
              } else if (++tentativas > 30) {
                clearInterval(esperar) // ~60s sem sinal: desiste silenciosamente
              }
            })
          }, 2000)
        })
        return { ok: true, warnings }
      }
      soltar(cmd)
      return { ok: true, warnings }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Fecha o jogo em execução (botão X do card "jogando").
  // Universal: mata o grupo de processos do jogo que lançamos (jogoAtivo) —
  // cobre custom/umu/legendary/lutris. Steam: pkill no padrão clássico (o
  // jogo é filho do cliente Steam, não nosso).
  ipcMain.handle("game:close", () => {
    try {
      if (jogoAtivo) {
        const { pid, alvo } = jogoAtivo
        jogoAtivo = null
        try { process.kill(-pid, "SIGTERM") } catch {}
        setTimeout(() => {
          try { process.kill(-pid, "SIGKILL") } catch {}
        }, 4000)
        // Reforço: qualquer processo com o executável do jogo na cmdline.
        if (alvo && !String(alvo).includes("://")) {
          execFile("pkill", ["-f", String(alvo)], () => {})
        }
      }
      execFile("pkill", ["-f", PADRAO_JOGO], () => {})
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Informações do jogo para a página (tamanhos reais + requisitos).
  // Epic: `legendary info --json` (disk/download size). Requisitos: Steam
  // appdetails (pc_requirements) — appid direto ou busca por título.
  // Cacheado em sysinfo_cache.json.
  ipcMain.handle("game:sysinfo", async (_e, g) => {
    try {
      return { ok: true, info: await getSysinfo(g) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Importa uma instalação existente (botão "IMPORTAR JOGO" da página do jogo).
  ipcMain.handle("game:import", async (_e, g) => {
    try {
      const legendary = g?.launch_cmd?.[0] || ""
      if (!/legendary$/.test(legendary)) return { ok: false, error: "Só jogos Epic (legendary) podem ser importados" }
      const r = await dialog.showOpenDialog(win, {
        title: "Pasta da instalação existente",
        properties: ["openDirectory"],
      })
      if (r.canceled || !r.filePaths[0]) return { ok: false, error: "cancelado" }
      const appName = String(g.id).replace(/^epic:/, "")
      await new Promise((res) => {
        const c = spawn(legendary, ["import", appName, r.filePaths[0]], { stdio: "ignore" })
        c.on("close", res)
        c.on("error", res)
        setTimeout(res, 120000)
      })
      await runIndexer()
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Adiciona um jogo manualmente ("Adicionar jogo"). Salva em custom_games.json
  // e devolve a biblioteca já mesclada.
  ipcMain.handle("customgame:add", (_e, { id, title, platform, exe } = {}) => {
    try {
      if (!title || !exe) return { ok: false, error: "título e executável são obrigatórios" }
      const all = readJsonFile(CUSTOM_GAMES, [])
      if (all.some((g) => g.id === id)) return { ok: false, error: "já existe um jogo com esse nome" }
      all.push({
        id,
        title,
        launcher: "custom",
        platform: platform === "linux" ? "linux" : "windows",
        exe,
        installed: true,
      })
      fs.writeFileSync(CUSTOM_GAMES, JSON.stringify(all, null, 2))
      return { ok: true, games: readLibrary() }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Edita um jogo custom existente (título/executável). O id é preservado.
  ipcMain.handle("customgame:update", (_e, { id, title, exe } = {}) => {
    try {
      const all = readJsonFile(CUSTOM_GAMES, [])
      const g = all.find((x) => x.id === id)
      if (!g) return { ok: false, error: "jogo não encontrado" }
      if (title) g.title = title
      if (exe) g.exe = exe
      fs.writeFileSync(CUSTOM_GAMES, JSON.stringify(all, null, 2))
      return { ok: true, games: readLibrary() }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // "Executar instalador antes": roda um instalador .exe no prefixo escolhido.
  ipcMain.handle("customgame:runInstaller", async (_e, { appid, wine, prefix } = {}) => {
    try {
      const r = await dialog.showOpenDialog(win, {
        title: "Selecionar instalador",
        properties: ["openFile"],
        filters: [{ name: "Executáveis", extensions: ["exe", "msi", "bat"] }],
      })
      if (r.canceled || !r.filePaths[0]) return { ok: false, error: "cancelado" }
      return await require("./winemanager").runExe(appid, r.filePaths[0], { wine, prefix })
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Abre o log de lançamento do jogo ("Registros" do menu de contexto).
  ipcMain.handle("gamelog:open", (_e, id) => {
    try {
      const f = path.join(LOG_DIR, `${String(id).replace(/[^a-z0-9._-]/gi, "_")}.log`)
      if (!fs.existsSync(f)) return { ok: false, error: "sem registros" }
      shell.openPath(f)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Desinstala o jogo ("Desinstalar" do menu de contexto).
  // opts: { removePrefix, removeSettings } — diálogo estilo Heroic (não-Steam).
  ipcMain.handle("game:uninstall", async (_e, { game: g, removePrefix, removeSettings } = {}) => {
    try {
      const launcher = g?.launcher || ""
      const id = String(g?.id || "")
      if (launcher === "steam") {
        const appid = id.replace(/^steam:/, "")
        const ss = require("./steamstore")
        // Download feito pelo Arcadia (acf marcado): remove na hora (pasta +
        // acf + SLSsteam), sem diálogo da Steam, e reindexa em tempo real.
        if (ss.arcadiaDownloaded().some((a) => a.appid === appid)) {
          ss.removeDownloaded(appid)
          limparAposDesinstalar(id, { removePrefix, removeSettings })
          await runIndexer()
          if (win && !win.isDestroyed()) win.webContents.send("library:changed")
          return { ok: true }
        }
        // Jogo owned: a Steam mostra o diálogo de confirmação dela.
        const child = spawn("steam", [`steam://uninstall/${appid}`], { detached: true, stdio: "ignore" })
        child.unref()
        return { ok: true }
      }
      const legendary = g?.launch_cmd?.[0] || ""
      if (launcher === "custom") {
        // Jogo adicionado manualmente: só sai do custom_games.json.
        const rest = readJsonFile(CUSTOM_GAMES, []).filter((x) => x.id !== id)
        try {
          fs.writeFileSync(CUSTOM_GAMES, JSON.stringify(rest, null, 2))
        } catch {}
        limparAposDesinstalar(id, { removePrefix, removeSettings })
        if (win && !win.isDestroyed()) win.webContents.send("library:changed")
        return { ok: true }
      }
      if (launcher === "epic" || /legendary$/.test(legendary)) {
        // Espera o uninstall terminar e reindexa ANTES de responder — assim o
        // refresh do renderer já vê o jogo como não instalado.
        await new Promise((res) => {
          const child = spawn(legendary, ["uninstall", "-y", id.replace(/^epic:/, "")], { detached: true, stdio: "ignore" })
          child.unref()
          child.on("close", res)
          child.on("error", res)
          setTimeout(res, 180000) // desiste de esperar após 3min
        })
        limparAposDesinstalar(id, { removePrefix, removeSettings })
        try { dm.cancel(id) } catch {} // some da fila de downloads também
        await runIndexer()
        if (win && !win.isDestroyed()) win.webContents.send("library:changed")
        return { ok: true }
      }
      // Heroic/Lutris: apaga a pasta de instalação registrada, se conhecida.
      limparAposDesinstalar(id, { removePrefix, removeSettings })
      return { ok: false, error: `Desinstalação não suportada para ${launcher || "esta loja"}` }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Salva uma edição do usuário e devolve a biblioteca já mesclada.
  ipcMain.handle("overrides:set", (_e, { id, patch } = {}) => {
    if (!id) return readLibrary()
    try {
      setOverride(OVERRIDES, id, patch)
    } catch (e) {
      /* disco cheio/permissão: segue com o que já havia */
    }
    return readLibrary()
  })

  ipcMain.handle("config:get", () => readConfig())

  // "Big Picture": fecha o modo desktop e abre o modo console (PS5, tela cheia).
  ipcMain.handle("app:enterConsole", () => {
    try {
      const child = spawn(process.execPath, ["."], {
        cwd: path.join(__dirname, ".."),
        detached: true,
        stdio: "ignore",
        env: { ...process.env, PS5_FULLSCREEN: "1", ARCADIA_MODE: "" },
      })
      child.unref()
      setTimeout(() => app.quit(), 500)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("config:set", (_e, cfg) => {
    // Pasta de prefixos mudou? Cria de verdade (ela não existia antes).
    if (cfg?.default_wine_prefix_path) {
      try {
        fs.mkdirSync(cfg.default_wine_prefix_path, { recursive: true })
      } catch {}
    }
    return writeConfig(cfg)
  })

  // Notícias de jogos (RSS PT-BR). Cache alinhado ao RELÓGIO: vale até o
  // próximo marco de 30 min (:00/:30) — não "30 min a partir do fetch".
  const SLOT_30 = 30 * 60 * 1000
  const slotAtual = Math.floor(Date.now() / SLOT_30)
  ipcMain.handle("news:get", async () => {
    try {
      const raw = fs.readFileSync(NEWS_CACHE, "utf-8")
      const cache = JSON.parse(raw)
      if (cache.slot === slotAtual && Array.isArray(cache.items)) {
        return cache.items
      }
    } catch {
      /* sem cache válido: busca */
    }
    try {
      const items = await getNews(40)
      fs.writeFileSync(NEWS_CACHE, JSON.stringify({ slot: slotAtual, items }), "utf-8")
      return items
    } catch (e) {
      console.error("[news:get]", e.message)
      return []
    }
  })

  // --- Runner Legendary (Epic) --------------------------------------------
  const runners = require("./runners")
  ipcMain.handle("runner:legendary:status", () => runners.legendary.status())

  // Baixa o binário (se preciso) e abre o login interativo num terminal.
  ipcMain.handle("runner:legendary:setup", async () => {
    try {
      await runners.legendary.ensureLegendary()
      return runners.legendary.login()
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // Biblioteca Epic normalizada (Game[]), ou erro para a UI mostrar.
  ipcMain.handle("runner:legendary:library", async () => {
    try {
      return { ok: true, games: await runners.legendary.library() }
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // --- Download manager (fila serial; evento dm:progress para a UI) --------
  const dm = require("./downloadmanager")
  dm.onProgress((items) => {
    if (win && !win.isDestroyed()) win.webContents.send("dm:progress", items)
  })
  // Download concluído: reindexar e avisar o renderer para recarregar a
  // biblioteca (o jogo aparece como instalado em tempo real).
  dm.onDone(async (item) => {
    try {
      // Steam (DepotDownloader): registra o jogo na Steam (acf + SLSsteam).
      if (item?.engine === "steam") {
        const ss = require("./steamstore")
        const appid = String(item.appid).replace(/^steam:/, "")
        ss.writeAcf({ appid, title: item.title, installdir: item.installdir, steamDir: item.steamDir })
        ss.registerSlssteam({ appid, token: item.token, dlcs: item.dlcs })
        // Avisa o renderer: oferecer restart da Steam (ou "mais tarde").
        if (win && !win.isDestroyed()) {
          win.webContents.send("store:downloaded", { appid, title: item.title })
        }
      }
      await runIndexer()
    } catch {}
    if (win && !win.isDestroyed()) win.webContents.send("library:changed")
  })

  // --- Loja Steam (estilo Acella: Hubcap + DepotDownloader + SLSsteam) -----
  const steamstore = require("./steamstore")
  ipcMain.handle("store:status", () => steamstore.status())
  ipcMain.handle("store:search", async (_e, query) => {
    try {
      return await steamstore.search(String(query || ""))
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Sugestões: só a lista de títulos da Steam, sem sondar provedores — é o que
  // permite responder a cada tecla sem inundar o Ryuu de requisições.
  ipcMain.handle("store:suggest", async (_e, query) => {
    try {
      return await steamstore.suggest(String(query || ""))
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Ficha completa do jogo (appdetails), para a página da loja no console.
  ipcMain.handle("store:details", async (_e, appid) => {
    try {
      return await steamstore.detalhes(String(appid || ""))
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Uma linha da home da loja, por gênero.
  ipcMain.handle("store:genre", async (_e, { genero, limite } = {}) => {
    try {
      return await steamstore.porGenero(String(genero || ""), Number(limite) || 24)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:recent", async () => {
    try {
      return await steamstore.popular()
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Fixes de jogos (GameBypass/OnlineFix, estilo luatools).
  ipcMain.handle("store:checkFixes", async (_e, appid) => steamstore.checkFixes(appid))
  ipcMain.handle("store:applyFix", async (_e, { appid, type, installPath } = {}) => {
    try {
      return await steamstore.applyFix(appid, type, installPath)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:installDir", (_e, game) => ({ path: steamstore.gameInstallDir(game) }))
  ipcMain.handle("store:libraries", () => steamstore.steamLibraries())
  ipcMain.handle("store:removeFromSteam", (_e, appid) => steamstore.removeFromSteam(appid))
  ipcMain.handle("store:removeDownloaded", (_e, appid) => {
    const r = steamstore.removeDownloaded(appid)
    // Sem este aviso a aba Lojas continuava mostrando "Na biblioteca" depois de
    // remover: o card se baseia na lista de jogos, que só recarrega neste
    // evento. Todos os outros pontos que mexem na biblioteca já o emitiam.
    if (r?.ok && win && !win.isDestroyed()) win.webContents.send("library:changed")
    return r
  })
  ipcMain.handle("store:installInfo", async (_e, appid) => {
    try {
      return await steamstore.getManifest(appid)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:install", async (_e, payload) => {
    try {
      return await dm.installSteam(payload || {})
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // "Add": adiciona o jogo à Steam sem baixar (estilo luatools-moon). O .lua
  // vai pro stplug-in da SLSsteam (keys/tokens) e o appid entra em
  // AdditionalApps — aí a própria Steam baixa o jogo pela CDN dela.
  ipcMain.handle("store:addToSteam", (_e, { appid, token, dlcs } = {}) => {
    try {
      const r = steamstore.addToSteam(String(appid || ""))
      if (!r.ok) return r
      steamstore.registerSlssteam({ appid: String(appid), token, dlcs })
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:ensureDotnet", async () => {
    try {
      return await steamstore.ensureDotnet()
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("slssteam:launchSteam", () => steamstore.launchSteamWithSls())
  ipcMain.handle("slssteam:install", () => steamstore.installSlssteam())
  ipcMain.handle("dm:queue", () => dm.getQueue())
  ipcMain.handle("dm:install", (_e, game) => dm.install(game || {}))
  ipcMain.handle("dm:pause", (_e, appid) => dm.pause(appid))
  ipcMain.handle("dm:retry", (_e, appid) => dm.retry(appid))
  ipcMain.handle("dm:dismiss", (_e, appid) => dm.descartar(appid))
  ipcMain.handle("dm:resume", (_e, appid) => dm.resume(appid))
  ipcMain.handle("dm:cancel", (_e, appid) => dm.cancel(appid))

  // --- Wine manager + ferramentas de prefixo --------------------------------
  const wm = require("./winemanager")
  ipcMain.handle("wine:list", async () => {
    try {
      return await wm.catalog()
    } catch (e) {
      return { installed: [...wm.installed(), ...wm.steamProtons()], available: [], error: String(e.message || e) }
    }
  })
  ipcMain.handle("wine:install", async (_e, { id, kind } = {}) => {
    try {
      return await wm.install(id, kind, (p) => win.webContents.send("wine:progress", p))
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })
  ipcMain.handle("wine:remove", (_e, id) => wm.remove(id))
  ipcMain.handle("wine:prefixTool", async (_e, { appid, tool, wine, prefix } = {}) => {
    try {
      return await wm.prefixTool(appid, tool, { wine, prefix })
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // Configurações por jogo (diálogo estilo Heroic). Salvas automaticamente.
  ipcMain.handle("gamesettings:get", (_e, id) => ({
    settings: getGameSettings(id),
    defaultPrefix: id ? defaultPrefix(id) : "",
  }))
  ipcMain.handle("gamesettings:set", (_e, { id, patch } = {}) => setGameSettings(id, patch))

  // Executa um .exe dentro do prefixo do jogo (diálogo de configurações).
  ipcMain.handle("wine:runExe", async (_e, { appid, wine, prefix } = {}) => {
    try {
      const r = await dialog.showOpenDialog(win, {
        title: "Executar EXE no prefixo",
        properties: ["openFile"],
        filters: [{ name: "Executáveis", extensions: ["exe", "msi", "bat"] }],
      })
      if (r.canceled || !r.filePaths[0]) return { ok: false, error: "cancelado" }
      return await wm.runExe(appid, r.filePaths[0], { wine, prefix })
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // Detalhe das conquistas do jogo (ícone/descrição/raridade/data).
  ipcMain.handle("achievements:get", (_e, appid) => {
    try {
      const store = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "achievements.json"), "utf-8"))
      return store?.[appid]?.items || []
    } catch {
      return []
    }
  })

  // Estatísticas do perfil: nível e insígnias (estilo Steam) — agrega
  // library.json + achievements.json.
  ipcMain.handle("profile:stats", () => {
    try {
      const lib = JSON.parse(fs.readFileSync(LIB, "utf-8"))
      let ach = {}
      try {
        ach = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "achievements.json"), "utf-8"))
      } catch {}
      let achDone = 0, achTotal = 0, achRaras = 0, jogos100 = 0, playMin = 0
      for (const g of lib) playMin += g.playtime_minutes || 0
      for (const ent of Object.values(ach)) {
        const items = ent?.items || []
        let done = 0
        for (const a of items) {
          achTotal++
          if (a.achieved) {
            achDone++
            done++
            const pct = typeof a.percent === "number" ? a.percent : parseFloat(a.percent) || 100
            if (pct <= 10) achRaras++
          }
        }
        if (items.length && done === items.length) jogos100++
      }
      return {
        jogos: lib.length,
        playtime_hours: Math.round(playMin / 60),
        ach_done: achDone,
        ach_total: achTotal,
        ach_raras: achRaras,
        jogos_100: jogos100,
      }
    } catch {
      return null
    }
  })

  // Feed de atividade: últimas conquistas desbloqueadas em qualquer jogo.
  ipcMain.handle("achievements:recent", () => {
    try {
      const ach = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "achievements.json"), "utf-8"))
      let lib = []
      try {
        lib = JSON.parse(fs.readFileSync(LIB, "utf-8"))
      } catch {}
      const jogos = {}
      for (const g of lib) {
        if (g.id?.startsWith("steam:")) jogos[g.id.split(":")[1]] = g
      }
      const feed = []
      for (const [appid, ent] of Object.entries(ach)) {
        for (const a of ent?.items || []) {
          if (a.achieved && a.unlock > 0) {
            feed.push({
              appid,
              game: jogos[appid]?.title || `App ${appid}`,
              cover: jogos[appid]?.cover || "",
              title: a.title,
              desc: a.desc,
              icon: a.icon,
              percent: a.percent,
              unlock: a.unlock,
            })
          }
        }
      }
      feed.sort((x, y) => y.unlock - x.unlock)
      return feed.slice(0, 20)
    } catch {
      return []
    }
  })

  // Escolhe uma PASTA (temas customizados, acessibilidade).
  ipcMain.handle("app:pickFolder", async () => {
    const res = await dialog.showOpenDialog(win, {
      title: "Escolher pasta",
      properties: ["openDirectory"],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false }
    return { ok: true, path: res.filePaths[0] }
  })

  // Escolhe um ARQUIVO qualquer (scripts pré/pós-jogo da aba AVANÇADO).
  ipcMain.handle("app:pickFile", async () => {
    const res = await dialog.showOpenDialog(win, {
      title: "Escolher arquivo",
      properties: ["openFile"],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false }
    return { ok: true, path: res.filePaths[0] }
  })

  // Espaço em disco de um path (para o diálogo de instalação).
  ipcMain.handle("app:diskSpace", async (_e, p) => {
    try {
      const { execFile } = require("child_process")
      const target = p && typeof p === "string" ? p : os.homedir()
      // Sobe até a primeira pasta que existe (o path pode ainda não ter sido criado).
      let probe = target
      while (!fs.existsSync(probe)) {
        const parent = path.dirname(probe)
        if (parent === probe) break
        probe = parent
      }
      const out = await new Promise((res, rej) =>
        execFile("df", ["-k", probe], (e, stdout) => (e ? rej(e) : res(stdout))),
      )
      const linha = String(out).trim().split("\n").pop().trim().split(/\s+/)
      // df -k: Filesystem 1K-blocks Used Available Use% Mounted on
      const totalKb = Number(linha[1])
      const availKb = Number(linha[3])
      return { ok: true, total: totalKb / 1024 / 1024, free: availKb / 1024 / 1024 } // GiB
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // Abre um link (notícia) no navegador padrão do sistema.
  ipcMain.handle("app:openExternal", (_e, url) => {
    if (typeof url === "string" && /^https?:\/\//.test(url)) shell.openExternal(url)
  })

  // Trailer local já baixado (file://) ou "" se ainda não temos.
  ipcMain.handle("trailer:path", (_e, id) => {
    const p = trailerLocal(id)
    return { path: p ? "file://" + p : "" }
  })

  // Baixa o trailer do YouTube (se ainda não existe). Devolve o caminho.
  ipcMain.handle("trailer:download", async (_e, { id, title } = {}) => {
    if (!id || !fs.existsSync(YTDLP)) return { ok: false, error: "yt-dlp ausente" }
    const r = await baixarTrailer(id, title || "")
    return r.ok ? { ok: true, path: "file://" + r.path } : r
  })

  // Lista vídeos do YouTube para escolha manual (sem baixar).
  ipcMain.handle("trailer:search", async (_e, { query } = {}) => {
    if (!YTDLP) {
      logTrailer("busca abortada: yt-dlp não instalado")
      return { ok: false, error: "yt-dlp não está instalado — instale o pacote yt-dlp" }
    }
    const { results, error } = await buscarTrailers(query || "")
    if (error) return { ok: false, error }
    return { ok: true, results }
  })

  // URL direta para pré-visualizar o vídeo num <video> (sem baixar).
  ipcMain.handle("trailer:streamUrl", async (_e, { url } = {}) => {
    if (!url || !fs.existsSync(YTDLP)) return { ok: false, error: "pedido inválido" }
    return streamTrailer(url)
  })

  // Baixa um vídeo específico do YouTube como trailer (escolha manual).
  ipcMain.handle("trailer:downloadUrl", async (_e, { id, url } = {}) => {
    if (!id || !url || !fs.existsSync(YTDLP)) return { ok: false, error: "pedido inválido" }
    const r = await baixarTrailerUrl(id, url)
    return r.ok ? { ok: true, path: "file://" + r.path } : r
  })

  // Baixa TODOS os trailers que faltam. Emite progresso e devolve a contagem.
  ipcMain.handle("trailer:downloadAll", async (_e) => {
    if (!fs.existsSync(YTDLP)) return { ok: false, error: "yt-dlp ausente" }
    let lib = []
    try {
      lib = JSON.parse(fs.readFileSync(LIB, "utf-8"))
    } catch {
      return { ok: false, error: "biblioteca não lida" }
    }
    const faltam = lib.filter((g) => !trailerLocal(g.id))
    let feitos = 0
    for (const g of faltam) {
      if (win) win.webContents.send("trailer:progress", {
        done: feitos, total: faltam.length, title: g.title,
      })
      await baixarTrailer(g.id, g.title || "")
      feitos++
    }
    if (win) win.webContents.send("trailer:progress", {
      done: feitos, total: faltam.length, title: "",
    })
    return { ok: true, count: feitos }
  })
  ipcMain.handle("app:quit", () => app.quit())
  ipcMain.handle("app:toggleFullscreen", () => {
    if (win) win.setFullScreen(!win.isFullScreen())
  })
  ipcMain.handle("app:setZoom", (_e, z) => {
    const factor = Math.min(2, Math.max(0.7, Number(z) || 1))
    if (win) win.webContents.setZoomFactor(factor)
    return factor
  })

  ipcMain.handle("library:refresh", async () => {
    await runIndexer()
    return readLibrary()
  })

  // Reconstrói TODOS os metadados (limpa cache e reindexar).
  ipcMain.handle("meta:rebuild", async () => {
    try {
      fs.unlinkSync(META_CACHE)
    } catch {
      /* sem cache, tudo bem */
    }
    await runIndexer()
    return readLibrary()
  })

  // Status das integrações para a aba Integrações.
  ipcMain.handle("integrations:status", () => {
    const cfg = readConfig()
    return {
      steam: Boolean(cfg.steam_api_key),
      slssteam: slssteamCount(),
      heroic: heroicConnected(),
    }
  })

  // Escolher imagem (avatar ou plano de fundo) — aceita GIF animado.
  // Procura arte online para um jogo. Junta o que cada fonte achou numa lista
  // só; se uma fonte falhar (chave errada, rede caída), as outras seguem.
  ipcMain.handle(
    "meta:art",
    async (_e, { gameId, titulo, kind, sgdbId, dimensions } = {}) => {
      if (!gameId || !SGDB_ENDPOINT[kind]) return { ok: false, error: "pedido inválido" }
      const cfg = readConfig()
      const chave = String(cfg.steamgriddb_api_key || "").trim()
      const igdbId = String(cfg.igdb_client_id || "").trim()
      const igdbSecret = String(cfg.igdb_client_secret || "").trim()
      const candidatos = []
      const erros = []
      let jogos = []

      // Steam: arte oficial, sem chave. Só existe para jogos da Steam.
      try {
        candidatos.push(...(await steamArt(gameId, kind)))
      } catch (e) {
        erros.push(`Steam: ${e.message}`)
      }

      // SteamGridDB: arte da comunidade, qualquer loja, inclui animados.
      if (chave) {
        try {
          let id = sgdbId
          if (!id) {
            jogos = await sgdbSearch(titulo || "", chave)
            id = jogos[0]?.id
          }
          if (id) candidatos.push(...(await sgdbArt(id, kind, chave, { dimensions })))
        } catch (e) {
          erros.push(`SteamGridDB: ${e.message}`)
        }
      } else {
        erros.push("SteamGridDB: sem chave de API (defina nas Configurações)")
      }

      // IGDB: arte de qualquer plataforma (capa e artworks/screenshots).
      if (igdbId && igdbSecret) {
        try {
          const gs = await igdbGames(titulo || "", igdbId, igdbSecret)
          candidatos.push(...igdbArtDe(gs, kind))
        } catch (e) {
          erros.push(`IGDB: ${e.message}`)
        }
      }

      // Xbox: catálogo público, sem chave. Capa retrato 1440x2160 e fundo 4K.
      try {
        const achados = await xboxSearch(titulo || "", ...xboxLocale(cfg))
        if (achados[0]) {
          const loc = await xboxProduto(achados[0].id, ...xboxLocale(cfg))
          if (loc) candidatos.push(...xboxArtDe(loc, kind))
        }
      } catch (e) {
        erros.push(`Xbox: ${e.message}`)
      }

      // Wallhaven: só para o fundo (hero). Wallpapers 16:9 em 4K de verdade.
      if (kind === "hero") {
        try {
          candidatos.push(...(await wallhavenBusca(titulo || "")))
        } catch (e) {
          erros.push(`Wallhaven: ${e.message}`)
        }
      }

      // PS Store: arte oficial da PlayStation (capa 2:3, fundo 4K, logo), pública.
      try {
        const tiles = await psnStoreSearch(titulo || "")
        const melhor = psnMelhorResultado(tiles, titulo || "")
        if (melhor) candidatos.push(...(await psnStoreArt(melhor.id, melhor.tipo, kind)))
      } catch (e) {
        erros.push(`PS Store: ${e.message}`)
      }

      return { ok: true, candidatos, jogos, erros }
    },
  )

  // Descrições candidatas: Steam (curta e completa) + IGDB (resumo e enredo).
  ipcMain.handle("meta:text", async (_e, { gameId, titulo } = {}) => {
    const cfg = readConfig()
    const igdbId = String(cfg.igdb_client_id || "").trim()
    const igdbSecret = String(cfg.igdb_client_secret || "").trim()
    const textos = []
    const erros = []

    try {
      textos.push(...(await steamTextos(gameId)))
    } catch (e) {
      erros.push(`Steam: ${e.message}`)
    }

    if (igdbId && igdbSecret) {
      try {
        textos.push(...igdbTextosDe(await igdbGames(titulo || "", igdbId, igdbSecret)))
      } catch (e) {
        erros.push(`IGDB: ${e.message}`)
      }
    } else {
      erros.push("IGDB: sem credenciais (defina nas Configurações)")
    }

    // Xbox: descrição no idioma da loja, sem chave.
    try {
      const achados = await xboxSearch(titulo || "", ...xboxLocale(cfg))
      if (achados[0]) {
        const loc = await xboxProduto(achados[0].id, ...xboxLocale(cfg))
        if (loc) textos.push(...xboxTextoDe(loc))
      }
    } catch (e) {
      erros.push(`Xbox: ${e.message}`)
    }

    return { ok: true, textos, erros }
  })

  // Baixa uma arte escolhida e guarda em art/. Mesmo destino do "Escolher".
  ipcMain.handle("art:download", async (_e, { id, kind, url } = {}) => {
    if (!id || !SGDB_ENDPOINT[kind] || !url) return { ok: false }
    const safeId = String(id).replace(/[^a-z0-9._-]/gi, "_")
    const base = path.join(ART_DIR, `${safeId}-${kind}-${Date.now()}`)
    try {
      fs.mkdirSync(ART_DIR, { recursive: true })
      const { path: dest } = await downloadTo(url, base, fs)
      const velha = artToDelete(readOverrides(OVERRIDES)[id]?.[kind], ART_DIR, path.sep)
      if (velha) {
        try {
          fs.unlinkSync(velha)
        } catch {
          /* já não existe */
        }
      }
      return { ok: true, path: dest }
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })

  // Escolhe uma arte para UM jogo e copia para art/. Diferente do avatar, o
  // nome do arquivo leva um timestamp: sem isso, trocar a capa reusaria o
  // mesmo caminho e o <img> continuaria mostrando a imagem antiga do cache.
  ipcMain.handle("art:pick", async (_e, { id, kind } = {}) => {
    if (!id || !["cover", "hero", "logo"].includes(kind)) return { ok: false }
    const titulos = {
      cover: "Escolher capa",
      hero: "Escolher plano de fundo",
      logo: "Escolher logo",
    }
    const res = await dialog.showOpenDialog(win, {
      title: titulos[kind],
      properties: ["openFile"],
      filters:
        kind === "hero"
          ? [
              // Fundo aceita live wallpaper: imagem/GIF ou vídeo.
              { name: "Imagens e vídeos", extensions: ["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"] },
              { name: "Vídeos (fundo animado)", extensions: ["webm", "mp4", "m4v", "mov"] },
              { name: "Imagens", extensions: ["png", "jpg", "jpeg", "gif", "webp"] },
            ]
          : [{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false }
    const src = res.filePaths[0]
    const ext = path.extname(src) || ".png"
    const safeId = String(id).replace(/[^a-z0-9._-]/gi, "_")
    const dest = path.join(ART_DIR, `${safeId}-${kind}-${Date.now()}${ext}`)
    try {
      fs.mkdirSync(ART_DIR, { recursive: true })
      fs.copyFileSync(src, dest)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
    // Apaga a arte anterior, se era nossa (não mexe no cache da Steam).
    const velha = artToDelete(readOverrides(OVERRIDES)[id]?.[kind], ART_DIR, path.sep)
    if (velha) {
      try {
        fs.unlinkSync(velha)
      } catch {
        /* já não existe: tudo bem */
      }
    }
    return { ok: true, path: dest }
  })

  ipcMain.handle("profile:pickImage", async (_e, kind) => {
    const key = kind === "background" ? "background" : "avatar"
    const res = await dialog.showOpenDialog(win, {
      title: key === "background" ? "Escolher plano de fundo" : "Escolher foto de perfil",
      properties: ["openFile"],
      filters:
        key === "background"
          ? [
              { name: "Imagens e vídeos", extensions: ["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"] },
              { name: "Vídeos (fundo animado)", extensions: ["webm", "mp4", "m4v", "mov"] },
              { name: "Imagens", extensions: ["png", "jpg", "jpeg", "gif", "webp"] },
            ]
          : [{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false }
    const src = res.filePaths[0]
    const ext = path.extname(src) || ".png"
    const dest = path.join(DATA_DIR, key + ext)
    try {
      fs.copyFileSync(src, dest)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
    // Salva o caminho LIMPO (file://) no config; o ?t= é só para atualizar a
    // visualização imediata (cache-buster), não deve ir para o disco.
    writeConfig({ profile: { [key]: "file://" + dest } })
    return { ok: true, path: "file://" + dest + "?t=" + Date.now() }
  })

  // Escolhe o arquivo cookies.txt do YouTube (para vídeos com restrição de idade).
  ipcMain.handle("trailer:pickCookies", async () => {
    const res = await dialog.showOpenDialog(win, {
      title: "Escolher cookies.txt do YouTube",
      properties: ["openFile"],
      filters: [{ name: "Cookies", extensions: ["txt"] }],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false }
    const p = res.filePaths[0]
    writeConfig({ youtube_cookies: p })
    return { ok: true, path: p }
  })

  // --- SLScheevo: conquistas em jogos injetados (SLSsteam) -----------------
  const SLSCHEEVO = path.join(BIN_DIR, "SLScheevo-Linux")
  const STEAM_STATS = path.join(os.homedir(), ".local/share/Steam/appcache/stats")

  // Status: binário instalado? quantos schemas já foram gerados?
  ipcMain.handle("slscheevo:status", () => {
    let schemas = 0
    try {
      schemas = fs.readdirSync(STEAM_STATS)
        .filter((f) => f.startsWith("UserGameStatsSchema_")).length
    } catch {}
    return { installed: fs.existsSync(SLSCHEEVO), schemas }
  })

  // Baixa o SLScheevo (release mais recente do GitHub) e abre num terminal —
  // ele é interativo (login Steam + 2FA), por isso precisa de TTY.
  ipcMain.handle("slscheevo:setup", async () => {
    try {
      if (!fs.existsSync(SLSCHEEVO)) {
        const rel = await fetch(
          "https://api.github.com/repos/xamionex/SLScheevo/releases/latest",
          { headers: { "User-Agent": "arcadia" } },
        ).then((r) => r.json())
        const asset = (rel.assets || []).find((a) => /linux/i.test(a.name || ""))
        if (!asset) return { ok: false, error: "release Linux não encontrada" }
        const tgz = path.join(BIN_DIR, "slscheevo.tar.gz")
        const buf = Buffer.from(
          await fetch(asset.browser_download_url).then((r) => r.arrayBuffer()),
        )
        fs.writeFileSync(tgz, buf)
        await new Promise((res, rej) =>
          execFile("tar", ["-xzf", tgz, "-C", BIN_DIR], (e) => (e ? rej(e) : res())),
        )
        fs.rmSync(tgz, { force: true })
        // o tar pode trazer o binário dentro de uma subpasta
        if (!fs.existsSync(SLSCHEEVO)) {
          const found = spawn("find", [BIN_DIR, "-name", "SLScheevo-Linux", "-type", "f"])
          let out = ""
          found.stdout.on("data", (d) => (out += d))
          await new Promise((res) => found.on("close", res))
          const src = out.trim().split("\n")[0]
          if (src) fs.renameSync(src, SLSCHEEVO)
        }
        fs.chmodSync(SLSCHEEVO, 0o755)
      }
      // Terminal disponível para a sessão interativa de login
      const terms = ["kitty", "kgx", "gnome-terminal", "konsole", "alacritty", "xterm"]
      const { execFileSync } = require("child_process")
      const term = terms.find((t) => {
        try { execFileSync("which", [t], { stdio: "ignore" }); return true } catch { return false }
      })
      if (!term) return { ok: false, error: "nenhum terminal encontrado (kitty, konsole…)" }
      const child = spawn(term, ["-e", SLSCHEEVO], {
        cwd: BIN_DIR,
        detached: true,
        stdio: "ignore",
      })
      child.unref()
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  createWindow()

  // Reindexa em BACKGROUND, sem travar a abertura. O app já subiu com o
  // library.json anterior; quando o índice terminar (Steam/Heroic/Lutris), avisa
  // o renderer para recarregar. Antes o arcadia.sh rodava o index.py ANTES do
  // Electron, segurando a tela preta por ~17s a cada boot. O delay deixa a
  // janela pintar e o carregamento inicial acontecer antes do trabalho pesado.
  setTimeout(() => {
    runIndexer().then(() => {
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
    })
  }, 1500)

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit()
})

// Ao sair, derruba o download ativo para não deixar o Legendary órfão (os
// downloads são detached, então não morrem junto do app sozinhos).
app.on("before-quit", () => {
  try { require("./downloadmanager").killActive() } catch {}
})
