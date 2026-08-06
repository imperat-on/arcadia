const { app, BrowserWindow, ipcMain, dialog, shell, session } = require("electron")
const { startAchievementWatcher, fetchAchievementsForApp } = require("./achievements")
const { iniciarVigia } = require("./achievements/cracked_watcher")
const { getNews } = require("./news")
const plugins = require("./plugins")
const updater = require("./updater")
const { showAchievementToast, closeAchievementToast } = require("./notify")
const path = require("path")
const fs = require("fs")
const os = require("os")
const { spawn, execFile } = require("child_process")
const { fetchRede } = require("./httpfetch")
// Escopo por conta dos arquivos locais — PRECISA estar no escopo do módulo
// (readLibrary e outros helpers rodam fora do whenReady; require dentro de
// bloco deixava "caminhoConta is not defined" → biblioteca vazia).
const { caminhoConta, definirConta, conta } = require("./supabase/conta")
const { readOverrides, setOverride, applyOverrides, artToDelete } = require("./overrides")
const {
  sgdbSearch,
  sgdbArt,
  wallhavenBusca,
  psnStoreSearch,
  psnMelhorResultado,
  psnStoreArt,
  steamArt,
  steamTextos,
  tituloBate,
  igdbProxy,
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

const STEAM_LANG_MAP = {
  "pt-BR": "portuguese",
  "en-US": "english",
  "es-ES": "spanish",
}

function steamLang() {
  try {
    const cfg = JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
    return STEAM_LANG_MAP[cfg.language] || "english"
  } catch {
    return "english"
  }
}
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
    "/usr/bin",
    "/usr/local/bin",
    "/bin",
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
// Snapshot da sessão encerrada: o interval limpa jogoAtivo antes do marcar
// fechar a sessão, então o registro de playtime local se ancora aqui.
let ultimoJogoAtivo = null
// Interval do poll de jogo. Se a janela for recriada sem matar o processo
// (comum no macOS ou em reinicializações), evita acumular timers antigos.
let runningGameInterval = null
// Foco real da janela (no gamescope o Chromium acha que está focado mesmo
// com o jogo por cima) — o renderer trava gamepad/trailer com isso.
let focado = true
// Vigia de jogo rodando (todos os modos): avisa o renderer nas transições
// abriu/fechou. O card "jogando" do modo desktop se ancora nisso.
let jogoRodando = false
// O poll SÓ arma quando lançamos um jogo (armarPollJogo) e desarma após 2
// ciclos sem sinal — idle não paga pgrep a cada 3s. No gamescope o mesmo
// tick resolve o foco (ARCADIA_GAMESCOPE=1), sem intervalo extra de 2s.
let sinalDeVida = 0
const armarPollJogo = () => {
  if (runningGameInterval) return
  sinalDeVida = 0
  runningGameInterval = setInterval(() => {
    const tick = () => {
      if (jogoAtivo) {
        try {
          process.kill(-jogoAtivo.pid, 0)
          marcar(true)
          sinalDeVida = 0
          return
        } catch {
          // Grupo do wrapper (ex: steam://rungameid) morreu. NÃO marca false
          // aqui — o pgrep abaixo confirma se o jogo real ainda vive.
          // ultimoJogoAtivo é preservado.
          jogoAtivo = null
        }
      }
      execFile("pgrep", ["-f", PADRAO_JOGO], (err) => {
        const rodando = !err
        marcar(rodando)
        if (rodando) {
          sinalDeVida = 0
          return
        }
        // Idle (sem jogo nosso e pgrep sem processo): 2 ciclos e desarma.
        if (++sinalDeVida >= 2) {
          clearInterval(runningGameInterval)
          runningGameInterval = null
        }
      })
    }
    if (process.env.ARCADIA_GAMESCOPE === "1") {
      // Foco do gamescope usa o mesmo tick — evita um pgrep extra por ciclo.
      execFile("pgrep", ["-f", PADRAO_JOGO], (err) => {
        const jogoRodando = !err // exit 0 = achou processo
        const ativo = !jogoRodando
        if (ativo !== focado) {
          focado = ativo
          if (win && !win.isDestroyed()) win.webContents.send("app:focus", ativo)
        }
        tick()
      })
    } else {
      tick()
    }
  }, 3000)
}
// Primário: grupo de processos do jogo que NÓS lançamos (jogoAtivo) — cobre
// custom/umu/legendary/lutris. Fallback: padrão clássico (jogos Steam, que
// são filhos do cliente Steam, não nossos).
const marcar = (rodando) => {
  if (rodando === jogoRodando) return
  jogoRodando = rodando
  if (win && !win.isDestroyed()) win.webContents.send("game:running", rodando)
  if (!rodando) {
    // Sessão encerrada: soma o tempo jogado no override (só fora da Steam —
    // a Steam já traz playtime real do indexer).
    const snap = ultimoJogoAtivo
    ultimoJogoAtivo = null
    if (snap && snap.gameId && snap.startedAt) {
      try {
        const min = Math.round((Date.now() - snap.startedAt) / 60000)
        if (min >= 1) {
          const prev = Number(readOverrides(caminhoConta(OVERRIDES))[snap.gameId]?.playtime_added_minutes) || 0
          setOverride(caminhoConta(OVERRIDES), snap.gameId, { playtime_added_minutes: prev + min })
          // Horas jogadas sobem pra conta (delta acumulado no servidor)
          try {
            require("./supabase/biblioteca").agendarPush()
          } catch {}
          if (win && !win.isDestroyed()) win.webContents.send("library:changed")
        }
      } catch {}
    }
  }
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
      [
        "-g",
        "-f",
        "best[height<=720][ext=mp4]/22/18/best[ext=mp4]/best",
        "--no-warnings",
        ...cookieArgs(),
        url,
      ],
      { timeout: 40000, maxBuffer: 1024 * 1024 * 4, env: YTDLP_ENV },
      (err, stdout, stderr) => {
        const link = String(stdout || "")
          .split("\n")
          .find((l) => l.startsWith("http"))
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
    const args = [`ytsearch12:${query} trailer`, "--flat-playlist", "--dump-json", "--no-warnings"]
    execFile(
      YTDLP,
      args,
      { timeout: 40000, maxBuffer: 1024 * 1024 * 8, env: YTDLP_ENV },
      (err, stdout, stderr) => {
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
          const msg =
            String(stderr || "")
              .split("\n")
              .filter((l) => /error/i.test(l))[0] ||
            (err.code === "ENOENT"
              ? "yt-dlp não encontrado"
              : `yt-dlp falhou (${err.code ?? err.message})`)
          logTrailer(`busca "${query}" falhou: ${msg}`)
          return resolve({ results: [], error: msg })
        }
        logTrailer(`busca "${query}": ${out.length} resultado(s)`)
        resolve({ results: out })
      },
    )
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
      const linha =
        errBuf
          .split("\n")
          .reverse()
          .find((l) => /error|ffmpeg/i.test(l)) || ""
      resolve({ ok: false, error: linha.trim() || "falha ao baixar" })
    })
    child.on("error", (e) => resolve({ ok: false, error: String(e.message || e) }))
  })
}

// Cache por mtime: readConfig é chamado dezenas de vezes no boot (createWindow,
// did-finish-load, watchers, handlers). Relê do disco só quando o arquivo muda
// (writeConfig troca o mtime pelo rename), então edições externas também pegam.
let _cfgCache = { mtimeMs: -1, data: {} }
function readConfig() {
  try {
    const m = fs.statSync(CONFIG).mtimeMs
    if (m !== _cfgCache.mtimeMs) {
      _cfgCache = { mtimeMs: m, data: JSON.parse(fs.readFileSync(CONFIG, "utf-8")) }
    }
    return _cfgCache.data
  } catch (e) {
    return {}
  }
}

// Chaves de API que NUNCA saem completas pro renderer (auditoria A-06): o form
// de configurações mostra a máscara; o config:set reconhece a máscara e
// preserva o valor real no disco.
const SEGREDOS = ["steam_api_key", "steamgriddb_api_key", "hubcap_api_key"]

function redigirSegredos(cfg) {
  if (!cfg || typeof cfg !== "object") return cfg
  const out = { ...cfg }
  for (const k of SEGREDOS) {
    const v = out[k]
    if (typeof v === "string" && v) {
      out[k] = v.length > 8 ? v.slice(0, 3) + "•••" + v.slice(-2) : "•••"
    }
  }
  return out
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
      execFile(
        legendary,
        ["info", "--json", appName],
        { timeout: 60000, maxBuffer: 32 * 1024 * 1024 },
        (e, stdout) => res(e ? "" : String(stdout)),
      )
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
        `https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(g.title)}&cc=br&l=${steamLang()}`,
      )
      appid = s?.items?.[0]?.id || ""
    } catch {}
  }
  if (appid) {
    try {
      const d = await fetchJson(
        `https://store.steampowered.com/api/appdetails?appids=${appid}&cc=br&l=${steamLang()}`,
      )
      const data = d?.[appid]?.data || {}
      const reqs = data.pc_requirements
      if (reqs && !Array.isArray(reqs)) {
        info.req_min = reqs.minimum || ""
        info.req_rec = reqs.recommended || ""
      }
      // Dados ricos p/ a página estilo Hydra (galeria, descrição, publisher).
      info.appid = appid
      info.short_description = data.short_description || ""
      // Prefere a versão mais rica (mais imagens inline) entre os 2 campos.
      {
        const ab = data.about_the_game || ""
        const det = data.detailed_description || ""
        const nImg = (s) => (String(s).match(/<img/g) || []).length
        info.about = nImg(det) > nImg(ab) ? det : ab || det
      }
      info.publishers = data.publishers || []
      info.developers = data.developers || []
      info.release_date = data.release_date?.date || ""
      info.controller_support = data.controller_support || "" // "full" | "partial"
      info.languages = data.supported_languages || "" // HTML; "*" = áudio
      info.header = data.header_image || ""
      info.background = data.background_raw || data.background || ""
      // Screenshots: { id, path_thumbnail, path_full }.
      info.screenshots = (data.screenshots || []).slice(0, 12).map((s) => ({
        thumb: s.path_thumbnail,
        full: s.path_full,
      }))
      // Trailers: mp4/webm (jogos antigos) + HLS (jogos novos só têm .m3u8).
      info.movies = (data.movies || []).slice(0, 6).map((m) => ({
        id: m.id,
        name: m.name,
        thumb: m.thumbnail,
        mp4: m.mp4?.max || m.mp4?.["480"] || "",
        webm: m.webm?.max || m.webm?.["480"] || "",
        hls: m.hls_h264 || "",
      }))
    } catch {}
  }
  return info
}

