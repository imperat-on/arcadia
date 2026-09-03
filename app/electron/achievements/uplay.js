"use strict"

// Suporte ao loader UPC/voices38 (upc_r2_loader64.dll). O loader atual usa um
// schema JSON plano na pasta do jogo e grava o estado no prefixo Wine:
//   <game>/achievements_schema.json
//   <prefix>/drive_c/users/steamuser/AppData/Roaming/Goldberg UplayEmu Saves/<id>/achievements.json
// Este módulo concentra somente parsing, identificação e preparação segura do
// schema. O runtime achievements.json é criado e atualizado pelo próprio jogo.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { loadAchievements } = require("./schema")

const USUARIO_WINE = "steamuser"
const UPLAY_SAVE_DIR = "Goldberg UplayEmu Saves"
const KNOWN_UPC_IDS = Object.freeze({
  // Assassin's Creed Black Flag Resynced (Steam build).
  3751950: "66088",
})

function plainObject(value) {
  return Boolean(value && typeof value === "object" && !Array.isArray(value))
}

// IDs UPC são inteiros decimais. A normalização impede que um valor vindo de
// configuração escape para um caminho arbitrário (../, separadores, etc.).
function normalizeNumericId(value) {
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value) || value < 0) return null
    return String(value)
  }
  const text = String(value ?? "").trim()
  if (!/^\d{1,18}$/.test(text)) return null
  return text.replace(/^0+(?=\d)/, "")
}

function earnedValue(value) {
  if (value === true) return true
  if (typeof value === "number") return Number.isFinite(value) && value > 0
  if (typeof value !== "string") return false
  const text = value.trim().toLowerCase()
  if (["true", "yes", "on", "unlocked"].includes(text)) return true
  if (!text) return false
  const number = Number(text)
  return Number.isFinite(number) && number > 0
}

// UPC grava Unix seconds. Aceitamos milissegundos para não quebrar arquivos
// produzidos por ferramentas auxiliares, mas sempre devolvemos milissegundos
// para o contrato dos parsers do cracked_watcher.
function epochMilliseconds(value) {
  const number = Number(value)
  if (!Number.isFinite(number) || number <= 0) return 0
  return Math.floor(number > 100000000000 ? number : number * 1000)
}

/**
 * Lê o runtime UPC. O formato correto é um objeto id -> estado; um schema
 * (earned: 0) nunca produz desbloqueio. A chave numérica é preservada em id.
 */
function parseUPC(conteudo) {
  let root
  try {
    root = JSON.parse(conteudo)
  } catch {
    return []
  }
  if (!plainObject(root)) return []

  const out = []
  for (const [rawId, value] of Object.entries(root)) {
    if (!plainObject(value) || !earnedValue(value.earned)) continue
    const id = normalizeNumericId(rawId) || String(rawId).trim()
    if (!id) continue
    out.push({
      id,
      name: String(value.apiname || value.name || value.displayName || id),
      unlockTime: epochMilliseconds(value.earned_time),
    })
  }
  return out
}

// Converte os nomes internos Steam usados no catálogo do Arcadia para o ID
// decimal que o loader UPC atual espera. Ex.: ACObsidian_Ach_40 -> "40".
function numericAchievementId(value) {
  const text = String(value ?? "").trim()
  if (!text) return null
  const direct = normalizeNumericId(text)
  if (direct !== null) return direct
  const match = /(?:^|[_-])ach(?:ievement)?[_-]?(\d+)$/i.exec(text)
  return match ? normalizeNumericId(match[1]) : null
}

function itemUplayId(item, index, { allowPositional = false } = {}) {
  if (!item || typeof item !== "object") return null
  for (const key of ["uplayId", "upcId", "objectiveId", "achievementId"]) {
    const id = numericAchievementId(item[key])
    if (id !== null) return id
  }
  const fromName = numericAchievementId(item.apiname || item.name)
  if (fromName !== null) return fromName
  return allowPositional && Number.isInteger(index) ? String(index + 1) : null
}

