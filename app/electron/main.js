const { app, BrowserWindow, ipcMain, dialog, shell, session, screen } = require("electron")
const { resolveLauncherMode, ignoreBrokenPipe } = require("./startup")

ignoreBrokenPipe(process.stdout)
ignoreBrokenPipe(process.stderr)

// ── INSTÂNCIA ÚNICA ───────────────────────────────────────────────────
// Um segundo lançamento (atalho, arcadia.sh, loja) só FOCA a janela que já
// está aberta. Sem este lock, cada execução abria outra cópia: RAM dobra
// (~900MB por cópia) e duas instâncias gravando o mesmo config.json/estado
// corrompem dados. O handler referencia a global `win` (declarada mais
// abaixo) — só executa quando o evento dispara, já com win inicializada.
const lockUnico = app.requestSingleInstanceLock()
if (!lockUnico) {
  app.quit()
} else {
  app.on("second-instance", () => {
    if (win && !win.isDestroyed()) {
      if (win.isMinimized()) win.restore()
      win.show()
      win.focus()
    }
  })
}

const { startAchievementWatcher, fetchAchievementsForApp } = require("./achievements")
const { iniciarVigia } = require("./achievements/cracked_watcher")
const { prepareUplayInstallation } = require("./achievements/uplay")
const { getNews } = require("./news")
const plugins = require("./plugins")
const updater = require("./updater")
const { showAchievementToast, closeAchievementToast } = require("./notify")
const path = require("path")
const fs = require("fs")
const os = require("os")
const { getDataDir } = require("./runtime-paths")
const { normalizeLibrary } = require("../../contracts")
const { createLibraryRepository } = require("./library-repository")
const { resolveLaunchRequest } = require("./launch-resolver")
const { createLaunchLog } = require("./launch-log")
const { createSnapshotService } = require("./snapshot-service")
const { createDiagnosticsService } = require("./diagnostics")
const { createSupportBundle } = require("./support-bundle")
const { createGameSettingsService } = require("./game-settings-service")
const { createEmulatorRegistry } = require("./emulator-registry")
const { getEmulatorStatus, preflightEmulator } = require("./emulator-status")
const { getRunningEmulatorStatus, preflightRunningEmulator } = require("./emulator-runtime")
const raClient = require("./retroachievements/client")
const raEmulatorConfig = require("./retroachievements/emulator-config")
const { getRetroachievementsConsoleId, getSystem } = require("./retro-systems")
const { spawn, spawnSync, execFile, execFileSync } = require("child_process")
const { restoreWindowFocus } = require("./window-focus")
const {
  buildExternalGamescopeCommand,
  canUseSystemdSession,
  createSystemdUnitName,
  parseSystemdShow,
  readCgroupPids,
  systemdStopArgs,
} = require("./gamescope-session")
const { createFocusSession, createLaunchSession } = require("./focus-session")
const { shouldTrackGameSession } = require("./game-session")
const { fetchRede } = require("./httpfetch")
const { findUmuLauncher, ensureUmuLauncher } = require("./umu-runtime")
const { createSteamNewsImageResolver, extractSteamNewsImage } = require("./steam-news")
const DiscordRpc = require("./discord-rpc")
const { catalogGet } = require("./catalog")
// Escopo por conta dos arquivos locais — PRECISA estar no escopo do módulo
// (readLibrary e outros helpers rodam fora do whenReady; require dentro de
// bloco deixava "caminhoConta is not defined" → biblioteca vazia).
const { caminhoConta, definirConta, conta } = require("./supabase/conta")
const { readOverrides, setOverride, applyOverrides, artToDelete } = require("./overrides")
const { filtrarPorPosse, OWNED_GAMES, ownedAdd, ownedRemove } = require("./owned")
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
const DATA_DIR = getDataDir()
const BOOT_VIDEO = path.join(DATA_DIR, "boot.mp4")
const BUNDLED_BOOT_VIDEO = app.isPackaged
  ? path.join(process.resourcesPath, "boot.mp4")
  : path.join(__dirname, "..", "..", "boot.mp4")
try {
  if (!fs.existsSync(BOOT_VIDEO) && fs.existsSync(BUNDLED_BOOT_VIDEO)) {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    fs.copyFileSync(BUNDLED_BOOT_VIDEO, BOOT_VIDEO, fs.constants.COPYFILE_EXCL)
  }
} catch (error) {
  console.warn(`[arcadia:boot] não foi possível instalar o vídeo: ${error.message || error}`)
}
const LIB = path.join(DATA_DIR, "library.json")
// O repositório concentra leitura/filtro por conta, sem alterar o restante da
// montagem (custom/pending/overrides) nem o contrato IPC de library:get.
const libraryRepository = createLibraryRepository({ dataDir: DATA_DIR, libraryPath: LIB })

const saveSnapshots = createSnapshotService({ snapshotsDir: path.join(DATA_DIR, "snapshots") })

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

// Padrão que casa o PROCESSO de um jogo rodando (Steam/Proton/Heroic/Lutris
// e os emuladores suportados). O poll só fica ativo enquanto lançamos um jogo.
// Usado somente pelo vigia read-only de "game:running".  Stop nunca usa
// um padrão global para decidir qual processo deve encerrar.
const PADRAO_JOGO = "steamapps/common/|steamapps/compatdata/|Heroic/Prefixes|lutris/runners|pcsx2|PCSX2|rpcs3|RPCS3|dolphin-emu|DolphinEmu|ppsspp|PPSSPP|duckstation|DuckStation|retroarch|RetroArch|melonds|melonDS|desmume|DeSmuME"
// Alguns launchers Windows (por exemplo, o Plutonium) fecham o executável
// inicial depois de criar o jogo real e o servidor. Nesse caso o grupo Unix
// que o Arcadia criou deixa de existir, mas a sessão ainda está viva. O
// rastreador abaixo usa o prefixo Wine da própria sessão e os executáveis
// observados nela; nunca faz uma busca global por nome nem interfere em outra
// conta/prefixo.
const HANDOFF_INFRA_EXES = new Set([
  "explorer.exe",
  "services.exe",
  "svchost.exe",
  "winedevice.exe",
  "plugplay.exe",
  "rpcss.exe",
  "conhost.exe",
  "start.exe",
  "wineboot.exe",
  "winecfg.exe",
  "xalia.exe",
])
const HANDOFF_GAME_EXES = new Set([
  "t6mp.exe",
  "t6zm.exe",
  "t6sp.exe",
  "iw5mp.exe",
  "iw5sp.exe",
])
// Logs de lançamento ("Habilitar logs detalhados", aba AVANÇADO).
const LOG_DIR = path.join(DATA_DIR, "logs")
const launchLog = createLaunchLog({ logDir: LOG_DIR })
const diagnostics = createDiagnosticsService({
  dataDir: DATA_DIR,
  appVersion: app.getVersion(),
  getQueue: () => require("./downloadmanager").getQueue(),
  getLibrary: () => readLibrary(),
})
const supportBundle = createSupportBundle({ dataDir: DATA_DIR })
// Script pós-jogo pendente (aba AVANÇADO): roda quando o jogo fechar.
let postGameScript = ""
// Jogo lançado por nós: { pid (líder do grupo), alvo }. O grupo de processos
// é o que fecha/vigia de forma universal (custom, umu, legendary, lutris).
let jogoAtivo = null
// Snapshot da sessão encerrada: o interval limpa jogoAtivo antes do marcar
// fechar a sessão, então o registro de playtime local se ancora aqui.
let ultimoJogoAtivo = null
// Depois de game:close, mantemos o PID separado até o grupo realmente sumir;
// zerar jogoAtivo cedo demais faria o gamescope finalizar por um falso negativo.
let jogoEncerrando = null
// Interval do poll de jogo. Se a janela for recriada sem matar o processo
// (comum no macOS ou em reinicializações), evita acumular timers antigos.
let runningGameInterval = null
// Foco real da janela (no gamescope o Chromium acha que está focado mesmo
// com o jogo por cima) — o renderer trava gamepad/trailer com isso.
let focado = true
// `win` fica no escopo do módulo porque os callbacks do poll sobrevivem à
// janela e precisam levantá-la quando o jogo termina.
let win

// O layout do Big Picture e do Desktop usa 1920x1080 como referência lógica.
// Em resoluções maiores, manter o mesmo fator de zoom deixa a interface
// fisicamente pequena. A escala usa a menor dimensão para não estourar a altura
// em ultrawide. Janelas Desktop não maximizadas usam o próprio tamanho, então
// abrir o app pequeno num monitor 4K não transforma os controles em gigantes.
function responsiveWindowScale(mode) {
  try {
    const bounds = win && !win.isDestroyed() ? win.getBounds() : null
    const display = bounds ? screen.getDisplayMatching(bounds) : screen.getPrimaryDisplay()
    const displaySize = mode === "console"
      ? display?.size
      : win?.isMaximized()
        ? display?.workAreaSize
        : bounds
    const width = Number(displaySize?.width) || 1920
    const height = Number(displaySize?.height) || 1080
    const ratio = Math.min(width / 1920, height / 1080)
    return Math.min(1.55, Math.max(1, ratio))
  } catch {
    return 1
  }
}

function zoomFactorFor(mode, logical) {
  const value = Number(logical) || (mode === "console" ? 1.3 : 1)
  const base = mode === "console" ? value : value * 1.2
  return Math.min(2, Math.max(mode === "console" ? 0.7 : 0.84, base * responsiveWindowScale(mode)))
}

let appliedZoomFactor = null

function applyWindowZoom(factor) {
  if (!win || win.isDestroyed() || !win.webContents) return
  if (appliedZoomFactor !== null && Math.abs(appliedZoomFactor - factor) < 0.005) return
  appliedZoomFactor = factor
  win.webContents.setZoomFactor(factor)
}

function reapplyWindowZoom() {
  if (!win || win.isDestroyed() || !win.webContents) return
  const mode = win.isFullScreen() ? "console" : "desktop"
  const config = readConfig()
  const logical = Number(config[mode === "console" ? "console_ui_scale" : "ui_scale"]) || (mode === "console" ? 1.3 : 1)
  applyWindowZoom(zoomFactorFor(mode, logical))
}
// Uma sessão pode gerar vários sinais "jogo ausente". A restauração deve ser
// feita uma vez só, no desarme do poll, para não roubar foco repetidamente.
let focoRestauradoSessao = false
// O timer é cancelado quando o jogo termina antes dos 2s ou quando o launch
// falha; sem isso a janela podia ser minimizada depois de um erro.
let minimizarTimer = null
// Estado de lançamento: cobre a janela entre o IPC inicial e o momento em que
// Steam/um wrapper finalmente cria o processo acompanhado.  O token permanece
// ocupado durante `stopping`, até a saída ser confirmada, para que callbacks
// atrasados nunca iniciem um jogo depois de Stop.
let launchInFlight = false
let lancamentoAtual = null

function estadoLancamento() {
  const snapshot = launchLifecycle.getSnapshot()
  const state = snapshot.state
  const gameId = state === "idle"
    ? ""
    : lancamentoAtual?.gameId || snapshot.meta?.gameId || jogoAtivo?.gameId || jogoEncerrando?.gameId || ultimoJogoAtivo?.gameId || ""
  return { state, gameId, token: snapshot.token?.id || null }
}

function emitirEstadoLancamento(state, token, meta = {}) {
  if (!win || win.isDestroyed()) return
  const record = lancamentoAtual && lancamentoAtual.token === token ? lancamentoAtual : null
  const gameId = state === "idle"
    ? ""
    : record?.gameId || meta?.gameId || jogoAtivo?.gameId || jogoEncerrando?.gameId || ""
  win.webContents.send("game:launchState", {
    state,
    gameId,
    token: token?.id || null,
  })
}

const launchLifecycle = createLaunchSession({
  onState: (state, token, meta) => emitirEstadoLancamento(state, token, meta),
})

function emitirFoco(ativo, forcar = false) {
  const valor = Boolean(ativo)
  const mudou = valor !== focado
  focado = valor
  if ((mudou || forcar) && win && !win.isDestroyed()) {
    win.webContents.send("app:focus", valor)
  }
}

const sessaoDeFoco = createFocusSession({
  onFocus: (ativo, forcar) => emitirFoco(ativo, forcar),
})

function iniciarSessaoDeFoco() {
  focoRestauradoSessao = false
  return sessaoDeFoco.begin()
}

function focoNativo(ativo) {
  return sessaoDeFoco.nativeFocus(ativo)
}

function restaurarFocoArcadia(canRestore) {
  if (focoRestauradoSessao) return false
  const restaurada = restoreWindowFocus(win, {
    canRestore,
    onFocused: () => sessaoDeFoco.finish(),
  })
  if (restaurada) focoRestauradoSessao = true
  // Sem uma BrowserWindow válida não há callback do adaptador, mas a sessão
  // ainda precisa sair do estado ativo para não bloquear o próximo launch.
  // Não marque como restaurada quando `canRestore` recusou: uma sessão nova
  // pode confirmar a saída e tentar novamente.
  let janelaIndisponivel = !win
  try {
    janelaIndisponivel = janelaIndisponivel || Boolean(win?.isDestroyed?.())
  } catch {
    janelaIndisponivel = true
  }
  if (!restaurada && janelaIndisponivel) {
    focoRestauradoSessao = true
    sessaoDeFoco.finish()
  }
  return restaurada
}

function launchIsCurrent(record) {
  return Boolean(record && lancamentoAtual === record && launchLifecycle.owns(record.token))
}

function clearLaunchTimers(record) {
  if (!record) return
  for (const key of ["preScriptTimer", "steamPollTimer", "steamRunTimer", "steamWaitTimer"]) {
    const timer = record[key]
    if (!timer) continue
    if (key === "steamPollTimer") clearInterval(timer)
    else clearTimeout(timer)
    record[key] = null
  }
  if (minimizarTimer && record.minimizeTimer === minimizarTimer) {
    clearTimeout(minimizarTimer)
    minimizarTimer = null
    record.minimizeTimer = null
  }
}

function launchChildExited(child) {
  return Boolean(child && (child.__arcadiaExited || child.exitCode != null || child.signalCode != null))
}

function stopLaunchChild(record, child, timerKey = "childKillTimer") {
  if (!child || launchChildExited(child)) return false
  try {
    child.kill?.("SIGTERM")
  } catch {}
  if (record && !record[timerKey]) {
    record[timerKey] = setTimeout(() => {
      record[timerKey] = null
      if (!launchChildExited(child)) {
        try {
          child.kill?.("SIGKILL")
        } catch {}
      }
    }, 2000)
  }
  return true
}

function releaseLaunch(record) {
  if (!launchIsCurrent(record)) return false
  clearLaunchTimers(record)
  if (record.preScriptKillTimer) {
    clearTimeout(record.preScriptKillTimer)
    record.preScriptKillTimer = null
  }
  if (record.childKillTimer) {
    clearTimeout(record.childKillTimer)
    record.childKillTimer = null
  }
  if (record.steamStarterKillTimer) {
    clearTimeout(record.steamStarterKillTimer)
    record.steamStarterKillTimer = null
  }
  const gameRecord = record.gameRecord
  if (gameRecord?.groupKillTimer) {
    clearTimeout(gameRecord.groupKillTimer)
    gameRecord.groupKillTimer = null
  }
  clearProcessSessionWatch(gameRecord)
  try {
    record.closeLog?.()
  } catch {}
  record.closeLog = null
  launchLifecycle.finish(record.token)
  lancamentoAtual = null
  launchInFlight = false
  return true
}

// A detached group is the reliable source for games launched by Arcadia.  The
// group is only ours while its leader is still the exact ChildProcess that we
// spawned.  In particular, never signal a recycled PID after the child exited.
function readProcessIdentity(pid) {
  if (process.platform !== "linux" || !Number.isInteger(pid) || pid <= 1) return null
  try {
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8")
    const close = stat.lastIndexOf(")")
    if (close < 0) return null
    const fields = stat.slice(close + 2).trim().split(/\s+/)
    // fields starts at /proc stat field 3: state. pgrp=field 5, starttime=22.
    const pgrp = Number(fields[2])
    const starttime = fields[19]
    if (!fields[0] || !Number.isFinite(pgrp) || !/^\d+$/.test(String(starttime || ""))) return null
    return { pid, state: fields[0], pgrp, starttime }
  } catch {
    return null
  }
}