// Cache de sysinfo em memória + flush coalescido. Antes lia E reescrevia o
// arquivo INTEIRO (~86KB) a cada jogo: o prefetch percorre a biblioteca toda,
// então eram N leituras + N escritas síncronas de um arquivo grande. Agora
// carrega uma vez, muta em memória e agenda UMA escrita.
let _sysinfoCache = null
let _sysinfoTimer = null
function _loadSysinfo() {
  if (_sysinfoCache === null) _sysinfoCache = readJsonFile(SYSINFO_CACHE, {})
  return _sysinfoCache
}
function _flushSysinfoSoon() {
  if (_sysinfoTimer) return
  _sysinfoTimer = setTimeout(() => {
    _sysinfoTimer = null
    try {
      fs.writeFileSync(SYSINFO_CACHE, JSON.stringify(_sysinfoCache))
    } catch {}
  }, 1500)
}

async function getSysinfo(g) {
  const id = String(g?.id || "")
  const cache = _loadSysinfo()
  // Os requisitos de sistema vêm traduzidos pela Steam. Guardar o idioma junto
  // evita que a página do jogo fique em português depois de trocar o app para
  // inglês — este cache não tem validade, era para sempre.
  const lang = steamLang()
  if (cache[id] && cache[id]._lang === lang) return cache[id]
  const info = await buildSysinfo(g)
  info._lang = lang
  cache[id] = info
  _flushSysinfoSoon()
  return info
}

const _protonCache = new Map()
async function getProtonDb(appid) {
  appid = String(appid || "").replace(/^steam:/, "")
  if (!appid) return null
  const hit = _protonCache.get(appid)
  if (hit && Date.now() - hit.at < 24 * 60 * 60 * 1000) return hit.data
  try {
    const data = await fetchJson(`https://www.protondb.com/api/v1/reports/summaries/${appid}.json`)
    const out = data
      ? {
          tier: data.tier || data.bestReportedTier || "",
          score: typeof data.score === "number" ? data.score : null,
          deckCompatibility: data.deckCompatibility || data.steamDeckCompatibilityCategory || "",
          total: data.total || data.reportCount || 0,
          url: `https://www.protondb.com/app/${appid}`,
        }
      : null
    // Teto de entradas: sem isto o Map só crescia enquanto o app ficasse aberto.
    if (_protonCache.size > 100) _protonCache.clear()
    _protonCache.set(appid, { at: Date.now(), data: out })
    return out
  } catch {
    if (_protonCache.size > 100) _protonCache.clear()
    _protonCache.set(appid, { at: Date.now(), data: null })
    return null
  }
}

// Resolver perfis Steam (nome + avatar) a partir do steamid, sem API key.
// Endpoint público XML — pega o mesmo dado que o site do perfil expõe.
// Cache em disco (perfil raramente muda) com TTL de 7 dias e failure-cache
// curto de 1h (evita re-tentar 50 perfis privados a cada abertura da tela).
const PROFILE_CACHE = path.join(DATA_DIR, "profile_cache.json")
const PROFILE_TTL = 7 * 24 * 60 * 60 * 1000
const PROFILE_FAIL_TTL = 60 * 60 * 1000
let _profileCache = null
function loadProfileCache() {
  if (_profileCache) return _profileCache
  try {
    _profileCache = JSON.parse(fs.readFileSync(caminhoConta(PROFILE_CACHE), "utf-8"))
  } catch {
    _profileCache = {}
  }
  return _profileCache
}
function saveProfileCache() {
  try {
    fs.writeFileSync(caminhoConta(PROFILE_CACHE), JSON.stringify(_profileCache))
  } catch {
    /* disco cheio: ignora */
  }
}
async function fetchProfile(steamid) {
  // XML público. Timeout curto: perfil que não responde em 3s não vale bloquear.
  const ctrl = new AbortController()
  const to = setTimeout(() => ctrl.abort(), 3000)
  try {
    const r = await fetchRede(`https://steamcommunity.com/profiles/${steamid}?xml=1`, {
      headers: { "User-Agent": "arcadia" },
      signal: ctrl.signal,
    })
    if (!r.ok) return null
    const xml = await r.text()
    const nome =
      xml.match(/<steamID><!\[CDATA\[(.*?)\]\]><\/steamID>/)?.[1] ||
      xml.match(/<steamID>(.*?)<\/steamID>/)?.[1]
    const avatar =
      xml.match(/<avatarMedium><!\[CDATA\[(.*?)\]\]><\/avatarMedium>/)?.[1] ||
      xml.match(/<avatarMedium>(.*?)<\/avatarMedium>/)?.[1]
    if (!nome && !avatar) return null
    return { name: (nome || "").trim(), avatar: (avatar || "").trim() }
  } catch {
    return null
  } finally {
    clearTimeout(to)
  }
}
// Roda `tarefas` (arrays de () => Promise) com no máximo `n` em paralelo.
async function pool(tarefas, n) {
  const out = new Array(tarefas.length)
  let i = 0
  await Promise.all(
    Array.from({ length: Math.min(n, tarefas.length) }, async () => {
      while (i < tarefas.length) {
        const idx = i++
        out[idx] = await tarefas[idx]()
      }
    }),
  )
  return out
}
async function resolveProfiles(steamids) {
  const cache = loadProfileCache()
  const agora = Date.now()
  const faltando = steamids.filter((id) => {
    const c = cache[id]
    if (!c) return true
    const ttl = c.name ? PROFILE_TTL : PROFILE_FAIL_TTL
    return agora - c.at > ttl
  })
  if (faltando.length) {
    const resultados = await pool(
      faltando.map((id) => () => fetchProfile(id)),
      6,
    )
    faltando.forEach((id, idx) => {
      const r = resultados[idx] || {}
      cache[id] = { at: agora, name: r.name || "", avatar: r.avatar || "" }
    })
    saveProfileCache()
  }
  return cache
}

// Estatísticas + resumo de reviews via APIs públicas (SteamSpy owners/ccu +
// Steam appreviews). Sem backend próprio; dados aproximados. Cache 6h.
const _statsCache = new Map()
async function getGameStats(appid) {
  appid = String(appid || "").replace(/^steam:/, "")
  if (!appid) return null
  const hit = _statsCache.get(appid)
  if (hit && Date.now() - hit.at < 6 * 60 * 60 * 1000) return hit.data
  let out = null
  try {
    const [spy, rev] = await Promise.all([
      fetchJson(`https://steamspy.com/api.php?request=appdetails&appid=${appid}`).catch(() => null),
      // num_per_page=20 traz os textos das reviews junto do resumo (mesma API).
      // language=english: avaliações sempre em inglês (pedido do usuário).
      fetchJson(
        `https://store.steampowered.com/appreviews/${appid}?json=1&language=english&purchase_type=all&filter=all&num_per_page=50`,
      ).catch(() => null),
    ])
    const q = rev?.query_summary || {}
    const pos = Number(q.total_positive) || 0
    const total = Number(q.total_reviews) || 0
    // Comentários individuais: perfil (steamid p/ identicon), texto, recomendação,
    // horas jogadas na review e data (timestamp p/ "há N dias").
    const comentariosBase = (rev?.reviews || [])
      .slice(0, 50)
      .map((r) => ({
        steamid: String(r.author?.steamid || ""),
        author: r.author?.steamid ? `Steam ${String(r.author.steamid).slice(-4)}` : "",
        avatar: "",
        text: String(r.review || "").trim(),
        positive: Boolean(r.voted_up),
        hours: r.author?.playtime_forever ? Math.round(r.author.playtime_forever / 60) : 0,
        hoursAtReview: r.author?.playtime_at_review
          ? Math.round(r.author.playtime_at_review / 60)
          : 0,
        helpful: Number(r.votes_up) || 0,
        timestamp: Number(r.timestamp_created) || 0,
      }))
      .filter((c) => c.text)
    // Enriquece com nome/avatar reais (cache disco 7d, pool 6 paralelas).
    const perfis = await resolveProfiles(comentariosBase.map((c) => c.steamid).filter(Boolean))
    const comments = comentariosBase.map((c) => {
      const p = perfis[c.steamid]
      return { ...c, author: p?.name || c.author, avatar: p?.avatar || "" }
    })
    out = {
      owners: spy?.owners || "",
      ccu: Number(spy?.ccu) || 0,
      reviewDesc: q.review_score_desc || "",
      reviewPositivePct: total ? Math.round((pos / total) * 100) : null,
      totalReviews: total,
      comments,
    }
    // Se tudo vazio, trata como sem dados (painel some).
    if (!out.owners && !out.ccu && !out.totalReviews) out = null
  } catch {
    out = null
  }
  // Teto de entradas: cada item carrega até 50 reviews completas.
  if (_statsCache.size > 30) _statsCache.clear()
  _statsCache.set(appid, { at: Date.now(), data: out })
  return out
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
  const g = readJsonFile(caminhoConta(CUSTOM_GAMES), []).find((x) => x.id === id)
  if (!g) return null
  const linux = g.platform === "linux"
  return exeLaunchCmd(id, g.exe, linux)
}

// Monta o comando para rodar um executável arbitrário do jogo <id> (usado por
// jogos custom e pelo override "Executável" da aba Localizações). exe .sh/binário
// Linux roda direto; .exe passa pelo Wine/Proton no prefixo do jogo.
function exeLaunchCmd(id, exe, linux) {
  if (!exe) return null
  if (linux === undefined) linux = !/\.exe$/i.test(String(exe))
  if (linux) return { cmd: [exe], env: {} }
  const wm = require("./winemanager")
  const s = getGameSettings(id)
  const prefixo = s.prefixPath || defaultPrefix(id)
  const g = { exe }
  let v = null
  if (s.wineVersion) {
    v = wm.steamProtons().find((w) => w.id === s.wineVersion)
  }

  // Proton da Steam: não usar wine direto — Proton provê o Steam Runtime +
  // WINEDLLOVERRIDES corretos.
  if (v?.kind === "steam" && fs.existsSync(path.join(v.path, "proton"))) {
    // Migrar prefixo layout wine-puro (<prefix>/drive_c) pra layout Proton
    // (<prefix>/pfx/drive_c). Sem isso Proton refuse ou faz merda.
    const drivec = path.join(prefixo, "drive_c")
    const pfxDrivec = path.join(prefixo, "pfx", "drive_c")
    if (fs.existsSync(drivec) && !fs.existsSync(pfxDrivec)) {
      try {
        fs.mkdirSync(path.join(prefixo, "pfx"), { recursive: true })
        for (const entry of fs.readdirSync(prefixo)) {
          if (entry === "pfx") continue
          fs.renameSync(path.join(prefixo, entry), path.join(prefixo, "pfx", entry))
        }
      } catch (e) {
        console.warn("arcadia: falha migrando prefixo:", e.message)
      }
    }
    // UMU (Heroic): normaliza runtime + prefixo + overrides. É como Heroic
    // lança tudo — funciona pra Steam Proton e GE-Proton igual.
    const umuRun = path.join(
      os.homedir(),
      ".config",
      "heroic",
      "tools",
      "runtimes",
      "umu",
      "umu-run",
    )
    if (fs.existsSync(umuRun)) {
      return {
        cmd: [umuRun, g.exe],
        env: {
          WINEPREFIX: prefixo,
          GAMEID: "arcadia",
          PROTONPATH: v.path,
          STORE: "none",
        },
      }
    }
    // Fallback: script proton direto.
    return {
      cmd: [path.join(v.path, "proton"), "run", g.exe],
      env: {
        STEAM_COMPAT_DATA_PATH: prefixo,
        STEAM_COMPAT_CLIENT_INSTALL_PATH: path.join(os.homedir(), ".steam", "steam"),
        STEAM_COMPAT_APP_ID: "0",
        WINEPREFIX: prefixo,
      },
    }
  }

  return null
}