/** Gera apenas o conteúdo do schema; não escreve em nenhuma pasta. */
function buildUplaySchema(items, { allowPositional = false } = {}) {
  if (!Array.isArray(items) || !items.length) throw new Error("catálogo UPC vazio")
  const schema = {}
  const seen = new Set()
  items.forEach((item, index) => {
    const id = itemUplayId(item, index, { allowPositional })
    if (id === null) throw new Error(`ID UPC ausente no item ${index + 1}`)
    if (seen.has(id)) throw new Error(`ID UPC duplicado: ${id}`)
    seen.add(id)
    schema[id] = {
      displayName: String(item.title || item.displayName || item.name || ""),
      // Algumas conquistas vêm da API pública sem descrição (o parser HTML do
      // fallback nem sempre pega a <h5>). O voices38 exige o campo preenchido
      // pra criar o schema; usar o title como fallback evita bloquear o
      // achievements_schema.json inteiro (e o jogo roda sem conquistas).
      description: String(item.desc || item.description || "") || String(item.title || item.name || ""),
      earned: 0,
    }
  })
  return schema
}

function validateUplaySchema(schema, { requireContiguous = false, requireMetadata = false } = {}) {
  if (!plainObject(schema)) return { ok: false, error: "schema-raiz-nao-e-objeto" }
  const ids = Object.keys(schema)
  if (!ids.length) return { ok: false, error: "schema-vazio" }
  for (const rawId of ids) {
    if (normalizeNumericId(rawId) !== rawId) {
      return { ok: false, error: `id-upc-invalido:${rawId}` }
    }
    if (!plainObject(schema[rawId])) return { ok: false, error: `entrada-invalida:${rawId}` }
    if (
      requireMetadata &&
      (!String(schema[rawId].displayName || "").trim() ||
        !String(schema[rawId].description || "").trim())
    ) {
      return { ok: false, error: `metadata-ausente:${rawId}` }
    }
  }
  if (requireContiguous) {
    const expected = Array.from({ length: ids.length }, (_, i) => String(i + 1))
    const sorted = ids.slice().sort((a, b) => Number(a) - Number(b))
    if (sorted.join("|") !== expected.join("|")) {
      return { ok: false, error: "ids-upc-nao-contiguos" }
    }
  }
  return { ok: true, count: ids.length, ids }
}

function resolveUplayId(appid, settings = {}, entry = {}) {
  const candidates = [
    settings.uplayId,
    settings.upcId,
    settings.uplayAppId,
    entry.uplayId,
    entry.upcId,
    KNOWN_UPC_IDS[String(appid)],
  ]
  for (const candidate of candidates) {
    const id = normalizeNumericId(candidate)
    if (id !== null) return id
  }
  return null
}

function uplayRuntimePath(prefixo, uplayId) {
  const id = normalizeNumericId(uplayId)
  if (id === null) return null
  if (process.platform === "win32") {
    const appdata = process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming")
    return path.join(appdata, UPLAY_SAVE_DIR, id, "achievements.json")
  }
  if (!prefixo) return null
  return path.join(
    prefixo,
    "drive_c",
    "users",
    USUARIO_WINE,
    "AppData",
    "Roaming",
    UPLAY_SAVE_DIR,
    id,
    "achievements.json",
  )
}

function uplaySaveRoot(prefixo) {
  if (process.platform === "win32") {
    const appdata = process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming")
    return path.join(appdata, UPLAY_SAVE_DIR)
  }
  if (!prefixo) return null
  return path.join(prefixo, "drive_c", "users", USUARIO_WINE, "AppData", "Roaming", UPLAY_SAVE_DIR)
}

function uplaySchemaPath(gameDir) {
  return gameDir ? path.join(gameDir, "achievements_schema.json") : null
}

function firstExisting(dir, names) {
  for (const name of names) {
    const file = path.join(dir, name)
    try {
      if (fs.statSync(file).isFile()) return file
    } catch {}
  }
  return null
}

