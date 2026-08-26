"use strict"

// Protocolo de assets arcadia-theme://.
// Serve apenas arquivos allowlisted do diretório validado de um tema.
// Rejeita traversal, symlinks, MIME não permitido e paths arbitrários.

const fsDefault = require("node:fs")
const path = require("node:path")
const { THEME_PROTOCOL, EXT_TO_MIME, ALLOWED_EXTENSIONS } = require("./constants")

// Extensões que NUNCA são servidas como assets (mesmo que estejam no pacote).
const SERVABLE_EXTENSIONS = new Set([
  ".png", ".jpg", ".jpeg", ".webp", ".gif",
  ".woff2",
  ".mp4", ".webm",
])

function getMimeType(filePath) {
  const ext = path.extname(filePath).toLowerCase()
  return EXT_TO_MIME[ext] || null
}

function isServable(filePath) {
  const ext = path.extname(filePath).toLowerCase()
  return SERVABLE_EXTENSIONS.has(ext)
}

function normalizeAssetPath(rawPath) {
  if (typeof rawPath !== "string") return null
  // Remove query strings e fragments
  const clean = rawPath.split("?")[0].split("#")[0]
  if (!clean) return null
  // Rejeita traversal, absolutos e caracteres perigosos
  if (clean.includes("..") || clean.includes("\\")) return null
  if (path.isAbsolute(clean)) return null
  const normalized = path.posix.normalize(clean)
  if (normalized.startsWith("../") || normalized.includes("/../")) return null
  return normalized
}

function resolveAssetPath(themeDir, assetPath) {
  const normalized = normalizeAssetPath(assetPath)
  if (!normalized) return { ok: false, resolved: "", error: "path_invalido" }

  const resolved = path.resolve(themeDir, normalized)

  // Verifica que o path resolvido está dentro do themeDir
  const base = path.resolve(themeDir)
  if (!resolved.startsWith(base + path.sep) && resolved !== base) {
    return { ok: false, resolved: "", error: "path_fora_do_tema" }
  }

  return { ok: true, resolved, error: "" }
}

function createProtocolHandler({ themesDir, fsImpl = fsDefault } = {}) {
  // Cache de temas registrados: id → themeDir
  const registeredThemes = new Map()

  function registerTheme(themeId, themeDir) {
    registeredThemes.set(themeId, themeDir)
  }

  function unregisterTheme(themeId) {
    registeredThemes.delete(themeId)
  }

  function getThemeDir(themeId) {
    return registeredThemes.get(themeId) || null
  }

  // Interpreta uma URL arcadia-theme://id/path e devolve o conteúdo.
  function handleRequest(url) {
    if (typeof url !== "string") return { ok: false, error: "url_invalida" }

    // Parse: arcadia-theme://id/path/to/asset
    const protocolPrefix = `${THEME_PROTOCOL}://`
    if (!url.startsWith(protocolPrefix)) return { ok: false, error: "protocolo_invalido" }

    const rest = url.slice(protocolPrefix.length)
    const slashIndex = rest.indexOf("/")
    if (slashIndex < 0) return { ok: false, error: "path_invalido" }

    const themeId = rest.slice(0, slashIndex)
    const assetPath = rest.slice(slashIndex + 1)

    const themeDir = getThemeDir(themeId)
    if (!themeDir) return { ok: false, error: "tema_nao_registrado" }

    const resolved = resolveAssetPath(themeDir, assetPath)
    if (!resolved.ok) return { ok: false, error: resolved.error }

    if (!isServable(resolved.resolved)) {
      return { ok: false, error: "extensao_nao_permitida" }
    }

    const mime = getMimeType(resolved.resolved)
    if (!mime) return { ok: false, error: "mime_nao_reconhecido" }

    // Verifica symlink
    try {
      const stat = fsImpl.lstatSync(resolved.resolved)
      if (stat.isSymbolicLink()) return { ok: false, error: "symlink_rejeitado" }
      if (!stat.isFile()) return { ok: false, error: "nao_e_arquivo" }
    } catch {
      return { ok: false, error: "arquivo_nao_encontrado" }
    }

    try {
      const data = fsImpl.readFileSync(resolved.resolved)
      return { ok: true, data, mime, path: resolved.resolved }
    } catch {
      return { ok: false, error: "leitura_falhou" }
    }
  }

  // Registra o protocolo customizado no Electron.
  // Chamado antes de criar a janela.
  function registerElectronProtocol(protocol) {
    if (!protocol || !protocol.registerProtocol) return
    protocol.registerProtocol(THEME_PROTOCOL, (request, callback) => {
      const result = handleRequest(request.url)
      if (!result.ok) {
        callback({ error: -6 }) // NET::ERR_FILE_NOT_FOUND
        return
      }
      callback({
        data: result.data,
        mimeType: result.mime,
        charset: "utf-8",
      })
    })
  }

  return {
    registerTheme,
    unregisterTheme,
    getThemeDir,
    handleRequest,
    registerElectronProtocol,
    normalizeAssetPath,
    resolveAssetPath,
    isServable,
    getMimeType,
  }
}

module.exports = {
  createProtocolHandler,
  normalizeAssetPath,
  resolveAssetPath,
  isServable,
  getMimeType,
  THEME_PROTOCOL,
}
