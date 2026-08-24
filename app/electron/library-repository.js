"use strict"

// Repositório local da biblioteca.
//
// O indexador escreve uma biblioteca global (library.json), enquanto a posse
// pertence à conta ativa (owned_games.json). Este módulo concentra as regras
// que precisam ser iguais em todos os consumidores do processo principal:
// leitura versionada, filtro por conta, migração preguiçosa da posse e
// persistência atômica. Não conhece Electron nem canais IPC; o main só precisa
// adaptar readLibrary() ao contrato já exposto ao renderer.

const fsDefault = require("node:fs")
const path = require("node:path")
const { getDataDir } = require("./runtime-paths")
const {
  LIBRARY_SCHEMA_VERSION,
  normalizeLibrary,
} = require("../../contracts")

const OWNED_GAMES_NAME = "owned_games.json"
const LIBRARY_NAME = "library.json"

// Mantemos as constantes de caminho para consumidores legados (owned.js
// exportava OWNED_GAMES). Os caminhos usados pela factory são resolvidos por
// chamada, portanto troca de conta e ARCADIA_DATA_DIR continuam seguras.
const DEFAULT_DATA_DIR = getDataDir()
const LIBRARY_FILE = path.join(DEFAULT_DATA_DIR, LIBRARY_NAME)
const OWNED_GAMES = path.join(DEFAULT_DATA_DIR, OWNED_GAMES_NAME)

let temporarySequence = 0

function objectOrEmpty(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {}
}

function parseLibraryDocument(value, normalize = normalizeLibrary) {
  if (Array.isArray(value)) {
    return { version: 0, legacy: true, games: normalize(value) }
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { version: null, legacy: false, games: [], error: "formato_invalido" }
  }
  if (value.version !== LIBRARY_SCHEMA_VERSION || !Array.isArray(value.games)) {
    return { version: value.version ?? null, legacy: false, games: [], error: "versao_incompativel" }
  }
  return {
    version: LIBRARY_SCHEMA_VERSION,
    legacy: false,
    generatedAt: value.generated_at ?? value.generatedAt ?? null,
    sources: objectOrEmpty(value.sources),
    errors: Array.isArray(value.errors) ? value.errors.map(String).slice(0, 32) : [],
    games: normalize(value.games),
  }
}

function readLibraryFile(filePath, fsImpl = fsDefault, normalize = normalizeLibrary) {
  try {
    return parseLibraryDocument(JSON.parse(fsImpl.readFileSync(filePath, "utf8")), normalize)
  } catch (error) {
    return {
      version: null,
      legacy: false,
      games: [],
      error: error && error.code === "ENOENT" ? "ausente" : "leitura_falhou",
    }
  }
}

function normaliseSources(value) {
  return objectOrEmpty(value)
}

function buildLibraryPayload(
  games,
  { sources = {}, generatedAt = Math.floor(Date.now() / 1000), errors = [], normalize = normalizeLibrary } = {},
) {
  const payload = {
    version: LIBRARY_SCHEMA_VERSION,
    generated_at: generatedAt,
    sources: normaliseSources(sources),
    games: normalize(games),
  }
  if (Array.isArray(errors) && errors.length) {
    payload.errors = errors.map(String).slice(0, 32)
  }
  return payload
}

/**
 * Escreve no mesmo diretório e só publica depois que o conteúdo inteiro foi
 * gravado. O temporário recebe um nome único: dois handlers simultâneos não
 * disputam o mesmo `arquivo.tmp`. Em caso de erro o arquivo anterior continua
 * intacto e o temporário é removido quando possível.
 */
function writeJsonAtomic(filePath, value, { fsImpl = fsDefault, spacing = 2 } = {}) {
  const target = path.resolve(String(filePath))
  const temporary = `${target}.tmp-${process.pid}-${++temporarySequence}`
  const text = typeof value === "string" ? value : JSON.stringify(value, null, spacing)
  fsImpl.mkdirSync(path.dirname(target), { recursive: true })
  try {
    fsImpl.writeFileSync(temporary, text, "utf8")
    fsImpl.renameSync(temporary, target)
  } catch (error) {
    try {
      fsImpl.unlinkSync(temporary)
    } catch {
      // A falha original é mais útil ao chamador que a limpeza best-effort.
    }
    throw error
  }
  return target
}

function writeLibraryFile(
  filePath,
  games,
  { sources = {}, generatedAt = Math.floor(Date.now() / 1000), errors = [], fsImpl = fsDefault, normalize = normalizeLibrary } = {},
) {
  const payload = buildLibraryPayload(games, { sources, generatedAt, errors, normalize })
  writeJsonAtomic(filePath, payload, { fsImpl })
  return payload
}

