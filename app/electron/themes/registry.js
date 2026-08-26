"use strict"

// Registro de temas Fullscreen instalados. Gerencia a lista de temas
// disponíveis, o tema ativo, o último tema saudável e o estado pendente.
// Usa escrita atômica (tmp + rename) e aceita fsImpl para testabilidade.

const fsDefault = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const {
  REGISTRY_VERSION,
  BUILTIN_DEFAULT_ID,
  BUILTIN_AURORA_ID,
  THEME_API_VERSION,
} = require("./constants")
const { validateManifest, apiCompat, publicManifest } = require("./manifest")

const REGISTRY_FILE = "registry.json"

function createRegistry({ themesDir, fsImpl = fsDefault } = {}) {
  const fullscreenDir = themesDir
    ? path.join(themesDir, "fullscreen")
    : null
  const registryPath = fullscreenDir
    ? path.join(fullscreenDir, REGISTRY_FILE)
    : null

  // Cache em memória para evitar releituras desnecessárias.
  let cache = null

  function ensureDir() {
    if (!fullscreenDir) return
    fsImpl.mkdirSync(fullscreenDir, { recursive: true, mode: 0o700 })
  }

  function isSymlink(file) {
    try {
      return fsImpl.lstatSync(file).isSymbolicLink()
    } catch {
      return false
    }
  }

  function atomicWrite(file, data) {
    ensureDir()
    if (isSymlink(file)) throw new Error("registro_symlink")
    const temporary = `${file}.${process.pid}.${Date.now()}.tmp`
    if (isSymlink(temporary)) throw new Error("registro_temp_symlink")
    fsImpl.writeFileSync(temporary, JSON.stringify(data, null, 2), {
      encoding: "utf8",
      mode: 0o600,
      flag: "wx",
    })
    if (isSymlink(temporary)) throw new Error("registro_temp_symlink")
    fsImpl.renameSync(temporary, file)
  }

  function readRaw() {
    if (!registryPath) return null
    try {
      if (isSymlink(registryPath)) return null
      const text = fsImpl.readFileSync(registryPath, "utf8")
      return JSON.parse(text)
    } catch {
      return null
    }
  }

  function load() {
    if (cache) return cache
    const raw = readRaw()
    if (!raw || typeof raw !== "object" || raw.version !== REGISTRY_VERSION) {
      cache = createEmptyRegistry()
    } else {
      cache = normalize(raw)
    }
    return cache
  }

  function save(reg) {
    cache = reg
    if (registryPath) {
      atomicWrite(registryPath, reg)
    }
  }

  function createEmptyRegistry() {
    return {
      version: REGISTRY_VERSION,
      activeId: BUILTIN_DEFAULT_ID,
      lastKnownGoodId: BUILTIN_DEFAULT_ID,
      pendingId: null,
      themes: {},
    }
  }

  function normalize(raw) {
    const reg = createEmptyRegistry()
    if (typeof raw.activeId === "string") reg.activeId = raw.activeId
    if (typeof raw.lastKnownGoodId === "string") reg.lastKnownGoodId = raw.lastKnownGoodId
    if (typeof raw.pendingId === "string" || raw.pendingId === null) {
      reg.pendingId = raw.pendingId || null
    }
    if (raw.themes && typeof raw.themes === "object" && !Array.isArray(raw.themes)) {
      for (const [id, entry] of Object.entries(raw.themes)) {
        if (typeof id !== "string" || !entry || typeof entry !== "object") continue
        reg.themes[id] = {
          enabled: entry.enabled !== false,
          version: typeof entry.version === "string" ? entry.version : "0.0.0",
          digest: typeof entry.digest === "string" ? entry.digest : "",
          installedAt: typeof entry.installedAt === "number" ? entry.installedAt : 0,
          updatedAt: typeof entry.updatedAt === "number" ? entry.updatedAt : 0,
        }
      }
    }
    return reg
  }

  // --- API pública ---

  function list() {
    const reg = load()
    const result = []

    // Built-ins sempre aparecem primeiro.
    result.push(makeBuiltinInfo(BUILTIN_DEFAULT_ID, "Arcadia Default", reg))
    result.push(makeBuiltinInfo(BUILTIN_AURORA_ID, "Arcadia Aurora", reg))

    // Temas instalados.
    for (const [id, entry] of Object.entries(reg.themes)) {
      const isActive = reg.activeId === id
      const compat = entry.enabled ? "ok" : "disabled"
      result.push({
        id,
        manifest: null, // preenchido pelo service quando disponível
        source: "local",
        installed: true,
        valid: entry.enabled,
        error: entry.enabled ? "" : "tema_desabilitado",
        state: isActive ? "active" : entry.enabled ? "valid" : "invalid",
        options: {},
        active: isActive,
      })
    }

    return result
  }

  function makeBuiltinInfo(id, name, reg) {
    return {
      id,
      manifest: {
        manifestVersion: 1,
        themeApiVersion: THEME_API_VERSION,
        id,
        name,
        author: "Arcadia",
        version: "1.0.0",
        description: "",
        mode: "fullscreen",
        entry: "theme.css",
        layouts: {},
        previews: [],
        features: ["tokens"],
        options: {},
        supports: {},
        homepage: "",
        license: "",
        compat: "ok",
      },
      source: "builtin",
      installed: true,
      valid: true,
      error: "",
      state: reg.activeId === id ? "active" : "valid",
      options: {},
      active: reg.activeId === id,
    }
  }

  function getActiveId() {
    return load().activeId
  }

  function getPendingId() {
    return load().pendingId
  }

  function getLastKnownGoodId() {
    return load().lastKnownGoodId
  }

  function isBuiltin(id) {
    return id === BUILTIN_DEFAULT_ID || id === BUILTIN_AURORA_ID
  }

  function getEntry(id) {
    const reg = load()
    if (isBuiltin(id)) return { builtin: true }
    return reg.themes[id] || null
  }

  function activate(id) {
    const reg = load()
    if (!isBuiltin(id) && !reg.themes[id]) {
      return { ok: false, error: "tema_nao_encontrado" }
    }
    if (!isBuiltin(id) && reg.themes[id] && !reg.themes[id].enabled) {
      return { ok: false, error: "tema_desabilitado" }
    }
    reg.pendingId = id
    save(reg)
    return { ok: true }
  }

  function confirmActivation(id) {
    const reg = load()
    if (reg.pendingId !== id) return { ok: false, error: "pendente_diferente" }
    reg.activeId = id
    reg.lastKnownGoodId = id
    reg.pendingId = null
    save(reg)
    return { ok: true }
  }

  function rollbackPending() {
    const reg = load()
    if (!reg.pendingId) return
    reg.pendingId = null
    save(reg)
  }

  function register(id, version, digest) {
    const reg = load()
    const now = Date.now()
    const existing = reg.themes[id]
    reg.themes[id] = {
      enabled: true,
      version,
      digest: digest || "",
      installedAt: existing ? existing.installedAt : now,
      updatedAt: now,
    }
    save(reg)
    return { ok: true }
  }

  function remove(id) {
    const reg = load()
    if (isBuiltin(id)) return { ok: false, error: "built_in_nao_removivel" }
    if (reg.activeId === id) return { ok: false, error: "tema_ativo" }
    if (!reg.themes[id]) return { ok: false, error: "tema_nao_encontrado" }
    delete reg.themes[id]
    save(reg)
    return { ok: true }
  }

  function disable(id) {
    const reg = load()
    if (isBuiltin(id)) return { ok: false, error: "built_in_nao_desabilitavel" }
    if (!reg.themes[id]) return { ok: false, error: "tema_nao_encontrado" }
    reg.themes[id].enabled = false
    save(reg)
    return { ok: true }
  }

  function recoverToLastKnownGood() {
    const reg = load()
    const fallback = reg.lastKnownGoodId || BUILTIN_DEFAULT_ID
    reg.activeId = fallback
    reg.pendingId = null
    save(reg)
    return fallback
  }

  function computeDigest(filePath) {
    try {
      const content = fsImpl.readFileSync(filePath)
      return crypto.createHash("sha256").update(content).digest("hex")
    } catch {
      return ""
    }
  }

  function reset() {
    cache = null
  }

  return {
    list,
    getActiveId,
    getPendingId,
    getLastKnownGoodId,
    isBuiltin,
    getEntry,
    activate,
    confirmActivation,
    rollbackPending,
    register,
    remove,
    disable,
    recoverToLastKnownGood,
    computeDigest,
    reset,
    createEmptyRegistry,
  }
}

module.exports = { createRegistry }
