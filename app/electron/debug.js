// Log de debug centralizado, best-effort. Nunca lança nem bloqueia: se o
// arquivo não der pra abrir, engole em silêncio (log é apoio, não o negócio).
// Escreve em logs/debug.log (respeita ARCADIA_DATA_DIR) com timestamp e
// prefixo por módulo, pra caçar falha sem ter que reproduzir interativo.
//
// Uso: const { log } = require("./debug"), depois log("achievements/loader", e)
// Passa o erro OU uma string. Objetos viram JSON (com stringify seguro).

const fs = require("fs")
const path = require("path")
const os = require("os")

const DATA_DIR =
  process.env.ARCADIA_DATA_DIR || path.join(os.homedir(), ".local/share/arcadia")
const FILE = path.join(DATA_DIR, "logs", "debug.log")

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
  try {
    fs.mkdirSync(path.dirname(FILE), { recursive: true })
    const linha = `${new Date().toISOString()} [${mod}] ${safe(v)}\n`
    fs.appendFileSync(FILE, linha)
  } catch {
    // best-effort: sem log se o disco falhar
  }
  // sempre reflete no console também (stdout/stderr do app)
  try {
    const e = v instanceof Error ? v : undefined
    if (e) console.error(`[${mod}]`, e)
    else console.log(`[${mod}]`, safe(v))
  } catch {}
}

module.exports = { log }