// --- Configurações por jogo (diálogo estilo Heroic) -------------------------
// Salvas em game_settings.json: { "<gameId>": { wineVersion, prefixPath, ... } }
const GAME_SETTINGS = path.join(DATA_DIR, "game_settings.json")
const SYSINFO_CACHE = path.join(DATA_DIR, "sysinfo_cache.json")
// Jogos adicionados manualmente ("Adicionar jogo"): entram na biblioteca.
const CUSTOM_GAMES = path.join(DATA_DIR, "custom_games.json")
// Stubs otimistas gravados quando o usuário adiciona um jogo pela loja Steam:
// aparecem na aba Jogos imediatamente, com arte da CDN. O indexer os substitui
// pela entrada real de library.json na próxima passada, e o stub é removido.
const PENDING_GAMES = path.join(DATA_DIR, "pending_games.json")

function readJsonFile(p, fallback) {
  try {
    return JSON.parse(fs.readFileSync(p, "utf-8"))
  } catch {
    return fallback
  }
}

async function fetchJson(url) {
  const r = await fetchRede(url, { headers: { "User-Agent": "arcadia" } })
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  return r.json()
}

// Cache por mtime (padrão de _cfgCache): readAllGameSettings roda no readLibrary
// e em vários handlers; setGameSettings invalida ao gravar.
let _gsCache = { mtimeMs: -1, data: {} }
function readAllGameSettings() {
  try {
    const m = fs.statSync(caminhoConta(GAME_SETTINGS)).mtimeMs
    if (m !== _gsCache.mtimeMs) {
      _gsCache = { mtimeMs: m, data: JSON.parse(fs.readFileSync(caminhoConta(GAME_SETTINGS), "utf-8")) }
    }
    return _gsCache.data
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
    fs.writeFileSync(caminhoConta(GAME_SETTINGS), JSON.stringify(all, null, 2))
    _gsCache = { mtimeMs: fs.statSync(caminhoConta(GAME_SETTINGS)).mtimeMs, data: all }
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
        fs.writeFileSync(caminhoConta(GAME_SETTINGS), JSON.stringify(all, null, 2))
        _gsCache = { mtimeMs: fs.statSync(caminhoConta(GAME_SETTINGS)).mtimeMs, data: all }
      } catch {}
    }
    try {
      fs.rmSync(path.join(LOG_DIR, `${String(id).replace(/[^a-z0-9._-]/gi, "_")}.log`), {
        force: true,
      })
    } catch {}
  }
}

