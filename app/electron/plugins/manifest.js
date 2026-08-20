"use strict"

// Contrato versionado para plugins locais. Este módulo é deliberadamente puro:
// não carrega nem executa o `entry` de um plugin. A validação acontece antes de
// qualquer registro e o host só expõe capacidades explicitamente declaradas.

const fsDefault = require("node:fs")
const path = require("node:path")

const MANIFEST_VERSION = 1
const SDK_API_VERSION = 1
const MANIFEST_FILE = "plugin.json"
const MAX_ID_LENGTH = 64
const MAX_NAME_LENGTH = 120
const MAX_DESCRIPTION_LENGTH = 1000
const MAX_VERSION_LENGTH = 64
const MAX_ENTRY_LENGTH = 240
const MAX_PERMISSIONS = 32

// Permissões não são globais/wildcards. Cada capacidade que o host vier a
// implementar precisa de um nome explícito neste allowlist e de uma checagem
// no SDK antes de executar a operação.
const PLUGIN_PERMISSIONS = Object.freeze([
  "library:read",
  "library:write",
  "games:read",
  "games:launch",
  "events:subscribe",
  "commands:register",
  "network",
  "filesystem:read",
  "filesystem:write",
  "process:spawn",
  "notifications",
  "settings:read",
  "settings:write",
])
const PERMISSION_SET = new Set(PLUGIN_PERMISSIONS)

// IDs são usados em chaves de registro e nunca podem ser caminhos, nomes
// reservados ou expressões ambíguas. O limite também evita entradas abusivas.
const ID_RE = /^[a-z0-9](?:[a-z0-9._-]{0,62}[a-z0-9])?$/
const SEMVER_RE = /^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/

function own(value, key) {
  return Object.prototype.hasOwnProperty.call(value, key)
}

function first(value, ...keys) {
  for (const key of keys) {
    if (own(value, key)) return value[key]
  }
  return undefined
}

function stringField(value, field, max, errors, { required = false, trim = true } = {}) {
  if (typeof value !== "string") {
    if (required || value !== undefined) errors.push(`${field}_invalido`)
    return ""
  }
  const normalized = trim ? value.trim() : value
  if (required && !normalized) errors.push(`${field}_obrigatorio`)
  if (normalized.length > max) errors.push(`${field}_muito_longo`)
  if (normalized.includes("\u0000")) errors.push(`${field}_invalido`)
  return normalized
}

function normalizeId(value) {
  if (typeof value !== "string") return ""
  const id = value.trim()
  // IDs são canônicos em minúsculas; não faça uma conversão silenciosa que
  // poderia fazer dois pacotes diferentes disputarem a mesma chave.
  return id && id === id.toLowerCase() && id.length <= MAX_ID_LENGTH && ID_RE.test(id) ? id : ""
}

function validateId(value) {
  const id = normalizeId(value)
  return id ? { ok: true, id, errors: [] } : { ok: false, id: "", errors: ["id_invalido"] }
}

function normalizeManifestVersion(value) {
  // `manifest_version`/`schemaVersion` são aceitos apenas como aliases de
  // leitura para facilitar migração; o objeto devolvido usa camelCase.
  if (value === MANIFEST_VERSION || value === String(MANIFEST_VERSION)) return MANIFEST_VERSION
  return null
}

function normalizeApiVersion(value) {
  if (value === undefined) return SDK_API_VERSION
  if (value === SDK_API_VERSION || value === String(SDK_API_VERSION)) return SDK_API_VERSION
  return null
}

function normalizePermissions(value, errors) {
  if (value === undefined) return []
  if (!Array.isArray(value) || value.length > MAX_PERMISSIONS) {
    errors.push("permissions_invalido")
    return []
  }
  const result = []
  const seen = new Set()
  for (const raw of value) {
    if (typeof raw !== "string") {
      errors.push("permission_invalida")
      continue
    }
    const permission = raw.trim()
    if (!PERMISSION_SET.has(permission)) {
      errors.push(`permission_nao_suportada:${permission.slice(0, 80)}`)
      continue
    }
    if (seen.has(permission)) {
      errors.push(`permission_duplicada:${permission}`)
      continue
    }
    seen.add(permission)
    result.push(permission)
  }
  return result
}

function normalizeEntry(value, errors, { required = true } = {}) {
  if (value === undefined && !required) return ""
  const entry = stringField(value, "entry", MAX_ENTRY_LENGTH, errors, { required })
  if (!entry) return ""
  if (path.isAbsolute(entry) || entry.includes("\u0000") || entry.includes("\\")) {
    errors.push("entry_invalido")
    return ""
  }
  const normalized = path.posix.normalize(entry)
  // `foo/../bar` is rejected instead of silently normalized: a manifest must
  // be unambiguous and must never escape its package directory.
  if (
    normalized !== entry ||
    normalized === "." ||
    normalized.startsWith("../") ||
    normalized.includes("/../") ||
    normalized.endsWith("/..")
  ) {
    errors.push("entry_invalido")
    return ""
  }
  return normalized
}

/**
 * Validate and canonicalize a manifest object.
 *
 * Accepted aliases are intentionally limited to old snake_case spellings;
 * unknown top-level fields are rejected so future permissions/capabilities do
 * not silently become executable host behavior.
 */