function processCommandLine(pid) {
  if (process.platform !== "linux" || !Number.isInteger(pid) || pid <= 1) return ""
  try {
    return fs.readFileSync(`/proc/${pid}/cmdline`, "utf8").replace(/\u0000/g, " ").trim()
  } catch {
    return ""
  }
}

function processEnvironment(pid) {
  if (process.platform !== "linux" || !Number.isInteger(pid) || pid <= 1) return {}
  try {
    const out = {}
    for (const entry of fs.readFileSync(`/proc/${pid}/environ`, "utf8").split("\u0000")) {
      const at = entry.indexOf("=")
      if (at > 0) out[entry.slice(0, at)] = entry.slice(at + 1)
    }
    return out
  } catch {
    return {}
  }
}

function executableNames(command) {
  return Array.from(new Set(
    (command || [])
      .map((value) => path.basename(String(value || "")).toLowerCase())
      .filter((value) => value.endsWith(".exe")),
  ))
}

function createHandoffSpec(command, env, startedAt = Date.now()) {
  if (process.platform !== "linux") return null
  const prefixes = Array.from(new Set(
    [env?.WINEPREFIX, env?.STEAM_COMPAT_DATA_PATH]
      .map((value) => String(value || "").trim())
      .filter((value) => value.startsWith("/") && value.length > 1),
  ))
  const names = executableNames(command)
  if (!prefixes.length || !names.length) return null
  return {
    prefixes,
    names,
    seen: new Map(),
    // A launcher may disappear a few seconds before its child window exists.
    // Keep that handoff window bounded so a genuinely failed launch still
    // returns focus instead of leaving the session stuck forever.
    graceUntil: Number(startedAt) + 30_000,
  }
}

function processBelongsToHandoff(pid, spec, record) {
  const cmdline = processCommandLine(pid)
  if (!cmdline) return null
  const env = processEnvironment(pid)
  const identity = readProcessIdentity(pid)
  if (!identity || /[ZX]/.test(String(identity.state || ""))) return null
  const prefix = spec.prefixes.find((value) =>
    env.WINEPREFIX === value || env.STEAM_COMPAT_DATA_PATH === value,
  )
  // Alguns processos Windows criados pelo wineserver não expõem mais o
  // ambiente Linux original. Se ainda estão no grupo líder que o Arcadia
  // abriu, a identidade do grupo é prova suficiente para registrá-los.
  const tokens = cmdline.split(/\s+/)
  const name = tokens
    .map((token) => path.basename(token).toLowerCase())
    // Depois do handoff o jogo pode ter outro nome (t6mp.exe/t6zm.exe).
    // Qualquer executável Windows dentro do prefixo exclusivo da sessão é
    // elegível, exceto os helpers permanentes do Wine.
    .find((token) => token.endsWith(".exe") && !HANDOFF_INFRA_EXES.has(token))
  if (!name) return null
  // Alguns builds do Wine limpam WINEPREFIX ao reparentar o processo e também
  // trocam o grupo Unix. Para os executáveis conhecidos do Plutonium, o nome e
  // um starttime posterior ao líder ainda formam uma identificação segura o
  // bastante para manter a sessão sem restaurar o foco por engano.
  const knownHandoffGame = HANDOFF_GAME_EXES.has(name)
  const leaderStart = Number(record?.processIdentity?.starttime)
  const childStartedAfterLeader = Number.isFinite(leaderStart)
    ? Number(identity.starttime) >= leaderStart
    : false
  if (!prefix && Number(identity.pgrp) !== Number(record?.pid) && !(knownHandoffGame && childStartedAfterLeader)) return null
  return { pid, name, identity }
}

function observeHandoff(record) {
  const spec = record?.handoff
  if (!spec || process.platform !== "linux") return { alive: false, waiting: false }

  // Primeiro revalida PIDs já vistos. Isso cobre o caso em que o launcher
  // entregou o jogo a um processo reparentado e ele não aparece mais no grupo
  // original.
  for (const [pid, saved] of spec.seen) {
    const current = readProcessIdentity(pid)
    if (
      current &&
      current.starttime === saved.starttime &&
      !/[ZX]/.test(String(current.state || ""))
    ) {
      return { alive: true, waiting: false }
    }
    spec.seen.delete(pid)
  }

  // Descobre novos processos do mesmo prefixo (t6mp.exe, t6zm.exe,
  // plutonium.exe etc.). O filtro por WINEPREFIX evita casar outro jogo com
  // nome semelhante executado por fora do Arcadia.
  let entries = []
  try {
    entries = fs.readdirSync("/proc")
  } catch {}
  for (const entry of entries) {
    if (!/^\d+$/.test(entry)) continue
    const found = processBelongsToHandoff(Number(entry), spec, record)
    if (!found) continue
    spec.seen.set(found.pid, found.identity)
    return { alive: true, waiting: false }
  }

  return { alive: false, waiting: Date.now() < Number(spec.graceUntil || 0) }
}

function processStartTimeKnown(identity) {
  return Boolean(
    identity &&
    Number(identity.pid) > 1 &&
    Number.isFinite(Number(identity.pgrp)) &&
    Number(identity.pgrp) === Number(identity.pid) &&
    /^\d+$/.test(String(identity.starttime)) &&
    Number(identity.starttime) > 0 &&
    !/[ZX]/.test(String(identity.state || "")),
  )
}

function refreshProcessIdentity(record) {
  if (!record?.pid || processStartTimeKnown(record.processIdentity)) return record?.processIdentity || null
  const identity = readProcessIdentity(Number(record.pid))
  if (processStartTimeKnown(identity)) record.processIdentity = identity
  return record.processIdentity || null
}

function processIdentityMatches(record) {
  const pid = Number(record?.pid)
  if (!Number.isInteger(pid) || pid <= 1 || record.childExited) return false
  if (record.child && Number(record.child.pid) !== pid) return false
  // A missing/unknown start time is not an ownership proof. Refresh once the
  // detached child has entered /proc, but still fail closed if it is gone.
  const owned = refreshProcessIdentity(record)
  if (!processStartTimeKnown(owned)) return false
  const current = readProcessIdentity(pid)
  if (!processStartTimeKnown(current)) return false
  return current.pgrp === owned.pgrp &&
    current.starttime === owned.starttime &&
    current.pgrp === pid
}

function grupoDoJogoVivo(rastreado = jogoAtivo || jogoEncerrando) {
  const pid = Number(rastreado?.pid)
  if (!Number.isInteger(pid) || pid <= 1 || rastreado.childExited) return false
  // Observation is deliberately conservative when /proc is unavailable: a
  // live ChildProcess is treated as alive, but processIdentityMatches() still
  // fails closed for every signal sent by Stop.
  const ownershipKnown = processStartTimeKnown(refreshProcessIdentity(rastreado))
  if (ownershipKnown && !processIdentityMatches(rastreado)) {
    // The leader may have exited while handing off to a child, or its PID may
    // have been recycled. We cannot signal either case, but an existing group
    // must keep the session alive rather than restoring focus over a game.
    try {
      process.kill(-pid, 0)
      return true
    } catch {
      return false
    }
  }
  if (!ownershipKnown && (!rastreado.child || launchChildExited(rastreado.child))) return false
  try {
    process.kill(-pid, 0)
    return true
  } catch {
    return false
  }
}

function sinalizarGrupoDoJogo(record, signal) {
  if (!processIdentityMatches(record)) return false
  try {
    process.kill(-record.pid, signal)
    return true
  } catch {
    return false
  }
}

// A systemd-backed Gamescope session owns the launcher handoff in a cgroup,
// while Gamescope + gamescopereaper remain the visible process group. The
// cgroup path comes only from `systemctl show` and is validated by
// readCgroupPids; it is never selected by a global process-name search.
const PROCESS_SESSION_POLL_MS = 1000
const PROCESS_SESSION_EMPTY_CONFIRMATIONS = 3
const PROCESS_SESSION_START_TIMEOUT_MS = 20_000

function processSessionReady(record) {
  const session = record?.processSession
  return !session || session.type !== "systemd" || session.ready === true
}

function clearProcessSessionWatch(record) {
  const session = record?.processSession
  if (!session?.pollTimer) return
  clearInterval(session.pollTimer)
  session.pollTimer = null
}

function pararSessaoSystemd(record) {
  const session = record?.processSession
  if (session?.type !== "systemd" || session.stopRequested) return false
  const args = systemdStopArgs(session.unit)
  if (!args) return false
  session.stopRequested = true
  try {
    const helper = spawn("systemctl", args, {
      detached: true,
      stdio: "ignore",
      env: process.env,
    })
    helper.on?.("error", (error) => {
      console.warn("arcadia: não foi possível parar a sessão systemd:", error.message || error)
    })
    helper.unref?.()
    return true
  } catch (error) {
    console.warn("arcadia: não foi possível parar a sessão systemd:", error.message || error)
    return false
  }
}

function encerrarGamescopeDaSessao(record) {
  // Stop the exact cgroup first. The group signal then removes Gamescope and
  // its reaper, without touching another game launched outside this session.
  pararSessaoSystemd(record)
  return sinalizarGrupoDoJogo(record, "SIGTERM")
}

function armarVigiaProcessSession(record) {
  const session = record?.processSession
  if (session?.type !== "systemd" || session.pollTimer) return
  session.startedAt = Date.now()
  session.ready = false
  session.emptyTicks = 0
  session.queryInFlight = false

  const verificar = () => {
    if (session.queryInFlight || session.stopRequested || record.stopRequested) return
    session.queryInFlight = true
    execFile(
      "systemctl",
      [
        "--user",
        "show",
        session.unit,
        "--property=ActiveState",
        "--property=SubState",
        "--property=ControlGroup",
      ],
      { env: process.env, timeout: 1500 },
      (error, stdout) => {
        session.queryInFlight = false
        if (session.stopRequested || record.stopRequested) return

        const properties = parseSystemdShow(stdout)
        const pids = error
          ? null
          : readCgroupPids(properties.ControlGroup, { cgroupRoot: session.cgroupRoot })
        if (Array.isArray(pids) && pids.length > 0) {
          session.cgroupPath = properties.ControlGroup
          session.ready = true
          session.emptyTicks = 0
          return
        }

        // A missing cgroup is not proof that the game ended. Keep the launch
        // barrier during startup and fail closed if systemd never creates it.
        if (!Array.isArray(pids)) {
          if (Date.now() - session.startedAt < PROCESS_SESSION_START_TIMEOUT_MS) return
          session.failed = true
        } else if (!session.ready) {
          if (Date.now() - session.startedAt < PROCESS_SESSION_START_TIMEOUT_MS) return
          session.failed = true
        }

        session.emptyTicks += 1
        if (session.emptyTicks < PROCESS_SESSION_EMPTY_CONFIRMATIONS) return
        session.ended = true
        clearProcessSessionWatch(record)
        encerrarGamescopeDaSessao(record)
      },
    )
  }

  session.pollTimer = setInterval(verificar, PROCESS_SESSION_POLL_MS)
  verificar()
}

function steamAppId(record) {
  const m = /^steam:(\d+)$/.exec(String(record?.gameId || ""))
  return m ? m[1] : ""
}

// Steam owns games launched through its URI, so there is no Arcadia process
// group to signal after the URI helper exits.  Ask Steam to stop this exact
// app only; do not fall back to a pattern-wide process kill.
function pararJogoSteam(record) {
  const appid = steamAppId(record)
  if (!appid || record?.steamStopRequested) return false
  record.steamStopRequested = true
  const command = record.steamCommand || "steam"
  try {
    const child = spawn(command, [`steam://stopgameid/${appid}`], {
      detached: true,
      stdio: "ignore",
      env: record.steamEnv || process.env,
    })
    // Spawn errors are expected when Steam was closed between poll ticks.  An
    // error listener is mandatory because this helper is intentionally detached.
    child.on?.("error", (error) => {
      console.warn("arcadia: não foi possível pedir parada à Steam:", error.message || error)
    })
    child.unref?.()
    return true
  } catch (error) {
    console.warn("arcadia: não foi possível pedir parada à Steam:", error.message || error)
    return false
  }
}

function launchStillHasPreScript(record) {
  return Boolean(record?.preScriptChild && !launchChildExited(record.preScriptChild))
}

function launchStillHasSteamStarter(record) {
  return Boolean(record?.steamStarterChild && !launchChildExited(record.steamStarterChild))
}

function finishCancelledLaunch(record) {
  if (!launchIsCurrent(record) || !record.stopRequested) return false
  // A stop request does not release the generation until every child that can
  // still lead to a game has exited.  This is the key race guard for Steam's
  // delayed URI callback and for pre-launch scripts.
  if (launchStillHasPreScript(record) || launchStillHasSteamStarter(record) || record.gameRecord || jogoAtivo || jogoEncerrando) return false
  releaseLaunch(record)
  if (record.focusApplied) {
    restaurarFocoArcadia(() => !lancamentoAtual && !jogoAtivo && !jogoEncerrando)
  }
  return true
}

function cancelarLancamento(record) {
  if (!launchIsCurrent(record)) return false
  const result = launchLifecycle.requestStop(record.token)
  if (!result.ok) return false
  record.stopRequested = true
  clearLaunchTimers(record)
  if (record.preScriptChild) {
    stopLaunchChild(record, record.preScriptChild, "preScriptKillTimer")
  }
  if (record.steamStarterChild) {
    stopLaunchChild(record, record.steamStarterChild, "steamStarterKillTimer")
  }
  // A direct child can be created just before the IPC Stop callback runs.  It
  // is handled through the same verified group path as an already-running game.
  if (record.gameRecord) pararJogo(record.gameRecord)
  finishCancelledLaunch(record)
  return true
}

function pararJogo(record) {
  if (!record) return false
  if (record.stopRequested) return false
  record.stopRequested = true
  if (record.launchSession) launchLifecycle.requestStop(record.launchSession.token)
  if (record.steamWrapper) pararJogoSteam(record)
  const sessionStopped = record.processSession?.type === "systemd"
    ? pararSessaoSystemd(record)
    : false
  const signaled = sinalizarGrupoDoJogo(record, "SIGTERM")
  if (signaled && !record.groupKillTimer) {
    record.groupKillTimer = setTimeout(() => {
      record.groupKillTimer = null
      // Revalidate both launch generation and process identity before SIGKILL.
      if ((jogoAtivo === record || jogoEncerrando === record) &&
          sinalizarGrupoDoJogo(record, "SIGKILL")) return
    }, 4000)
  }
  return signaled || sessionStopped || Boolean(record.steamWrapper)
}

