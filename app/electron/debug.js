// Log de debug centralizado, best-effort. Nunca lança nem bloqueia: se o
// arquivo não der pra abrir, engole em silêncio (log é apoio, não o negócio).
// Escreve em logs/debug.log (respeita ARCADIA_DATA_DIR) com timestamp e
// prefixo por módulo, pra caçar falha sem ter que reproduzir interativo.

const fs = require("fs")
const path = require("path")
const { getDataDir } = require("./runtime-paths")

const DATA_DIR = getDataDir()
const FILE = path.join(DATA_DIR, "logs", "debug.log")
const MAX_BYTES = 1024 * 1024

// Dir de logs já criado: o mkdir é um syscall, e log() roda em paths de erro
// (fallback de schema, migração de conta). Só cria a primeira vez.
let dirPronto = false

function safe(v) {
  if (v instanceof Error) return v.stack || v.message
  if (typeof v === "string") return v
  try {
    return JSON.stringify(v)
  } catch {
    return String(v)
  }
}

function log(mod, v) {
  const linha = `${new Date().toISOString()} [${mod}] ${safe(v)}\n`
  try {
    if (!dirPronto) {
      fs.mkdirSync(path.dirname(FILE), { recursive: true })
      dirPronto = true
    }
    // Bounding simples: debug.log não pode crescer pra sempre. Se passou de
    // ~1MB, trunca pro começo e recomeça.
    try {
      if (fs.statSync(FILE).size > MAX_BYTES) fs.writeFileSync(FILE, "")
    } catch {}
    fs.appendFileSync(FILE, linha)
  } catch {
    // best-effort: sem log se o disco falhar
  }
  // sempre reflete no console também (stdout/stderr do app)
  if (v instanceof Error) console.error(`[${mod}]`, v)
  else console.log(`[${mod}]`, safe(v))
}

module.exports = { log }