function isAccountModule(value) {
  return value && typeof value === "object" && !Array.isArray(value)
}

function accountValue(account) {
  if (typeof account === "function") return account()
  if (isAccountModule(account)) {
    if (typeof account.conta === "function") return account.conta()
    if (typeof account.current === "function") return account.current()
    if (Object.prototype.hasOwnProperty.call(account, "current")) return account.current
    if (Object.prototype.hasOwnProperty.call(account, "username")) return account.username
  }
  return null
}

function defaultAccountModule() {
  // Lazy require keeps this module usable in pure tests and in tooling that
  // only needs parse/write helpers, without eagerly initializing account state.
  try {
    return require("./supabase/conta")
  } catch {
    return null
  }
}

function resolveOptionPath(value, fallback) {
  if (typeof value === "function") return value()
  return value || fallback
}

function createLibraryRepository({
  dataDir = getDataDir(),
  libraryPath,
  libraryFile,
  ownedPath,
  ownedFile,
  ownedGamesPath,
  ownedFileName = OWNED_GAMES_NAME,
  account,
  accountProvider,
  getAccount,
  isGuest,
  accountPath,
  fsImpl = fsDefault,
  normalize = normalizeLibrary,
  now = () => Date.now(),
} = {}) {
  const root = path.resolve(String(dataDir || getDataDir()))
  const configuredLibraryPath = libraryPath ?? libraryFile
  const globalLibraryPath = () =>
    resolveOptionPath(configuredLibraryPath, path.join(root, LIBRARY_NAME))
  const configuredOwnedPath = ownedPath ?? ownedFile ?? ownedGamesPath
  const accountModule =
    account ??
    accountProvider ??
    (typeof getAccount === "function" ? { conta: getAccount } : defaultAccountModule())

  function currentAccount() {
    try {
      return accountValue(accountModule)
    } catch {
      return null
    }
  }

  function loggedIn() {
    if (typeof isGuest === "function") {
      try {
        return !isGuest()
      } catch {
        return false
      }
    }
    return Boolean(currentAccount())
  }

  function currentOwnedPath() {
    const fallback = path.join(root, ownedFileName)
    if (typeof accountPath === "function") {
      return path.resolve(String(accountPath(currentAccount(), ownedFileName)))
    }
    if (configuredOwnedPath !== undefined) {
      const resolved = resolveOptionPath(configuredOwnedPath, fallback)
      return path.resolve(String(resolved))
    }
    // caminhoArquivoConta() already scopes the basename and creates the
    // account directory. Prefer it when available, just as conta.js does.
    if (isAccountModule(accountModule) && typeof accountModule.caminhoArquivoConta === "function") {
      return path.resolve(String(accountModule.caminhoArquivoConta(ownedFileName)))
    }
    if (isAccountModule(accountModule) && typeof accountModule.caminhoConta === "function") {
      return path.resolve(String(accountModule.caminhoConta(path.join(root, ownedFileName))))
    }
    const username = currentAccount()
    if (username) {
      return path.join(root, "contas", String(username), ownedFileName)
    }
    return fallback
  }

  function readDocument() {
    return readLibraryFile(globalLibraryPath(), fsImpl, normalize)
  }

  function readGlobal() {
    return readDocument().games
  }

  function readOwned() {
    try {
      const raw = JSON.parse(fsImpl.readFileSync(currentOwnedPath(), "utf8"))
      return Array.isArray(raw) ? raw : null
    } catch {
      return null
    }
  }

  function idsFromGames(games) {
    return normalize(games)
      .map((game) => game.id)
      .filter((id, index, ids) => ids.indexOf(id) === index)
  }

  /**
   * Materializa somente uma conta logada. Arquivo ausente (ou inválido) é
   * tratado como a migração lazy: a conta vê os jogos atuais e ganha uma lista
   * inicial. Guest nunca cria owned_games.json na raiz.
   */
  function materializeOwned(games, { force = false } = {}) {
    if (!loggedIn()) return false
    if (!force && readOwned() !== null) return false
    writeOwned(idsFromGames(games))
    return true
  }

  function filterByOwnership(games) {
    const list = Array.isArray(games) ? games : []
    if (!loggedIn()) return list
    const rawOwned = readOwned()
    if (rawOwned === null) {
      // Conveniência de migração, nunca requisito da leitura. Uma falha de
      // escrita deve deixar a leitura funcionando e devolvendo todos.
      try {
        materializeOwned(list)
      } catch {
        /* best-effort */
      }
      return list
    }
    const owned = new Set(rawOwned)
    return list.filter((game) => game && owned.has(game.id))
  }

  function readLibrary() {
    return filterByOwnership(readGlobal())
  }

  function writeOwned(ids) {
    if (!loggedIn()) return null
    const safe = Array.isArray(ids)
      ? ids
          .filter((id) => typeof id === "string" || typeof id === "number")
          .map((id) => String(id).trim())
          .filter(Boolean)
          .filter((id, index, list) => list.indexOf(id) === index)
      : []
    writeJsonAtomic(currentOwnedPath(), safe, { fsImpl })
    return safe
  }

  function ownedSet() {
    if (!loggedIn()) return new Set()
    return new Set(readOwned() || [])
  }

  function addOwned(id) {
    if (!loggedIn()) return false
    if (typeof id !== "string" && typeof id !== "number") return false
    const value = String(id).trim()
    if (!value) return false
    const ids = readOwned() || []
    if (ids.some((item) => String(item) === value)) return false
    writeOwned([...ids, value])
    return true
  }

  function removeOwned(id) {
    if (!loggedIn()) return false
    if (typeof id !== "string" && typeof id !== "number") return false
    const value = String(id).trim()
    const ids = readOwned()
    if (ids === null) return false
    const next = ids.filter((item) => String(item) !== value)
    if (next.length === ids.length) return false
    writeOwned(next)
    return true
  }

  function writeLibrary(games, options = {}) {
    const generatedAt = options.generatedAt === undefined ? Math.floor(now() / 1000) : options.generatedAt
    return writeLibraryFile(globalLibraryPath(), games, {
      ...options,
      generatedAt,
      fsImpl,
      normalize,
    })
  }

  return {
    dataDir: root,
    libraryPath: globalLibraryPath,
    ownedPath: currentOwnedPath,
    readDocument,
    readLibraryDocument: readDocument,
    readGlobal,
    readLibrary,
    read: readLibrary,
    getLibrary: readLibrary,
    readOwned,
    getOwned: readOwned,
    filterByOwnership,
    filterByAccount: filterByOwnership,
    filtrarPorPosse: filterByOwnership,
    materializeOwned,
    materializarPosse: materializeOwned,
    writeOwned,
    gravarOwned: writeOwned,
    ownedSet,
    addOwned,
    add: addOwned,
    ownedAdd: addOwned,
    removeOwned,
    remove: removeOwned,
    ownedRemove: removeOwned,
    writeLibrary,
    saveLibrary: writeLibrary,
    save: writeLibrary,
    write: writeLibrary,
  }
}

