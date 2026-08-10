// Diagnóstico do estado interno do Arcadia pra caçar bug sem reproduzir
// interativo. Roda com `electron . --diagnostico` (imprime e sai) ou como
// IPC `app:diagnostico` (devolve JSON pro renderer).
//
// O que mostra: conta ativa e o caminho de dados em uso, quais arquivos de
// conquistas existem por escopo, quais UserGameStatsSchema_*.bin a Steam tem,
// e a fila/estado do sync. Cada linha ajuda a responder "onde isso falhou".

const fs = require("fs")
const path = require("path")
const os = require("os")

function collect() {
  const { conta, caminhoArquivoConta, DATA_DIR } = require("./supabase/conta")
  const out = {
    contaAtiva: conta() || "guest (raiz)",
    dataDir: DATA_DIR,
    sessionExiste: fs.existsSync(path.join(DATA_DIR, "session.json")),
    achievements: {},
    steamBins: [],
    sync: {},
  }

  // achievements.json por escopo (raiz + cada conta)
  const raiz = path.join(DATA_DIR, "achievements.json")
  if (fs.existsSync(raiz)) out.achievements.raiz = lenApps(raiz)
  const contasDir = path.join(DATA_DIR, "contas")
  if (fs.existsSync(contasDir)) {
    out.achievements.contas = {}
    for (const u of fs.readdirSync(contasDir)) {
      const f = path.join(contasDir, u, "achievements.json")
      if (fs.existsSync(f)) out.achievements.contas[u] = lenApps(f)
    }
  }

  // Bins da Steam (schema + progresso por appid)
  const statsDir = path.join(os.homedir(), ".local/share/Steam/appcache/stats")
  if (fs.existsSync(statsDir)) {
    out.steamBins = fs
      .readdirSync(statsDir)
      .filter((f) => /^UserGameStats/i.test(f))
      .sort()
  } else {
    out.steamBins = ["(stats dir nao existe)"]
  }

  // Estado do sync
  const syncState = caminhoArquivoConta("sync_state.json")
  if (fs.existsSync(syncState)) {
    try {
      out.sync.estado = JSON.parse(fs.readFileSync(syncState, "utf-8"))
    } catch {}
  }
  const syncQueue = caminhoArquivoConta("sync_queue.json")
  if (fs.existsSync(syncQueue)) {
    try {
      out.sync.fila = JSON.parse(fs.readFileSync(syncQueue, "utf-8"))
    } catch {}
  }

  // Erros recentes do debug.log (ultimas linhas)
  const debugLog = path.join(DATA_DIR, "logs", "debug.log")
  if (fs.existsSync(debugLog)) {
    try {
      const linhas = fs.readFileSync(debugLog, "utf-8").trim().split("\n")
      out.debugLog = linhas.slice(-20)
    } catch {}
  }
  return out
}

function lenApps(f) {
  try {
    const d = JSON.parse(fs.readFileSync(f, "utf-8"))
    return { apps: Object.keys(d).length, itens: Object.values(d).reduce((n, e) => n + (e.items ? e.items.length : 0), 0) }
  } catch {
    return "ilegivel"
  }
}

module.exports = { collect }

// Roda standalone: electron . --diagnostico → imprime JSON e sai.
if (require.main === module) {
  const { app } = require("electron")
  app.whenReady().then(() => {
    const out = collect()
    console.log("== ARCADIA DIAGNOSTICO ==")
    for (const [k, v] of Object.entries(out)) {
      console.log(k + ":", JSON.stringify(v, null, 2))
    }
    app.quit()
  })
}
