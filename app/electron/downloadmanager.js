// Download manager: fila serial persistida, progresso parseado do stdout do
// runner (Legendary), pause/resume via sinal, eventos throttled para a UI.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn } = require("child_process")
const { RUNNERS_DIR, ensureLegendary } = require("./runners/download")

const DATA_DIR = path.join(os.homedir(), ".local/share/arcadia")
const QUEUE_FILE = path.join(DATA_DIR, "downloads.json")
const GAMES_DIR = path.join(DATA_DIR, "games")
const BIN = path.join(RUNNERS_DIR, "legendary")

// Linhas típicas do Legendary (0.20.34):
//   [DLManager] INFO: = Progress: 15.13% (1500/761), Running for 00:05:32, ETA: 00:02:11
//   [DLManager] INFO:  - Downloaded: 150.20 MiB, Written: 300.10 MiB
//   [DLManager] INFO:  + Download speed: 15.23 MiB/s
// O percentual é de ARQUIVOS; o progresso em MiB vem na linha "Downloaded".
const RE_PROGRESS = /Progress:\s*([\d.]+)%\s*\(([\d.]+)\/([\d.]+)\).*?ETA:?\s*([\d:]+)/i
const RE_DOWNLOADED = /Downloaded:\s*([\d.]+)\s*MiB/i
const RE_SPEED = /Download speed:\s*([\d.]+)\s*MiB\/s/i
// "[cli] INFO: Install path: /pasta/do/jogo" — pasta real da instalação
// (guardada para o cancel poder APAGAR os arquivos parciais).
const RE_INSTALL_PATH = /Install path:\s*(.+)$/im

let queue = []
let activeChild = null
let emitFn = null
let lastEmit = ""
let lastEmitAt = 0

// O Legendary é Python e usa multiprocessing (as linhas "[DLManager]" são dos
// processos-worker que baixam em paralelo). Matar/pausar só o PID do pai deixa
// os workers órfãos baixando — por isso o sinal tem que ir para o GRUPO todo.
// Com `detached: true` o filho vira líder de um novo grupo (setsid), e
// process.kill(-pid) atinge o grupo inteiro. Fallback: sinaliza só o filho.
function signalGroup(child, sig) {
  if (!child || !child.pid) return
  try {
    process.kill(-child.pid, sig) // -pid = grupo inteiro
  } catch {
    try { child.kill(sig) } catch {}
  }
}

