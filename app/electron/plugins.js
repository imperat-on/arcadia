// Plugins opcionais do Arcadia: mantém o core como mostrador de jogos puro.
// SLSsteam/luatools só habilitam features quando instalados/ativos.

const fs = require("fs")
const path = require("path")
const os = require("os")

const HOME = os.homedir()
const DATA_DIR = path.join(HOME, ".local/share/arcadia")
const BIN_DIR = path.join(DATA_DIR, "bin")
const REGISTRY = path.join(BIN_DIR, "plugins.json")
const CONFIG = path.join(DATA_DIR, "config.json")
const SLSSTEAM_SO = path.join(HOME, ".local/share/SLSsteam/SLSsteam.so")

// Arcadia é um CARREGADOR de plugins, não um instalador. Ele NÃO baixa nem
// hospeda SLSsteam — apenas DETECTA se o usuário já colocou o arquivo no
// sistema (caminho padrão ou informado por ele no config). Zero URLs aqui.
function readConfig() {
  try { return JSON.parse(fs.readFileSync(CONFIG, "utf-8")) || {} } catch { return {} }
}

// Existe no caminho custom (config) OU no caminho padrão?
function detectar(chaveCustom, padrao) {
  const custom = String(readConfig()[chaveCustom] || "").trim()
  if (custom) return fs.existsSync(custom)
  return fs.existsSync(padrao)
}

const PLUGINS = {
  slssteam: {
    id: "slssteam",
    name: "plugins.slssteam_nome",
    descKey: "plugins.slssteam_desc",
    installed: () => detectar("slssteam_path", SLSSTEAM_SO),
  },
  "luatools-fixes": {
    id: "luatools-fixes",
    name: "plugins.luatools_nome",
    descKey: "plugins.luatools_desc",
    installed: () => Boolean(readRegistry()["luatools-fixes"]?.enabled),
  },
}

function readRegistry() {
  try { return JSON.parse(fs.readFileSync(REGISTRY, "utf-8")) || {} } catch { return {} }
}

function writeRegistry(reg) {
  fs.mkdirSync(BIN_DIR, { recursive: true })
  const tmp = `${REGISTRY}.tmp`
  fs.writeFileSync(tmp, JSON.stringify(reg, null, 2))
  fs.renameSync(tmp, REGISTRY)
}

function mark(id, enabled = true) {
  const reg = readRegistry()
  reg[id] = { ...(reg[id] || {}), enabled, updatedAt: Date.now() }
  writeRegistry(reg)
}

function isEnabled(id) {
  const p = PLUGINS[id]
  if (!p) return false
  const reg = readRegistry()[id]
  if (reg?.enabled === false) return false
  // Detecta instalações antigas no disco. luatools é só flag.
  return p.installed()
}

function list() {
  return Object.values(PLUGINS).map((p) => ({
    id: p.id,
    name: p.name,
    descKey: p.descKey,
    installed: p.installed(),
    enabled: isEnabled(p.id),
  }))
}

// "Ativar" um plugin = confirmar que o Arcadia consegue detectá-lo no sistema.
// Não baixa nada. Se não for detectado, orienta o usuário a colocar o arquivo e
// informar o caminho (aba Plugins). luatools-fixes é só uma flag local.
async function install(id) {
  const p = PLUGINS[id]
  if (!p) return { ok: false, error: "plugin desconhecido" }
  if (id === "luatools-fixes") {
    mark(id, true)
    return { ok: true }
  }
  if (p.installed()) {
    mark(id, true)
    return { ok: true }
  }
  return { ok: false, error: "not_detected" }
}

// "Desativar": só limpa a flag/registro. NUNCA apaga o arquivo do plugin — ele
// foi colocado pelo usuário, fora do Arcadia; remover seria mexer no que não é nosso.
async function remove(id) {
  const reg = readRegistry()
  reg[id] = { ...(reg[id] || {}), enabled: false, updatedAt: Date.now() }
  writeRegistry(reg)
  return { ok: true }
}

module.exports = { list, install, remove, isEnabled }