// Vigia de jogo rodando (todos os modos): avisa o renderer nas transições
// abriu/fechou. O card "jogando" do modo desktop se ancora nisso.
let jogoRodando = false
// O poll SÓ arma quando lançamos um jogo (armarPollJogo) e desarma após 2
// ciclos sem sinal confirmado — idle não paga pgrep a cada 3s. No gamescope o
// mesmo tick resolve o foco (ARCADIA_GAMESCOPE=1), sem intervalo extra de 2s.
let sinalDeVida = 0
let processoJogoVisto = false
let sessaoArmadaEm = 0
let runningGameGeneration = 0
const MAX_INICIO_STEAM_MS = 60_000
const armarPollJogo = () => {
  if (runningGameInterval) return
  const generation = ++runningGameGeneration
  focoRestauradoSessao = false
  sinalDeVida = 0
  processoJogoVisto = false
  sessaoArmadaEm = Date.now()
  runningGameInterval = setInterval(() => {
    if (runningGameGeneration !== generation || !runningGameInterval) return
    const tick = () => {
      if (runningGameGeneration !== generation || !runningGameInterval) return
      const finalizarSeAusente = () => {
        // Never publish a transient false while the asynchronous launch is
        // still starting. This keeps a late wrapper callback from clearing the
        // pending game state in the renderer.
        if (lancamentoAtual && launchLifecycle.isStarting(lancamentoAtual.token)) {
          sinalDeVida = 0
          return
        }
        // Keep the session barrier for two empty observations. A single empty
        // read can race a child handoff or a Steam URI transition.
        if (++sinalDeVida < 2) return
        // Recheck the owned group immediately before declaring termination. A
        // signal/exec callback can otherwise race with a new process.
        if (grupoDoJogoVivo(jogoAtivo || jogoEncerrando)) {
          sinalDeVida = 0
          return
        }
        clearInterval(runningGameInterval)
        runningGameInterval = null
        runningGameGeneration++
        // Jogo fechou de verdade: encerra a sessão (playtime, pós-jogo,
        // gameId). O FALSE só sai em `finalizarSessao`.
        finalizarSessao()
      }

      const grupo = jogoAtivo || jogoEncerrando
      if (grupo) {
        const vivo = grupoDoJogoVivo(grupo)
        // Observa enquanto o grupo ainda está vivo para registrar o launcher
        // e qualquer processo Windows que ele crie antes de ser reparentado.
        const handoff = observeHandoff(grupo)
        const wrapperSteam = ultimoJogoAtivo?.steamWrapper === true
        if (vivo || handoff.alive) {
          // A sessão systemd pode estar viva enquanto ainda cria o processo
          // real (ou durante o handoff do updater). Só confirmar o jogo após
          // o cgroup mostrar pelo menos um processo; Gamescope/reaper sozinhos
          // não contam como jogo confirmado.
          if (!wrapperSteam) {
            if (processSessionReady(grupo)) {
              processoJogoVisto = true
              marcar(true)
            }
            sinalDeVida = 0
            return
          }
        } else if (handoff.waiting) {
          // O launcher pode sair antes de o processo Windows final criar sua
          // janela. Durante essa pequena janela não restauramos o foco nem
          // publicamos o jogo como encerrado.
          sinalDeVida = 0
          return
        } else if (jogoAtivo === grupo) {
          // Mantém a barreira de sessão durante os dois ciclos de ausência;
          // sem isso um novo launch poderia ocupar o intervalo antes do
          // finalizer e sobrescrever o snapshot/playtime anterior.
          jogoAtivo = null
          jogoEncerrando = grupo
        }
      }

      // An executable launched directly by Arcadia is owned by its detached
      // process group; a global name match must not keep it alive or make an
      // unrelated game look like this session. pgrep is only an activity hint
      // for Steam, whose game is owned by the Steam client.
      if (ultimoJogoAtivo?.steamWrapper !== true) {
        finalizarSeAusente()
        return
      }
      execFile("pgrep", ["-f", PADRAO_JOGO], (err) => {
        if (runningGameGeneration !== generation || !runningGameInterval) return
        const rodando = !err
        if (rodando) processoJogoVisto = true
        // Steam pode levar vários segundos entre a morte do wrapper URI e a
        // criação do executável real. Sem esta janela, dois pgrep vazios
        // restaurariam o foco enquanto o jogo ainda está abrindo.
        if (
          !processoJogoVisto &&
          !ultimoJogoAtivo?.stopRequested &&
          Date.now() - sessaoArmadaEm < MAX_INICIO_STEAM_MS
        ) {
          sinalDeVida = 0
          return
        }
        if (rodando) {
          marcar(true)
          sinalDeVida = 0
          return
        }
        finalizarSeAusente()
      })
    }

    if (process.env.ARCADIA_GAMESCOPE === "1") {
      // O Chromium dentro do gamescope não recebe blur/focus. O pgrep é
      // necessário para Steam (o jogo é filho do cliente), mas não encontra
      // executáveis não-Steam em pastas arbitrárias. O grupo detached que o
      // Arcadia criou cobre esses jogos e os wrappers de Proton/gamescope.
      execFile("pgrep", ["-f", PADRAO_JOGO], (err) => {
        if (runningGameGeneration !== generation || !runningGameInterval) return
        const jogoPorGrupo = grupoDoJogoVivo()
        const jogoPorPadrao = !err // Steam/Proton conhecido pelo cmdline
        const jogoEmCena = jogoPorGrupo || jogoPorPadrao
        // O primeiro ciclo sem pgrep pode ser só a troca do wrapper Steam
        // para o processo real. O launcher já foi marcado fora de foco na
        // borda do launch; nenhuma transição FALSE é emitida aqui.
        if (jogoEmCena && focado) focoNativo(false)
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
// Sessão encerrada DE VERDADE (jogo fechou e o poll desarmou): soma o tempo
// jogado, roda o script pós-jogo e solta o gameId. SÓ AQUI — o marcar(false)
// TRANSITÓRIO (wrapper do launch morreu antes do jogo subir) não pode
// finalizar; Steam ainda recebe uma janela de inicialização para trocar o
// wrapper pela aplicação real. O desarme (2 ciclos sem sinal após a confirmação)
// é o único momento em que dá pra ter certeza que o jogo fechou mesmo.
const finalizarSessao = () => {
  const snap = ultimoJogoAtivo
  const launch = snap?.launchSession || lancamentoAtual
  // This function is called only after two empty polls.  Recheck the exact
  // detached group before touching focus/state, so a late child cannot be
  // hidden by a false termination transition.
  if (grupoDoJogoVivo(snap || jogoAtivo || jogoEncerrando)) return false
  if (minimizarTimer) {
    clearTimeout(minimizarTimer)
    minimizarTimer = null
  }
  // Mesmo que nenhum tick tenha publicado TRUE (saída antes dos 3s), o
  // renderer pode estar em estado pendente desde o launch. O false forçado
  // limpa esse estado sem alterar a regra de restauração de foco.
  marcar(false, true)
  const sessaoConfirmada = processoJogoVisto
  ultimoJogoAtivo = null
  jogoAtivo = null
  jogoEncerrando = null
  processoJogoVisto = false
  sessaoArmadaEm = 0
  if (sessaoConfirmada && snap && snap.gameId && snap.startedAt) {
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
  // Script pós-jogo configurado (se houver). Se nenhum processo chegou a
  // ser confirmado, a tentativa falhou antes de virar uma sessão de jogo.
  const scriptPosJogo = sessaoConfirmada ? postGameScript : ""
  postGameScript = ""
  if (scriptPosJogo) {
    const script = scriptPosJogo
    try {
      const p = spawn(script, [], { detached: true, stdio: "ignore" })
      p.on?.("error", (error) => {
        console.warn("arcadia: script pós-jogo falhou:", error.message || error)
      })
      p.unref?.()
    } catch (error) {
      console.warn("arcadia: script pós-jogo falhou:", error.message || error)
    }
  }
  // Release the token only after process termination is confirmed. This emits
  // `idle` and lets a following launch start without stale callbacks.
  if (launch) releaseLaunch(launch)
  else launchInFlight = false
  // Só chega aqui depois de dois ciclos sem jogo. Nunca restaure no primeiro
  // `marcar(false)`: o wrapper Steam pode morrer antes do jogo real subir.
  restaurarFocoArcadia(() => !jogoAtivo && !jogoEncerrando && !lancamentoAtual)
  return true
}

const marcar = (rodando, forcar = false) => {
  if (rodando === jogoRodando && !forcar) return
  jogoRodando = rodando
  if (win && !win.isDestroyed()) {
    win.webContents.send("game:running", rodando)
    // Canal com o gameId: o botão Running/Stop da página do jogo (desktop)
    // precisa saber QUAL jogo está rodando — o boolean do game:running não
    // basta. gameId vazio = jogo detectado mas não lançado por nós (ex:
    // aberto direto pela Steam) — o renderer não associa a página.
    const id = rodando ? jogoAtivo?.gameId || ultimoJogoAtivo?.gameId || "" : ""
    win.webContents.send("game:active", { rodando, gameId: id })
  }
  // NOTA: a finalização da sessão (playtime/pós-jogo/limpeza do gameId) fica
  // no DESARME do poll (finalizarSessao) — nunca no marcar(false) transitório.
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
const SLS_CONFIG = path.join(HOME, ".config/SLSsteam/config.yaml")

// Diário do subsistema de trailers. O serviço recebe o logger para continuar
// diagnosticando falhas sem depender do Electron.
const TRAILER_LOG = path.join(LOG_DIR, "trailers.log")
function logTrailer(msg) {
  try {
    fs.mkdirSync(LOG_DIR, { recursive: true })
    fs.appendFileSync(TRAILER_LOG, `${new Date().toISOString()} ${msg}\n`)
  } catch {}
}

// Cache por mtime: readConfig é chamado dezenas de vezes no boot (createWindow,
// did-finish-load, watchers, handlers). Relê do disco só quando o arquivo muda
// (writeConfig troca o mtime pelo rename), então edições externas também pegam.
let _cfgCache = { mtimeMs: -1, data: {} }
function readConfig() {
  try {
    // AUDITORIA (vazamento): o config.json guarda a hubcap_api_key em claro —
    // permissão 644 deixaria QUALQUER usuário do sistema lê-la. 600 = só o
    // dono. Aplicado a cada leitura (idempotente; o writeConfig recria o
    // arquivo com ummask padrão, então o chmod volta a rodar na próxima leitura).
    try {
      fs.chmodSync(CONFIG, 0o600)
    } catch {}
    const m = fs.statSync(CONFIG).mtimeMs
    if (m !== _cfgCache.mtimeMs) {
      _cfgCache = { mtimeMs: m, data: JSON.parse(fs.readFileSync(CONFIG, "utf-8")) }
    }
    return _cfgCache.data
  } catch (e) {
    return {}
  }
}

const { createTrailerService } = require("./trailer-service")
const trailerService = createTrailerService({
  trailersDir: TRAILERS_DIR,
  ytdlpPath: YTDLP,
  ffmpegDir: FFMPEG_DIR,
  env: YTDLP_ENV,
  getCookiesPath: () => String(readConfig().youtube_cookies || "").trim(),
  logger: logTrailer,
})

const discordRpc = new DiscordRpc(readConfig)

// Chaves de API que NUNCA saem completas pro renderer (auditoria A-06): o form
// de configurações mostra a máscara; o config:set reconhece a máscara e
// preserva o valor real no disco.
const SEGREDOS = [
  "steam_api_key",
  "steamgriddb_api_key",
  "hubcap_api_key",
  "retroachievements_token",
  "retroachievements_web_api_key",
]

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
    const catalogo = await catalogGet(`/catalog/v1/sysinfo/${appid}`)
    if (catalogo.data?.data && typeof catalogo.data.data === "object") {
      Object.assign(info, catalogo.data.data)
    }
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
  const cached = cache[id]
  // Entradas antigas não têm `movies` porque trailers foram adicionados depois
  // do formato original. Rebusca só essas entradas para não deixar a loja sem
  // trailer até o usuário limpar o cache manualmente.
  const steamSemFilmes =
    (g?.launcher === "steam" || id.startsWith("steam:")) && !Array.isArray(cached?.movies)
  if (cached && cached._lang === lang && !steamSemFilmes) return cached
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
  const ttlHit = hit?.vazio ? 60 * 1000 : 6 * 60 * 60 * 1000
  if (hit && Date.now() - hit.at < ttlHit) return hit.data
  let out = null
  try {
    // Stats agregadas via servidor (o servidor coleta do SteamSpy e cacheia).
    const [statsRes, revRes] = await Promise.all([
      catalogGet(`/catalog/v1/stats/${appid}`),
      // Reviews da comunidade primeiro (fonte de verdade própria); se não
      // houver, cai no fallback da Steam (appreviews) para a tela não ficar
      // vazia. Modo híbrido: servidor armazena, Steam cobre o início.
      catalogGet(`/catalog/v1/reviews/${appid}`),
    ])
    const stats = statsRes.data?.data
    let comments = []
    let reviewDesc = ""
    let reviewPositivePct = null
    let totalReviews = 0

    // 1. reviews da comunidade (do servidor)
    const comReviews = Array.isArray(revRes.data?.reviews) ? revRes.data.reviews : []
    if (comReviews.length) {
      comments = comReviews.map((r) => ({
        steamid: "",
        author: r.username || "Usuário",
        avatar: "",
        text: String(r.text || "").trim(),
        positive: Boolean(r.positive),
        hours: Number(r.hours) || 0,
        hoursAtReview: Number(r.hours) || 0,
        helpful: 0,
        timestamp: new Date(r.created_at || 0).getTime() / 1000,
        daComunidade: true,
      }))
      totalReviews = comments.length
      reviewPositivePct = comments.length
        ? Math.round((comments.filter((c) => c.positive).length / comments.length) * 100)
        : null
      reviewDesc = reviewPositivePct >= 70 ? "Muito positivas" : "Positivas"
    } else {
      // 2. fallback: reviews da Steam (appreviews) — só quando não há da comunidade
      const rev = await fetchJson(
        `https://store.steampowered.com/appreviews/${appid}?json=1&language=english&purchase_type=all&filter=all&num_per_page=50`,
      ).catch(() => null)
      const q = rev?.query_summary || {}
      const pos = Number(q.total_positive) || 0
      totalReviews = Number(q.total_reviews) || 0
      reviewDesc = q.review_score_desc || ""
      reviewPositivePct = totalReviews ? Math.round((pos / totalReviews) * 100) : null
      const base = (rev?.reviews || [])
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
      const perfis = await resolveProfiles(base.map((c) => c.steamid).filter(Boolean))
      comments = base.map((c) => {
        const p = perfis[c.steamid]
        return { ...c, author: p?.name || c.author, avatar: p?.avatar || "" }
      })
    }

    out = {
      owners: stats?.owners || "",
      ccu: Number(stats?.ccu) || 0,
      reviewDesc,
      reviewPositivePct,
      totalReviews,
      comments,
    }
    // Se tudo vazio, trata como sem dados (painel some).
    if (!out.owners && !out.ccu && !out.totalReviews) out = null
  } catch {
    out = null
  }
  // Teto de entradas: cada item carrega até 50 reviews completas.
  if (_statsCache.size > 30) _statsCache.clear()
  // Não cacheia resultado VAZIO por 6h: se a Steam rate-limitou (0 reviews)
  // naquele momento, o próximo acesso pode ter sucesso. Cache vazio tem TTL
  // curto (1min) — não prende o painel em "sem reviews".
  _statsCache.set(appid, { at: Date.now(), data: out, vazio: !out })
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
  const executable = path.resolve(String(exe))
  if (linux === undefined) linux = !/\.exe$/i.test(executable)
  if (linux) return { cmd: [executable], env: {}, cwd: path.dirname(executable) }
  const wm = require("./winemanager")
  const s = getGameSettings(id)
  const prefixo = s.prefixPath || defaultPrefix(id)
  const g = { exe: executable }
  let v = null
  if (s.wineVersion) {
    v = wm.steamProtons().find((w) => w.id === s.wineVersion)
  }

  // Proton da Steam: não usar wine direto — Proton provê o Steam Runtime +
  // WINEDLLOVERRIDES corretos.
  if (v?.kind === "steam" && fs.existsSync(path.join(v.path, "proton"))) {
    // O Proton cria pfx.lock diretamente em STEAM_COMPAT_DATA_PATH. Em um
    // prefixo novo, o diretório pai precisa existir antes do primeiro launch.
    fs.mkdirSync(prefixo, { recursive: true })
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
    // Lutris também converte Proton para UMU. Executar `proton run` diretamente
    // fora da Steam deixa diferenças no runtime e no gerenciamento das janelas
    // Wine — launchers multi-janela como Plutonium expõem isso no foco do mouse.
    const umu = findUmuLauncher()
    if (umu) {
      try {
        wm.installGraphicsLibs(path.join(prefixo, "pfx"), v.wine, {
          dxvk: s.autoDXVK !== false,
          nvapi: Boolean(s.autoNVAPI),
          vkd3d: Boolean(s.autoVKD3D),
        })
      } catch {}
      return {
        cmd: [umu, g.exe],
        cwd: path.dirname(executable),
        env: {
          WINEPREFIX: prefixo,
          GAMEID: "umu-default",
          PROTONPATH: v.path,
          STORE: "none",
          WINEARCH: "win64",
          PROTON_VERB: "waitforexitandrun",
        },
      }
    }
    try {
      wm.installGraphicsLibs(path.join(prefixo, "pfx"), v.wine, {
        dxvk: s.autoDXVK !== false,
        nvapi: Boolean(s.autoNVAPI),
        vkd3d: Boolean(s.autoVKD3D),
      })
    } catch {}
    // Fallback: script proton direto.
    return {
      cmd: [path.join(v.path, "proton"), "run", g.exe],
      cwd: path.dirname(executable),
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
// Entradas gravadas quando o usuário adiciona ou baixa um jogo pela loja Steam:
// aparecem na aba Jogos imediatamente, com arte da CDN.
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

const resolveSteamNewsImage = createSteamNewsImageResolver({ fetchImpl: fetchRede })

// O caminho é resolvido por operação porque muda com a conta ativa.
const gameSettingsService = createGameSettingsService({
  getPath: () => caminhoConta(GAME_SETTINGS),
})
const emulatorRegistry = createEmulatorRegistry({
  dataDir: DATA_DIR,
  platform: process.platform,
  homeDir: os.homedir(),
  env: process.env,
})

// Aliases locais preservam os consumidores existentes enquanto o domínio fica
// testável fora do Electron.
function readAllGameSettings() {
  return gameSettingsService.readAll()
}
function getGameSettings(id) {
  return gameSettingsService.get(id)
}
function setGameSettings(id, patch) {
  return gameSettingsService.set(id, patch)
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
    // Use the service so emulator settings and its in-memory cache are removed
    // atomically along with the older Wine settings.
    gameSettingsService.remove(id)
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

// Gamescope's keep-alive flag is optional across distro versions. Probe once
// instead of making an old Gamescope reject every external launch. The
// systemd handoff remains useful even when the flag is unavailable.
let gamescopeKeepAliveSupport = null
function gamescopeSupportsKeepAlive() {
  if (gamescopeKeepAliveSupport !== null) return gamescopeKeepAliveSupport
  try {
    const result = spawnSync("gamescope", ["--help"], {
      encoding: "utf8",
      timeout: 2000,
      stdio: ["ignore", "pipe", "pipe"],
    })
    const help = `${result.stdout || ""}\n${result.stderr || ""}`
    gamescopeKeepAliveSupport = result.status === 0 && /(?:^|\s)--keep-alive(?:\s|$)/m.test(help)
  } catch {
    gamescopeKeepAliveSupport = false
  }
  return gamescopeKeepAliveSupport
}

function systemdUserManagerReady() {
  try {
    // `show-environment` is a cheap, read-only probe of the current user's
    // manager. No unit is created during capability detection.
    execFileSync("systemctl", ["--user", "show-environment"], {
      env: process.env,
      stdio: "ignore",
      timeout: 1500,
    })
    return true
  } catch {
    return false
  }
}

function createExternalProcessSession(tokenId) {
  if (!canUseSystemdSession({
    platform: process.platform,
    binExists,
    env: process.env,
    fsImpl: fs,
  })) return null
  if (!systemdUserManagerReady()) return null
  const unit = createSystemdUnitName({ pid: process.pid, tokenId })
  if (!unit) return null
  return {
    type: "systemd",
    unit,
    keepAlive: gamescopeSupportsKeepAlive(),
    cgroupRoot: "/sys/fs/cgroup",
  }
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

function applyGameSettings(cmd, s, gameId, launchTokenId = 0, extraEnvironmentKeys = []) {
  const warnings = []
  const env = { ...process.env }
  if (s.esync) env.WINEESYNC = "1"
  if (s.fsync) env.WINEFSYNC = "1"
  if (s.wineWayland) env.PROTON_ENABLE_WAYLAND = "1"
  if (s.wow64) env.PROTON_USE_WOW64 = "1"
  if (s.fsrHack) env.WINE_FULLSCREEN_FSR = "1"
  if (s.autoNVAPI) env.DXVK_ENABLE_NVAPI = "1"
  if (s.dxvkHud) env.DXVK_HUD = s.dxvkHud
  if (s.verboseLogs) {
    env.WINEDEBUG = env.WINEDEBUG || "+timestamp,+pid,+tid,+seh,+warn"
    if (!s.dxvkHud) env.DXVK_HUD = "full"
  }
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
  let processSession = null
  // Gamescope embrulha o comando (não se aplica a jogos Steam — a Steam tem
  // sua própria integração com gamescope). Em Linux, colocamos o comando
  // primário num serviço transitório do systemd. Isso é importante para
  // launchers que fecham o updater depois de criar outro processo: o
  // gamescopereaper vê o `systemd-run --wait` vivo, e o cgroup mantém o novo
  // processo dentro da sessão. `--keep-alive` cobre a janela de handoff no
  // compositor; o vigia encerra a superfície quando o cgroup esvazia.
  if (s.gamescope && path.basename(String(cmd[0])) !== "steam") {
    if (binExists("gamescope")) {
      // HDR clients need the Gamescope WSI layer.  Set this only for an
      // external Gamescope launch; Steam owns its own Gamescope integration
      // and must not receive this environment change from Arcadia.
      if (s.gsHdr === true) env.ENABLE_GAMESCOPE_WSI = "1"
      processSession = createExternalProcessSession(launchTokenId)
      if (!processSession && process.platform === "linux") {
        warnings.push(
          "systemd do usuário indisponível — handoff de launchers pode não ser acompanhado pelo Gamescope",
        )
      }
      const wrapped = buildExternalGamescopeCommand(finalCmd, {
        // Keep the existing gsWidth/gsHeight/gsFps contract.  gsFps is the
        // nested refresh (`-r`), not the independent FPS limiter.
        width: s.gsWidth,
        height: s.gsHeight,
        fps: s.gsFps,
        hdr: s.gsHdr === true,
        // Existing gamescope settings had no window mode; keep the UI's
        // fullscreen behavior for those records without changing the generic
        // builder's legacy default (windowed).
        windowMode: s.gsWindowMode ?? "fullscreen",
        framerateLimit: s.gsFramerateLimit,
        keepAlive: Boolean(processSession?.keepAlive),
        systemdUnit: processSession?.unit || "",
        // Gamescope rewrites DISPLAY/Wayland variables only after it starts.
        // `--setenv=NAME` makes systemd-run copy those runtime values from its
        // own environment instead of the user's stale systemd environment.
        environmentKeys: [
          ...Object.keys(env),
          ...extraEnvironmentKeys,
          "WINEPREFIX",
          "DISPLAY",
          "WAYLAND_DISPLAY",
          "GAMESCOPE_WAYLAND_DISPLAY",
          "ENABLE_GAMESCOPE_WSI",
          "XDG_SESSION_TYPE",
          "XDG_CURRENT_DESKTOP",
          "STEAM_GAME_DISPLAY_0",
        ],
      })
      finalCmd = wrapped.cmd
      // Keep this metadata separate from argv: Stop uses the exact transient
      // unit instead of a process-name pattern.
      processSession = wrapped.processSession
        ? { ...processSession, ...wrapped.processSession }
        : null
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
  return { cmd: finalCmd, env, warnings, processSession }
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
  // library.json é um snapshot global legado na raiz; os demais são por conta.
  return [LIB, caminhoConta(CUSTOM_GAMES), caminhoConta(OVERRIDES), caminhoConta(PENDING_GAMES), caminhoConta(GAME_SETTINGS), caminhoConta(OWNED_GAMES)]
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
      // Persiste o ícone no override: assim a sidebar mostra o ícone desde a
      // primeira montagem (sem esperar a cura rodar de novo a cada abertura).
      if (it.icon && !g.icon) {
        g.icon = it.icon
        setOverride(caminhoConta(OVERRIDES), g.id, { icon: it.icon })
      }
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

// Preenche icon/capa dos jogos Steam a partir do cache de items (que vem do
// servidor, /catalog/v1/items). Síncrono e leve: a sidebar mostra o ícone
// desde a primeira montagem, sem depender da cura em background. Só preenche
// o que falta (não sobrescreve override/arte escolhida pelo usuário).
const ITENS_CACHE = path.join(DATA_DIR, "store_items_cache.json")
function preencherArte(games) {
  let cache
  try {
    cache = JSON.parse(fs.readFileSync(ITENS_CACHE, "utf-8"))
  } catch {
    return
  }
  for (const g of games) {
    const appid = /^steam:(\d+)$/.exec(String(g.id || ""))?.[1]
    if (!appid) continue
    const it = cache[appid]
    if (!it) continue
    if (!g.icon && it.icon) g.icon = it.icon
    if (!g.cover && it.capa) g.cover = it.capa
    if (!g.hero && it.heroi) g.hero = it.heroi
  }
}

function readLibrary() {
  try {
    const chave = _libMtimeKey()
    if (chave === _libCache.chave) return _libCache.games
    const globais = libraryRepository.readGlobal()
    const games = libraryRepository.filterByOwnership(globais)
    games.push(...normalizeLibrary(readJsonFile(caminhoConta(CUSTOM_GAMES), [])))
    // Entradas adicionadas pela loja vivem em pending_games.json. Um jogo
    // baixado pode sobrepor um snapshot antigo e marcar-se instalado sem
    // depender de uma varredura local de providers.
    const pendentes = normalizeLibrary(readJsonFile(caminhoConta(PENDING_GAMES), []))
    const porId = new Map(games.map((g, index) => [g.id, index]))
    for (const p of pendentes) {
      const index = porId.get(p.id)
      if (index === undefined) {
        porId.set(p.id, games.length)
        games.push(p)
      } else if (p.installed === true) {
        games[index] = { ...games[index], ...p }
      }
    }
    applyOverrides(games, readOverrides(caminhoConta(OVERRIDES)))
    // Enriquece cada jogo Steam com ícone/capa vindos do catálogo do servidor
    // (cache de items em disco, populado pelo itensDaLoja). Síncrono: a
    // sidebar mostra o ícone desde a primeira montagem, sem depender da cura
    // em background. Prefere o que o usuário já escolheu (overrides/art).
    preencherArte(games)
    // Tempo de sessão local: o renderer recebe o playtime já somado.
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
      if (g && (settings[g.id]?.exePath || (settings[g.id]?.emulatorId && settings[g.id]?.romPath))) {
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
    const validGames = normalizeLibrary(games)
    _libCache = { chave, games: validGames }
    return validGames
  } catch (e) {
    return []
  }
}

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

function avisarBiblioteca(win) {
  if (win && !win.isDestroyed()) win.webContents.send("library:changed")
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

function removerStubPendente(appid) {
  const id = "steam:" + appid
  const atuais = readJsonFile(caminhoConta(PENDING_GAMES), [])
  const restantes = atuais.filter((g) => g && g.id !== id)
  if (restantes.length !== atuais.length)
    fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(restantes, null, 2))
  return restantes.length !== atuais.length
}

function marcarJogoSteamInstalado(appid, title, art = {}) {
  const id = "steam:" + String(appid || "")
  if (!/^steam:\d+$/.test(id)) return
  const atuais = readJsonFile(caminhoConta(PENDING_GAMES), [])
  const base = "https://cdn.cloudflare.steamstatic.com/steam/apps/" + String(appid)
  const atual = atuais.find((g) => g && g.id === id)
  const entrada = {
    id,
    title: String(title || atual?.title || `Steam ${appid}`).trim(),
    launcher: "steam",
    launch_cmd: ["steam", `steam://rungameid/${appid}`],
    installed: true,
    cover: art.cover || atual?.cover || `${base}/library_600x900.jpg`,
    hero: art.hero || atual?.hero || `${base}/library_hero.jpg`,
    logo: atual?.logo || `${base}/logo.png`,
  }
  if (atual) Object.assign(atual, entrada)
  else atuais.push(entrada)
  fs.writeFileSync(caminhoConta(PENDING_GAMES), JSON.stringify(atuais, null, 2))
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

// Vigia de conquistas (toast estilo PS5 ao desbloquear). Além do toast,
// marca o item no achievements.json na hora.
let pararAchievementWatcher = null

// Callback único de desbloqueio: marca o item no achievements.json (o painel
// lê de lá) e avisa o renderer.
function onUnlockAchievement(payload) {
  let novo = false
  try {
    const arq = caminhoConta(path.join(DATA_DIR, "achievements.json"))
    const store = JSON.parse(fs.readFileSync(arq, "utf-8"))
    const it = (store?.[payload.appid]?.items || []).find(
      (x) =>
        `${x.block}|${x.bit}` === payload.key ||
        (payload.apiname &&
          String(x.apiname || "").toLowerCase() === String(payload.apiname).toLowerCase()),
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
    // Check if all achievements for this game are now unlocked
    let isPlatinum = false
    try {
      const arq2 = caminhoConta(path.join(DATA_DIR, "achievements.json"))
      const store2 = JSON.parse(fs.readFileSync(arq2, "utf-8"))
      const gameItems = store2?.[payload.appid]?.items || []
      if (gameItems.length > 0 && gameItems.every((item) => item.achieved)) {
        isPlatinum = true
      }
    } catch {}

    if (win && !win.isDestroyed()) win.webContents.send("achievement:unlocked", payload)
    showAchievementToast(payload, { platinum: isPlatinum })
  }
}

function createWindow() {
  const cfgIni = readConfig()
  const launcherMode = resolveLauncherMode(process.env, cfgIni)
  appliedZoomFactor = null
  // O preload só pode expor o modo inicial depois que o main resolveu todas as
  // fontes (env legado + preferência). A mesma variável também mantém a
  // resolução idempotente caso a janela seja recriada.
  process.env.ARCADIA_MODE = launcherMode
  win = new BrowserWindow({
    width: 1600,
    height: 900,
    backgroundColor: "#000000",
    autoHideMenuBar: true,
    icon: path.join(__dirname, "..", "public", "logo-512.png"),
    fullscreen: launcherMode === "console",
    // Não mostra até o primeiro paint estar pronto: sem isto a janela abre
    // branca/vazia e só depois o React pinta. Com ready-to-show o usuário vê
    // a janela já com conteúdo, sem flash branco.
    show: false,
    // As duas interfaces podem trocar com F11 na mesma BrowserWindow. Uma
    // janela com moldura não pode virar frameless em runtime, então o shell
    // usa sempre a faixa própria do desktop; no console o fullscreen a oculta.
    frame: false,
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

  // Recalcula o zoom depois que o usuário maximiza, redimensiona a janela ou
  // a move para outro monitor. O debounce evita chamar setZoomFactor dezenas
  // de vezes durante um arraste; o applier também elimina chamadas repetidas.
  let zoomResizeTimer = null
  const agendarEscalaDaJanela = () => {
    if (zoomResizeTimer) clearTimeout(zoomResizeTimer)
    zoomResizeTimer = setTimeout(() => {
      zoomResizeTimer = null
      reapplyWindowZoom()
    }, 120)
  }
  win.on("resize", agendarEscalaDaJanela)
  win.on("move", agendarEscalaDaJanela)
  win.on("closed", () => {
    if (zoomResizeTimer) clearTimeout(zoomResizeTimer)
    zoomResizeTimer = null
    appliedZoomFactor = null
  })

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
    // Modo ativo decide qual chave vale: fullscreen = console_ui_scale,
    // janela = ui_scale. Antes só aplicava o ui_scale do desktop — um 1.25
    // salvo no console vazava pro desktop no próximo load.
    let config = readConfig()
    // Recalibração única: o antigo 120% vira o novo 100%, sem alterar o
    // tamanho que a pessoa já vê. Instalações novas começam diretamente em 100%.
    if (!win.isFullScreen() && config.desktop_scale_base_v2 !== true) {
      const escalaAntiga = Number(config.ui_scale)
      const escalaLogica = Number.isFinite(escalaAntiga)
        ? Math.min(1.1, Math.max(0.7, escalaAntiga / 1.2))
        : 1
      writeConfig({ ui_scale: escalaLogica, desktop_scale_base_v2: true })
      config = readConfig()
    }
    // O padrão anterior de 100% deixava os rótulos compactos do desktop
    // pequenos demais. Promove somente esse padrão para 110%; valores que o
    // usuário já escolheu no controle de acessibilidade continuam intactos.
    if (!win.isFullScreen() && config.desktop_font_scale_v3 !== true) {
      const escalaAtual = Number(config.ui_scale)
      const escalaLegivel = !Number.isFinite(escalaAtual) || escalaAtual === 1
        ? 1.1
        : Math.min(1.1, Math.max(0.7, escalaAtual))
      writeConfig({ ui_scale: escalaLegivel, desktop_font_scale_v3: true })
      config = readConfig()
    }
    reapplyWindowZoom()
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
  win.on("blur", () => focoNativo(false))
  win.on("focus", () => focoNativo(true))
  // Vigia de conquistas: toast em tempo real + marca o item no
  // achievements.json (o painel lê de lá; sem isso só atualizava no refresh).
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
  // Modo diagnóstico: imprime o estado interno (conta, achievements, bins do
  // Steam, fila de sync, erros recentes) e fecha. Sem janela.
  if (process.argv.includes("--diagnostico")) {
    const { collect } = require("./diagnostic")
    console.log("== ARCADIA DIAGNOSTICO ==")
    console.log(JSON.stringify(collect(), null, 2))
    app.exit(0)
    return
  }
  // Resolve o modo antes de criar a BrowserWindow/preload. Sem isto o
  // `start_in_console_mode` só era conhecido pelo script de shell e o
  // renderer recebia o fallback desktop mesmo quando a preferência estava
  // ligada.
  process.env.ARCADIA_MODE = resolveLauncherMode(process.env, readConfig())
  configurarLojaSteam()
  // Instala o launcher UMU em segundo plano. O primeiro jogo Proton também
  // aguarda esta mesma operação caso seja aberto antes de ela terminar.
  ensureUmuLauncher().catch(() => {})
  startSysinfoPrefetch()
  // Conta online (backend proprio): registra IPC de auth e espelha eventos pro renderer.
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
  // O IPC de conta é o único dono da restauração. Reusar a mesma Promise
  // evita duas chamadas concorrentes de setSession() no boot.
  const { garantirSessao } = require("./supabase/ipc")
  const contaPronta = garantirSessao()
    .then(async (r) => {
      // Reconstrói as conquistas dos schemas DA STEAM DEPOIS de a conta estar
      // ativa. Antes rodava no createWindow como guest e gravava na raiz, então
      // o painel (conta) ficava só com o que o watcher pegou ao vivo. Pro
      // Cyberpunk (sem bin do Steam, jogo crackeado) isso significava aparecer
      // apenas 1 item mínimo. No-op se não há schema bin nem fallback.
      try {
        await require("./achievements/loader").loadAllSchemas()
      } catch (e) {
        console.error("[achievements] boot load:", e)
      }
      // A restauração agora emite SIGNED_IN quando a sessão salva é válida.
      // O listener central do IPC inicia realtime e reconcilia biblioteca,
      // conquistas e sources com o escopo da conta já selecionado.
      if (r?.session) {
        try {
          // Pré-aquece a arte dos jogos steam que o pull de biblioteca vai
          // criar como stub: sem isto, o stub nascia com capa cinza e a
          // sidebar só ganhava o ícone depois de a cura bater na Steam.
          const steamstore = require("./steamstore")
          const pendentes = require("./supabase/conta").caminhoArquivoConta("pending_games.json")
          let appids = []
          try {
            const ps = JSON.parse(require("fs").readFileSync(pendentes, "utf-8"))
            appids = ps
              .filter((p) => p && /^steam:\d+$/.test(String(p.id || "")))
              .map((p) => String(p.id).replace(/^steam:/, ""))
          } catch {
            appids = []
          }
          if (appids.length) steamstore.popularItens(appids).catch(() => {})
        } catch (e) {
          console.error("[biblioteca] boot pre-aquecimento:", e)
        }
        // O reconcile já é disparado pelo evento SIGNED_IN da restauração,
        // com o escopo da conta definido pelo IPC antes da operação.
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

  // Recarrega apiname/título/desc/ícones dos itens do achievements.json a partir
  // dos UserGameStatsSchema_*.bin da Steam.
  ipcMain.handle("achievements:schemas:load", async () => {
    try {
      const { loadAllSchemas } = require("./achievements/loader")
      return { ok: true, ...(await loadAllSchemas()) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle("app:diagnostico", async () => {
    try {
      return { ok: true, ...require("./diagnostic").collect() }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle("app:focusState", () => {
    // O renderer pode montar/recarregar depois que o jogo já começou. Reenvia
    // o estado de jogo no mesmo replay, sem criar outro canal IPC.
    if (win && !win.isDestroyed()) {
      win.webContents.send("game:running", jogoRodando)
      const id = jogoRodando ? jogoAtivo?.gameId || ultimoJogoAtivo?.gameId || "" : ""
      win.webContents.send("game:active", { rodando: jogoRodando, gameId: id })
      win.webContents.send("game:launchState", estadoLancamento())
    }
    return focado
  })
  ipcMain.handle("app:diagnostics", () => diagnostics.collect())
  ipcMain.handle("app:diagnosticsExport", async () => {
    const res = await dialog.showOpenDialog(win, {
      title: "Exportar diagnóstico",
      properties: ["openDirectory", "createDirectory"],
    })
    if (res.canceled || !res.filePaths[0]) return { ok: false, canceled: true }
    return supportBundle.create({ outputDir: res.filePaths[0], report: diagnostics.collect() })
  })
  ipcMain.handle("saves:list", (_e, gameId) => saveSnapshots.list(gameId))
  ipcMain.handle("saves:create", (_e, payload = {}) => saveSnapshots.create(payload))
  ipcMain.handle("saves:restore", (_e, payload = {}) => saveSnapshots.restore(payload))
  ipcMain.handle("saves:delete", (_e, payload = {}) => saveSnapshots.remove(payload))

  ipcMain.handle("game:launch", async (_e, payload) => {
    const resolverOptions = {
      findGame: (id) => readLibrary().find((game) => game.id === id),
      customLaunchCmd,
      getGameSettings,
      exeLaunchCmd,
      emulatorLaunch: (_id, game, settings) => {
        // A settings key alone never authorizes launching a ROM: the game must
        // still exist in the account-scoped library.
        if (!game || !settings?.emulatorId) return null
        return emulatorRegistry.resolveLaunch({
          emulatorId: settings.emulatorId,
          romPath: settings.romPath,
          extraArgs: settings.emulatorArgs,
          corePath: settings.emulatorCorePath,
          // Use Hydra-compatible deterministic flags only from the main
          // process; the renderer cannot choose arbitrary launch templates.
          launchMode: "hydra",
        })
      },
    }
    let resolved = resolveLaunchRequest(payload, resolverOptions)
    // Sem UMU, exeLaunchCmd produz temporariamente o fallback `proton run`.
    // Prepara o runtime gerenciado e resolve outra vez antes de criar qualquer
    // processo, garantindo o mesmo caminho de execução em toda instalação.
    if (
      resolved.ok &&
      path.basename(String(resolved.rawCmd?.[0] || "")) === "proton" &&
      resolved.envExtra?.STEAM_COMPAT_DATA_PATH
    ) {
      const umu = await ensureUmuLauncher()
      if (!umu.ok) return umu
      resolved = resolveLaunchRequest(payload, resolverOptions)
    }
    if (!resolved.ok) return resolved
    const acompanhaSolicitada = shouldTrackGameSession(resolved.rawCmd)
    if (launchInFlight || launchLifecycle.isBusy() || jogoRodando || jogoAtivo || jogoEncerrando) {
      return { ok: false, error: "Já existe um lançamento ou jogo em execução." }
    }
    // Installation URIs are not game sessions. They still use the launch
    // mutex while Steam opens the dialog, but must not make every game page
    // display a pending/cancel button.
    const launchGameId = acompanhaSolicitada ? resolved.gameId || "" : ""
    const launchToken = launchLifecycle.begin({ gameId: launchGameId })
    if (!launchToken) {
      return { ok: false, error: "Já existe um lançamento ou jogo em execução." }
    }
    const launch = {
      token: launchToken,
      gameId: launchGameId,
      child: null,
      gameRecord: null,
      preScriptChild: null,
      preScriptTimer: null,
      preScriptKillTimer: null,
      steamPollTimer: null,
      steamRunTimer: null,
      steamWaitTimer: null,
      steamStarterChild: null,
      steamStarterKillTimer: null,
      steamWrapper: false,
      steamCommand: null,
      steamEnv: null,
      handoffSpec: null,
      cwd: "",
      groupKillTimer: null,
      closeLog: null,
      focusApplied: false,
      stopRequested: false,
      spawnError: null,
      processSession: null,
    }
    lancamentoAtual = launch
    launchInFlight = true
    // Nunca herda script pós-jogo de uma tentativa anterior que falhou antes
    // de criar a sessão acompanhada.
    postGameScript = ""

    // DuckStation/PCSX2 frequently exit silently when no valid BIOS exists.
    // Check the local dump before wrapping/spawning; RPCS3 firmware is exposed
    // as status but is not hard-blocked because RPCS3 can install/configure it
    // through its own UI.
    try {
      if (resolved.gameId && resolved.mode !== "steam") {
        const emulatorSettings = getGameSettings(resolved.gameId)
        if (emulatorSettings?.emulatorId) {
          const profile = emulatorRegistry.getProfile(emulatorSettings.emulatorId)
          const detected = emulatorRegistry.list().find((item) => item.id === emulatorSettings.emulatorId)
          const preflight = preflightEmulator({
            emulatorId: emulatorSettings.emulatorId,
            executablePath: detected?.executable || profile?.executable || "",
            biosPath: profile?.biosPath || "",
          })
          if (!preflight.ok) {
            if (win && !win.isDestroyed()) {
              win.webContents.send("game:launchError", { gameId: resolved.gameId, error: preflight.error })
            }
            releaseLaunch(launch)
            return preflight
          }
          const running = preflightRunningEmulator({
            emulatorId: emulatorSettings.emulatorId,
            executablePath: detected?.executable || profile?.executable || "",
          })
          if (!running.ok) {
            if (win && !win.isDestroyed()) {
              win.webContents.send("game:launchError", { gameId: resolved.gameId, error: running.error })
            }
            releaseLaunch(launch)
            return running
          }
          // RetroAchievements: se há credencial salva e o emulador escolhido
          // tem client RA nativo, garante que a config dele está atualizada
          // antes de lançar. Melhor esforço — uma falha aqui não deve impedir
          // o jogo de abrir sem conquistas.
          try {
            const raCfg = readConfig()
            const raUsername = String(raCfg.retroachievements_username || "")
            const raToken = String(raCfg.retroachievements_token || "")
            if (raUsername && raToken) {
              raEmulatorConfig.configureEmulatorCredentials(emulatorSettings.emulatorId, {
                username: raUsername,
                token: raToken,
                home: HOME,
              })
            }
          } catch {}
        }
      }
    } catch (e) {
      releaseLaunch(launch)
      return { ok: false, error: String(e) }
    }

    let closeLaunchLog = () => {}
    launch.closeLog = closeLaunchLog
    // `steam://install` é uma ação da loja, não uma sessão de jogo: a Steam
    // precisa ficar visível para o diálogo de instalação e não deve alimentar o
    // vigia/foco/minimização do jogo.
    let acompanhaSessao = true
    let focoDeLancamentoAplicado = false
    try {
      let { rawCmd, gameId, envExtra, cwd: launchCwd } = resolved
      // Antes do applyGameSettings, que pode embrulhar tudo no gamescope — daí
      // em diante o cmd[0] já não é mais o binário da Steam.
      rawCmd = steamSilencioso(rawCmd)
      const steamLaunchUri =
        path.basename(String(rawCmd?.[0])) === "steam" &&
        rawCmd.some((arg) => /^steam:\/\/(?:run|rungameid)\//i.test(String(arg)))
      const sls = steamComInjecao(rawCmd)
      rawCmd = sls.cmd
      launch.steamWrapper = steamLaunchUri
      launch.steamCommand = rawCmd?.[0] || null
      // Aplica as configurações do jogo (env vars, prefixo, gamescope).
      const s = getGameSettings(gameId)
      const lib = gameId ? readLibrary().find((x) => x.id === gameId) : null
      discordRpc.setGame(lib?.title || gameId, lib?.launcher)
      const { cmd, env: envBase, warnings, processSession } = applyGameSettings(
        rawCmd,
        s,
        gameId,
        launch.token.id,
        [...Object.keys(envExtra || {}), ...Object.keys(sls.env || {})],
      )
      // UPC/voices38 precisa do schema ao lado do executável. Para jogos
      // conhecidos, a preparação é idempotente, validada e reversível; para
      // outros loaders ou catálogos ambíguos ela apenas emite um aviso e não
      // inventa IDs. Nunca toca DLL, EXE ou save do jogo.
      try {
        const appidMatch = /^steam:(\d+)$/.exec(String(gameId || ""))
        const uplayExe = s.exePath || lib?.exe || ""
        if (appidMatch && uplayExe) {
          const prepared = prepareUplayInstallation({
            gameDir: path.dirname(String(uplayExe)),
            appid: appidMatch[1],
            settings: s,
            entry: lib || { id: gameId, exe: uplayExe },
          })
          if (!prepared.ok) {
            warnings.push(`UPC/voices38: ${prepared.error}`)
          } else if (prepared.skipped && prepared.reason !== "loader-upc-nao-detectado") {
            warnings.push(`UPC/voices38: ${prepared.reason}`)
          }
        }
      } catch (e) {
        warnings.push(`UPC/voices38: ${String(e.message || e)}`)
      }
      launch.processSession = processSession
      launch.cwd = launchCwd || ""
      // O env da SLSsteam entra DEPOIS: applyGameSettings monta o ambiente a
      // partir do process.env e apagaria o LD_AUDIT.
      const env = { ...envBase, ...envExtra, ...sls.env }
      launch.steamEnv = env
      // O executável fora da Steam pode ser um launcher que entrega o jogo a
      // outro processo (Plutonium é um exemplo). Guarda o prefixo e os nomes
      // Windows antes dos wrappers para que o vigia continue a sessão após o
      // processo inicial desaparecer.
      launch.handoffSpec = createHandoffSpec(rawCmd, env)
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
      const openedLog = launchLog.open(gameId, cmd)
      closeLaunchLog = openedLog.close
      launch.closeLog = closeLaunchLog
      const stdio = openedLog.stdio
      if (openedLog.error) {
        console.warn("arcadia: log fd falhou:", openedLog.error.message)
      }

      // Script pré-jogo (aba AVANÇADO): espera terminar (máx. 60s) antes de lançar.
      // Keep the child on the launch record so Stop can cancel the wait instead
      // of allowing its callback to launch a game later.
      if (s.scriptPre) {
        await new Promise((res) => {
          let settled = false
          const done = () => {
            if (settled) return
            settled = true
            if (launch.preScriptTimer) {
              clearTimeout(launch.preScriptTimer)
              launch.preScriptTimer = null
            }
            launch.preScriptChild = null
            if (launch.stopRequested) finishCancelledLaunch(launch)
            res()
          }
          let child
          try {
            child = spawn(s.scriptPre, [], { stdio: "ignore" })
          } catch {
            done()
            return
          }
          launch.preScriptChild = child
          child.once?.("close", done)
          child.once?.("error", done)
          launch.preScriptTimer = setTimeout(() => {
            launch.preScriptTimer = null
            // Do not drop the child reference on timeout: Stop must still be
            // able to cancel it, and launching while the script is alive would
            // create an unowned process.  The owned child is terminated before
            // the pre-script promise is released.
            if (!launchChildExited(child)) {
              stopLaunchChild(launch, child, "preScriptKillTimer")
            } else {
              done()
            }
          }, 60000)
          if (launch.stopRequested) stopLaunchChild(launch, child, "preScriptKillTimer")
        })
      }
      if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
        if (launchIsCurrent(launch)) {
          finishCancelledLaunch(launch)
          return { ok: false, canceled: true, error: "Lançamento cancelado." }
        }
        return { ok: false, canceled: true, error: "Lançamento cancelado." }
      }
      // Script pós-jogo: o vigia de processo roda quando o jogo fechar.
      // Instalação via steam://install não cria uma sessão acompanhada.
      acompanhaSessao = shouldTrackGameSession(cmd)
      if (acompanhaSessao) postGameScript = s.scriptPost || ""

      // Valida binários ANTES de qualquer spawn (steam URI ou direto).
      const binErro = validarBinariosLaunch(cmd, gameId)
      if (binErro) {
        closeLaunchLog()
        discordRpc.clear()
        clearLaunchTimers(launch)
        releaseLaunch(launch)
        if (win && !win.isDestroyed()) {
          win.webContents.send("game:launchError", { gameId, error: binErro })
        }
        return { ok: false, error: binErro }
      }

      // "Minimizar Arcadia ao iniciar um jogo" (Config. Gerais). O console já
      // é fullscreen; minimizar essa janela durante a transição fazia o WM
      // perder a superfície e deixava teclado/cliques sem dono. Em desktop,
      // continua valendo a preferência e a restauração ocorre no fim da sessão.
      // O jogo ainda pode levar alguns frames para criar a janela. Marcar o
      // launcher como fora de foco já nesta borda evita que a tecla que
      // disparou o launch (ou um clique repetido) seja processada pelo React
      // durante essa transição. O foco só volta após falha ou fim confirmado.
      if (acompanhaSessao) {
        focoDeLancamentoAplicado = true
        launch.focusApplied = true
        iniciarSessaoDeFoco()
      }

      if (acompanhaSessao && readConfig().minimize_on_game_launch && win && !win.isDestroyed() && !win.isFullScreen()) {
        if (minimizarTimer) clearTimeout(minimizarTimer)
        const agendarMinimizacao = () => {
          minimizarTimer = null
          launch.minimizeTimer = null
          if (!launchIsCurrent(launch) || launch.stopRequested || !win || win.isDestroyed() || win.isFullScreen()) return
          // Não minimize antes de o poll confirmar algum processo: uma saída
          // rápida ou o wrapper Steam transitório não deve esconder a janela.
          if (!processoJogoVisto) {
            launch.minimizeTimer = setTimeout(agendarMinimizacao, 500)
            minimizarTimer = launch.minimizeTimer
            return
          }
          win.minimize()
        }
        launch.minimizeTimer = setTimeout(agendarMinimizacao, 2000)
        minimizarTimer = launch.minimizeTimer
      }

      const soltar = (c, acompanhar = acompanhaSessao) => {
        if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
          finishCancelledLaunch(launch)
          return false
        }
        let child
        try {
          child = spawn(c[0], c.slice(1), {
            detached: true,
            stdio,
            env,
            // Wine launchers depend on their executable directory for sibling
            // DLLs/configuration. Lutris starts Plutonium from that directory;
            // matching it also prevents a second window from stealing focus.
            cwd: launch.cwd || undefined,
          })
        } catch (error) {
          launch.childExited = true
          launch.spawnError = `spawn falhou: ${error.message}`
          clearLaunchTimers(launch)
          closeLaunchLog()
          releaseLaunch(launch)
          // No child was created, so this is a confirmed failed launch. Only
          // now may focus return to Arcadia.
          if (acompanhar) restaurarFocoArcadia(() => !lancamentoAtual && !jogoAtivo && !jogoEncerrando)
          if (win && !win.isDestroyed()) {
            win.webContents.send("game:launchError", {
              gameId,
              error: `spawn falhou: ${error.message}`,
            })
          }
          return false
        }
        launch.child = child
        child.once?.("close", () => {
          child.__arcadiaExited = true
          closeLaunchLog()
          if (launch.stopRequested && !launch.gameRecord) finishCancelledLaunch(launch)
        })
        child.on?.("error", (err) => {
          child.__arcadiaExited = true
          launch.childExited = true
          clearLaunchTimers(launch)
          closeLaunchLog()
          if (!acompanhar) {
            releaseLaunch(launch)
          } else if (launchIsCurrent(launch)) {
            // Treat an asynchronous spawn error as a stopped, empty session;
            // let the poll's confirmed absence perform focus restoration.
            launchLifecycle.requestStop(launch.token)
            launch.stopRequested = true
            if (jogoAtivo?.launchSession === launch) {
              jogoEncerrando = jogoAtivo
              jogoAtivo = null
            }
          }
          console.warn("arcadia: spawn erro:", err.message)
          if (win && !win.isDestroyed()) {
            win.webContents.send("game:launchError", {
              gameId,
              error: `spawn falhou: ${err.message}`,
            })
          }
        })
        // unref DEPOIS do listener registrado
        child.unref?.()
        if (acompanhar) {
          // Registra o grupo de processos do jogo (o spawn detached vira líder).
          // launcher sai da biblioteca (o payload do launch só traz gameId).
          const lib = gameId ? readLibrary().find((x) => x.id === gameId) : null
          const gameRecord = {
            pid: child.pid,
            child,
            processIdentity: readProcessIdentity(Number(child.pid)),
            alvo: c[c.length - 1],
            gameId: gameId || "",
            launcher:
              lib?.launcher ||
              (path.basename(String(c[0])) === "steam" &&
              /^steam:\/\/(?:run|rungameid)\//i.test(String(c[1] || ""))
                ? "steam"
                : ""),
            // Preserve the URI classification before gamescope can wrap the
            // command; otherwise Stop would miss Steam games running through
            // a gamescope command line.
            steamWrapper: Boolean(launch.steamWrapper),
            steamCommand: launch.steamCommand || c[0],
            steamEnv: launch.steamEnv || env,
            processSession: launch.processSession
              ? { ...launch.processSession }
              : null,
            handoff: launch.handoffSpec,
            startedAt: Date.now(),
            launchSession: launch,
          }
          launch.gameRecord = gameRecord
          jogoEncerrando = null
          jogoAtivo = gameRecord
          ultimoJogoAtivo = gameRecord
          launchLifecycle.markRunning(launch.token)
          armarVigiaProcessSession(gameRecord)
          armarPollJogo()
        } else {
          // `steam://install` has no tracked game session.
          releaseLaunch(launch)
        }
        launchInFlight = false
        return true
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
        const spawnSteamHelper = (args) => {
          try {
            const helper = spawn(cmd[0], args, {
              detached: true,
              stdio: "ignore",
              env,
            })
            helper.on?.("error", (error) => {
              console.warn("arcadia: auxiliar Steam falhou:", error.message || error)
            })
            helper.once?.("close", () => {
              helper.__arcadiaExited = true
              if (launch.steamStarterChild === helper) {
                launch.steamStarterChild = null
                finishCancelledLaunch(launch)
              }
            })
            helper.unref?.()
            return helper
          } catch (error) {
            console.warn("arcadia: auxiliar Steam falhou:", error.message || error)
            return null
          }
        }
        const run = () => {
          if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
            finishCancelledLaunch(launch)
            return false
          }
          return soltar(cmd, acompanhaSessao)
        }
        execFile("pgrep", ["-x", "steam"], (err) => {
          if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
            finishCancelledLaunch(launch)
            return
          }
          if (!err) {
            // Steam rodando: sai do BPM e lança.
            spawnSteamHelper(["steam://exitbigpicture"])
            launch.steamRunTimer = setTimeout(() => {
              launch.steamRunTimer = null
              run()
            }, 900)
            return
          }
          // Steam FECHADA: abre o cliente (mesmo binário/env de `cmd`/`env` —
          // com injeção SLSsteam quando aplicável; usar o "steam" do PATH
          // aqui reintroduziria a Steam pura, ver comentário de steamComInjecao),
          // espera subir, garante saída do BPM (ela pode restaurar a sessão
          // anterior em BPM — principalmente no gamescope) e só então lança o jogo.
          const starter = spawnSteamHelper([])
          launch.steamStarterChild = starter
          let tentativas = 0
          launch.steamPollTimer = setInterval(() => {
            if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
              clearLaunchTimers(launch)
              finishCancelledLaunch(launch)
              return
            }
            execFile("pgrep", ["-x", "steam"], (e2) => {
              if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
                finishCancelledLaunch(launch)
                return
              }
              if (!e2) {
                if (launch.steamPollTimer) {
                  clearInterval(launch.steamPollTimer)
                  launch.steamPollTimer = null
                }
                launch.steamWaitTimer = setTimeout(() => {
                  launch.steamWaitTimer = null
                  if (!launchIsCurrent(launch) || launch.stopRequested || launchLifecycle.isStopping(launch.token)) {
                    finishCancelledLaunch(launch)
                    return
                  }
                  spawnSteamHelper(["steam://exitbigpicture"])
                  launch.steamRunTimer = setTimeout(() => {
                    launch.steamRunTimer = null
                    run()
                  }, 1200)
                }, 3000) // cliente subiu: espera a UI estabilizar
              } else if (++tentativas > 30) {
                if (launch.steamPollTimer) {
                  clearInterval(launch.steamPollTimer)
                  launch.steamPollTimer = null
                }
                // A Steam nem chegou a criar a sessão que o poll vigia.
                clearLaunchTimers(launch)
                releaseLaunch(launch)
                if (acompanhaSessao) {
                  restaurarFocoArcadia(() => !lancamentoAtual && !jogoAtivo && !jogoEncerrando)
                }
                if (win && !win.isDestroyed()) {
                  win.webContents.send("game:launchError", {
                    gameId,
                    error: "Steam não iniciou em 60s.",
                  })
                }
              }
            })
          }, 2000)
        })
        return { ok: true, warnings }
      }
      const started = soltar(cmd)
      if (!started) {
        return launch.stopRequested
          ? { ok: false, canceled: true, error: "Lançamento cancelado." }
          : { ok: false, error: launch.spawnError || "Não foi possível iniciar o jogo." }
      }
      return { ok: true, warnings }
    } catch (e) {
      const canceled = launch.stopRequested || launchLifecycle.isStopping(launch.token)
      clearLaunchTimers(launch)
      closeLaunchLog()
      discordRpc.clear()
      // Once a tracked child exists, only the poll/finalizer may release focus.
      // For a pre-spawn failure there is no process to wait for, so release and
      // restore immediately after that fact is known.
      if (launchIsCurrent(launch) && !launch.gameRecord) {
        releaseLaunch(launch)
        if (focoDeLancamentoAplicado) {
          restaurarFocoArcadia(() => !lancamentoAtual && !jogoAtivo && !jogoEncerrando)
        }
      }
      return canceled
        ? { ok: false, canceled: true, error: "Lançamento cancelado." }
        : { ok: false, error: String(e) }
    }
  })

  // Fecha o jogo em execução (botão X do card "jogando").  Stop only signals
  // an owned detached group or the exact Steam app id.  It never uses a global
  // process-name pattern, which could terminate another user's game.
  ipcMain.handle("game:close", () => {
    try {
      discordRpc.clear()
      const tracked = jogoAtivo || jogoEncerrando
      if (tracked) {
        if (jogoAtivo === tracked) {
          jogoEncerrando = tracked
          processoJogoVisto = true
          jogoAtivo = null
        }
        pararJogo(tracked)
        return { ok: true, state: "stopping" }
      }
      if (lancamentoAtual && launchLifecycle.isBusy()) {
        cancelarLancamento(lancamentoAtual)
        return { ok: true, state: "stopping", starting: true }
      }
      // Preserve the historical idempotent API: a repeated Stop is harmless.
      return { ok: true, state: "idle" }
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
      ownedAdd(g.id)
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  // Adiciona um jogo manualmente ("Adicionar jogo"). Salva em custom_games.json
  // e devolve a biblioteca já mesclada. Jogos de emulador não guardam ROM/path
  // no registro da biblioteca: esses dados ficam nas configurações locais.
  ipcMain.handle("customgame:add", (_e, payload = {}) => {
    try {
      const { id, title, platform, exe, emulatorId, romPath, emulatorArgs, emulatorCorePath } = payload || {}
      const customId = typeof id === "string" ? id.trim() : ""
      const customTitle = typeof title === "string" ? title.trim() : ""
      const isEmulator = platform === "emulator"
      if (!/^custom:[a-z0-9][a-z0-9._-]{0,100}$/.test(customId) || !customTitle || customTitle.length > 200) {
        return { ok: false, error: "título ou identificador inválido" }
      }
      if (!isEmulator && (typeof exe !== "string" || !exe.trim() || exe.includes("\u0000"))) {
        return { ok: false, error: "título e executável são obrigatórios" }
      }
      if (isEmulator) {
        const resolved = emulatorRegistry.resolveLaunch({
          emulatorId,
          romPath,
          extraArgs: emulatorArgs,
          corePath: emulatorCorePath,
        })
        // Resolve apenas valida o perfil/ROM e monta argv; não executa nada.
        if (!resolved.ok) return { ok: false, error: resolved.error || "configuração do emulador inválida" }
      } else if (!exe) {
        return { ok: false, error: "título e executável são obrigatórios" }
      }
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      if (all.some((g) => g.id === customId))
        return { ok: false, error: "já existe um jogo com esse nome" }
      all.push({
        id: customId,
        title: customTitle,
        launcher: "custom",
        platform: isEmulator ? "emulator" : (platform === "linux" ? "linux" : "windows"),
        ...(isEmulator ? {} : { exe }),
        installed: true,
      })
      fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(all, null, 2))
      ownedAdd(customId)
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
  ipcMain.handle("customgame:update", (_e, payload = {}) => {
    try {
      const { id, title, exe, platform } = payload || {}
      const customId = typeof id === "string" ? id.trim() : ""
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      const g = all.find((x) => x.id === customId)
      if (!g) return { ok: false, error: "jogo não encontrado" }
      if (title !== undefined) {
        if (typeof title !== "string" || !title.trim() || title.trim().length > 200) return { ok: false, error: "título inválido" }
        g.title = title.trim()
      }
      if (platform === "emulator" || platform === "windows" || platform === "linux") g.platform = platform
      if (exe !== undefined) {
        if (typeof exe !== "string" || exe.includes("\u0000")) return { ok: false, error: "executável inválido" }
        if (exe) g.exe = exe
      }
      if (g.platform === "emulator") delete g.exe
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
        // acf + SLSsteam), sem diálogo da Steam, e atualiza em tempo real.
        if (ss.arcadiaDownloaded().some((a) => a.appid === appid)) {
          ss.removeDownloaded(appid)
          limparAposDesinstalar(id, { removePrefix, removeSettings })
          setOverride(caminhoConta(OVERRIDES), id, { hidden: true })
          if (win && !win.isDestroyed()) win.webContents.send("library:changed")
          // Desinstalar também remove da CONTA no servidor.
          ownedRemove(id)
          removerStubPendente(appid)
          try {
            require("./supabase/biblioteca").agendarPush()
          } catch {}
          return { ok: true }
        }
        // Jogo owned: a Steam mostra o diálogo de confirmação dela. O jogo
        // também sai da coleção da conta (desinstalar = não ter mais).
        ownedRemove(id)
        removerStubPendente(appid)
        setOverride(caminhoConta(OVERRIDES), id, { hidden: true })
        try {
          require("./supabase/biblioteca").agendarPush()
        } catch {}
        const child = spawn("steam", [`steam://uninstall/${appid}`], {
          detached: true,
          stdio: "ignore",
        })
        child.unref()
        return { ok: true }
      }
      const legendary = g?.launch_cmd?.[0] || ""
      if (launcher === "custom" || launcher === "retro") {
        // Jogo manual/retrô: só sai do custom_games.json.
        const rest = readJsonFile(caminhoConta(CUSTOM_GAMES), []).filter((x) => x.id !== id)
        try {
          fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(rest, null, 2))
        } catch {}
        ownedRemove(id)
        // Remove da coleção da conta no servidor
        try {
          require("./supabase/biblioteca").agendarPush()
        } catch {}
        limparAposDesinstalar(id, { removePrefix, removeSettings })
        if (win && !win.isDestroyed()) win.webContents.send("library:changed")
        return { ok: true }
      }
      if (launcher === "epic" || /legendary$/.test(legendary)) {
        // Espera o uninstall terminar antes de responder.
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
        // Desinstalar também remove da CONTA no servidor (mesma regra dos
        // outros launchers: desinstalar = não ter mais o jogo na coleção).
        ownedRemove(id)
        try {
          require("./supabase/biblioteca").agendarPush()
        } catch {}
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

  // Compatibilidade com versões antigas: ainda pode abrir o modo console via reinício.
  ipcMain.handle("app:enterConsole", () => {
    try {
      // Libera o lock de instância única ANTES de spawnar: o processo novo
      // (console) pede o lock no boot e, com o desktop ainda vivo (o quit
      // leva ~500ms), o lock negado MATAVA o console na hora — o modo Big
      // Picture nunca abria (regressão do single-instance lock).
      try {
        app.releaseSingleInstanceLock()
      } catch {}
      // --no-sandbox como o arcadia.sh: sem a flag o Electron sobe com o
      // sandbox e o app não abre neste ambiente (CachyOS).
      const child = spawn(process.execPath, [".", "--no-sandbox"], {
        cwd: path.join(__dirname, ".."),
        detached: true,
        stdio: "ignore",
        env: { ...process.env, PS5_FULLSCREEN: "1", ARCADIA_MODE: "console" },
      })
      child.unref()
      setTimeout(() => app.quit(), 500)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("app:setMode", (_e, mode) => {
    if (mode !== "console" && mode !== "desktop") return { ok: false, error: "modo inválido" }
    if (!win || win.isDestroyed()) return { ok: false, error: "janela indisponível" }
    try {
      win.setFullScreen(mode === "console")
      process.env.ARCADIA_MODE = mode
      const cfg = readConfig()
      const logical = Number(cfg[mode === "console" ? "console_ui_scale" : "ui_scale"]) || (mode === "console" ? 1.3 : 1)
      const factor = zoomFactorFor(mode, logical)
      applyWindowZoom(factor)
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
  })
  ipcMain.handle("config:set", (_e, cfg) => {
    // SEGURANÇA (auditoria A-06): o renderer recebe as chaves MASCARADAS no
    // config:get; se ele devolver a máscara de volta (form inalterado), mantém
    // o valor real no disco.
    const atual = readConfig()
    for (const k of [
      "steam_api_key",
      "steamgriddb_api_key",
      "hubcap_api_key",
      "retroachievements_token",
      "retroachievements_web_api_key",
    ]) {
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
    // Trocou de idioma: avisa as telas para recarregar os dados já cacheados.
    const janela = BrowserWindow.fromWebContents(_e.sender)
    if (cfg?.language && cfg.language !== idiomaAntes) {
      avisarBiblioteca(janela)
    }
    if (Object.prototype.hasOwnProperty.call(cfg || {}, "slssteam_path")) {
      janela?.webContents.send("plugins:changed")
    }
    // Perfil mudou (avatar/background/nome...): o desktop só recarrega o
    // config no mount/`library:changed` — sem isto, um background trocado no
    // Big Picture não refletia no desktop até trocar de modo.
    if (cfg?.profile && typeof cfg.profile === "object") {
      janela?.webContents.send("library:changed")
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
    newsEmVoo = catalogGet("/catalog/v1/news")
      .then(async (r) => {
        const dados = r.data?.data
        const items = Array.isArray(dados) ? dados : dados?.noticias || dados?.items
        return Array.isArray(items) && items.length ? items.slice(0, 40) : getNews(40)
      })
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

  ipcMain.handle("news:game", async (_event, rawAppid) => {
    const appid = String(rawAppid || "").trim()
    if (!/^\d{1,12}$/.test(appid)) return []
    try {
      const endpoint = new URL("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/")
      endpoint.searchParams.set("appid", appid)
      endpoint.searchParams.set("count", "6")
      endpoint.searchParams.set("maxlength", "500")
      endpoint.searchParams.set("feeds", "steam_community_announcements")
      const payload = await fetchJson(endpoint.href)
      const items = payload?.appnews?.newsitems
      if (!Array.isArray(items)) return []
      return Promise.all(items.slice(0, 6).map(async (item) => {
        const html = String(item.contents || "")
        // GetNewsForApp remove o markup/imagem em muitos anúncios. Quando
        // isso acontece, a página oficial ainda expõe a capa em og:image.
        const image = extractSteamNewsImage(html) || await resolveSteamNewsImage(item)
        const summary = html
          .replace(/\[img\][\s\S]*?\[\/img\]/gi, " ")
          .replace(/\[[^\]]+\]/g, " ")
          .replace(/<[^>]+>/g, " ")
          .replace(/\s+/g, " ")
          .trim()
        return {
          id: String(item.gid || item.url || item.date),
          title: String(item.title || "Atualização"),
          summary,
          source: String(item.author || "Steam"),
          url: String(item.url || `https://store.steampowered.com/news/app/${appid}`),
          image,
          date: new Date(Number(item.date || 0) * 1000).toISOString(),
        }
      }))
    } catch (error) {
      console.error("[news:game]", error.message)
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
  // Download concluído: registra o jogo como instalado e avisa o renderer
  // para recarregar a biblioteca em tempo real.
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
        marcarJogoSteamInstalado(String(item.appid).replace(/^steam:/, ""), item.title, {
          cover: item.cover,
          hero: item.hero,
        })
      }
      ownedAdd(String(item.appid))
    } catch {}
    if (win && !win.isDestroyed()) win.webContents.send("library:changed")
  })

  // --- Loja Steam (estilo Acella: Hubcap + DepotDownloader + SLSsteam) -----
  const steamstore = require("./steamstore")
  ipcMain.handle("store:status", async () => ({
    ...(await steamstore.status()),
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
    if (r?.ok) {
      // Remover da Steam também tira o jogo da CONTA no servidor — a coleção
      // sincroniza entre máquinas, então quem remove aqui não deve ver o jogo
      // "possuído" em outro dispositivo.
      ownedRemove("steam:" + String(appid || ""))
      removerStubPendente(String(appid || ""))
      setOverride(caminhoConta(OVERRIDES), "steam:" + String(appid || ""), { hidden: true })
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
      avisarBiblioteca(win)
    }
    return r
  })
  ipcMain.handle("store:removeDownloaded", (_e, appid) => {
    const r = steamstore.removeDownloaded(appid)
    // Sem este aviso a aba Lojas continuava mostrando "Na biblioteca" depois de
    // remover: o card se baseia na lista de jogos, que só recarrega neste
    // evento. Todos os outros pontos que mexem na biblioteca já o emitiam.
    if (r?.ok) {
      // Remover o download também tira o jogo da CONTA no servidor (a coleção
      // sincroniza entre máquinas): sem ownedRemove+push o jogo continuava
      // "possuído" e aparecia em outras máquinas logadas na mesma conta.
      ownedRemove("steam:" + String(appid || ""))
      removerStubPendente(String(appid || ""))
      setOverride(caminhoConta(OVERRIDES), "steam:" + String(appid || ""), { hidden: true })
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
      avisarBiblioteca(win)
    }
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
      setOverride(caminhoConta(OVERRIDES), "steam:" + String(appid || ""), { hidden: null })
      const r = steamstore.addToSteam(String(appid || ""))
      if (!r.ok) {
        // Sem .lua o registro na Steam falha, mas o jogo ainda entra na
        // biblioteca — antes o Add morria aqui e o jogo não aparecia em
        // lugar nenhum.
        try {
          adicionarStubPendente(String(appid), title)
          ownedAdd("steam:" + appid)
        } catch {}
        avisarBiblioteca(win)
        try {
          require("./supabase/biblioteca").agendarPush()
        } catch {}
        return r
      }
      const reg = steamstore.registerSlssteam({ appid: String(appid), token, dlcs })
      if (!reg?.ok) return reg || { ok: false, error: "falha ao registrar na SLSsteam" }
      try {
        adicionarStubPendente(String(appid), title)
        ownedAdd("steam:" + appid)
      } catch {}
      avisarBiblioteca(win)
      // Sincroniza a coleção com a conta (jogos seguem entre máquinas). O Add
      // com SLSsteam ativa vai por aqui (store:addToSteam) — sem isto, o jogo
      // só subia no próximo login.
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
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
      // Título real quando vier vazio: o fallback "Steam <appid>" subiria pro
      // servidor e o jogo chegaria com nome feio nas outras máquinas. Busca o
      // nome na steamcmd (1 chamada, ~300ms) só quando falta.
      let titulo = String(title || "").trim()
      if (!titulo || titulo === `steam:${appid}`) {
        try {
          const nome = await steamstore.fetchAppName(appid)
          if (nome) titulo = nome
        } catch {}
      }
      adicionarStubPendente(appid, titulo || title, { cover, hero: hero || heroi })
      ownedAdd("steam:" + appid)
      avisarBiblioteca(win)
      // Sincroniza a coleção com a conta (jogos seguem entre máquinas). Sem
      // isto, adicionar pela loja nunca subia pro servidor — só no próximo
      // boot. Mesmo padrão do customgame:add.
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
      return { ok: true }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })
  ipcMain.handle("store:removeFromLibrary", (_e, appid) => {
    try {
      const janela = BrowserWindow.fromWebContents(_e.sender)
      const id = "steam:" + String(appid || "")
      const removed = removerStubPendente(String(appid || ""))
      if (!removed && readLibrary().some((g) => g.id === id))
        setOverride(caminhoConta(OVERRIDES), id, { hidden: true })
      ownedRemove(id)
      avisarBiblioteca(janela || win)
      // Sincroniza a remoção com a conta (some das outras máquinas no pull).
      try {
        require("./supabase/biblioteca").agendarPush()
      } catch {}
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
  ipcMain.handle("store:ensureDepotDownloader", async () => {
    try {
      return await steamstore.ensureDepotDownloader()
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
  ipcMain.handle("dm:setPriority", (_e, appid, priority) => dm.setPriority(appid, priority))
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

  // Emuladores: só catálogo/detecção e montagem de argv; nenhum handler executa
  // binário ou passa comando por shell. A execução ocorre pelo fluxo game:launch.
  ipcMain.handle("emulators:list", () => ({ ok: true, emulators: emulatorRegistry.list() }))
  ipcMain.handle("emulators:detect", () => emulatorRegistry.detect())
  ipcMain.handle("emulators:profiles", () => ({ ok: true, profiles: emulatorRegistry.profiles() }))
  ipcMain.handle("emulators:profile:set", (_e, profile) => emulatorRegistry.setProfile(profile || {}))
  ipcMain.handle("emulators:profile:remove", (_e, id) => emulatorRegistry.removeProfile(typeof id === "string" ? id : ""))
  ipcMain.handle("emulators:resolve", (_e, payload) => emulatorRegistry.resolveLaunch(payload || {}))
  ipcMain.handle("emulators:status", () => {
    const profiles = emulatorRegistry.profiles()
    const statuses = emulatorRegistry.list().map((item) => {
      const executablePath = item.executable || profiles[item.id]?.executable || ""
      const status = getEmulatorStatus({
        emulatorId: item.id,
        executablePath,
        biosPath: profiles[item.id]?.biosPath,
      })
      const running = getRunningEmulatorStatus({ emulatorId: item.id, executablePath })
      return { ...status, running: running.running, runningPid: running.pid }
    })
    return { ok: true, statuses }
  })
  // A busca de ROMs só devolve arquivos locais validados pelo registry; não
  // sincroniza caminhos nem executa o conteúdo encontrado.
  ipcMain.handle("emulators:roms", (_e, payload) => emulatorRegistry.scanRoms(payload || {}))
  ipcMain.handle("emulators:roms:index", () => ({ ok: true, emulators: emulatorRegistry.roms() }))

  // RetroAchievements: login troca usuário+senha por um token de sessão (a
  // senha nunca é persistida por aqui); a chave salva em config.json é esse
  // token, não a senha. `retroachievements:login` é o único ponto que vê a
  // senha, e só a repassa pro client — nunca grava, nunca loga.
  ipcMain.handle("retroachievements:login", async (_e, { username, password } = {}) => {
    const result = await raClient.loginRequest({ username, password })
    if (!result.ok) return result
    writeConfig({
      retroachievements_username: result.username,
      retroachievements_token: result.token,
    })
    return { ok: true, username: result.username }
  })

  ipcMain.handle("retroachievements:logout", () => {
    writeConfig({ retroachievements_username: "", retroachievements_token: "" })
    return { ok: true }
  })

  ipcMain.handle("retroachievements:status", () => {
    const cfg = readConfig()
    const username = String(cfg.retroachievements_username || "")
    const hasToken = Boolean(cfg.retroachievements_token)
    return { ok: true, connected: Boolean(username && hasToken), username }
  })

  // Aplica a credencial salva no arquivo de config nativo de um emulador
  // específico (chamado antes do launch de um jogo retro, e também
  // disponível manualmente pela UI de Configurações/Emulador).
  ipcMain.handle("retroachievements:applyToEmulator", (_e, { emulatorId } = {}) => {
    const cfg = readConfig()
    const username = String(cfg.retroachievements_username || "")
    const token = String(cfg.retroachievements_token || "")
    if (!username || !token) return { ok: false, error: "nao_conectado" }
    return raEmulatorConfig.configureEmulatorCredentials(emulatorId, { username, token, home: HOME })
  })

  // Web API Key: credencial DIFERENTE do connect_token acima — o connect_token
  // só serve pro emulador desbloquear em tempo real; para LER progresso/lista
  // de conquistas pela API pública (API_*.php) é preciso a Web API Key de
  // controlpanel.php. Validamos antes de salvar pra não guardar chave inválida.
  ipcMain.handle("retroachievements:setApiKey", async (_e, { apiKey } = {}) => {
    const cfg = readConfig()
    const username = String(cfg.retroachievements_username || "")
    if (!username) return { ok: false, error: "nao_conectado" }
    const result = await raClient.verifyApiKey({ username, apiKey })
    if (!result.ok) return result
    writeConfig({ retroachievements_web_api_key: String(apiKey || "").trim() })
    return { ok: true }
  })

  // Progresso de um jogo retro: resolve o gameId da RA por título dentro do
  // console certo (fallback sem hash — ver retro-systems.js) e busca a lista
  // de achievements + o que o usuário já desbloqueou.
  ipcMain.handle("retroachievements:gameProgress", async (_e, { title, systemId } = {}) => {
    const cfg = readConfig()
    const username = String(cfg.retroachievements_username || "")
    const apiKey = String(cfg.retroachievements_web_api_key || "")
    if (!username || !apiKey) return { ok: false, error: "sem_web_api_key" }
    const consoleId = getRetroachievementsConsoleId(systemId)
    if (!consoleId) return { ok: false, error: "sistema_nao_suportado" }
    const found = await raClient.findGameByTitle({ username, apiKey, title, consoleId })
    if (!found.ok) return found
    if (!found.game) return { ok: true, game: null, achievements: [] }
    return raClient.getGameProgress({ username, apiKey, gameId: found.game.id })
  })

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
    const p = trailerService.localPath(id)
    return { path: p ? "file://" + p : "" }
  })

  // Baixa o trailer do YouTube (se ainda não existe). Devolve o caminho.
  ipcMain.handle("trailer:download", async (_e, { id, title } = {}) => {
    if (!id || !trailerService.isAvailable()) return { ok: false, error: "yt-dlp ausente" }
    const r = await trailerService.download(id, title || "")
    return r.ok ? { ok: true, path: "file://" + r.path } : r
  })

  // Lista vídeos do YouTube para escolha manual (sem baixar).
  ipcMain.handle("trailer:search", async (_e, { query } = {}) => {
    if (!trailerService.isAvailable()) {
      logTrailer("busca abortada: yt-dlp não instalado")
      return { ok: false, error: "yt-dlp não está instalado — instale o pacote yt-dlp" }
    }
    const { results, error } = await trailerService.search(query || "")
    if (error) return { ok: false, error }
    return { ok: true, results }
  })

  // URL direta para pré-visualizar o vídeo num <video> (sem baixar).
  ipcMain.handle("trailer:streamUrl", async (_e, { url } = {}) => {
    if (!url || !trailerService.isAvailable()) return { ok: false, error: "pedido inválido" }
    return trailerService.streamUrl(url)
  })

  // Baixa um vídeo específico do YouTube como trailer (escolha manual).
  ipcMain.handle("trailer:downloadUrl", async (_e, { id, url } = {}) => {
    if (!id || !url || !trailerService.isAvailable()) return { ok: false, error: "pedido inválido" }
    const r = await trailerService.downloadUrl(id, url, {
      onProgress: (data) => {
        if (win && !win.isDestroyed()) win.webContents.send("trailer:dlprogress", data)
      },
    })
    return r.ok ? { ok: true, path: "file://" + r.path } : r
  })

  // Baixa TODOS os trailers que faltam. Emite progresso e devolve a contagem.
  ipcMain.handle("trailer:downloadAll", async (_e) => {
    if (!trailerService.isAvailable()) return { ok: false, error: "yt-dlp ausente" }
    let lib = []
    try {
      lib = readLibrary()
    } catch {
      return { ok: false, error: "biblioteca não lida" }
    }
    const faltam = lib.filter((g) => !trailerService.localPath(g.id))
    let feitos = 0
    for (const g of faltam) {
      if (win && !win.isDestroyed())
        win.webContents.send("trailer:progress", {
          done: feitos,
          total: faltam.length,
          title: g.title,
        })
      await trailerService.download(g.id, g.title || "")
      feitos++
    }
    if (win && !win.isDestroyed())
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
    const [fixesCatalogo, ryuuCatalogo] = await Promise.all([
      catalogGet("/catalog/v1/fixes"),
      catalogGet("/catalog/v1/ryuu"),
    ])
    for (const [arquivo, resposta, valido] of [
      [
        path.join(DATA_DIR, "cache", "fixes-index.json"),
        fixesCatalogo,
        (d) => Array.isArray(d?.genericFixes) || Array.isArray(d?.onlineFixes),
      ],
      [
        path.join(DATA_DIR, "cache", "ryuu-index.json"),
        ryuuCatalogo,
        (d) => Array.isArray(d) || (d?.fixes && typeof d.fixes === "object"),
      ],
    ]) {
      if (!valido(resposta.data?.data)) continue
      try {
        fs.mkdirSync(path.dirname(arquivo), { recursive: true })
        const tmp = `${arquivo}.tmp`
        fs.writeFileSync(tmp, JSON.stringify(resposta.data.data))
        fs.renameSync(tmp, arquivo)
      } catch {}
    }
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
    if (win) {
      const consoleMode = !win.isFullScreen()
      win.setFullScreen(consoleMode)
      process.env.ARCADIA_MODE = consoleMode ? "console" : "desktop"
    }
  })
  ipcMain.handle("app:setFullscreen", (_e, on) => {
    const consoleMode = Boolean(on)
    if (win) win.setFullScreen(consoleMode)
    process.env.ARCADIA_MODE = consoleMode ? "console" : "desktop"
  })
  ipcMain.handle("app:setZoom", (_e, z, modo) => {
    // Escalas do console e do desktop são independentes: o setZoomFactor é
    // GLOBAL na janela, então aplicar o 1.25 do console sobrescrevia o zoom
    // do desktop (e vice-versa). Cada chamada carrega o modo que a originou;
    // só aplica se for o modo ativo — senão o zoom fica com o do outro modo.
    const ativo = win?.isFullScreen() ? "console" : "desktop"
    if (modo && modo !== ativo) {
      const chave = modo === "console" ? "console_ui_scale" : "ui_scale"
      return Number(readConfig()?.[chave]) || 1
    }
    const logical = Number(z) || 1
    const factor = zoomFactorFor(ativo, logical)
    applyWindowZoom(factor)
    return factor
  })

  ipcMain.handle("library:refresh", async () => {
    return curarCapasSteam(readLibrary())
  })

  // Reconstrói os metadados locais (limpa o cache da loja).
  ipcMain.handle("meta:rebuild", async () => {
    try {
      fs.unlinkSync(META_CACHE)
    } catch {
      /* sem cache, tudo bem */
    }
    return readLibrary()
  })

  // HowLongToBeat: tempos de jogo (falha silenciosa, sem linha na UI).
  ipcMain.handle("hltb:get", async (_e, titulo) => {
    try {
      const jogo = readLibrary().find(
        (g) => String(g.title || "").toLowerCase() === String(titulo || "").toLowerCase(),
      )
      const appid = String(jogo?.id || "").replace(/^steam:/, "")
      if (/^\d+$/.test(appid)) {
        const remoto = await catalogGet(`/catalog/v1/hltb/${appid}`)
        const dados = remoto.data?.data
        if (
          dados &&
          typeof dados === "object" &&
          [dados.main, dados.mainExtra, dados.completionist].some((v) => Number(v) > 0)
        ) {
          return dados
        }
      }
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

  // --- Fontes de download (catalogo no servidor + espelho local) -----------
  const sources = require("./sources")
  ipcMain.handle("sources:list", () => ({ ok: true, sources: sources.list() }))
  ipcMain.handle("sources:add", (_e, url) => sources.addSource(url))
  ipcMain.handle("sources:remove", (_e, id) => sources.removeSource(id))
  ipcMain.handle("sources:sync", () => sources.syncSources())
  ipcMain.handle("sources:search", async (_e, { query, limit } = {}) => ({
    ok: true,
    results: await sources.search(query, Number(limit) || 40),
  }))
  ipcMain.handle("sources:game", async (_e, ref) => sources.getGame(ref))

  // --- Loja Retro: somente fontes Hydra com status Classics ---------------
  // Feature flag para ativar o catálogo V2 (canônico)
  // V2 is the production default. Set ARCADIA_RETRO_V2=0 only as an explicit
  // rollback while the legacy cache remains supported.
  const RETRO_CATALOG_V2_ENABLED = process.env.ARCADIA_RETRO_V2 !== "0"

  const RETRO_SERVER_ENABLED = process.env.ARCADIA_RETRO_SERVER !== "0"
  const retroCatalog = RETRO_CATALOG_V2_ENABLED && RETRO_SERVER_ENABLED
    ? require("./retro-server-catalog").createRetroServerCatalog({ dataDir: DATA_DIR })
    : RETRO_CATALOG_V2_ENABLED
      ? require("./retro-catalog-v2").createRetroCatalogV2({ dataDir: DATA_DIR })
    : require("./retro-catalog")

  ipcMain.handle("retro:list", async (_e, payload) => {
    try {
      return await retroCatalog.list(payload || {})
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })
  ipcMain.handle("retro:game", async (_e, id) => {
    try {
      return await retroCatalog.getGame(id)
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  // Novo endpoint V2 para obter URIs de uma oferta específica
  ipcMain.handle("retro:offer", async (_e, offerId) => {
    try {
      if (!RETRO_CATALOG_V2_ENABLED) {
        return { ok: false, error: "V2 catalog not enabled" }
      }
      return await retroCatalog.getOffer(offerId)
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  // Migração manual do V1 para V2
  ipcMain.handle("retro:migrate", async () => {
    try {
      if (!RETRO_CATALOG_V2_ENABLED) {
        return { ok: false, error: "V2 catalog not enabled" }
      }
      const result = await retroCatalog.migrateFromV1()
      return { ok: true, result }
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  // Estatísticas do catálogo
  ipcMain.handle("retro:stats", async () => {
    try {
      if (!RETRO_CATALOG_V2_ENABLED) {
        return { ok: false, error: "V2 catalog not enabled" }
      }
      const stats = retroCatalog.repository.getStats()
      return { ok: true, stats }
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  // Auditoria de cobertura de arte/metadados (somente leitura).
  ipcMain.handle("retro:audit", async (_e, payload = {}) => {
    try {
      if (!RETRO_CATALOG_V2_ENABLED || typeof retroCatalog.audit !== "function") {
        return { ok: false, error: "retro_audit_unavailable" }
      }
      return await retroCatalog.audit(payload || {})
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  // Adiciona somente o item retrô à biblioteca. O ROM ainda não está instalado
  // neste momento, portanto não passa pela validação de executável/emulador do
  // customgame:add. Quando o download terminar, a entrada será atualizada sem
  // perder a posse da conta.
  ipcMain.handle("retro:libraryAdd", (_e, payload = {}) => {
    try {
      const id = typeof payload.id === "string" ? payload.id.trim() : ""
      const title = typeof payload.title === "string" ? payload.title.trim() : ""
      if (!/^retro:[a-z0-9][a-z0-9._:-]{1,500}$/i.test(id) || !title || title.length > 1024) {
        return { ok: false, error: "jogo retrô inválido" }
      }
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      const existing = all.find((game) => game.id === id)
      if (existing) return { ok: true, added: false, games: readLibrary() }
      const cleanList = (value, max = 32) => Array.isArray(value)
        ? value.filter((item) => typeof item === "string").map((item) => item.trim()).filter(Boolean).slice(0, max)
        : undefined
      all.push({
        id,
        title,
        launcher: "retro",
        platform: typeof payload.platform === "string" ? payload.platform.slice(0, 80) : "retro",
        systemId: typeof payload.systemId === "string" ? payload.systemId.slice(0, 120) : undefined,
        cover: typeof payload.cover === "string" ? payload.cover.slice(0, 2000) : undefined,
        hero: typeof payload.hero === "string" ? payload.hero.slice(0, 2000) : undefined,
        description: typeof payload.description === "string" ? payload.description.slice(0, 10000) : undefined,
        genres: cleanList(payload.genres),
        releaseYear: Number.isInteger(payload.releaseYear) ? payload.releaseYear : undefined,
        installed: false,
        retro: true,
      })
      fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(all, null, 2))
      ownedAdd(id)
      try { require("./supabase/biblioteca").agendarPush() } catch {}
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
      return { ok: true, added: true, games: readLibrary() }
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

  ipcMain.handle("retro:libraryRemove", (_e, id) => {
    try {
      const value = typeof id === "string" ? id.trim() : ""
      if (!/^retro:[a-z0-9][a-z0-9._:-]{1,500}$/i.test(value)) {
        return { ok: false, error: "jogo retrô inválido" }
      }
      const all = readJsonFile(caminhoConta(CUSTOM_GAMES), [])
      // Versões antigas salvaram alguns jogos Retro (incluindo entradas
      // importadas do LaunchBox) com launcher="custom". O namespace retro: é
      // a identidade canônica; exigir launcher="retro" tornava a remoção um
      // falso sucesso e deixava esses jogos presos na biblioteca.
      const rest = all.filter((game) => game.id !== value)
      if (rest.length === all.length) return { ok: true, games: readLibrary() }
      fs.writeFileSync(caminhoConta(CUSTOM_GAMES), JSON.stringify(rest, null, 2))
      ownedRemove(value)
      try { require("./supabase/biblioteca").agendarPush() } catch {}
      if (win && !win.isDestroyed()) win.webContents.send("library:changed")
      return { ok: true, games: readLibrary() }
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })

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

  // Registry/SDK local de plugins. Os canais antigos permanecem estáveis;
  // os novos só trafegam metadados sanitizados (nunca o caminho privado do
  // registro). O módulo plugins valida manifest, entry e permissões antes de
  // aceitar qualquer diretório informado pelo usuário.
  const pluginIdFromIpc = (value) => (typeof value === "string" ? value : "")
  const pluginPathFromIpc = (value) => {
    if (typeof value === "string") return value
    if (value && typeof value === "object" && typeof value.path === "string") return value.path
    return ""
  }
  const notifyPluginsChanged = () => {
    if (win && !win.isDestroyed()) win.webContents.send("plugins:changed")
  }
  ipcMain.handle("plugins:list", () => ({ ok: true, plugins: plugins.list() }))
  ipcMain.handle("plugins:details", () => ({ ok: true, plugins: plugins.listDetailed() }))
  ipcMain.handle("plugins:get", (_e, id) => {
    const plugin = plugins.get(pluginIdFromIpc(id))
    return plugin ? { ok: true, plugin } : { ok: false, error: "plugin_desconhecido" }
  })
  ipcMain.handle("plugins:register", (_e, value) => {
    const r = plugins.register(pluginPathFromIpc(value))
    if (r?.ok) notifyPluginsChanged()
    return r
  })
  ipcMain.handle("plugins:unregister", (_e, id) => {
    const r = plugins.unregister(pluginIdFromIpc(id))
    if (r?.ok) notifyPluginsChanged()
    return r
  })
  ipcMain.handle("plugins:enable", (_e, id) => {
    const r = plugins.enable(pluginIdFromIpc(id))
    if (r?.ok) notifyPluginsChanged()
    return r
  })
  ipcMain.handle("plugins:disable", (_e, id) => {
    const r = plugins.disable(pluginIdFromIpc(id))
    if (r?.ok) notifyPluginsChanged()
    return r
  })
  // Verifica o digest do entry sem expor o caminho privado do registro.
  ipcMain.handle("plugins:verify", (_e, id) => plugins.verify(pluginIdFromIpc(id)))
  ipcMain.handle("plugins:install", async (_e, id) => {
    const r = await plugins.install(pluginIdFromIpc(id))
    if (r?.ok) notifyPluginsChanged()
    return r
  })
  ipcMain.handle("plugins:remove", async (_e, id) => {
    const r = await plugins.remove(pluginIdFromIpc(id))
    if (r?.ok) notifyPluginsChanged()
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
    const retroPlatform = /^retro:([^:]+)/.exec(String(gameId || ""))?.[1] || null

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
      candidatos.push(...igdbArtDe(await igdbProxy(titulo || "", { plataforma: retroPlatform }), kind))
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
    // banner/background tratam como imagem de fundo (o mesmo filtro/diálogo)
    const ehFundo = kind === "background" || kind === "banner"
    const key = ehFundo ? "background" : "avatar"
    const res = await dialog.showOpenDialog(win, {
      title: ehFundo ? "Escolher imagem de fundo" : "Escolher foto de perfil",
      properties: ["openFile"],
      filters:
        ehFundo
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
    // banner salva em arquivo próprio (não colide com o background)
    const nome = kind === "banner" ? "banner" : key
    const dest = path.join(DATA_DIR, nome + ext)
    try {
      fs.copyFileSync(src, dest)
    } catch (e) {
      return { ok: false, error: String(e) }
    }
    // Salva o caminho LIMPO (file://) no config; o ?t= é só para atualizar a
    // visualização imediata (cache-buster), não deve ir para o disco.
    writeConfig({ profile: { [nome]: "file://" + dest } })
    return { ok: true, path: "file://" + dest + "?t=" + Date.now() }
  })

  ipcMain.handle("avatar:load", async (_e, url) => {
    try {
      if (!/^https?:\/\//i.test(String(url || ""))) return { ok: false }
      const r = await fetch(String(url), { headers: { accept: "image/*" } })
      if (!r.ok) return { ok: false, error: `HTTP ${r.status}` }
      const type = (r.headers.get("content-type") || "image/gif").split(";")[0]
      const buf = Buffer.from(await r.arrayBuffer())
      if (buf.length > 5 * 1024 * 1024) return { ok: false, error: "avatar_grande" }
      return { ok: true, src: `data:${type};base64,${buf.toString("base64")}` }
    } catch (e) {
      return { ok: false, error: String(e.message || e) }
    }
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

  // Se o monitor muda de resolução enquanto o Big Picture está aberto,
  // reaplica a escala sem alterar a preferência lógica do usuário.
  const atualizarEscalaDoDisplay = () => {
    if (win && !win.isDestroyed()) reapplyWindowZoom()
  }
  screen.on("display-metrics-changed", atualizarEscalaDoDisplay)
  screen.on("display-added", atualizarEscalaDoDisplay)
  screen.on("display-removed", atualizarEscalaDoDisplay)

  createWindow()

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
  if (minimizarTimer) {
    clearTimeout(minimizarTimer)
    minimizarTimer = null
  }
  discordRpc.close()
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