function persist() {
  try {
    const sane = queue.map(({ appid, appName, title, cover, status, percent, done, total, eta, speed, error, installPath, installDir, engine, installdir, depots, token, dlcs, steamDir }) =>
      ({ appid, appName, title, cover, status, percent, done, total, eta, speed, error, installPath, installDir, engine, installdir, depots, token, dlcs, steamDir }))
    // Atômico (ver writeConfig): a fila é gravada a cada 3s durante o
    // download, então é justamente o arquivo com mais chance de ser pego
    // pela metade num fechamento abrupto.
    const tmp = `${QUEUE_FILE}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(sane, null, 2))
    fs.renameSync(tmp, QUEUE_FILE)
  } catch {}
}

function load() {
  try {
    queue = JSON.parse(fs.readFileSync(QUEUE_FILE, "utf-8"))
    // Ao abrir: o que estava "downloading" morreu com o app → vira "paused".
    // Concluídos não voltam (tela limpa — só fila ativa/erros).
    queue = queue.filter((it) => {
      if (it.status === "downloading") it.status = "paused"
      return it.status !== "done"
    })
    persist()
  } catch {
    queue = []
  }
}

// Emite a fila para a UI, só se algo visível mudou (throttle por conteúdo+tempo).
function emit(force = false) {
  if (!emitFn) return
  const snap = JSON.stringify(queue)
  const now = Date.now()
  if (!force && snap === lastEmit) return
  if (!force && now - lastEmitAt < 500) return
  lastEmit = snap
  lastEmitAt = now
  emitFn(queue)
}

function update(appid, patch) {
  const it = queue.find((q) => q.appid === appid)
  if (!it) return
  Object.assign(it, patch)
  persist()
  emit()
}

function next() {
  if (activeChild) return
  const it = queue.find((q) => q.status === "queued")
  if (!it) {
    persist()
    emit(true)
    return
  }
  it.status = "downloading"
  it.error = ""
  persist()
  emit(true)

  // Steam (engine "steam"): DepotDownloader via dotnet, estilo Acella.
  if (it.engine === "steam") {
    const ss = require("./steamstore")
    const appidLimpo = String(it.appid).replace(/^steam:/, "")
    // Itens antigos da fila podem não ter o size dos depots (total=0) —
    // re-busca o manifesto para ter total em MiB no progresso.
    const precisaSize = !(it.depots || []).length || (it.depots || []).some((d) => !d.size)
    const comDepots = precisaSize
      ? ss.getManifest(appidLimpo).then((m) => {
          if (m.ok && m.depots?.length) {
            it.depots = m.depots
            if (m.token) it.token = m.token
          }
        }).catch(() => {})
      : Promise.resolve()
    comDepots.then(() => {
      ss.prepareDownload({
        appid: appidLimpo,
        installdir: it.installdir,
        depots: it.depots || [],
        steamDir: it.steamDir || ss.findSteamDir(),
      }).then((prep) => {
        if (!prep.ok) return finish(it, "error", prep.error || "falha ao preparar o download")
        // Um processo por depot, em sequência (estilo Acella). A fila fica no
        // item para que pause/cancel/retomada saibam onde paramos.
        it.fila = prep.cmds
        it.filaIdx = 0
        // Só os depots que realmente vão baixar entram no total. Somando todos
        // (inclusive os pulados por falta de .manifest), a barra jamais
        // chegaria a 100% e o ETA ficaria eternamente errado.
        it.depotsBaixando = prep.cmds.map((c) => String(c.depotId))
        iniciarFilho(it, prep.cmds[0].cmd, prep.cmds[0].args)
      }).catch((e) => finish(it, "error", String(e)))
    })
    return
  }

  const args = ["install", it.appName, "--base-path", it.installPath || GAMES_DIR, "-y"]
  // "Máximo de núcleos da CPU durante downloads" (Config. Gerais; 0 = livre).
  try {
    const cfg = JSON.parse(fs.readFileSync(path.join(DATA_DIR, "config.json"), "utf-8"))
    const cores = Number(cfg.download_cpu_cores || 0)
    if (cores > 0) args.push("--max-workers", String(cores))
  } catch {}
  iniciarFilho(it, BIN, args)
}

// Spawna o processo do download (Legendary ou dotnet/DepotDownloader) e
// conecta parsing de progresso + encerramento. Grupo próprio p/ sinais.
function iniciarFilho(it, cmd, args) {
  // detached: cada download vira seu próprio grupo de processos, para o
  // cancel/pause conseguir matar/parar os workers junto do pai.
  activeChild = spawn(cmd, args, { env: { ...process.env }, detached: true })
  const child = activeChild
  it.pid = child.pid
  let percentMax = 0

  // Steam: total = soma dos depots (bytes do manifesto → MiB); baixado/veloc.
  // medidos pela pasta no disco a cada 3s (DepotDownloader não imprime isso).
  let poller = null
  let ultimoMiB = 0
  if (it.engine === "steam" && it.installDir) {
    const baixando = it.depotsBaixando ? new Set(it.depotsBaixando) : null
    const totalMiB = (it.depots || [])
      .filter((d) => !baixando || baixando.has(String(d.depotId)))
      .reduce((acc, d) => acc + (Number(d.size) || 0), 0) / (1024 * 1024)
    if (totalMiB > 0) update(it.appid, { total: Math.round(totalMiB) })
    ultimoMiB = dirSizeMiB(it.installDir)
    poller = setInterval(() => {
      const atual = dirSizeMiB(it.installDir)
      const speed = Math.max(0, (atual - ultimoMiB) / 3)
      ultimoMiB = atual
      const patch = { done: Math.round(atual), speed: Math.round(speed * 10) / 10 }
      if (totalMiB > 0) {
        patch.percent = Math.min(100, Math.round((atual / totalMiB) * 1000) / 10)
        patch.eta = speed > 0.1 ? fmtEta((totalMiB - atual) / speed) : ""
      }
      update(it.appid, patch)
    }, 3000)
  }

  // O Legendary emite o percentual de arquivos na linha "Progress" e os MiB
  // baixados na linha seguinte ("Downloaded"). Guardamos ambos.
  // Última linha de erro do DepotDownloader. Sem guardá-la, a UI só conseguia
  // dizer "código 1" e o motivo real (ex.: 401 em manifesto antigo com conta
  // anônima) se perdia — impossível diagnosticar sem rodar na mão.
  let ultimoErro = ""
  const onOut = (text) => {
    for (const linha of String(text).split("\n")) {
      if (/error|unable|aborting|401|403|denied|not completely/i.test(linha) && linha.trim()) {
        ultimoErro = linha.trim().slice(0, 300)
      }
    }
    const p = RE_PROGRESS.exec(text)
    if (p) {
      update(it.appid, {
        percent: parseFloat(p[1]),
        done: parseFloat(p[2]),
        total: parseFloat(p[3]),
        eta: p[4],
      })
    }
    const dl = RE_DOWNLOADED.exec(text)
    if (dl) {
      // done vira MiB baixados; total continua sendo o total de arquivos —
      // a UI mostra percent como número principal.
      update(it.appid, { doneMiB: parseFloat(dl[1]) })
    }
    const s = RE_SPEED.exec(text)
    if (s) update(it.appid, { speed: parseFloat(s[1]) })
    const ip = RE_INSTALL_PATH.exec(text)
    if (ip && !it.installDir) update(it.appid, { installDir: ip[1].trim() })
    // DepotDownloader: progresso genérico em % (pega o maior visto).
    if (it.engine === "steam") {
      const g = /(\d{1,3}(?:\.\d+)?)%/.exec(text)
      if (g) {
        const pct = Math.min(100, parseFloat(g[1]))
        if (pct > percentMax) {
          percentMax = pct
          if (!it.total) update(it.appid, { percent: pct })
        }
      }
    }
  }
  child.stdout.on("data", (d) => onOut(String(d)))
  child.stderr.on("data", (d) => onOut(String(d)))
  child.on("error", () => { if (poller) clearInterval(poller); finish(it, "error", "falha ao iniciar o download") })
  child.on("close", (code) => {
    if (poller) clearInterval(poller)
    if (it.status === "error" || it.status === "canceled") {
      activeChild = null
      next()
      return
    }
    if (it.status === "paused") {
      activeChild = null // pausado: fica na fila até dmResume
      return
    }
    const fila = it.fila || []
    if (fila.length) {
      // Um depot que falha não derruba o jogo inteiro — é assim que o Acella
      // trata (aviso e segue). Depots opcionais (idiomas, DLC) falham sozinhos
      // com frequência; só damos erro se NENHUM depot tiver baixado.
      if (code === 0) it.depotsOk = (it.depotsOk || 0) + 1
      else {
        it.depotsFalhos = it.depotsFalhos || []
        it.depotsFalhos.push(fila[it.filaIdx]?.depotId || "?")
        dlog(`depot ${fila[it.filaIdx]?.depotId} falhou (código ${code}) em ${it.title}: ${(ultimoErro || "").slice(0, 200)}`)
      }
      if (it.filaIdx < fila.length - 1) {
        it.filaIdx++
        activeChild = null
        it.pid = null
        const prox = fila[it.filaIdx]
        update(it.appid, { depotAtual: it.filaIdx + 1, depotsTotal: fila.length })
        return iniciarFilho(it, prox.cmd, prox.args)
      }
      if (it.depotsOk) return finish(it, "done")
      return finish(it, "error", ultimoErro || `download falhou (código ${code})`)
    }
    if (code === 0) finish(it, "done")
    else finish(it, "error", ultimoErro || `download falhou (código ${code})`)
  })
}

// Tamanho da pasta em MiB (du -sm) — base do progresso dos downloads Steam.
function dirSizeMiB(dir) {
  try {
    const { execFileSync } = require("child_process")
    const out = execFileSync("du", ["-sm", dir], { encoding: "utf-8", timeout: 10000 })
    return parseInt(out.split("\t")[0], 10) || 0
  } catch {
    return 0
  }
}

function fmtEta(seg) {
  const s = Math.max(0, Math.round(seg))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const ss = s % 60
  return h > 0 ? `${h}h${String(m).padStart(2, "0")}m` : `${String(m).padStart(2, "0")}:${String(ss).padStart(2, "0")}`
}

function finish(it, status, error = "") {
  activeChild = null
  update(it.appid, { status, error, speed: 0 })
  emit(true)
  // Download concluído: avisa o main (reindex + refresh da biblioteca) e
  // tira o item da fila logo depois — a tela fica só com o que interessa.
  if (status === "done") {
    if (doneFn) {
      try { doneFn(it) } catch {}
    }
    setTimeout(() => {
      queue = queue.filter((q) => q.appid !== it.appid)
      persist()
      emit(true)
    }, 6000)
  }
  next()
}

async function install({ appid, title, cover, installPath }) {
  const appName = String(appid).replace(/^epic:/, "")
  if (!appName || appName === appid) return { ok: false, error: "não é um jogo Epic" }
  if (queue.some((q) => q.appid === appid && ["queued", "downloading", "paused"].includes(q.status))) {
    return { ok: true } // já está na fila
  }
  // Um item que falhou continuava na fila e o novo pedido criava um SEGUNDO
  // card com o mesmo appid — chave duplicada no React, tela bagunçada. Só
  // pode existir um item por jogo: o antigo sai.
  queue = queue.filter((q) => q.appid !== appid)
  await ensureLegendary()
  const destino = installPath || GAMES_DIR
  fs.mkdirSync(destino, { recursive: true })
  queue.push({
    appid, appName, title, cover,
    status: "queued", percent: 0, done: 0, total: 0, eta: "", speed: 0, error: "",
    installPath: destino,
  })
  persist()
  emit(true)
  next()
  return { ok: true }
}

// Jogo Steam via DepotDownloader (estilo Acella): payload vem de store:install.
async function installSteam({ appid, title, cover, installdir, depots, token, dlcs, steamDir }) {
  const id = `steam:${appid}`
  if (queue.some((q) => q.appid === id && ["queued", "downloading", "paused"].includes(q.status))) {
    return { ok: true }
  }
  queue = queue.filter((q) => q.appid !== id) // ver comentário em install()
  const ss = require("./steamstore")
  const dir = steamDir || ss.findSteamDir()
  queue.push({
    appid: id, appName: String(appid), title, cover,
    engine: "steam", installdir, depots, token, dlcs, steamDir: dir,
    status: "queued", percent: 0, done: 0, total: 0, eta: "", speed: 0, error: "",
    installPath: path.join(dir, "steamapps", "common"),
    installDir: path.join(dir, "steamapps", "common", installdir),
  })
  persist()
  emit(true)
  next()
  return { ok: true }
}

// Tenta de novo um download que falhou: mantém o item (com o destino e os
// depots já escolhidos) e apenas o recoloca na fila, zerando o erro.
function retry(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it || it.status !== "error") return
  it.fila = null
  it.filaIdx = 0
  it.depotsOk = 0
  it.depotsFalhos = []
  update(appid, { status: "queued", error: "", percent: 0, done: 0, speed: 0, eta: "" })
  next()
}

// Tira da lista um item já finalizado (erro/concluído). Não mexe em disco.
function descartar(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it || ["downloading", "queued", "paused"].includes(it.status)) return
  queue = queue.filter((q) => q.appid !== appid)
  persist()
  emit(true)
}

function pause(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it) return
  if (it.status === "downloading" && activeChild) {
    signalGroup(activeChild, "SIGSTOP") // para o grupo inteiro, não só o pai
    update(appid, { status: "paused", speed: 0 })
  } else if (it.status === "queued") {
    update(appid, { status: "paused" })
  }
}

function resume(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it || it.status !== "paused") return
  if (activeChild && it.pid) {
    signalGroup(activeChild, "SIGCONT") // retoma o grupo inteiro
    update(appid, { status: "downloading" })
    return
  }
  // Sem processo vivo (app reiniciado): reentra na fila — o Legendary retoma
  // do ponto em que parou, os arquivos parciais ficam no disco.
  update(appid, { status: "queued" })
  next()
}

// Log diagnóstico do manager (cancel/uninstall) em logs/downloads.log.
const LOG_DIR = path.join(DATA_DIR, "logs")
function dlog(msg) {
  try {
    fs.mkdirSync(LOG_DIR, { recursive: true })
    fs.appendFileSync(path.join(LOG_DIR, "downloads.log"), `[${new Date().toISOString()}] ${msg}\n`)
  } catch {}
}

// Aguarda o processo (e o grupo) realmente morrer, para não apagar a pasta
// enquanto os workers ainda escrevem nela.
function waitExit(child, ms = 5000) {
  return new Promise((res) => {
    if (!child || child.exitCode !== null || child.signalCode) return res()
    let done = false
    const fin = () => { if (!done) { done = true; res() } }
    child.once("close", fin)
    child.once("exit", fin)
    setTimeout(fin, ms)
  })
}

async function cancel(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it) return
  dlog(`cancel: ${it.title} (${appid}) status=${it.status} installPath=${it.installPath}`)
  // Download ativo (baixando OU pausado via SIGSTOP): mata o GRUPO de processos
  // — o pai Legendary E os workers do multiprocessing. Sem atingir o grupo, os
  // workers ficavam órfãos baixando (e reescrevendo a pasta após o rmSync).
  if (activeChild && (it.status === "downloading" || it.status === "paused")) {
    const child = activeChild
    update(appid, { status: "canceled" })
    signalGroup(child, "SIGCONT") // destrava se estava pausado (senão ignora KILL)
    signalGroup(child, "SIGKILL")
    await waitExit(child) // só apaga quando os workers já morreram
  }
  queue = queue.filter((q) => q.appid !== appid)
  persist()
  emit(true)

  // Cancelar = começar do zero. O Legendary registra o jogo em installed.json
  // ANTES do download terminar — enquanto o registro existir, reinstalar
  // RESUME dos parciais (mesmo apagando a pasta). Por isso o passo principal
  // é o `legendary uninstall`, que remove o registro E os arquivos.
  if (it.appName && it.engine !== "steam") {
    await new Promise((res) => {
      const u = spawn(BIN, ["uninstall", "-y", it.appName], { stdio: ["ignore", "pipe", "pipe"] })
      let out = ""
      u.stdout.on("data", (d) => (out += d))
      u.stderr.on("data", (d) => (out += d))
      u.on("close", (code) => {
        dlog(`cancel: legendary uninstall ${it.appName} exit=${code} out=${out.trim().slice(0, 300)}`)
        res()
      })
      u.on("error", (e) => {
        dlog(`cancel: uninstall erro ${e}`)
        res()
      })
      setTimeout(res, 120000)
    })
  }

  // Fallback: varre o installPath atrás de <título> ou <título><4-8 alnum>
  // (o Legendary pode nomear a pasta com sufixo aleatório, ex.
  // "ViewfinderXGGk9"). Segurança: só apaga subpastas do installPath da fila
  // ou do GAMES_DIR.
  const base = it.installPath || GAMES_DIR
  const baseOk = base.startsWith(GAMES_DIR) || fs.existsSync(base)
  const slug = String(it.title || "").replace(/[^A-Za-z0-9]/g, "")
  const re = slug ? new RegExp(`^${slug}([A-Za-z0-9]{4,8})?$`) : null
  const candidatos = [it.installDir].filter(Boolean)
  try {
    for (const d of fs.readdirSync(base, { withFileTypes: true })) {
      if (d.isDirectory() && re && re.test(d.name)) candidatos.push(path.join(base, d.name))
    }
  } catch {}
  for (const alvo of new Set(candidatos)) {
    const dentro = alvo.startsWith(base.endsWith(path.sep) ? base : base + path.sep) || alvo.startsWith(GAMES_DIR + path.sep)
    if (baseOk && alvo && alvo !== "/" && dentro && fs.existsSync(alvo)) {
      try {
        fs.rmSync(alvo, { recursive: true, force: true })
        dlog(`cancel: pasta apagada ${alvo}`)
      } catch (e) {
        dlog(`cancel: falha ao apagar ${alvo}: ${e}`)
      }
    }
  }
}

function getQueue() {
  return queue
}

// Chamado pelo main: fn recebe a fila inteira a cada mudança.
function onProgress(fn) {
  emitFn = fn
}

// Chamado pelo main: fn(item) quando um download termina com sucesso.
let doneFn = null
function onDone(fn) {
  doneFn = fn
}

// Encerramento do app: mata o download ativo (grupo inteiro). Como agora os
// downloads são detached, sem isto o Legendary sobreviveria órfão ao fechar.
function killActive() {
  if (activeChild) signalGroup(activeChild, "SIGKILL")
}

load()
module.exports = { install, installSteam, pause, resume, retry, descartar, cancel, getQueue, onProgress, onDone, killActive }