// Divide uma linha de argumentos respeitando aspas: a "b c" d -> [a, b c, d].
function splitArgs(str) {
  return (String(str || "").match(/"[^"]*"|'[^']*'|\S+/g) || []).map((t) =>
    t.replace(/^["']|["']$/g, ""),
  )
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

// Valida binários do comando de launch ANTES do spawn. Retorna mensagem de
// erro ou null se OK. Wrappers (gamescope/gamemoderun/mangohud) já são
// validados em applyGameSettings.
function validarBinariosLaunch(cmd, gameId) {
  const bin = cmd[0]
  if (!bin) return "cmd[0] vazio"
  // Bin principal — se é caminho absoluto, exigir existência.
  if (bin.startsWith("/") && !fs.existsSync(bin)) {
    return `Binário não existe: ${bin}`
  }
  // Executável do jogo (heurística: último arg com .exe ou path absoluto)
  const exe = cmd.find((a) => /\.(exe|bat|msi)$/i.test(a) || (a.startsWith("/") && a !== bin))
  if (exe && exe.startsWith("/") && !fs.existsSync(exe)) {
    return `Executável do jogo não existe: ${exe}`
  }
  return null
}

// Monta env/args de lançamento a partir das configurações do jogo.
// Retorna { cmd, env, warnings } já com gamescope (se ligado) e variáveis.
/**
 * Faz a Steam subir escondida quando ela é aberta só para lançar um jogo.
 *
 * O comando do jogo é `["steam", "steam://rungameid/<appid>"]`. Com o cliente
 * fechado, isso abre a janela da Steam na frente de tudo — no Big Picture ela
 * toma a tela por cima do launcher. Com `-silent` o cliente vai direto para a
 * bandeja: processo rodando (que é o que o DRM exige), sem janela nenhuma.
 *
 * Com a Steam já aberta a flag é ignorada; a invocação só encaminha a URL para
 * a instância viva. O efeito aparece exatamente no caso que incomoda.
 *
 * `steam://install` fica de fora DE PROPÓSITO: ali o diálogo de instalação da
 * Steam é o que a pessoa foi ver, e escondê-lo faria o jogo nunca instalar.
 */
function steamSilencioso(cmd) {
  if (!Array.isArray(cmd) || cmd.length < 2) return cmd
  // O binário pode ser o wrapper do slsteam-moon, que repassa os argumentos.
  if (path.basename(String(cmd[0])) !== "steam") return cmd
  if (cmd.includes("-silent")) return cmd
  const abreJogo = cmd.some((a) => /^steam:\/\/(rungameid|run)\//.test(String(a)))
  if (!abreJogo) return cmd
  return [cmd[0], "-silent", ...cmd.slice(1)]
}

/**
 * Troca o `steam` do comando pela Steam COM a SLSsteam carregada, e avisa
 * quando não dá para consertar.
 *
 * O `steam` do PATH é a Steam pura (/usr/bin/steam) — o wrapper do slsteam-moon
 * não está no PATH. Com o cliente fechado, lançar um jogo subia a Steam SEM
 * injeção, e o jogo que só existe pelo bloco AdditionalApps não abria.
 *
 * Com o cliente JÁ aberto sem injeção não há conserto aqui: a URL vai para a
 * instância de pé, e trocar a injeção exigiria matá-la — decisão do usuário,
 * pelo botão "Reiniciar Steam". Nesse caso devolvemos um aviso, em vez de
 * deixar o jogo falhar em silêncio.
 */
function steamComInjecao(cmd) {
  const avisos = []
  if (!Array.isArray(cmd) || path.basename(String(cmd[0])) !== "steam")
    return { cmd, env: {}, avisos }
  const url = cmd.find((a) => /^steam:\/\/(rungameid|run)\//.test(String(a)))
  if (!url) return { cmd, env: {}, avisos }

  const steam = require("./steamstore").comandoSteam()
  const novo = [steam.cmd, ...cmd.slice(1)]

  const appid = (String(url).match(/\/(\d+)/) || [])[1]
  if (appid && require("./steamstore").appidsInjetados().has(appid)) {
    if (require("./steamstore").steamInjetada() === false) {
      avisos.push(
        "A Steam está aberta sem a SLSsteam — este jogo não vai abrir. " +
          'Use "Reiniciar Steam" na loja para recarregá-la.',
      )
    }
  }
  return { cmd: novo, env: steam.env, avisos }
}

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
  // MANGOHUD=1 só quando o binário não existe (fallback); com o wrapper o
  // mangohud já se ativa sozinho e a var vira redundância.
  if (s.mangohud && !binExists("mangohud")) env.MANGOHUD = "1"
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
      const v = wm.steamProtons().find((w) => w.id === s.wineVersion)
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
  if (s.gameArgs && path.basename(String(cmd[0])) !== "steam")
    finalCmd = [...finalCmd, ...splitArgs(s.gameArgs)]
  // Gamescope embrulha o comando (não se aplica a jogos Steam — a Steam tem
  // sua própria integração com gamescope). --disable-gamemode evita o abort
  // do gamescopereaper no gamemode_request_end (bug libgamemodeauto/dbus);
  // quem quer GameMode usa o checkbox (gamemoderun), que funciona.
  if (s.gamescope && path.basename(String(cmd[0])) !== "steam") {
    if (binExists("gamescope")) {
      const args = [
        "--disable-gamemode",
        "-W",
        String(s.gsWidth || 1920),
        "-H",
        String(s.gsHeight || 1080),
      ]
      if (s.gsFps) args.push("-r", String(s.gsFps))
      finalCmd = ["gamescope", ...args, "--", ...finalCmd]
    } else {
      warnings.push("gamescope não está instalado — iniciando sem ele")
    }
  }
  // GameMode (Feral): embrulha tudo com gamemoderun (a Steam tem o dela).
  if (s.gamemode && path.basename(String(cmd[0])) !== "steam") {
    if (binExists("gamemoderun")) {
      finalCmd = ["gamemoderun", ...finalCmd]
    } else {
      warnings.push("gamemoderun não está instalado — iniciando sem ele")
    }
  }
  // MangoHud: embrulha com o binário `mangohud` (LD_PRELOAD correto p/ GL e
  // Vulkan). Só MANGOHUD=1 não basta em jogos OpenGL (ex.: Godot).
  if (s.mangohud && path.basename(String(cmd[0])) !== "steam") {
    if (binExists("mangohud")) {
      finalCmd = ["mangohud", ...finalCmd]
    } else {
      warnings.push("mangohud não está instalado — iniciando sem ele")
    }
  }
  // Wrappers customizados (aba AVANÇADO): os mais externos por último.
  if (path.basename(String(cmd[0])) !== "steam") {
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

// Mercado e idioma da loja da Microsoft. Segue o idioma configurado.
function xboxLocale(cfg) {
  const lang = cfg.language || "en-US"
  const XBOX_MAP = {
    "pt-BR": "pt-br",
    "en-US": "en-us",
    "es-ES": "es-es",
  }
  return [String(cfg.xbox_market || "BR"), XBOX_MAP[lang] || "en-us"]
}

// Lê library.json, aplica as edições do usuário e converte caminhos de arte
// locais em file:// para o <img>. Jogos adicionados manualmente entram aqui.
// Cache por mtime combinado dos 3 arquivos-fonte. readLibrary é chamado no
// prefetch e em ~8 handlers; reparsear+reconstruir file:// toda vez é
// desperdício quando nada mudou. Callers só iteram ou serializam via IPC
// (structured clone), então devolver a referência cacheada é seguro.
let _libCache = { chave: "", games: [] }
function _libMtimeKey() {
  return [caminhoConta(LIB), caminhoConta(CUSTOM_GAMES), caminhoConta(OVERRIDES), caminhoConta(PENDING_GAMES), caminhoConta(GAME_SETTINGS)]
    .map((p) => {
      try {
        return fs.statSync(p).mtimeMs
      } catch {
        return 0
      }
    })
    .join(":")
}

function capaSteamRuim(appid, cover) {
  const s = String(cover || "")
  if (!s) return true
  if (s.endsWith(`/steam/apps/${appid}/header.jpg`)) return true
  if (s.endsWith(`/steam/apps/${appid}/library_600x900.jpg`)) return true
  if (s.endsWith(`/library_header.jpg`)) return true
  if (/librarycache\/\d+\/[^/]+\/library_header\.jpg$/i.test(s)) return true
  return /\/steam\/apps\/\d+\/capsule_\d+x\d+\.jpg(?:\?|$)/.test(s)
}

async function curarCapasSteam(games) {
  const overrides = readOverrides(caminhoConta(OVERRIDES))
  // Alvo: jogo Steam com capa ruim OU sem ícone (a lista da sidebar usa ícone).
  const alvos = games
    .map((g) => ({ g, appid: /^steam:(\d+)$/.exec(String(g.id || ""))?.[1] }))
    .filter(
      ({ g, appid }) =>
        appid && ((!overrides[g.id]?.cover && capaSteamRuim(appid, g.cover)) || !g.icon),
    )
  if (!alvos.length) return games
  try {
    const { itensDaLoja } = require("./steamstore")
    const { mapa } = await itensDaLoja(alvos.map((a) => a.appid))
    let mudou = false
    const pendentes = readJsonFile(caminhoConta(PENDING_GAMES), [])
    for (const { g, appid } of alvos) {
      const it = mapa.get(appid)
      if (!it) continue
      if (it.capa && !overrides[g.id]?.cover && capaSteamRuim(appid, g.cover)) g.cover = it.capa
      if (it.heroi && capaSteamRuim(appid, g.hero)) g.hero = it.heroi
      if (it.icon && !g.icon) g.icon = it.icon
      const p = pendentes.find((x) => x?.id === g.id)
      if (p) {
        p.cover = g.cover
        if (g.hero) p.hero = g.hero
        if (g.icon) p.icon = g.icon
        mudou = true
      }
    }
    if (mudou) fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(pendentes, null, 2))
  } catch {}
  return games
}

function readLibrary() {
  try {
    const chave = _libMtimeKey()
    if (chave === _libCache.chave) return _libCache.games
    const games = JSON.parse(fs.readFileSync(caminhoConta(LIB), "utf-8"))
    games.push(...readJsonFile(caminhoConta(CUSTOM_GAMES), []))
    // Stubs otimistas: só entram se ainda não foram indexados de verdade.
    const jaTem = new Set(games.map((g) => g.id))
    for (const p of readJsonFile(caminhoConta(PENDING_GAMES), [])) {
      if (p && p.id && !jaTem.has(p.id)) games.push(p)
    }
    applyOverrides(games, readOverrides(caminhoConta(OVERRIDES)))
    // Tempo de sessão local (jogos NÃO-Steam): o renderer recebe o playtime
    // já somado. A Steam não entra — o indexer traz o playtime real dela.
    for (const g of games) {
      if (g.playtime_added_minutes) {
        g.playtime_minutes = (Number(g.playtime_minutes) || 0) + Number(g.playtime_added_minutes)
      }
    }
    // Executável definido na aba Localizações torna o jogo jogável: o launch já
    // roda esse .exe (exeLaunchCmd), mas sem marcar installed a UI só oferecia
    // "Instalar" e o botão Jogar nunca aparecia.
    const settings = readAllGameSettings()
    for (const g of games) {
      if (g && settings[g.id]?.exePath) {
        if (g.installed === false) g.installed = true
        g.temExe = true // frontend decide se mostra o menu Steam vs fora-da-Steam
      }
    }
    for (const g of games) {
      for (const k of ["cover", "hero", "logo"]) {
        if (typeof g[k] === "string" && g[k].startsWith("/")) {
          g[k] = "file://" + g[k]
        }
      }
    }
    _libCache = { chave, games }
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

// Avisa o renderer e, em seguida, reindexa em SEGUNDO PLANO para avisar de
// novo com a biblioteca já atualizada.
//
// O primeiro aviso cobre o que só depende do que já está em disco (o card da
// loja voltando a oferecer "Add", por exemplo). O segundo é o que faz o jogo
// recém-adicionado APARECER nas abas Jogos e Biblioteca: quem o descobre é o
// index.py, lendo o bloco AdditionalApps da SLSsteam. Reindexar leva ~12s, e
// travar o handler por esse tempo deixaria o botão preso.
// Procura commit novo no GitHub e avisa o renderer. Nunca aplica nada: quem
// decide é o usuário, no diálogo. Silencioso quando não há o que dizer — sem
// internet, com trabalho local em andamento ou já atualizado, ninguém precisa
// ver aviso nenhum.
async function procurarAtualizacao(win) {
  try {
    if (readConfig().check_updates_on_start === false) return
    if (!(await updater.estado()).podeAtualizar) return
    const r = await updater.verificar()
    if (!r.ok || !r.atrasado) return
    if (win && !win.isDestroyed()) win.webContents.send("update:available", r)
  } catch {}
}

function avisarBiblioteca(win, reindexar = true) {
  const emitir = () => {
    if (win && !win.isDestroyed()) win.webContents.send("library:changed")
  }
  emitir()
  if (reindexar)
    runIndexer().then(() => {
      try {
        limparPendentesIndexados()
      } catch {}
      emitir()
    })
}

// Grava um stub em pending_games.json com o mesmo formato de library.json:
// id "steam:<appid>", launcher steam, arte da CDN, flag pendente pra UI opcional.
// Se o appid já está indexado (ou já é pendente), não faz nada.
function adicionarStubPendente(appid, title, art = {}) {
  const id = "steam:" + appid
  if (readLibrary().some((g) => g.id === id)) return
  const atuais = readJsonFile(caminhoConta(PENDING_GAMES), [])
  if (atuais.some((g) => g && g.id === id)) return
  const base = "https://cdn.cloudflare.steamstatic.com/steam/apps/" + appid
  atuais.push({
    id,
    title: String(title || "").trim() || `Steam ${appid}`,
    launcher: "steam",
    launch_cmd: ["steam", `steam://rungameid/${appid}`],
    installed: false,
    cover: art.cover || `${base}/library_600x900.jpg`,
    hero: art.hero || `${base}/library_hero.jpg`,
    logo: `${base}/logo.png`,
    pendente: true,
  })
  fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(atuais, null, 2))
}

// Após o indexer rodar, qualquer stub cujo id já apareça em library.json é
// removido — a entrada real substitui o stub sem duplicar.
function limparPendentesIndexados() {
  const atuais = readJsonFile(caminhoConta(PENDING_GAMES), [])
  if (!atuais.length) return
  let reais
  try {
    reais = readLibrary()
  } catch {
    return
  }
  const idsReais = new Set(reais.map((g) => g.id))
  const restantes = atuais.filter((g) => g && g.id && !idsReais.has(g.id))
  if (restantes.length !== atuais.length) {
    fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(restantes, null, 2))
  }
}

function removerStubPendente(appid) {
  const id = "steam:" + appid
  const atuais = readJsonFile(caminhoConta(PENDING_GAMES), [])
  const restantes = atuais.filter((g) => g && g.id !== id)
  if (restantes.length !== atuais.length)
    fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(restantes, null, 2))
  return restantes.length !== atuais.length
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
        if (line && !/^\s/.test(line) && line.includes(":") && !line.trimStart().startsWith("#")) {
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
// Vigia de conquistas (toast estilo PS5 ao desbloquear). Além do toast,
// marca o item no achievements.json NA HORA — sem isso o painel só
// atualizava no próximo reindex da biblioteca.
let pararAchievementWatcher = null

// Callback único de desbloqueio: marca o item no achievements.json (o painel
// lê de lá) e avisa o renderer.
function onUnlockAchievement(payload) {
  let novo = false
  try {
    const arq = caminhoConta(path.join(DATA_DIR, "achievements.json"))
    const store = JSON.parse(fs.readFileSync(arq, "utf-8"))
    const it = (store?.[payload.appid]?.items || []).find(
      (x) => `${x.block}|${x.bit}` === payload.key,
    )
    if (it && !it.achieved) {
      it.achieved = true
      it.unlock = payload.unlock
      fs.writeFileSync(arq, JSON.stringify(store))
      novo = true
      // Conta online: enfileira o desbloqueio pro sync (só se o item tem
      // apiname — sem ele não dá pra referenciar na nuvem). Nunca bloqueia
      // o caminho do launch: enqueue é síncrono local e o sync roda depois.
      try {
        if (it.apiname) {
          const syncMod = require("./supabase/sync")
          syncMod.enqueue([
            {
              appid: payload.appid,
              apiname: it.apiname,
              unlocked_at: payload.unlock,
              title: it.title,
              icon: it.icon,
              percent: it.percent,
            },
          ])
          syncMod.scheduleNow()
        }
      } catch {}
    }
  } catch {}
  // Só dispara toast e IPC se a conquista era realmente nova — evita flood
  // em sincronização inicial e duplicatas entre os dois watchers.
  if (novo) {
    if (win && !win.isDestroyed()) win.webContents.send("achievement:unlocked", payload)
    showAchievementToast(payload)
  }
}

function createWindow() {
  const cfgIni = readConfig()
  win = new BrowserWindow({
    width: 1600,
    height: 900,
    backgroundColor: "#000000",
    autoHideMenuBar: true,
    icon: path.join(__dirname, "..", "public", "logo-512.png"),
    fullscreen: process.env.PS5_FULLSCREEN === "1",
    // Não mostra até o primeiro paint estar pronto: sem isto a janela abre
    // branca/vazia e só depois o React pinta. Com ready-to-show o usuário vê
    // a janela já com conteúdo, sem flash branco.
    show: false,
    // Modo desktop usa barra de título própria (botões estilo macOS na UI), então
    // a janela é frameless. "Usar janela sem moldura" (Config. Gerais) força o
    // mesmo no modo console. Requer reiniciar o app.
    frame: process.env.ARCADIA_MODE !== "desktop" && !cfgIni.frameless_window,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      // webSecurity ON (auditoria A-06): sem ele, um XSS no renderer vira
      // exfiltração livre e fetch cross-origin sem CORS. Capas locais
      // file:// continuam carregando (mesmo scheme), capas/avatares remotos
      // são https e passam pela CSP do index.html.
      webSecurity: true,
      // Deixa o trailer tocar sozinho COM som ao focar o jogo (estilo PS5).
      autoplayPolicy: "no-user-gesture-required",
      // A página da loja Steam é embutida num <webview> (StoreGamePage).
      webviewTag: true,
    },
  })
  // Força o preload da webview da loja Steam pelo main (mais seguro que via
  // atributo no HTML) e trava Node/integração na página de terceiros.
  win.webContents.on("will-attach-webview", (_e, wp) => {
    wp.preload = path.join(__dirname, "webview-steam-preload.js")
    wp.nodeIntegration = false
    wp.contextIsolation = true
  })
  win.loadFile(path.join(__dirname, "..", "dist", "index.html"))
  win.once("ready-to-show", () => win.show())

  // ── ANTIDOTO: alt-tab → tela preta (Wayland/NVIDIA) ──────────────────
  // O compositor perde a superfície da janela quando ela é ocluída; sem
  // repaint na volta o Chromium entrega um frame morto (fundo #000). Três
  // defesas: (1) não congelar o renderer em background (frame sempre fresco
  // ao restaurar), (2) invalidar o repaint a cada show, (3) recarregar sozinho
  // se o processo de render morrer (render-process-gone) em vez de deixar a
  // janela preta até o usuário fechar. Tudo com log para diagnóstico.
  win.webContents.setBackgroundThrottling(false)
  win.on("show", () => {
    try {
      win.webContents.invalidate()
    } catch { /* janela já destruída */ }
  })
  win.webContents.on("render-process-gone", (_e, detalhes) => {
    console.error("[janela] render-process-gone:", JSON.stringify(detalhes))
    // Reason "crashed" é recuperável; "killed"/"oom" também. Só ignora se a
    // janela já foi fechada ou o app está saindo.
    if (win && !win.isDestroyed() && !app.isQuitting) {
      setTimeout(() => {
        if (win && !win.isDestroyed() && win.isVisible()) {
          win.webContents.reload()
        }
      }, 500)
    }
  })
  win.webContents.on("unresponsive", () => console.error("[janela] renderer unresponsive"))
  win.webContents.on("responsive", () => console.error("[janela] renderer responsive de novo"))
  // Fim do antídoto alt-tab.

  // Aplica a escala salva assim que a página carrega.
  win.webContents.on("did-finish-load", () => {
    const z = Number(readConfig().ui_scale) || 1
    win.webContents.setZoomFactor(Math.min(2, Math.max(0.7, z)))
    // Modo console (tela cheia): cursor OCULTO por padrão, mas aparece ao
    // mexer o mouse e some após ~2s parado (navegação continua por gamepad).
    if (win.isFullScreen()) {
      win.webContents
        .executeJavaScript(
          `
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
      `,
        )
        .catch(() => {})
    }
    // Temas customizados: injeta todos os .css da pasta configurada.
    try {
      const dir = String(readConfig().custom_css_path || "").trim()
      if (dir && fs.existsSync(dir)) {
        const css = fs
          .readdirSync(dir)
          .filter((f) => f.endsWith(".css"))
          .map((f) => fs.readFileSync(path.join(dir, f), "utf-8"))
          .join("\n")
        if (css) win.webContents.insertCSS(css)
      }
    } catch {}

    // Atualização do Arcadia: verifica DEPOIS da janela carregar e sem
    // esperar — checar antes atrasaria a abertura por causa de uma ida à
    // rede que pode nem ter resposta.
    procurarAtualizacao(win)
  })
  // Foco real da janela (no gamescope o Chromium acha que está focado mesmo
  // com o jogo por cima) — o renderer trava gamepad/trailer com isso.
  win.on("blur", () => win?.webContents.send("app:focus", false))
  win.on("focus", () => win?.webContents.send("app:focus", true))
  // Vigia de conquistas: toast em tempo real + marca o item no
  // achievements.json (o painel lê de lá; sem isso só atualizava no reindex).
  if (pararAchievementWatcher) pararAchievementWatcher()
  pararAchievementWatcher = startAchievementWatcher(onUnlockAchievement)
  iniciarVigia(onUnlockAchievement)

  // Modo gamescope: o Electron roda no X aninhado e NÃO recebe blur/focus
  // quando o jogo abre no desktop. O foco é resolvido dentro do poll de jogo
  // (armarPollJogo, escopo do módulo) — sem jogo rodando não há pgrep.
}

// Pré-configura a partition do webview da loja Steam: cookies de idade para
// pular a verificação de "birth date" nas páginas de conteúdo adulto. É a
// mesma coisa que a Steam grava depois que o usuário passa o age-gate uma vez.
function configurarLojaSteam() {
  try {
    const ses = session.fromPartition("persist:steamstore")
    const exp = Math.floor(Date.now() / 1000) + 10 * 365 * 24 * 3600
    // birthtime = 1990-01-01 UTC (631152000). Adulto o bastante para tudo.
    const cookies = [
      { name: "birthtime", value: "631152000" },
      { name: "lastagecheckage", value: "1-January-1990" },
      { name: "wants_mature_content", value: "1" },
    ]
    for (const c of cookies) {
      ses.cookies
        .set({
          url: "https://store.steampowered.com",
          name: c.name,
          value: c.value,
          domain: ".steampowered.com",
          path: "/",
          secure: true,
          expirationDate: exp,
        })
        .catch(() => {})
    }
  } catch {}
}

app.whenReady().then(() => {
  configurarLojaSteam()
  startSysinfoPrefetch()
  // Conta online (Supabase): registra IPC de auth e espelha eventos pro renderer.
  try {
    const { registerAccountIpc } = require("./supabase/ipc")
    registerAccountIpc(
      (channel, payload) => {
        for (const w of require("electron").BrowserWindow.getAllWindows()) {
          if (!w.isDestroyed()) w.webContents.send(channel, payload)
        }
      },
      (username) => {
        // Troca o escopo dos arquivos locais (library/achievements/horas...)
        // pra conta logada (ou guest quando null) e recarrega a UI.
        definirConta(username)
        for (const w of require("electron").BrowserWindow.getAllWindows()) {
          if (!w.isDestroyed()) w.webContents.send("library:changed")
        }
      },
    )
  } catch (e) {
    console.error("[supabase] falha ao registrar IPC de conta:", e)
  }
  // Não há prefetch de vitrine: a loja é a página web da Steam embutida
  // (StoreConsole/webview), que se cacheia sozinha. O que vale a pena é abrir
  // a conexão com a Steam cedo — a primeira requisição do processo custa ~3,4s
  // de DNS + TLS, e sem isto ela caía na primeira tecla digitada na busca.
  setTimeout(() => {
    require("./steamstore")
      .aquecer()
      .catch(() => {})
  }, 5000)
  // Garante que o escopo da conta do boot (sessão restaurada) já está ativo
  // antes de o renderer ler library/conquistas — sem isso a UI pisca com os
  // dados guest e só depois troca (mesmo bug do "nome antigo").
  const { restoreSession } = require("./supabase/client")
  const contaPronta = restoreSession()
    .then((r) => {
      if (r?.session?.user?.user_metadata?.username) {
        definirConta(r.session.user.user_metadata.username)
      }
      return null
    })
    .catch(() => null)

  ipcMain.handle("library:get", async () => {
    await contaPronta
    const games = readLibrary()
    // Enriquecimento de capas/ícones NUNCA pode bloquear a resposta — se a
    // rede do Steam Store travar, o renderer ficaria com a lista vazia.
    // Devolve a lista NA HORA e cura as capas em background; só avisa o
    // renderer se a cura MUDOU alguma capa (senão vira loop de reload).
    const antes = games.map((g) => g.icon || g.cover || "").join("|")
    const cura = curarCapasSteam(games)
    const trava = new Promise((res) => setTimeout(() => res(null), 15000))
    Promise.race([cura, trava])
      .then(() => {
        const depois = games.map((g) => g.icon || g.cover || "").join("|")
        if (antes !== depois) {
          for (const w of require("electron").BrowserWindow.getAllWindows()) {
            if (!w.isDestroyed()) w.webContents.send("library:changed")
          }
        }
      })
      .catch(() => {})
    return games
  })
  ipcMain.handle("achievements:get", async (_e, appid) => {
    try {
      const store = JSON.parse(fs.readFileSync(caminhoConta(path.join(DATA_DIR, "achievements.json")), "utf-8"))
      const local = store?.[appid]?.items || []
      if (local.length) return local
    } catch {}
    try {
      return await fetchAchievementsForApp(appid)
    } catch {
      return []
    }
  })

  // Força desbloqueio de UMA conquista escrevendo direto no .bin do Steam
  // (sem cliente Steam rodando). Acha o accountId no nome do .bin existente e
  // o block|bit no schema; se o .bin nunca foi criado (jogo nunca rodou),
  // devolve erro amigável — não dá pra criar do zero sem saber o accountId.
  ipcMain.handle("achievements:force:unlock", async (_e, { appid, apiname } = {}) => {
    try {
      const steamBin = require("./achievements/steam_bin")
      let accountId = null
      for (const f of fs.readdirSync(steamBin.STATS_DIR)) {
        const m = /^UserGameStats_(\d+)_(\d+)\.bin$/.exec(f)
        if (m && m[2] === String(appid)) {
          accountId = m[1]
          break
        }
      }
      if (!accountId) {
        return { ok: false, error: "conquistas.desbloquear_erro_sem_bin" }
      }
      const schema = require("./achievements/schema")
      const items = schema.loadAchievements()?.[String(appid)]?.items || []
      const item = items.find((i) => i.apiname === apiname)
      if (!item || item.block == null || item.bit == null) {
        return { ok: false, error: "apiname sem block|bit no schema" }
      }
      const file = path.join(steamBin.STATS_DIR, `UserGameStats_${accountId}_${appid}.bin`)
      return steamBin.writeAchievementUnlock(file, item.block, item.bit)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Recarrega apiname/título/desc/ícones dos itens do achievements.json a partir
  // dos UserGameStatsSchema_*.bin da Steam (ponte pro cadeado de "Desbloquear").
  ipcMain.handle("achievements:schemas:load", async () => {
    try {
      const { loadAllSchemas } = require("./achievements/loader")
      return { ok: true, ...loadAllSchemas() }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle("game:launch", async (_e, payload) => {
    // Aceita { cmd, gameId } (novo) ou o array cmd direto (legado).
    let rawCmd = Array.isArray(payload) ? payload : payload?.cmd
    const gameId = Array.isArray(payload) ? undefined : payload?.gameId
    // Modo explícito (menu Steam vs fora-da-Steam): "steam" força o launch_cmd
    // da loja; "exe" força o executável do prefixo wine. Sem modo: decide sozinho
    // (exePath vence quando existe).
    const mode = Array.isArray(payload) ? undefined : payload?.mode
    // SEGURANÇA (auditoria A-04): o cmd vindo do renderer NUNCA é executado
    // como veio — um XSS executaria binário arbitrário. O comando é resolvido
    // AQUI no main a partir dos dados locais (library.json/loja). Única exceção
    // pro legado: o atalho steam://install|run/<appid> (padrão fixo).
    if (typeof gameId === "string" && gameId) {
      const g = readLibrary().find((x) => x.id === gameId)
      if (g && Array.isArray(g.launch_cmd) && g.launch_cmd.length) rawCmd = g.launch_cmd
    } else if (Array.isArray(rawCmd)) {
      const legado = rawCmd.map((c) => String(c))
      if (!(legado.length === 2 && legado[0] === "steam" && /^steam:\/\/(install|run)\/[0-9]+$/.test(legado[1]))) {
        return { ok: false, error: "Comando de lançamento rejeitado (padrão não permitido)." }
      }
    }
    // Jogo adicionado manualmente: monta o comando na hora (wine + exe).
    let envExtra = {}
    if (typeof gameId === "string" && gameId.startsWith("custom:")) {
      const built = customLaunchCmd(gameId)
      if (!built) {
        return {
          ok: false,
          error: `Jogo custom não encontrado em custom_games.json (id: ${gameId}).`,
        }
      }
      rawCmd = built.cmd
      envExtra = built.env || {}
    } else if (typeof gameId === "string" && mode !== "steam") {
      // Override "Executável" (aba Localizações): roda o exe escolhido em vez do
      // launch_cmd padrão da loja. Sem modo, só quando há exePath configurado.
      const exe = getGameSettings(gameId).exePath
      if (exe) {
        const built = exeLaunchCmd(gameId, exe)
        if (!built) {
          return { ok: false, error: "Executável configurado não encontrado (exePath vazio)." }
        }
        if (built?.cmd?.length) {
          rawCmd = built.cmd
          envExtra = built.env || {}
        }
      }
    }
    if (!Array.isArray(rawCmd) || rawCmd.length === 0) {
      return {
        ok: false,
        error:
          "Sem comando de lançamento (cmd vazio). Verifique o executável do jogo em Configurações.",
      }
    }
    // Antes do applyGameSettings, que pode embrulhar tudo no gamescope — daí
    // em diante o cmd[0] já não é mais o binário da Steam.
    rawCmd = steamSilencioso(rawCmd)
    const sls = steamComInjecao(rawCmd)
    rawCmd = sls.cmd
    try {
      // Aplica as configurações do jogo (env vars, prefixo, gamescope).
      const s = getGameSettings(gameId)
      const { cmd, env: envBase, warnings } = applyGameSettings(rawCmd, s, gameId)
      // O env da SLSsteam entra DEPOIS: applyGameSettings monta o ambiente a
      // partir do process.env e apagaria o LD_AUDIT.
      const env = { ...envBase, ...envExtra, ...sls.env }
      // Quando o alvo é a Steam (puro OU wrapper do slsteam-moon), tira as
      // libs herdadas do Electron do caminho — Chromium arrasta LD_LIBRARY_PATH
      // apontando pras suas próprias libstdc++/libcurl, e a Steam linka nelas
      // e cai no 0x3008 no startup do transporte. LD_AUDIT NÃO se apaga: o
      // fallback sem wrapper depende dele, e o wrapper reescreve o dele
      // sozinho.
      if (path.basename(String(rawCmd[0])) === "steam") {
        delete env.LD_LIBRARY_PATH
        delete env.LD_PRELOAD
        delete env.STEAM_RUNTIME_LIBRARY_PATH
      }
      warnings.push(...sls.avisos)
      for (const w of warnings) console.warn("arcadia:", w)
      if (warnings.length && win && !win.isDestroyed()) {
        win.webContents.send("game:launchWarning", { gameId, warnings })
      }
      // Jogo custom: SEMPRE roda no prefixo dele (padrão ou customizado) —
      // sem isso caía no ~/.wine do sistema e o jogo não abria.
      if (typeof gameId === "string" && gameId.startsWith("custom:")) {
        env.WINEPREFIX = env.WINEPREFIX || s.prefixPath || defaultPrefix(gameId)
      }

      // Log SEMPRE ligado: stdout/stderr do jogo em logs/<id>.log (append com
      // rotação simples). verboseLogs só controla DXVK_HUD/WINEDEBUG — falha
      // de log não bloqueia o launch.
      let stdio = "ignore"
      try {
        fs.mkdirSync(LOG_DIR, { recursive: true })
        const logPath = path.join(
          LOG_DIR,
          `${String(gameId || "jogo").replace(/[^a-z0-9._-]/gi, "_")}.log`,
        )
        // Rotação simples: se >5MB, renomeia pra .old (sobrescreve .old anterior)
        try {
          const st = fs.statSync(logPath)
          if (st.size > 5 * 1024 * 1024) fs.renameSync(logPath, logPath + ".old")
        } catch {}
        const fd = fs.openSync(logPath, "a")
        fs.writeSync(fd, `\n\n=== ${new Date().toISOString()} launch: ${JSON.stringify(cmd)} ===\n`)
        stdio = ["ignore", fd, fd]
      } catch (e) {
        console.warn("arcadia: log fd falhou:", e.message)
        // segue com "ignore" — não bloqueia launch por falha de log
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

      // Valida binários ANTES de qualquer spawn (steam URI ou direto).
      const binErro = validarBinariosLaunch(cmd, gameId)
      if (binErro) {
        if (win && !win.isDestroyed()) {
          win.webContents.send("game:launchError", { gameId, error: binErro })
        }
        return { ok: false, error: binErro }
      }

      const soltar = (c) => {
        const child = spawn(c[0], c.slice(1), { detached: true, stdio, env })
        child.on("error", (err) => {
          console.warn("arcadia: spawn erro:", err.message)
          if (win && !win.isDestroyed()) {
            win.webContents.send("game:launchError", {
              gameId,
              error: `spawn falhou: ${err.message}`,
            })
          }
        })
        // unref DEPOIS do listener registrado
        child.unref()
        // Registra o grupo de processos do jogo (o spawn detached vira líder).
        // launcher sai da biblioteca (o payload do launch só traz gameId).
        const lib = gameId ? readLibrary().find((x) => x.id === gameId) : null
        jogoAtivo = {
          pid: child.pid,
          alvo: c[c.length - 1],
          gameId: gameId || "",
          launcher: lib?.launcher || "",
          startedAt: Date.now(),
        }
        ultimoJogoAtivo = jogoAtivo
        armarPollJogo()
      }
      // Steam: se estiver em Big Picture, sai dele ANTES de abrir o jogo —
      // senão o steam://rungameid herda o modo BPM em vez da Steam normal.
      // MAS só manda o exitbigpicture se a Steam JÁ estiver rodando: com ela
      // fechada, esse URI inicia a Steam EM Big Picture (efeito colateral).
      if (
        path.basename(String(cmd[0])) === "steam" &&
        typeof cmd[1] === "string" &&
        cmd[1].startsWith("steam://")
      ) {
        const run = () => soltar(cmd)
        execFile("pgrep", ["-x", "steam"], (err) => {
          if (!err) {
            // Steam rodando: sai do BPM e lança.
            try {
              const bp = spawn(cmd[0], ["steam://exitbigpicture"], {
                detached: true,
                stdio: "ignore",
                env,
              })
              bp.unref()
            } catch {}
            setTimeout(run, 900)
            return
          }
          // Steam FECHADA: abre o cliente (mesmo binário/env de `cmd`/`env` —
          // com injeção SLSsteam quando aplicável; usar o "steam" do PATH
          // aqui reintroduziria a Steam pura, ver comentário de steamComInjecao),
          // espera subir, garante saída do BPM (ela pode restaurar a sessão
          // anterior em BPM — principalmente no gamescope) e só então lança o jogo.
          try {
            const st = spawn(cmd[0], [], { detached: true, stdio: "ignore", env })
            st.unref()
          } catch {}
          let tentativas = 0
          const esperar = setInterval(() => {
            execFile("pgrep", ["-x", "steam"], (e2) => {
              if (!e2) {
                clearInterval(esperar)
                setTimeout(() => {
                  try {
                    const bp = spawn(cmd[0], ["steam://exitbigpicture"], {
                      detached: true,
                      stdio: "ignore",
                      env,
                    })
                    bp.unref()
                  } catch {}
                  setTimeout(run, 1200)
                }, 3000) // cliente subiu: espera a UI estabilizar
              } else if (++tentativas > 30) {
                clearInterval(esperar) // ~60s sem sinal: desiste
                if (win && !win.isDestroyed()) {
                  win.webContents.send("game:launchError", {
                    gameId,
                    error: "Steam não iniciou em 60s.",
                  })
                }
                return
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
        try {
          process.kill(-pid, "SIGTERM")
        } catch {}
        setTimeout(() => {
          try {
            process.kill(-pid, "SIGKILL")
          } catch {}
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

  // ProtonDB (tier/Steam Deck/score) via API pública. Só faz sentido no Linux.
  ipcMain.handle("game:protondb", async (_e, appid) => {
    try {
      return { ok: true, info: await getProtonDb(appid) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Estatísticas (donos/jogadores) + resumo de reviews via APIs públicas.
  ipcMain.handle("game:stats", async (_e, appid) => {
    try {
      return { ok: true, info: await getGameStats(appid) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Importa uma instalação existente (botão "IMPORTAR JOGO" da página do jogo).
  ipcMain.handle("game:import", async (_e, g) => {
    try {
      const legendary = g?.launch_cmd?.[0] || ""
      if (!/legendary$/.test(legendary))
        return { ok: false, error: "Só jogos Epic (legendary) podem ser importados" }
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
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      if (all.some((g) => g.id === id))
        return { ok: false, error: "já existe um jogo com esse nome" }
      all.push({
        id,
        title,
        launcher: "custom",
        platform: platform === "linux" ? "linux" : "windows",
        exe,
        installed: true,
      })
      fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(all, null, 2))
      // Sincroniza a coleção com a conta (jogos seguem entre máquinas)
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
      return { ok: true, games: readLibrary() }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Edita um jogo custom existente (título/executável). O id é preservado.
  ipcMain.handle("customgame:update", (_e, { id, title, exe } = {}) => {
    try {
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      const g = all.find((x) => x.id === id)
      if (!g) return { ok: false, error: "jogo não encontrado" }
      if (title) g.title = title
      if (exe) g.exe = exe
      fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(all, null, 2))
      // Renomeou → título novo sobe pra conta
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
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
        const child = spawn("steam", [`steam://uninstall/${appid}`], {
          detached: true,
          stdio: "ignore",
        })
        child.unref()
        return { ok: true }
      }
      const legendary = g?.launch_cmd?.[0] || ""
      if (launcher === "custom") {
        // Jogo adicionado manualmente: só sai do custom_games.json.
        const rest = readJsonFile(caminhoConta(CUSTOM_GAMES), []).filter((x) => x.id !== id)
        try {
          fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(rest, null, 2))
        } catch {}
        // Remove da coleção da conta no servidor
        try {
          require("./supabase/biblioteca").agendarPush()
        } catch {}
        limparAposDesinstalar(id, { removePrefix, removeSettings })
        if (win && !win.isDestroyed()) win.webContents.send("library:changed")
        return { ok: true }
      }
      if (launcher === "epic" || /legendary$/.test(legendary)) {
        // Espera o uninstall terminar e reindexa ANTES de responder — assim o
        // refresh do renderer já vê o jogo como não instalado.
        await new Promise((res) => {
          const child = spawn(legendary, ["uninstall", "-y", id.replace(/^epic:/, "")], {
            detached: true,
            stdio: "ignore",
          })
          child.unref()
          child.on("close", res)
          child.on("error", res)
          setTimeout(res, 180000) // desiste de esperar após 3min
        })
        limparAposDesinstalar(id, { removePrefix, removeSettings })
        try {
          dm.cancel(id)
        } catch {} // some da fila de downloads também
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
      setOverride(caminhoConta(OVERRIDES), id, patch)
    } catch (e) {
      /* disco cheio/permissão: segue com o que já havia */
    }
    return readLibrary()
  })

  ipcMain.handle("config:get", () => redigirSegredos(readConfig()))

  // ── Atualização do Arcadia ───────────────────────────────────────────────
  ipcMain.handle("update:state", () => updater.estado())
  ipcMain.handle("update:check", () => updater.verificar())
  ipcMain.handle("update:apply", async (_e, { depsMudaram } = {}) => {
    const janela = BrowserWindow.fromWebContents(_e.sender)
    const r = await updater.aplicar((p) => {
      if (janela && !janela.isDestroyed()) janela.webContents.send("update:progress", p)
    }, Boolean(depsMudaram))
    if (!r.ok) return r
    // Reinício no MESMO modo: o `dist/` acabou de ser refeito e o processo
    // atual ainda tem em memória o front-end antigo. Mesmo esquema do
    // "Big Picture", só que preservando o modo em que já estávamos.
    try {
      const child = spawn(process.execPath, ["."], {
        cwd: path.join(__dirname, ".."),
        detached: true,
        stdio: "ignore",
        env: { ...process.env },
      })
      child.unref()
      setTimeout(() => app.quit(), 500)
    } catch (e) {
      return { ok: true, reiniciou: false, error: String(e) }
    }
    return { ...r, reiniciou: true }
  })

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
    // SEGURANÇA (auditoria A-06): o renderer recebe as chaves MASCARADAS no
    // config:get; se ele devolver a máscara de volta (form inalterado), mantém
    // o valor real no disco.
    const atual = readConfig()
    for (const k of ["steam_api_key", "steamgriddb_api_key", "hubcap_api_key"]) {
      if (typeof cfg?.[k] === "string" && cfg[k].includes("•") && cfg[k] === redigirSegredos(atual)[k]) {
        cfg[k] = atual[k] // preserva a chave real
      }
    }
    // Pasta de prefixos mudou? Cria de verdade (ela não existia antes).
    if (cfg?.default_wine_prefix_path) {
      try {
        fs.mkdirSync(cfg.default_wine_prefix_path, { recursive: true })
      } catch {}
    }
    const idiomaAntes = readConfig().language
    const r = writeConfig(cfg)
    // SEGURANÇA: a resposta NUNCA devolve as chaves em claro (o config:get
    // já é redigido; o set devolvia o config inteiro com hubcap_api_key...).
    if (r?.config) r.config = redigirSegredos(r.config)
    // Trocou de idioma: as descrições e requisitos já baixados estão na língua
    // antiga. Reindexar sozinho é o que faz a biblioteca aparecer traduzida
    // sem o usuário ter de descobrir que existe um botão de atualizar.
    const janela = BrowserWindow.fromWebContents(_e.sender)
    if (cfg?.language && cfg.language !== idiomaAntes) {
      avisarBiblioteca(janela, true)
    }
    if (Object.prototype.hasOwnProperty.call(cfg || {}, "slssteam_path")) {
      janela?.webContents.send("plugins:changed")
    }
    return r
  })

  // Notícias de jogos (RSS PT-BR). Cache alinhado ao RELÓGIO: vale até o
  // próximo marco de 30 min (:00/:30) — não "30 min a partir do fetch".
  // Buscar os 6 feeds custa ~10s (o Promise.all espera o mais lento). Antes,
  // ao virar o slot a aba ficava esse tempo todo em branco. Agora vale
  // stale-while-revalidate: entrega o cache velho na hora e renova por trás.
  // Só a primeira execução da vida (sem cache nenhum) espera de verdade.
  const SLOT_30 = 30 * 60 * 1000
  let newsEmVoo = null

  function lerNewsCache() {
    try {
      const cache = JSON.parse(fs.readFileSync(NEWS_CACHE, "utf-8"))
      if (Array.isArray(cache.items) && cache.items.length) return cache
    } catch {}
    return null
  }

  function renovarNews(slot) {
    if (newsEmVoo) return newsEmVoo
    newsEmVoo = getNews(40)
      .then((items) => {
        if (items.length) {
          try {
            fs.writeFileSync(NEWS_CACHE, JSON.stringify({ slot, items }), "utf-8")
          } catch {}
        }
        return items
      })
      .finally(() => {
        newsEmVoo = null
      })
    return newsEmVoo
  }

  ipcMain.handle("news:get", async () => {
    // O slot é calculado por chamada: fixá-lo na inicialização congelava o
    // cache enquanto o app ficasse aberto.
    const slot = Math.floor(Date.now() / SLOT_30)
    const cache = lerNewsCache()
    if (cache) {
      if (cache.slot !== slot) renovarNews(slot).catch(() => {})
      return cache.items
    }
    try {
      return await renovarNews(slot)
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
        ss.writeAcf({
          appid,
          title: item.title,
          installdir: item.installdir,
          steamDir: item.steamDir,
        })
        if (plugins.isEnabled("slssteam"))
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
  ipcMain.handle("store:status", () => ({
    ...steamstore.status(),
    slssteam: plugins.isEnabled("slssteam"),
    luatools: plugins.isEnabled("luatools-fixes"),
  }))
  ipcMain.handle("store:search", async (_e, query) => {
    try {
      return await steamstore.search(String(query || ""))
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Sugestões: só a lista de títulos da Steam, sem sondar provedores — é o que
  // permite responder a cada tecla sem inundar o Ryuu de requisições.
  // Aquece a conexão com a Steam quando a aba da loja abre — sem isso a
  // primeira tecla digitada pagava o handshake TLS inteiro (~3s).
  ipcMain.handle("store:warm", async () => {
    try {
      return await steamstore.aquecer()
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:suggest", async (_e, query) => {
    try {
      return await steamstore.suggest(String(query || ""))
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // Compatibilidade: aceita string legado ({ lista }) ou objeto novo.
  ipcMain.handle("store:recent", async (_e, arg) => {
    try {
      const { lista, limite, offset } = typeof arg === "string" ? { lista: arg } : arg || {}
      return await steamstore.popular(
        lista ? String(lista) : undefined,
        Number(limite) || 40,
        Number(offset) || 0,
      )
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:installDir", (_e, game) => {
    const p = steamstore.gameInstallDir(game)
    if (p) return { path: p }
    const exe = getGameSettings(game?.id).exePath
    return { path: exe ? path.dirname(exe) : "" }
  })
  ipcMain.handle("store:libraries", () => steamstore.steamLibraries())
  ipcMain.handle("store:removeFromSteam", (_e, appid) => {
    const r = steamstore.removeFromSteam(appid)
    if (r?.ok) avisarBiblioteca(win)
    return r
  })
  ipcMain.handle("store:removeDownloaded", (_e, appid) => {
    const r = steamstore.removeDownloaded(appid)
    // Sem este aviso a aba Lojas continuava mostrando "Na biblioteca" depois de
    // remover: o card se baseia na lista de jogos, que só recarrega neste
    // evento. Todos os outros pontos que mexem na biblioteca já o emitiam.
    if (r?.ok) avisarBiblioteca(win)
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
    if (!plugins.isEnabled("slssteam")) return { ok: false, plugin: "slssteam" }
    try {
      return await dm.installSteam(payload || {})
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  // "Add": adiciona o jogo à Steam sem baixar (estilo luatools-moon). Só
  // funciona com o plugin SLSsteam instalado; sem ele, o catálogo continua puro.
  ipcMain.handle("store:addToSteam", (_e, { appid, token, dlcs, title } = {}) => {
    if (!plugins.isEnabled("slssteam")) return { ok: false, plugin: "slssteam" }
    try {
      const r = steamstore.addToSteam(String(appid || ""))
      if (!r.ok) {
        // Sem .lua o registro na Steam falha, mas o jogo ainda entra na
        // biblioteca — antes o Add morria aqui e o jogo não aparecia em
        // lugar nenhum.
        try {
          adicionarStubPendente(String(appid), title)
        } catch {}
        avisarBiblioteca(win)
        return r
      }
      const reg = steamstore.registerSlssteam({ appid: String(appid), token, dlcs })
      if (!reg?.ok) return reg || { ok: false, error: "falha ao registrar na SLSsteam" }
      try {
        adicionarStubPendente(String(appid), title)
      } catch {}
      avisarBiblioteca(win)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:addToLibrary", async (_e, { appid, title, cover, hero, heroi } = {}) => {
    try {
      appid = String(appid || "")
      const cega = `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`
      if (!cover || cover === cega) {
        try {
          const { mapa } = await steamstore.itensDaLoja([appid])
          const it = mapa.get(appid)
          if (it?.capa) cover = it.capa
          if (it?.heroi && !hero && !heroi) hero = it.heroi
        } catch {}
      }
      // Reexibe: um "Remover" anterior pode ter marcado hidden=true (jogo
      // indexado); sem limpar aqui o Add não trazia o jogo de volta.
      setOverride(caminhoConta(OVERRIDES), "steam:" + appid, { hidden: null })
      adicionarStubPendente(appid, title, { cover, hero: hero || heroi })
      avisarBiblioteca(win)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:removeFromLibrary", (_e, appid) => {
    try {
      const id = "steam:" + String(appid || "")
      const removed = removerStubPendente(String(appid || ""))
      if (!removed && readLibrary().some((g) => g.id === id))
        setOverride(caminhoConta(OVERRIDES), id, { hidden: true })
      avisarBiblioteca(win)
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
  ipcMain.handle("slssteam:launchSteam", () =>
    plugins.isEnabled("slssteam")
      ? steamstore.launchSteamWithSls()
      : { ok: false, plugin: "slssteam" },
  )
  ipcMain.handle("dm:queue", () => dm.getQueue())
  ipcMain.handle("dm:install", (_e, game) => dm.install(game || {}))
  ipcMain.handle("dm:pause", (_e, appid) => dm.pause(appid))
  ipcMain.handle("dm:retry", (_e, appid) => dm.retry(appid))
  ipcMain.handle("dm:dismiss", (_e, appid) => dm.descartar(appid))
  ipcMain.handle("dm:resume", (_e, appid) => dm.resume(appid))
  ipcMain.handle("dm:cancel", (_e, appid) => dm.cancel(appid))

  // --- Wine manager + ferramentas de prefixo --------------------------------
  const wm = require("./winemanager")
  ipcMain.handle("wine:list", () => ({
    installed: require("./winemanager").steamProtons(),
    available: [],
  }))
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

  // Estatísticas do perfil: jogos e horas jogadas (agregado de library.json).
  ipcMain.handle("profile:stats", () => {
    try {
      const lib = readLibrary()
      let playMin = 0
      for (const g of lib) playMin += g.playtime_minutes || 0
      return {
        jogos: lib.length,
        playtime_hours: Math.round(playMin / 60),
      }
    } catch {
      return null
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
      lib = readLibrary()
    } catch {
      return { ok: false, error: "biblioteca não lida" }
    }
    const faltam = lib.filter((g) => !trailerLocal(g.id))
    let feitos = 0
    for (const g of faltam) {
      if (win)
        win.webContents.send("trailer:progress", {
          done: feitos,
          total: faltam.length,
          title: g.title,
        })
      await baixarTrailer(g.id, g.title || "")
      feitos++
    }
    if (win)
      win.webContents.send("trailer:progress", {
        done: feitos,
        total: faltam.length,
        title: "",
      })
    return { ok: true, count: feitos }
  })
  ipcMain.handle("app:quit", () => app.quit())

  // ─── Fixes (crack/bypass/online) — port do luatools-moon ────────────────
  const fixes = require("./fixes")
  // Garante permissão de execução do worker (build/git nem sempre preserva).
  try {
    fs.chmodSync(path.join(__dirname, "fix_downloader.sh"), 0o755)
  } catch {
    /* ok */
  }

  ipcMain.handle("fixes:check", async (_e, appid) => {
    const a = String(appid || "").replace(/^steam:/, "")
    if (!a) return { ok: false }
    const [generic, online, crack] = await Promise.all([
      fixes.checkGenericFix(a).catch(() => ({ available: false, status: 0 })),
      fixes.checkOnlineFix(a).catch(() => ({ available: false, status: 0 })),
      fixes.checkCrackFix(a).catch(() => ({ available: false, status: 0 })),
    ])
    const auth = fixes.getRyuuAuthStatus()
    return { ok: true, appid: a, generic, online, crack, authConfigured: auth.configured }
  })

  ipcMain.handle("fixes:apply", async (_e, { appid, url, type, installPath }) => {
    const a = String(appid || "").replace(/^steam:/, "")
    if (!a || !url || !installPath) return { ok: false, error: "missing_args" }
    return fixes.applyFix({ appid: a, url, type, installPath })
  })

  ipcMain.handle("fixes:status", (_e, appid) => {
    const a = String(appid || "").replace(/^steam:/, "")
    return fixes.getStatus(a)
  })

  ipcMain.handle("fixes:cancel", (_e, appid) => {
    const a = String(appid || "").replace(/^steam:/, "")
    return fixes.cancelApply(a)
  })

  ipcMain.handle("fixes:installed", (_e, { appid, installPath }) => {
    if (!installPath) return { ok: true, installed: false }
    return { ok: true, installed: fixes.isFixed(installPath) }
  })

  ipcMain.handle("fixes:unfix", (_e, { appid, installPath }) => {
    return fixes.unfix(installPath)
  })

  ipcMain.handle("fixes:launcherRedirect", (_e, { installPath }) => {
    const r = fixes.buildLauncherRedirect(installPath)
    return { ok: true, redirect: r }
  })

  ipcMain.handle("fixes:setRyuuAuth", (_e, key) => fixes.setRyuuAuth(key))
  ipcMain.handle("fixes:ryuuAuthStatus", () => fixes.getRyuuAuthStatus())
  ipcMain.handle("fixes:clearRyuuAuth", () => fixes.clearRyuuAuth())

  // Controles de janela (botões estilo macOS na barra custom do modo desktop).
  ipcMain.handle("win:minimize", () => win?.minimize())
  ipcMain.handle("win:maximize", () => {
    if (!win) return false
    if (win.isMaximized()) win.unmaximize()
    else win.maximize()
    return win.isMaximized()
  })
  ipcMain.handle("win:close", () => win?.close())
  ipcMain.handle("app:toggleFullscreen", () => {
    if (win) win.setFullScreen(!win.isFullScreen())
  })
  ipcMain.handle("app:setFullscreen", (_e, on) => {
    if (win) win.setFullScreen(Boolean(on))
  })
  ipcMain.handle("app:setZoom", (_e, z) => {
    const factor = Math.min(2, Math.max(0.7, Number(z) || 1))
    if (win) win.webContents.setZoomFactor(factor)
    return factor
  })

  ipcMain.handle("library:refresh", async () => {
    await runIndexer()
    return curarCapasSteam(readLibrary())
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

  // HowLongToBeat: tempos de jogo (falha silenciosa, sem linha na UI).
  ipcMain.handle("hltb:get", async (_e, titulo) => {
    try {
      return await require("./hltb").hltbBuscar(titulo)
    } catch {
      return null
    }
  })

  // Status das integrações para a aba Integrações.
  ipcMain.handle("integrations:status", () => {
    const cfg = readConfig()
    return {
      steam: Boolean(cfg.steam_api_key),
      slssteam: plugins.isEnabled("slssteam") ? slssteamCount() : 0,
      heroic: heroicConnected(),
    }
  })

  // --- Fontes de download (JSONs estilo Hydra, 100% locais) ----------------
  const sources = require("./sources")
  ipcMain.handle("sources:list", () => ({ ok: true, sources: sources.list() }))
  ipcMain.handle("sources:add", (_e, url) => sources.addSource(url))
  ipcMain.handle("sources:remove", (_e, id) => sources.removeSource(id))
  ipcMain.handle("sources:sync", () => sources.syncSources())
  ipcMain.handle("sources:search", (_e, { query, limit } = {}) => ({
    ok: true,
    results: sources.search(query, Number(limit) || 40),
  }))
  ipcMain.handle("sources:game", (_e, ref) => sources.getGame(ref))

  // --- Torrent (worker Python + libtorrent; ver electron/torrent.js) -------
  const torrent = require("./torrent")
  torrent.onProgress((items) => {
    if (win && !win.isDestroyed()) win.webContents.send("torrent:progress", items)
  })
  // Retoma downloads que estavam ativos quando o app fechou.
  torrent.retomar().catch(() => {})
  ipcMain.handle("torrent:start", (_e, payload) => torrent.start(payload || {}))
  ipcMain.handle("torrent:pause", (_e, gameId) => torrent.pause(gameId))
  ipcMain.handle("torrent:resume", (_e, gameId) => torrent.resume(gameId))
  ipcMain.handle("torrent:cancel", (_e, gameId) => torrent.cancel(gameId))
  ipcMain.handle("torrent:files", (_e, { magnet, timeoutMs } = {}) =>
    torrent.files(magnet, timeoutMs),
  )
  ipcMain.handle("torrent:setLimit", (_e, bytes) => torrent.setLimit(bytes))
  ipcMain.handle("torrent:list", () => ({ ok: true, downloads: torrent.list() }))

  ipcMain.handle("plugins:list", () => ({ ok: true, plugins: plugins.list() }))
  ipcMain.handle("plugins:install", async (_e, id) => {
    const r = await plugins.install(String(id || ""))
    if (r?.ok && win && !win.isDestroyed()) win.webContents.send("plugins:changed")
    return r
  })
  ipcMain.handle("plugins:remove", async (_e, id) => {
    const r = await plugins.remove(String(id || ""))
    if (r?.ok && win && !win.isDestroyed()) win.webContents.send("plugins:changed")
    return r
  })

  // Escolher imagem (avatar ou plano de fundo) — aceita GIF animado.
  // Procura arte online para um jogo. Junta o que cada fonte achou numa lista
  // só; se uma fonte falhar (chave errada, rede caída), as outras seguem.
  ipcMain.handle("meta:art", async (_e, { gameId, titulo, kind, sgdbId, dimensions } = {}) => {
    if (!gameId || !SGDB_ENDPOINT[kind]) return { ok: false, error: "pedido inválido" }
    const cfg = readConfig()
    const chave = String(cfg.steamgriddb_api_key || "").trim()
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
    try {
      candidatos.push(...igdbArtDe(await igdbProxy(titulo || ""), kind))
    } catch (e) {
      erros.push(`IGDB: ${e.message}`)
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
  })

  // Descrições candidatas. A ordem é a ordem da qualidade: primeiro as fontes
  // que traduzem (Steam e Xbox), depois a que só fala inglês (IGDB).
  ipcMain.handle("meta:text", async (_e, { gameId, titulo } = {}) => {
    const cfg = readConfig()
    const textos = []
    const erros = []

    try {
      // Jogo de fora da Steam (Epic, GOG, custom) não tem appid no id, mas
      // quase sempre TEM página na Steam. O `suggest` já faz a busca por
      // título, ranqueia e cacheia — a mesma que a loja usa.
      //
      // O `tituloBate` não é zelo excessivo: a busca da Steam é fuzzy e sempre
      // devolve ALGO. "Astro Bot", que é exclusivo de PlayStation, volta como
      // "Sackboy — Disfraz de ASTRO BOT" — sem a peneira, a descrição de uma
      // fantasia de DLC apareceria etiquetada como a do jogo.
      let appid = ""
      if (!/^steam:/.test(String(gameId || "")) && titulo) {
        const s = await require("./steamstore").suggest(titulo)
        const achado = (s?.jogos || []).find((j) => tituloBate(j.title, titulo))
        appid = achado?.appid || ""
      }
      textos.push(...(await steamTextos(gameId, steamLang(), appid)))
    } catch (e) {
      erros.push(`Steam: ${e.message}`)
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

    // IGDB por último: cobre o que não está em loja de PC nenhuma, mas o texto
    // vem sempre em inglês e o servidor não é nosso — se falhar, vira só mais
    // um erro na lista e as outras fontes seguem valendo.
    try {
      textos.push(...igdbTextosDe(await igdbProxy(titulo || "")))
    } catch (e) {
      erros.push(`IGDB: ${e.message}`)
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
      const velha = artToDelete(readOverrides(caminhoConta(OVERRIDES))[id]?.[kind], ART_DIR, path.sep)
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
              {
                name: "Imagens e vídeos",
                extensions: ["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"],
              },
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
    const velha = artToDelete(readOverrides(caminhoConta(OVERRIDES))[id]?.[kind], ART_DIR, path.sep)
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
              {
                name: "Imagens e vídeos",
                extensions: ["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4", "m4v", "mov"],
              },
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

  createWindow()

  // Popula apiname nos itens via UserGameStatsSchema_*.bin (faz a ponte pro
  // cadeado de "Desbloquear" funcionar). Se não tem schema, é no-op.
  try {
    require("./achievements/loader").loadAllSchemas()
  } catch {}

  // Links externos abertos pela página da loja Steam (Community Hub, publisher,
  // etc.): manda pro navegador do sistema em vez de abrir janela presa dentro
  // do app. Só afeta contents do tipo webview (a página de terceiros).
  app.on("web-contents-created", (_e, contents) => {
    if (contents.getType() !== "webview") return
    contents.setWindowOpenHandler(({ url }) => {
      if (/^https?:\/\//.test(url)) shell.openExternal(url)
      return { action: "deny" }
    })
  })

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
  // Marca que o app está saindo — o handler de render-process-gone não
  // tenta recarregar a janela durante o shutdown.
  app.isQuitting = true
  try {
    require("./downloadmanager").killActive()
  } catch {}
  // Derruba o toast de conquista para não deixar janela always-on-top órfã.
  try {
    closeAchievementToast()
  } catch {}
  // Garante que metadados baixados na sessão atual não se percam se o app for
  // fechado antes do debounce de 1500ms do sysinfo cache.
  try {
    if (_sysinfoCache) fs.writeFileSync(SYSINFO_CACHE, JSON.stringify(_sysinfoCache))
  } catch {}
})