function inspectUplayInstallation(gameDir) {
  const root = gameDir ? path.resolve(String(gameDir)) : ""
  if (!root) return { detected: false, gameDir: root }
  const iniPath = firstExisting(root, ["upc_r2.ini"])
  const loaderPath = firstExisting(root, ["upc_r2_loader64.dll", "upc_r2_loader.dll"])
  const voicesPath = firstExisting(root, ["voices38.dll"])
  const schemaPath = uplaySchemaPath(root)
  return {
    detected: Boolean(iniPath && loaderPath),
    gameDir: root,
    iniPath,
    loaderPath,
    voicesPath,
    schemaPath,
  }
}

function iniSetting(text, key) {
  const re = new RegExp(`^\\s*${key}\\s*=\\s*([^\\r\\n;#]*)`, "im")
  const match = re.exec(String(text || ""))
  return match ? match[1].trim() : ""
}

function setIniSetting(text, key, value) {
  const source = String(text || "")
  const re = new RegExp(`^(\\s*${key}\\s*=\\s*)[^\\r\\n]*$`, "im")
  if (re.test(source)) return source.replace(re, `$1${value}`)
  const settings = /^(\s*\[Settings\]\s*\r?\n)/im.exec(source)
  if (settings) {
    const at = settings.index + settings[0].length
    return source.slice(0, at) + `${key} = ${value}\n` + source.slice(at)
  }
  return `[Settings]\n${key} = ${value}\n${source}`
}

function isSymlinkOrNonRegular(filePath) {
  try {
    const st = fs.lstatSync(filePath)
    return st.isSymbolicLink() || !st.isFile()
  } catch {
    return false
  }
}

function pathExists(filePath) {
  try {
    fs.lstatSync(filePath)
    return true
  } catch {
    return false
  }
}

function backupFile(filePath) {
  if (!filePath || !pathExists(filePath)) return null
  if (isSymlinkOrNonRegular(filePath)) throw new Error(`arquivo inseguro: ${filePath}`)
  const stamp = new Date()
    .toISOString()
    .replace(/[-:TZ.]/g, "")
    .slice(0, 14)
  let backup = `${filePath}.arcadia-backup-${stamp}`
  let n = 1
  while (pathExists(backup)) backup = `${filePath}.arcadia-backup-${stamp}-${n++}`
  fs.copyFileSync(filePath, backup)
  try {
    fs.chmodSync(backup, 0o600)
  } catch {}
  return backup
}

function atomicWrite(filePath, content, mode = 0o644) {
  if (!filePath) throw new Error("caminho vazio")
  if (pathExists(filePath) && isSymlinkOrNonRegular(filePath)) {
    throw new Error(`destino inseguro: ${filePath}`)
  }
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  const temporary = `${filePath}.arcadia-tmp-${process.pid}-${Date.now()}`
  let fd = null
  try {
    fd = fs.openSync(
      temporary,
      fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL,
      mode,
    )
    fs.writeFileSync(fd, content, { encoding: "utf8" })
    try {
      fs.fsyncSync(fd)
    } catch {}
    fs.closeSync(fd)
    fd = null
    fs.chmodSync(temporary, mode)
    if (pathExists(filePath) && isSymlinkOrNonRegular(filePath)) {
      throw new Error(`destino inseguro: ${filePath}`)
    }
    fs.renameSync(temporary, filePath)
  } finally {
    if (fd !== null) {
      try {
        fs.closeSync(fd)
      } catch {}
    }
    try {
      fs.rmSync(temporary, { force: true })
    } catch {}
  }
}

function readCatalogItems(appid, supplied) {
  if (Array.isArray(supplied)) return supplied
  const store = loadAchievements()
  return store?.[String(appid)]?.items || []
}

/**
 * Prepara uma instalação conhecida sem tocar em DLL/EXE/saves. Uma instalação
 * desconhecida ou um schema já existente inválido nunca é sobrescrito sem
 * confirmação explícita.
 */
