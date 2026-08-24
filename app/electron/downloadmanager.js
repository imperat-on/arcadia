// Download manager: fila serial persistida, progresso parseado do stdout do
// runner (Legendary), pause/resume via sinal, eventos throttled para a UI.

const fs = require("fs")
const path = require("path")
const { spawn } = require("child_process")
const { RUNNERS_DIR, ensureLegendary } = require("./runners/download")
const { getDataDir } = require("./runtime-paths")
const { nextQueued, normalizePriority } = require("./download-queue-policy")
const {
  DEFAULT_MAX_RECOVERY_ATTEMPTS,
  normalizeRecoveryAttempts,
  recoveryDecision,
  verificationCommand,
  verificationOutputLooksFailed,
  integrityMode,
} = require("./download-integrity")

const DATA_DIR = getDataDir()
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
let recoveryTimer = null

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
    try {
      child.kill(sig)
    } catch {}
  }
}

function persist() {
  try {
    const sane = queue.map(
      ({
        appid,
        appName,
        title,
        cover,
        status,
        percent,
        done,
        total,
        eta,
        speed,
        error,
        installPath,
        installDir,
        engine,
        installdir,
        depots,
        token,
        dlcs,
        steamDir,
        priority,
        integrity,
        recoveryAttempts,
        depotAtualId,
        depotsOk,
        depotsFalhos,
      }) => ({
        appid,
        appName,
        title,
        cover,
        status,
        percent,
        done,
        total,
        eta,
        speed,
        error,
        installPath,
        installDir,
        engine,
        installdir,
        depots,
        token,
        dlcs,
        steamDir,
        priority: normalizePriority(priority),
        integrity,
        recoveryAttempts: normalizeRecoveryAttempts(recoveryAttempts),
        depotAtualId,
        depotsOk,
        depotsFalhos,
      }),
    )
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
    if (!Array.isArray(queue)) queue = []
    queue = queue.map((item) => ({
      ...item,
      priority: normalizePriority(item?.priority),
      recoveryAttempts: normalizeRecoveryAttempts(item?.recoveryAttempts),
      integrity:
        item?.integrity && typeof item.integrity === "object"
          ? { ...item.integrity, method: item.integrity.method || integrityMode(item) }
          : { state: "pending", method: integrityMode(item) },
    }))
    // Ao abrir: o que estava "downloading" morreu com o app → vira "paused".
    // Concluídos não voltam (tela limpa — só fila ativa/erros). A fase de
    // verificação também precisa voltar a pausado: sem isto um processo morto
    // poderia ser confundido com uma instalação íntegra.
    queue = queue.filter((it) => {
      if (it.status === "downloading") {
        it.status = "paused"
        if (it.integrity?.state === "verifying") it.integrity.state = "paused"
      }
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
  const it = nextQueued(queue)
  if (!it) {
    persist()
    emit(true)
    return
  }
  it.status = "downloading"
  it.error = ""
  it.integrity = {
    ...(it.integrity && typeof it.integrity === "object" ? it.integrity : {}),
    state: "downloading",
    method: integrityMode(it),
  }
  persist()
  emit(true)

  // Steam (engine "steam"): DepotDownloader via dotnet, estilo Acella.
  if (it.engine === "steam") {
    const ss = require("./steamstore")
    const appidLimpo = String(it.appid).replace(/^steam:/, "")

    // Uma falha em um depot não pode transformar uma instalação incompleta em
    // "done". O manager tenta o item inteiro novamente (o DepotDownloader
    // valida e reaproveita os blocos bons) e só avança quando todos os depots
    // selecionados terminarem com código 0.
    const filaAtual = Array.isArray(it.fila) && it.fila.length ? it.fila : null
    if (filaAtual && Number.isInteger(it.filaIdx) && filaAtual[it.filaIdx]) {
      const atual = filaAtual[it.filaIdx]
      it.depotAtualId = String(atual.depotId || "")
      persist()
      iniciarFilho(it, atual.cmd, atual.args, { phase: "depot" })
      return
    }

    // Itens antigos da fila podem não ter o size dos depots (total=0) —
    // re-busca o manifesto para ter total em MiB no progresso.
    const precisaSize = !(it.depots || []).length || (it.depots || []).some((d) => !d.size)
    const comDepots = precisaSize
      ? ss
          .getManifest(appidLimpo)
          .then((m) => {
            if (m.ok && m.depots?.length) {
              it.depots = m.depots
              if (m.token) it.token = m.token
            }
          })
          .catch(() => {})
      : Promise.resolve()
    comDepots.then(() => {
      // O usuário pode cancelar/pausar enquanto o manifesto é consultado.
      // Não ressuscitar um processo depois que o item saiu da fila.
      if (!queue.includes(it) || it.status !== "downloading") return
      Promise.resolve()
        // prepareDownload é síncrono hoje, mas manter a fronteira Promise
        // permite que uma versão futura faça I/O sem quebrar a fila.
        .then(() =>
          ss.prepareDownload({
            appid: appidLimpo,
            installdir: it.installdir,
            depots: it.depots || [],
            steamDir: it.steamDir || ss.findSteamDir(),
          }),
        )
        .then((prep) => {
          if (!queue.includes(it) || it.status !== "downloading") return
          if (!prep.ok) return finish(it, "error", prep.error || "falha ao preparar o download")
          // Um processo por depot, em sequência (estilo Acella). A fila fica no
          // item em memória; comandos externos nunca são persistidos em JSON.
          it.fila = prep.cmds
          it.filaIdx = 0
          it.depotsOk = 0
          it.depotsFalhos = []
          // Só os depots que realmente vão baixar entram no total. Somando todos
          // (inclusive os pulados por falta de .manifest), a barra jamais
          // chegaria a 100% e o ETA ficaria eternamente errado.
          it.depotsBaixando = prep.cmds.map((c) => String(c.depotId))
          it.depotAtualId = String(prep.cmds[0].depotId || "")
          persist()
          iniciarFilho(it, prep.cmds[0].cmd, prep.cmds[0].args, { phase: "depot" })
        })
        .catch((e) => finish(it, "error", String(e)))
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
  iniciarFilho(it, BIN, args, { phase: "download" })
}

// Spawna o processo do download (Legendary ou dotnet/DepotDownloader) e
// conecta parsing de progresso + encerramento. Grupo próprio p/ sinais.
function iniciarFilho(it, cmd, args, { phase = "download" } = {}) {
  // detached: cada download vira seu próprio grupo de processos, para o
  // cancel/pause conseguir matar/parar os workers junto do pai.
  activeChild = spawn(cmd, args, { env: { ...process.env }, detached: true })
  const child = activeChild
  it.pid = child.pid
  let percentMax = 0
  let finalizado = false

  // Steam: total = soma dos depots (bytes do manifesto → MiB); baixado/veloc.
  // medidos pela pasta no disco a cada 3s (DepotDownloader não imprime isso).
  let poller = null
  let ultimoMiB = 0
  if (phase === "depot" && it.engine === "steam" && it.installDir) {
    const baixando = it.depotsBaixando ? new Set(it.depotsBaixando) : null
    const totalMiB =
      (it.depots || [])
        .filter((d) => !baixando || baixando.has(String(d.depotId)))
        .reduce((acc, d) => acc + (Number(d.size) || 0), 0) /
      (1024 * 1024)
    if (totalMiB > 0) update(it.appid, { total: Math.round(totalMiB) })
    dirSizeMiB(it.installDir, (mi) => {
      ultimoMiB = mi
    })
    poller = setInterval(() => {
      dirSizeMiB(it.installDir, (atual) => {
        const speed = Math.max(0, (atual - ultimoMiB) / 3)
        ultimoMiB = atual
        const patch = { done: Math.round(atual), speed: Math.round(speed * 10) / 10 }
        if (totalMiB > 0) {
          patch.percent = Math.min(100, Math.round((atual / totalMiB) * 1000) / 10)
          patch.eta = speed > 0.1 ? fmtEta((totalMiB - atual) / speed) : ""
        }
        update(it.appid, patch)
      })
    }, 3000)
  }

  // O Legendary emite o percentual de arquivos na linha "Progress" e os MiB
  // baixados na linha seguinte ("Downloaded"). Guardamos ambos. O output da
  // verificação também é guardado para que o código de erro seja útil, mas
  // nunca é tratado como prova de integridade: quem decide é o exit code do
  // comando nativo.
  let ultimoErro = ""
  let verifyOutput = ""
  const onOut = (text) => {
    const chunk = String(text)
    if (phase === "verify") verifyOutput = (verifyOutput + chunk).slice(-16000)
    for (const linha of chunk.split("\n")) {
      if (/error|unable|aborting|401|403|denied|not completely/i.test(linha) && linha.trim()) {
        ultimoErro = linha.trim().slice(0, 300)
      }
    }
    if (phase === "verify") return
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

  const tratarFechamento = (code, signal, spawnError = "") => {
    if (finalizado) return
    finalizado = true
    if (poller) clearInterval(poller)

    // O processo pode ter sido encerrado pelo cancelamento/pausa. Nunca
    // converta uma ação explícita do usuário em retry automático.
    if (it.status === "error" || it.status === "canceled") {
      activeChild = null
      it.pid = null
      next()
      return
    }
    if (it.status === "paused") {
      activeChild = null // pausado: fica na fila até dmResume
      it.pid = null
      persist()
      return
    }

    const detail = spawnError || ultimoErro
    // Alguns wrappers antigos imprimem "failed" e ainda saem 0. O código é
    // a fonte principal, mas um marcador explícito de corrupção/incompleto na
    // saída nunca pode virar instalação íntegra.
    const outputInvalid = phase === "verify" && verificationOutputLooksFailed(verifyOutput)
    const decision = recoveryDecision({
      phase,
      engine: it.engine,
      code: outputInvalid ? 1 : code,
      signal,
      status: it.status,
      attempts: it.recoveryAttempts,
      maxAttempts: DEFAULT_MAX_RECOVERY_ATTEMPTS,
      error: detail,
    })

    // Cada depot só é considerado íntegro quando o DepotDownloader (com
    // -validate) retorna 0. Uma falha invalida a instalação inteira; os blocos
    // já bons permanecem no disco e são reaproveitados no próximo retry.
    if (phase === "depot") {
      const fila = it.fila || []
      const depot = fila[it.filaIdx]
      if (decision.action !== "done") {
        it.depotsFalhos = it.depotsFalhos || []
        it.depotsFalhos.push(String(depot?.depotId || "?"))
        dlog(
          `depot ${depot?.depotId || "?"} falhou (código ${code}) em ${it.title}: ${(detail || "").slice(0, 200)}`,
        )
        // Comandos externos são descartados antes de persistir. O próximo
        // ciclo prepara um manifesto novo e não executa JSON adulterado.
        it.fila = null
        it.filaIdx = 0
        it.depotAtualId = ""
        it.depotsOk = 0
      } else {
        it.depotsOk = (it.depotsOk || 0) + 1
        if (it.filaIdx < fila.length - 1) {
          it.filaIdx++
          activeChild = null
          it.pid = null
          const prox = fila[it.filaIdx]
          it.depotAtualId = String(prox.depotId || "")
          update(it.appid, {
            depotAtual: it.filaIdx + 1,
            depotsTotal: fila.length,
            integrity: { ...(it.integrity || {}), state: "downloading" },
          })
          persist()
          return iniciarFilho(it, prox.cmd, prox.args, { phase: "depot" })
        }
        // Todos os depots selecionados foram validados. Só agora o item pode
        // entrar no callback onDone que registra a instalação na biblioteca.
        it.integrity = {
          ...(it.integrity || {}),
          state: "verified",
          method: "depot-manifest",
          verifiedAt: new Date().toISOString(),
        }
        activeChild = null
        it.pid = null
        persist()
        return finish(it, "done")
      }
    }

    activeChild = null
    it.pid = null
    if (decision.action === "verify") {
      const verify = verificationCommand(it, BIN, GAMES_DIR)
      if (!verify) {
        it.integrity = { ...(it.integrity || {}), state: "failed", method: integrityMode(it) }
        return finish(it, "error", "verificação de integridade indisponível")
      }
      it.integrity = { ...(it.integrity || {}), state: "verifying", method: integrityMode(it) }
      persist()
      emit(true)
      return iniciarFilho(it, verify.cmd, verify.args, { phase: "verify" })
    }
    if (decision.action === "done") {
      it.integrity = {
        ...(it.integrity || {}),
        state: "verified",
        method: integrityMode(it),
        verifiedAt: new Date().toISOString(),
      }
      persist()
      return finish(it, "done")
    }
    if (decision.action === "retry") return scheduleRecovery(it, decision)

    it.integrity = { ...(it.integrity || {}), state: "failed", method: integrityMode(it) }
    finish(it, "error", decision.error)
  }

  child.stdout.on("data", (d) => onOut(String(d)))
  child.stderr.on("data", (d) => onOut(String(d)))
  child.on("error", (error) => tratarFechamento(null, null, String(error?.message || error)))
  child.on("close", (code, signal) => tratarFechamento(code, signal))
}

// Recoloca um item em fila sem apagar parciais. Atraso curto + limite de
// tentativas evita loops infinitos em credenciais/manifests inválidos e dá à
// rede uma chance de se recuperar. O método é deliberadamente interno: IPC
// continua expondo apenas dm:retry para uma nova tentativa manual.
function scheduleRecovery(it, decision) {
  activeChild = null
  it.status = "queued"
  it.recoveryAttempts = normalizeRecoveryAttempts(decision.attempts)
  it.error = `falha: ${decision.error}; nova tentativa ${it.recoveryAttempts}/${DEFAULT_MAX_RECOVERY_ATTEMPTS}`
  it.integrity = {
    ...(it.integrity && typeof it.integrity === "object" ? it.integrity : {}),
    state: "retrying",
    method: integrityMode(it),
    lastError: decision.error,
  }
  // Uma falha Steam já limpou `fila`; para Epic os arquivos parciais também
  // ficam no lugar e o Legendary retoma/valida no próximo install.
  persist()
  emit(true)
  if (recoveryTimer) clearTimeout(recoveryTimer)
  recoveryTimer = setTimeout(() => {
    recoveryTimer = null
    if (queue.includes(it) && it.status === "queued") next()
  }, Math.max(0, Number(decision.delayMs) || 0))
  // Não manter o processo Node/Electron aberto somente por este timer.
  recoveryTimer.unref?.()
}

// Tamanho da pasta em MiB (du -sm) — base do progresso dos downloads Steam.
// Assíncrono: em pastas grandes/ativas o `du` pode levar um tempo perceptível,
// e a versão síncrona travava o processo principal do Electron (IPC, janela,
// vigia de jogo) a cada 3s durante o download inteiro.
function dirSizeMiB(dir, cb) {
  const { execFile } = require("child_process")
  execFile("du", ["-sm", dir], { encoding: "utf-8", timeout: 10000 }, (err, out) => {
    if (err) return cb(0)
    cb(parseInt(String(out).split("\t")[0], 10) || 0)
  })
}

function fmtEta(seg) {
  const s = Math.max(0, Math.round(seg))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const ss = s % 60
  return h > 0
    ? `${h}h${String(m).padStart(2, "0")}m`
    : `${String(m).padStart(2, "0")}:${String(ss).padStart(2, "0")}`
}

function finish(it, status, error = "") {
  activeChild = null
  const patch = { status, error, speed: 0 }
  if (status === "error") {
    patch.integrity = {
      ...(it.integrity && typeof it.integrity === "object" ? it.integrity : {}),
      state: "failed",
      method: integrityMode(it),
    }
  }
  update(it.appid, patch)
  emit(true)
  // Download concluído: avisa o main (reindex + refresh da biblioteca) e
  // tira o item da fila logo depois — a tela fica só com o que interessa.
  if (status === "done") {
    if (doneFn) {
      try {
        doneFn(it)
      } catch {}
    }
    setTimeout(() => {
      queue = queue.filter((q) => q.appid !== it.appid)
      persist()
      emit(true)
    }, 6000)
  }
  next()
}

async function install({ appid, title, cover, installPath, priority = 0 }) {
  const appName = String(appid).replace(/^epic:/, "")
  if (!appName || appName === appid) return { ok: false, error: "não é um jogo Epic" }
  if (
    queue.some((q) => q.appid === appid && ["queued", "downloading", "paused"].includes(q.status))
  ) {
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
    appid,
    appName,
    title,
    cover,
    status: "queued",
    percent: 0,
    done: 0,
    total: 0,
    eta: "",
    speed: 0,
    error: "",
    installPath: destino,
    integrity: { state: "pending", method: "legendary-verify" },
    recoveryAttempts: 0,
    priority: normalizePriority(priority),
  })
  persist()
  emit(true)
  next()
  return { ok: true }
}

// Jogo Steam via DepotDownloader (estilo Acella): payload vem de store:install.
async function installSteam({ appid, title, cover, installdir, depots, token, dlcs, steamDir, priority = 0 }) {
  const id = `steam:${appid}`
  if (queue.some((q) => q.appid === id && ["queued", "downloading", "paused"].includes(q.status))) {
    return { ok: true }
  }
  queue = queue.filter((q) => q.appid !== id) // ver comentário em install()
  const ss = require("./steamstore")
  installdir = ss.sanitizeInstallDir(installdir)
  const dir = steamDir || ss.findSteamDir()
  queue.push({
    appid: id,
    appName: String(appid),
    title,
    cover,
    engine: "steam",
    installdir,
    depots,
    token,
    dlcs,
    steamDir: dir,
    status: "queued",
    percent: 0,
    done: 0,
    total: 0,
    eta: "",
    speed: 0,
    error: "",
    installPath: path.join(dir, "steamapps", "common"),
    installDir: path.join(dir, "steamapps", "common", installdir),
    integrity: { state: "pending", method: "depot-manifest" },
    recoveryAttempts: 0,
    priority: normalizePriority(priority),
  })
  persist()
  emit(true)
  next()
  return { ok: true }
}

function setPriority(appid, priority) {
  const item = queue.find((entry) => entry.appid === appid)
  if (!item || item.status === "done" || item.status === "error" || item.status === "canceled") return false
  item.priority = normalizePriority(priority)
  persist()
  emit(true)
  return true
}

// Tenta de novo um download que falhou: mantém o item (com o destino e os
// depots já escolhidos) e apenas o recoloca na fila, zerando o erro.
function retry(appid) {
  const it = queue.find((q) => q.appid === appid)
  if (!it || it.status !== "error") return
  it.fila = null
  it.filaIdx = 0
  it.depotAtualId = ""
  it.depotsOk = 0
  it.depotsFalhos = []
  it.recoveryAttempts = 0
  it.integrity = { state: "pending", method: integrityMode(it) }
  if (recoveryTimer) {
    clearTimeout(recoveryTimer)
    recoveryTimer = null
  }
  update(appid, {
    status: "queued",
    error: "",
    percent: 0,
    done: 0,
    speed: 0,
    eta: "",
    integrity: it.integrity,
  })
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
    const fin = () => {
      if (!done) {
        done = true
        res()
      }
    }
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
    // Tira da fila e avisa a tela ANTES de esperar o processo morrer. O
    // update() abaixo passava pelo throttle do emit e podia não sair, e o
    // emit forçado só vinha depois do waitExit — até 5s em que o usuário já
    // tinha apertado e nada mudava na tela.
    update(appid, { status: "canceled" })
    queue = queue.filter((q) => q.appid !== appid)
    persist()
    emit(true)
    signalGroup(child, "SIGCONT") // destrava se estava pausado (senão ignora KILL)
    signalGroup(child, "SIGKILL")
    await waitExit(child) // só apaga os arquivos quando os workers já morreram
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
        dlog(
          `cancel: legendary uninstall ${it.appName} exit=${code} out=${out.trim().slice(0, 300)}`,
        )
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
    const dentro =
      alvo.startsWith(base.endsWith(path.sep) ? base : base + path.sep) ||
      alvo.startsWith(GAMES_DIR + path.sep)
    if (baseOk && alvo && alvo !== "/" && dentro && fs.existsSync(alvo)) {
      try {
        fs.rmSync(alvo, { recursive: true, force: true })
        dlog(`cancel: pasta apagada ${alvo}`)
      } catch (e) {
        dlog(`cancel: falha ao apagar ${alvo}: ${e}`)
      }
    }
  }
  // Cancelar um item QUEUED não passa pelo handler de close, que é quem
  // normalmente chama next() — sem isto o próximo da fila ficava parado.
  next()
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
  if (!activeChild) return
  // Fechamento do Electron não é uma falha de rede: marca pausado antes do
  // SIGKILL para que o evento close não agende retry e não ressuscite o grupo
  // durante o shutdown. O próximo boot poderá retomar explicitamente.
  const it = queue.find((q) => q.pid === activeChild.pid)
  if (it && it.status === "downloading") {
    it.status = "paused"
    it.integrity = { ...(it.integrity || {}), state: "paused", method: integrityMode(it) }
    persist()
  }
  signalGroup(activeChild, "SIGKILL")
}

load()
module.exports = {
  install,
  installSteam,
  setPriority,
  pause,
  resume,
  retry,
  descartar,
  cancel,
  getQueue,
  onProgress,
  onDone,
  killActive,
}
