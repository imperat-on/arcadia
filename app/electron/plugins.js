"use strict"

// Fachada compatível da API histórica de plugins do Arcadia. A implementação
// real vive em plugins/manifest.js, plugins/registry.js e plugins/sdk.js;
// manter esta fachada evita quebrar versões da UI que já chamam list/install/
// remove enquanto a superfície versionada evolui.

const fs = require("node:fs")
const path = require("node:path")
const os = require("node:os")
const { getDataDir } = require("./runtime-paths")
const { createPluginRegistry } = require("./plugins/registry")
const { createPluginSdk } = require("./plugins/sdk")
const { normalizeId, publicManifest } = require("./plugins/manifest")

const HOME = os.homedir()
const DATA_DIR = getDataDir()
const BIN_DIR = path.join(DATA_DIR, "bin")
// Caminho legado: continua sendo lido/escrito como espelho para instalações
// anteriores. O registro canônico agora é plugins/registry.json.
const REGISTRY = path.join(BIN_DIR, "plugins.json")
const CONFIG = path.join(DATA_DIR, "config.json")
const SLSSTEAM_SO = path.join(HOME, ".local/share/SLSsteam/SLSsteam.so")

function readConfig() {
  try {
    const value = JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
    return value && typeof value === "object" && !Array.isArray(value) ? value : {}
  } catch {
    return {}
  }
}

// Existe no caminho custom (config) OU no caminho padrão?
function detectar(chaveCustom, padrao) {
  const custom = String(readConfig()[chaveCustom] || "").trim()
  if (custom) {
    try {
      const stat = fs.lstatSync(custom)
      return stat.isFile() && !stat.isSymbolicLink()
    } catch {
      return false
    }
  }
  try {
    const stat = fs.lstatSync(padrao)
    return stat.isFile() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function readRegistry() {
  try {
    const stat = fs.lstatSync(REGISTRY)
    if (!stat.isFile() || stat.isSymbolicLink()) return {}
    const value = JSON.parse(fs.readFileSync(REGISTRY, "utf-8"))
    return value && typeof value === "object" && !Array.isArray(value) ? value : {}
  } catch {
    return {}
  }
}

const BUILTIN_DEFINITIONS = [
  {
    id: "slssteam",
    manifest: {
      manifestVersion: 1,
      apiVersion: 1,
      id: "slssteam",
      name: "SLSsteam",
      version: "0.0.0",
      description: "Integração opcional com o cliente Steam.",
      permissions: ["library:read", "library:write", "games:launch", "filesystem:read", "process:spawn"],
    },
    name: "plugins.slssteam_nome",
    descKey: "plugins.slssteam_desc",
    installed: () => detectar("slssteam_path", SLSSTEAM_SO),
  },
  {
    id: "luatools-fixes",
    manifest: {
      manifestVersion: 1,
      apiVersion: 1,
      id: "luatools-fixes",
      name: "LuaTools fixes",
      version: "0.0.0",
      description: "Correções opcionais de integração LuaTools.",
      permissions: ["library:read", "library:write"],
    },
    name: "plugins.luatools_nome",
    descKey: "plugins.luatools_desc",
    // LuaTools é uma integração representada por uma flag local, não por um
    // arquivo que Arcadia possa instalar. A flag é consultada para manter o
    // significado histórico de `installed` na tela de Plugins.
    installed: () => Boolean(readRegistry()["luatools-fixes"]?.enabled),
  },
]

const builtinById = new Map(BUILTIN_DEFINITIONS.map((definition) => [definition.id, definition]))
const registry = createPluginRegistry({
  dataDir: DATA_DIR,
  legacyRegistryPath: REGISTRY,
  builtins: BUILTIN_DEFINITIONS,
})

function localizedFields(id, item) {
  const definition = builtinById.get(id)
  return {
    name: definition?.name || item?.manifest?.name || id,
    descKey: definition?.descKey || "",
  }
}

function oldShape(item) {
  const fields = localizedFields(item.id, item)
  return {
    id: item.id,
    name: fields.name,
    descKey: fields.descKey,
    installed: Boolean(item.installed),
    enabled: Boolean(item.enabled),
  }
}

function detailedShape(item) {
  const fields = localizedFields(item.id, item)
  return {
    ...oldShape(item),
    manifest: publicManifest(item.manifest),
    valid: item.valid !== false,
    error: item.error || "",
    source: item.source || "local",
  }
}

function list() {
  // Do not add fields here: this exact shape is consumed by old renderers.
  return registry.listDetailed().map(oldShape)
}

function listDetailed() {
  return registry.listDetailed().map(detailedShape)
}

function get(id) {
  const normalized = normalizeId(id)
  if (!normalized) return null
  const item = registry.get(normalized)
  return item ? detailedShape(item) : null
}

function isEnabled(id) {
  return Boolean(registry.get(String(id || ""))?.enabled)
}

// "Ativar" um plugin = confirmar que o Arcadia consegue detectá-lo no
// sistema. Não baixa nada. Se não for detectado, orienta o usuário a colocar
// o arquivo e informar o caminho (aba Plugins). LuaTools é só uma flag local.
async function install(id) {
  const normalized = normalizeId(id)
  if (!normalized || !builtinById.has(normalized)) return { ok: false, error: "plugin desconhecido" }
  const definition = builtinById.get(normalized)
  if (normalized !== "luatools-fixes") {
    let installed = false
    try { installed = Boolean(definition.installed()) } catch {}
    if (!installed) return { ok: false, error: "not_detected" }
  }
  return registry.setEnabled(normalized, true, { allowUndetected: normalized === "luatools-fixes" })
}

// "Desativar": só limpa a flag/registro. NUNCA apaga o arquivo do plugin —
// ele foi colocado pelo usuário, fora do Arcadia; remover seria mexer no que
// não é nosso.
async function remove(id) {
  const normalized = normalizeId(id)
  if (!normalized || !builtinById.has(normalized)) return { ok: false, error: "plugin desconhecido" }
  return registry.setEnabled(normalized, false, { allowUndetected: true })
}

function register(pluginPath, options) {
  return registry.register(pluginPath, options)
}

function unregister(id) {
  return registry.unregister(id)
}

function enable(id) {
  return registry.setEnabled(id, true)
}

function disable(id) {
  return registry.setEnabled(id, false)
}

function manifest(id) {
  return get(id)?.manifest || null
}

function permissions(id) {
  return manifest(id)?.permissions || []
}

function hasPermission(id, permission) {
  return registry.hasPermission(id, permission)
}

function verify(id) {
  return registry.verify(id)
}

function verifyPackage(pluginPath) {
  return registry.verifyPackage(pluginPath)
}

function sdk(id) {
  return createPluginSdk({ registry, pluginId: id })
}

module.exports = {
  list,
  listDetailed,
  get,
  install,
  remove,
  isEnabled,
  register,
  unregister,
  enable,
  disable,
  manifest,
  permissions,
  hasPermission,
  verify,
  verifyPackage,
  sdk,
  // Exported for focused unit tests/diagnostics without exposing paths through
  // the renderer IPC surface.
  registry,
  BUILTIN_DEFINITIONS,
}