function prepareUplayInstallation({
  gameDir,
  appid,
  settings = {},
  entry = {},
  items,
  autoEnable = true,
} = {}) {
  const info = inspectUplayInstallation(gameDir)
  if (!info.detected) return { ok: true, skipped: true, reason: "loader-upc-nao-detectado", info }

  const uplayId = resolveUplayId(appid, settings, entry)
  const result = { ok: true, skipped: false, info, uplayId, backupPaths: [], changed: false }

  // Validate the INI before creating a schema. This avoids a partial install if
  // the destination is a symlink or cannot be read.
  let currentIni = null
  let needsIniUpdate = false
  let iniMode = 0o600
  if (autoEnable && info.iniPath) {
    if (isSymlinkOrNonRegular(info.iniPath)) {
      return { ok: false, error: "ini-inseguro", info, uplayId, backupPaths: result.backupPaths }
    }
    try {
      currentIni = fs.readFileSync(info.iniPath, "utf8")
      needsIniUpdate = iniSetting(currentIni, "Achievements") !== "1"
      try {
        iniMode = fs.statSync(info.iniPath).mode & 0o777
      } catch {}
    } catch {
      return {
        ok: false,
        error: "ini-nao-pode-ser-lido",
        info,
        uplayId,
        backupPaths: result.backupPaths,
      }
    }
  }

  if (info.schemaPath && pathExists(info.schemaPath)) {
    if (isSymlinkOrNonRegular(info.schemaPath)) {
      return {
        ok: false,
        error: "schema-existente-inseguro",
        requiresConfirmation: true,
        info,
        uplayId,
      }
    }
    try {
      const schema = JSON.parse(fs.readFileSync(info.schemaPath, "utf8"))
      const valid = validateUplaySchema(schema)
      if (!valid.ok)
        return { ok: false, error: valid.error, requiresConfirmation: true, info, uplayId }
      result.schema = "existing"
    } catch {
      return {
        ok: false,
        error: "schema-existente-invalido",
        requiresConfirmation: true,
        info,
        uplayId,
      }
    }
  } else {
    // Sem uma relação UPC↔Steam confirmada não usamos a ordem do catálogo como
    // palpite. O Black Flag é a primeira entrada curada; novos jogos podem ser
    // habilitados acrescentando a relação em KNOWN_UPC_IDS.
    const known = Object.prototype.hasOwnProperty.call(KNOWN_UPC_IDS, String(appid))
    if (!known || !uplayId) {
      return { ok: true, skipped: true, reason: "catalogo-upc-nao-confirmado", info, uplayId }
    }
    let schema
    try {
      schema = buildUplaySchema(readCatalogItems(appid, items))
    } catch (e) {
      return { ok: false, error: `schema-upc-nao-gerado:${e.message}`, info, uplayId }
    }
    const valid = validateUplaySchema(schema, {
      requireContiguous: true,
      requireMetadata: true,
    })
    if (!valid.ok) return { ok: false, error: valid.error, info, uplayId }
    const content = JSON.stringify(schema, null, 2) + "\n"
    atomicWrite(info.schemaPath, content)
    result.schema = "created"
    result.changed = true
  }

  if (needsIniUpdate && currentIni !== null) {
    const backup = backupFile(info.iniPath)
    if (backup) result.backupPaths.push(backup)
    atomicWrite(info.iniPath, setIniSetting(currentIni, "Achievements", "1"), iniMode || 0o600)
    result.changed = true
  }
  return result
}

module.exports = {
  USUARIO_WINE,
  UPLAY_SAVE_DIR,
  KNOWN_UPC_IDS,
  normalizeNumericId,
  earnedValue,
  epochMilliseconds,
  parseUPC,
  numericAchievementId,
  itemUplayId,
  buildUplaySchema,
  validateUplaySchema,
  resolveUplayId,
  uplayRuntimePath,
  uplaySaveRoot,
  uplaySchemaPath,
  inspectUplayInstallation,
  iniSetting,
  setIniSetting,
  backupFile,
  atomicWrite,
  prepareUplayInstallation,
}
