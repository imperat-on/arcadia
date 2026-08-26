"use strict"

// Serviço de temas Fullscreen. API unificada que orquestra manifesto,
// registro, CSS normalizado, layouts e descoberta local.

const fsDefault = require("node:fs")
const path = require("node:path")
const {
  BUILTIN_DEFAULT_ID,
  BUILTIN_AURORA_ID,
  MANIFEST_FILE,
  THEME_API_VERSION,
} = require("./constants")
const { readManifest, publicManifest, apiCompat } = require("./manifest")
const { normalizeThemeCss } = require("./css")
const { validateLayout } = require("./layout")
const { createRegistry } = require("./registry")

function createThemeService({ themesDir, fsImpl = fsDefault } = {}) {
  const registry = createRegistry({ themesDir, fsImpl })
  const fullscreenDir = themesDir ? path.join(themesDir, "fullscreen") : null

  // Cache de payloads normalizados: id → { css, layouts, manifest, ... }
  const payloadCache = new Map()

  function scanInstalledThemes() {
    if (!fullscreenDir) return []
    const result = []
    try {
      const entries = fsImpl.readdirSync(fullscreenDir, { withFileTypes: true })
      for (const entry of entries) {
        if (!entry.isDirectory()) continue
        const id = entry.name
        if (registry.isBuiltin(id)) continue
        const manifestPath = path.join(fullscreenDir, id, MANIFEST_FILE)
        const manifestResult = readManifest(manifestPath, { fsImpl })
        if (manifestResult.ok) {
          result.push({ id, manifest: manifestResult.manifest })
        }
      }
    } catch {}
    return result
  }

  function resolveThemeDir(id) {
    if (!fullscreenDir) return null
    return path.join(fullscreenDir, id)
  }

  // Lê e normaliza o CSS de um tema, com cache por digest.
  function loadThemeCss(id) {
    if (id === BUILTIN_DEFAULT_ID || id === BUILTIN_AURORA_ID) {
      // Built-ins: CSS está no bundle, não em arquivo separado.
      return { ok: true, css: "", errors: [] }
    }
    const themeDir = resolveThemeDir(id)
    if (!themeDir) return { ok: false, css: "", errors: ["tema_nao_encontrado"] }

    const manifestPath = path.join(themeDir, MANIFEST_FILE)
    const manifestResult = readManifest(manifestPath, { fsImpl })
    if (!manifestResult.ok) return { ok: false, css: "", errors: manifestResult.errors }

    const entryPath = path.join(themeDir, manifestResult.manifest.entry)
    try {
      const css = fsImpl.readFileSync(entryPath, "utf8")
      return normalizeThemeCss(css, id)
    } catch {
      return { ok: false, css: "", errors: ["css_leitura_falhou"] }
    }
  }

  // Lê e valida layouts de um tema.
  function loadThemeLayouts(id) {
    if (id === BUILTIN_DEFAULT_ID || id === BUILTIN_AURORA_ID) return {}
    const themeDir = resolveThemeDir(id)
    if (!themeDir) return {}

    const manifestPath = path.join(themeDir, MANIFEST_FILE)
    const manifestResult = readManifest(manifestPath, { fsImpl })
    if (!manifestResult.ok) return {}

    const layouts = {}
    for (const [surface, layoutRelPath] of Object.entries(manifestResult.manifest.layouts || {})) {
      const layoutPath = path.join(themeDir, layoutRelPath)
      try {
        const text = fsImpl.readFileSync(layoutPath, "utf8")
        const parsed = JSON.parse(text)
        const result = validateLayout(parsed)
        if (result.ok) {
          layouts[surface] = result.layout
        }
      } catch {}
    }
    return layouts
  }

  // Resolve opções do tema mesclando defaults com preferências do usuário.
  function resolveOptions(id, userOptions) {
    const info = get(id)
    if (!info?.manifest?.options) return {}
    const resolved = {}
    for (const [key, def] of Object.entries(info.manifest.options)) {
      const userVal = userOptions?.[key]
      if (userVal !== undefined) {
        resolved[key] = userVal
      } else {
        resolved[key] = def.default
      }
    }
    return resolved
  }

  // Gera URLs de preview normalizadas (sem paths locais).
  function getPreviewUrls(id) {
    if (id === BUILTIN_DEFAULT_ID || id === BUILTIN_AURORA_ID) return []
    const themeDir = resolveThemeDir(id)
    if (!themeDir) return []

    const manifestPath = path.join(themeDir, MANIFEST_FILE)
    const manifestResult = readManifest(manifestPath, { fsImpl })
    if (!manifestResult.ok) return []

    return (manifestResult.manifest.previews || []).map((p) => `arcadia-theme://${id}/${p}`)
  }

  // --- API pública ---

  function list() {
    const items = registry.list()
    const installed = scanInstalledThemes()
    const installedMap = new Map(installed.map((t) => [t.id, t]))

    for (const item of items) {
      if (item.source === "builtin") continue
      const found = installedMap.get(item.id)
      if (found) {
        item.manifest = publicManifest(found.manifest)
        const compat = apiCompat(found.manifest.themeApiVersion)
        if (compat === "higher") {
          item.valid = false
          item.state = "incompatible"
          item.error = "api_incompativel"
        }
      } else {
        item.valid = false
        item.state = "missing"
        item.error = "tema_ausente"
      }
    }

    return items
  }

  function get(id) {
    if (registry.isBuiltin(id)) {
      const info = registry.list().find((t) => t.id === id)
      return info || null
    }
    const themeDir = resolveThemeDir(id)
    if (!themeDir) return null
    const manifestPath = path.join(themeDir, MANIFEST_FILE)
    const result = readManifest(manifestPath, { fsImpl })
    if (!result.ok) return null
    const entry = registry.getEntry(id)
    return {
      id,
      manifest: publicManifest(result.manifest),
      source: "local",
      installed: !!entry,
      valid: true,
      error: "",
      state: registry.getActiveId() === id ? "active" : "valid",
      options: {},
      active: registry.getActiveId() === id,
    }
  }

  // Payload seguro para o renderer. Nunca contém paths locais.
  function getPayload(id, userOptions) {
    const info = get(id)
    if (!info) return null

    const cssResult = loadThemeCss(id)
    const layouts = loadThemeLayouts(id)
    const options = resolveOptions(id, userOptions)
    const previews = getPreviewUrls(id)

    return {
      id,
      name: info.manifest?.name || id,
      themeApiVersion: info.manifest?.themeApiVersion || THEME_API_VERSION,
      css: cssResult.ok ? cssResult.css : "",
      cssErrors: cssResult.errors,
      layouts,
      options,
      previews,
      compat: info.manifest?.compat || "ok",
      source: info.source,
    }
  }

  function activate(id) {
    const result = registry.activate(id)
    return result
  }

  function confirmActivation(id) {
    return registry.confirmActivation(id)
  }

  function rollbackPending() {
    registry.rollbackPending()
  }

  function remove(id) {
    if (registry.isBuiltin(id)) return { ok: false, error: "built_in_nao_removivel" }
    const result = registry.remove(id)
    if (!result.ok) return result

    const themeDir = resolveThemeDir(id)
    if (themeDir) {
      try { fsImpl.rmSync(themeDir, { recursive: true, force: true }) } catch {}
    }
    payloadCache.delete(id)
    return { ok: true }
  }

  function recoverToLastKnownGood() {
    return registry.recoverToLastKnownGood()
  }

  function getActiveId() { return registry.getActiveId() }
  function getPendingId() { return registry.getPendingId() }
  function getLastKnownGoodId() { return registry.getLastKnownGoodId() }

  function reset() { registry.reset() }

  return {
    list,
    get,
    getPayload,
    activate,
    confirmActivation,
    rollbackPending,
    remove,
    recoverToLastKnownGood,
    getActiveId,
    getPendingId,
    getLastKnownGoodId,
    reset,
    registry,
    loadThemeCss,
    loadThemeLayouts,
  }
}

module.exports = { createThemeService }