function validateManifest(input, { requireEntry = true } = {}) {
  const errors = []
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, manifest: null, errors: ["manifest_invalido"] }
  }
  // JSON.parse can produce an own `__proto__` key. Reject prototype-related
  // names even though we never merge arbitrary manifest objects into state.
  for (const key of Object.keys(input)) {
    if (key === "__proto__" || key === "prototype" || key === "constructor") {
      errors.push("campo_invalido")
    }
  }

  const aliasGroups = [
    ["manifestVersion", "manifest_version", "schemaVersion"],
    ["apiVersion", "api_version"],
    ["entry", "entrypoint"],
  ]
  for (const group of aliasGroups) {
    if (group.filter((key) => own(input, key)).length > 1) errors.push(`campo_duplicado:${group[0]}`)
  }

  const manifestVersion = normalizeManifestVersion(first(input, "manifestVersion", "manifest_version", "schemaVersion"))
  if (manifestVersion === null) errors.push("manifest_version_invalida")

  const apiVersion = normalizeApiVersion(first(input, "apiVersion", "api_version"))
  if (apiVersion === null) errors.push("api_version_invalida")

  const idResult = validateId(input.id)
  if (!idResult.ok) errors.push(...idResult.errors)
  const id = idResult.id

  const name = stringField(input.name, "name", MAX_NAME_LENGTH, errors, { required: true })
  const version = stringField(input.version, "version", MAX_VERSION_LENGTH, errors, { required: true })
  if (version && !SEMVER_RE.test(version)) errors.push("version_invalida")
  const description = stringField(input.description, "description", MAX_DESCRIPTION_LENGTH, errors)
  const entry = normalizeEntry(first(input, "entry", "entrypoint"), errors, { required: requireEntry })
  const permissions = normalizePermissions(input.permissions, errors)

  // Do not accept arbitrary fields. Keeping this list explicit makes the
  // manifest a capability declaration, not an object that can smuggle host
  // options into future code.
  const known = new Set([
    "manifestVersion",
    "manifest_version",
    "schemaVersion",
    "apiVersion",
    "api_version",
    "id",
    "name",
    "version",
    "description",
    "entry",
    "entrypoint",
    "permissions",
  ])
  for (const key of Object.keys(input)) {
    if (!known.has(key)) errors.push(`campo_desconhecido:${key.slice(0, 80)}`)
  }

  if (errors.length) return { ok: false, manifest: null, errors: [...new Set(errors)] }
  return {
    ok: true,
    manifest: {
      manifestVersion,
      apiVersion,
      id,
      name,
      version,
      description,
      entry,
      permissions,
    },
    errors: [],
  }
}

function parseManifest(text, options) {
  let value
  try {
    value = JSON.parse(String(text))
  } catch {
    return { ok: false, manifest: null, errors: ["json_invalido"] }
  }
  return validateManifest(value, options)
}

function readManifest(file, { fsImpl = fsDefault, ...options } = {}) {
  if (typeof file !== "string" || !file.trim()) return { ok: false, manifest: null, errors: ["caminho_invalido"] }
  let stat
  try {
    stat = fsImpl.lstatSync(file)
    if (!stat.isFile() || stat.isSymbolicLink()) return { ok: false, manifest: null, errors: ["manifest_invalido"] }
    return parseManifest(fsImpl.readFileSync(file, "utf8"), options)
  } catch {
    return { ok: false, manifest: null, errors: ["manifest_nao_encontrado"] }
  }
}

function isInside(root, target) {
  const base = path.resolve(root)
  const child = path.resolve(target)
  return child === base || child.startsWith(`${base}${path.sep}`)
}

/**
 * Resolve an entry without allowing absolute paths, traversal, or symlinks
 * that leave the plugin package.
 */
function resolvePluginEntry(pluginDir, entry, { fsImpl = fsDefault } = {}) {
  if (typeof pluginDir !== "string" || !pluginDir.trim() || typeof entry !== "string" || !entry.trim()) {
    return { ok: false, path: "", error: "entry_invalido" }
  }
  if (path.isAbsolute(entry) || entry.includes("\u0000") || entry.includes("\\")) {
    return { ok: false, path: "", error: "entry_invalido" }
  }
  const root = path.resolve(pluginDir)
  const candidate = path.resolve(root, entry)
  if (!isInside(root, candidate) || candidate === root) {
    return { ok: false, path: "", error: "entry_fora_do_plugin" }
  }
  try {
    const rootStat = fsImpl.lstatSync(root)
    const entryStat = fsImpl.lstatSync(candidate)
    if (!rootStat.isDirectory() || rootStat.isSymbolicLink() || !entryStat.isFile()) {
      return { ok: false, path: "", error: "entry_invalido" }
    }
    // realpath catches a regular file hidden behind a symlink. A plugin may
    // still contain symlinks for assets, but its executable entry cannot leave
    // the directory that was explicitly registered.
    const realRoot = fsImpl.realpathSync(root)
    const realEntry = fsImpl.realpathSync(candidate)
    if (!isInside(realRoot, realEntry)) return { ok: false, path: "", error: "entry_fora_do_plugin" }
    return { ok: true, path: candidate, error: "" }
  } catch {
    return { ok: false, path: "", error: "entry_nao_encontrado" }
  }
}

function publicManifest(manifest) {
  if (!manifest || typeof manifest !== "object") return null
  // Return a fresh object and a fresh permissions array so renderer callers
  // cannot mutate the registry's canonical state by reference.
  return {
    manifestVersion: manifest.manifestVersion,
    apiVersion: manifest.apiVersion,
    id: manifest.id,
    name: manifest.name,
    version: manifest.version,
    description: manifest.description || "",
    entry: manifest.entry || "",
    permissions: Array.isArray(manifest.permissions) ? [...manifest.permissions] : [],
  }
}

module.exports = {
  MANIFEST_VERSION,
  SDK_API_VERSION,
  MANIFEST_FILE,
  MAX_ID_LENGTH,
  PLUGIN_PERMISSIONS,
  normalizeId,
  validateId,
  validateManifest,
  parseManifest,
  readManifest,
  resolvePluginEntry,
  isInside,
  publicManifest,
}