// Facade compatível para consumidores que ainda não receberam o adaptador do
// main. Cada operação usa um repositório sem cache, então testes/conta/troca de
// ARCADIA_DATA_DIR não ficam presos ao estado da chamada anterior.
function defaultRepository() {
  return createLibraryRepository()
}

function readLibrary() {
  return defaultRepository().readLibrary()
}
function readOwned() {
  return defaultRepository().readOwned()
}
function filterByOwnership(games) {
  return defaultRepository().filterByOwnership(games)
}
const filtrarPorPosse = filterByOwnership
function materializeOwned(games, options) {
  return defaultRepository().materializeOwned(games, options)
}
function writeOwned(ids) {
  return defaultRepository().writeOwned(ids)
}
const gravarOwned = writeOwned
function ownedSet() {
  return defaultRepository().ownedSet()
}
function ownedAdd(id) {
  return defaultRepository().addOwned(id)
}
function ownedRemove(id) {
  return defaultRepository().removeOwned(id)
}
function writeLibrary(filePathOrGames, gamesOrOptions, maybeOptions) {
  // Same signature as library-store when the first argument is a path, plus a
  // convenient default-repository signature writeLibrary(games, options).
  if (typeof filePathOrGames === "string") {
    return writeLibraryFile(filePathOrGames, gamesOrOptions, maybeOptions || {})
  }
  return defaultRepository().writeLibrary(filePathOrGames, gamesOrOptions || {})
}

module.exports = {
  LIBRARY_SCHEMA_VERSION,
  LIBRARY_NAME,
  LIBRARY_FILE,
  LIBRARY: LIBRARY_FILE,
  OWNED_GAMES_NAME,
  OWNED_GAMES,
  parseLibraryDocument,
  parse: parseLibraryDocument,
  readLibraryFile,
  readLibraryDocument: readLibraryFile,
  buildLibraryPayload,
  writeJsonAtomic,
  writeLibraryFile,
  createLibraryRepository,
  readLibrary,
  readOwned,
  filterByOwnership,
  filterByAccount: filterByOwnership,
  filtrarPorPosse,
  materializeOwned,
  materializarPosse: materializeOwned,
  writeOwned,
  gravarOwned,
  ownedSet,
  ownedAdd,
  ownedRemove,
  writeLibrary,
}
