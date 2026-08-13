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

  // Bins da Steam (schema + progresso por appid). A lista é de nomes de
  // arquivo, se o dir não existe fica vazia (o shape é sempre um array).
  const { findSteamDir } = require("./steam-path")
  const statsDir = path.join(findSteamDir(), "appcache", "stats")
  if (fs.existsSync(statsDir)) {
    out.steamBins = fs
      .readdirSync(statsDir)
      .filter((f) => /^UserGameStats/i.test(f))
      .sort()
  }

  // Estado do sync. Corrupto vira "(ilegivel)", não some: é exatamente o bug
  // que o diagnóstico existe pra mostrar.
  out.sync.estado = lerJson(caminhoArquivoConta("sync_state.json"))
  out.sync.fila = lerJson(caminhoArquivoConta("sync_queue.json"))

  // Erros recentes do debug.log (ultimas linhas)
  const linhas = lerJson(path.join(DATA_DIR, "logs", "debug.log"), (s) => s.trim().split("\n"))
  if (Array.isArray(linhas)) out.debugLog = linhas.slice(-20)
  return out
}

// Lê um arquivo de estado e devolve o valor parseado. Se faltar devolve
// undefined, se vier corrompido devolve "(ilegivel)" pra aparecer no dump em
// vez de sumir como se não existisse.
function lerJson(f, transform) {
  if (!fs.existsSync(f)) return undefined
  try {
    const raw = fs.readFileSync(f, "utf-8")
    return transform ? transform(raw) : JSON.parse(raw)
  } catch {
    return "(ilegivel)"
  }
}

function lenApps(f) {
  const d = lerJson(f)
  if (typeof d !== "object" || d === null || Array.isArray(d)) return "ilegivel"
  return { apps: Object.keys(d).length, itens: Object.values(d).reduce((n, e) => n + (e.items ? e.items.length : 0), 0) }
}

module.exports = { collect }
