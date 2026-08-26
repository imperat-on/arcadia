"use strict"

// Constantes do sistema de temas Fullscreen do Arcadia. Esse módulo é puro e
// não depende de Electron: todos os limites e versões da API de tema vivem aqui
// para serem testados e compartilhados entre manifest, css, layout, package e
// protocol. Nunca devem ser números soltos espalhados pelo código.

// Versões do formato e da API visual.
const MANIFEST_VERSION = 1
const THEME_API_VERSION = 1
const LAYOUT_SCHEMA_VERSION = 1
const REGISTRY_VERSION = 1

// Arquivos canônicos dentro de um pacote .arcadiatheme.
const MANIFEST_FILE = "theme.json"
const ENTRY_FILE_DEFAULT = "theme.css"
const PACKAGE_EXTENSION = ".arcadiatheme"
const THEME_PROTOCOL = "arcadia-theme"

// IDs canônicos dos temas embutidos. Nunca podem ser removidos/sobrescritos.
const BUILTIN_DEFAULT_ID = "arcadia.default"
const BUILTIN_AURORA_ID = "arcadia.aurora"

// IDs são usados como chave de registro e como host do protocolo de assets.
// Devem ser minúsculos, canônicos e nunca podem ser rotas/names reservados.
const MAX_ID_LENGTH = 64
const ID_RE = /^[a-z0-9](?:[a-z0-9._-]{0,62}[a-z0-9])?$/
const SEMVER_RE = /^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/

const MAX_NAME_LENGTH = 120
const MAX_AUTHOR_LENGTH = 120
const MAX_DESCRIPTION_LENGTH = 1000
const MAX_VERSION_LENGTH = 64
const MAX_ENTRY_LENGTH = 240
const MAX_HOMEPAGE_LENGTH = 1000
const MAX_FEATURES = 64
const MAX_PREVIEWS = 16
const MAX_LAYOUTS = 16
const MAX_OPTIONS = 64
const MAX_OPTION_KEY_LENGTH = 64

// Built-in "features" que o host declara suportar na v1.
const SUPPORTED_FEATURES = new Set([
  "tokens",
  "assets",
  "layout:home",
  "layout:overview",
])

// Limites de tamanho de pacotes e arquivos (seção 9.2 do plano).
const LIMITS = Object.freeze({
  packageCompressedBytes: 128 * 1024 * 1024, // 128 MiB
  packageExtractedBytes: 256 * 1024 * 1024, // 256 MiB
  maxFiles: 2048,
  manifestBytes: 64 * 1024, // 64 KiB
  cssTotalBytes: 2 * 1024 * 1024, // 2 MiB
  layoutBytes: 256 * 1024, // 256 KiB
  imageBytes: 32 * 1024 * 1024, // 32 MiB
  videoBytes: 128 * 1024 * 1024, // 128 MiB
  fontBytes: 8 * 1024 * 1024, // 8 MiB
  maxDepth: 12,
  maxLayoutAreas: 32,
  maxLayoutSlots: 64,
  maxLayoutDepth: 8,
  maxKeyframes: 1024,
  maxFontFaces: 128,
})

// Tipos de arquivo permitidos dentro de um pacote de tema externo (9.1).
const ALLOWED_EXTENSIONS = new Set([
  ".css",
  ".json",
  ".png",
  ".jpg",
  ".jpeg",
  ".webp",
  ".gif",
  ".woff2",
  ".mp4",
  ".webm",
  ".md",
  ".txt",
])

// Extensões explicitamente proibidas (nunca executáveis/scripts).
const FORBIDDEN_EXTENSIONS = new Set([
  ".js",
  ".mjs",
  ".cjs",
  ".ts",
  ".tsx",
  ".jsx",
  ".html",
  ".htm",
  ".wasm",
  ".exe",
  ".dll",
  ".so",
  ".dylib",
  ".sh",
  ".bat",
  ".cmd",
  ".ps1",
  ".py",
  ".rb",
  ".php",
  ".svg",
])

// Tipos de arquivo previstos no CSS (imagem/vídeo/fonte). Assets que não sejam
// imagens/fontes são rejeitados no protocolo. Áudio nunca é permitido na v1.
const ALLOWED_ASSET_MIME_PREFIXES = ["image/", "video/", "font/"]

// MIME por extensão usados pelo protocolo de assets.
const GLOBAL_ASSET_REGEXES = []
const EXT_TO_MIME = Object.freeze({
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".woff2": "font/woff2",
  ".mp4": "video/mp4",
  ".webm": "video/webm",
})

// Estados de compatibilidade de API entre tema e host.
const API_COMPAT = Object.freeze({
  OK: "ok", // igual
  LOWER: "lower", // API menor suportada (carrega com compatibilidade)
  HIGHER: "higher", // API maior (marca incompatível, não ativa)
})

module.exports = {
  MANIFEST_VERSION,
  THEME_API_VERSION,
  LAYOUT_SCHEMA_VERSION,
  REGISTRY_VERSION,
  MANIFEST_FILE,
  ENTRY_FILE_DEFAULT,
  PACKAGE_EXTENSION,
  THEME_PROTOCOL,
  BUILTIN_DEFAULT_ID,
  BUILTIN_AURORA_ID,
  MAX_ID_LENGTH,
  ID_RE,
  SEMVER_RE,
  MAX_NAME_LENGTH,
  MAX_AUTHOR_LENGTH,
  MAX_DESCRIPTION_LENGTH,
  MAX_VERSION_LENGTH,
  MAX_ENTRY_LENGTH,
  MAX_HOMEPAGE_LENGTH,
  MAX_FEATURES,
  MAX_PREVIEWS,
  MAX_LAYOUTS,
  MAX_OPTIONS,
  MAX_OPTION_KEY_LENGTH,
  SUPPORTED_FEATURES,
  LIMITS,
  ALLOWED_EXTENSIONS,
  FORBIDDEN_EXTENSIONS,
  ALLOWED_ASSET_MIME_PREFIXES,
  EXT_TO_MIME,
  API_COMPAT,
}