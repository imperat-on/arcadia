"use strict"

// Catálogo e perfis de emuladores. Este módulo só valida arquivos e monta
// argv; nunca usa shell, nunca executa binários e não conhece Electron.
const fsDefault = require("node:fs")
const path = require("node:path")
const os = require("node:os")

const REGISTRY_VERSION = 1
const REGISTRY_FILENAME = "emulators.json"
const MAX_PROFILES = 64
const MAX_ARGS = 32
const MAX_ARG_LENGTH = 1024
const ID_RE = /^[a-z0-9][a-z0-9._-]{1,63}$/
const COMMAND_RE = /^[A-Za-z0-9][A-Za-z0-9._+@-]{0,127}$/

const DEFINITIONS = Object.freeze({
  pcsx2: Object.freeze({
    id: "pcsx2",
    name: "PCSX2",
    systems: ["PlayStation 2"],
    description: "Emulador de PlayStation 2",
    candidates: ["pcsx2-qt", "pcsx2"],
  }),
  rpcs3: Object.freeze({
    id: "rpcs3",
    name: "RPCS3",
    systems: ["PlayStation 3"],
    description: "Emulador de PlayStation 3",
    candidates: ["rpcs3"],
  }),
  dolphin: Object.freeze({
    id: "dolphin",
    name: "Dolphin",
    systems: ["GameCube", "Wii"],
    description: "Emulador de GameCube e Wii",
    candidates: ["dolphin-emu"],
  }),
  ppsspp: Object.freeze({
    id: "ppsspp",
    name: "PPSSPP",
    systems: ["PlayStation Portable"],
    description: "Emulador de PSP",
    candidates: ["ppsspp"],
  }),
  duckstation: Object.freeze({
    id: "duckstation",
    name: "DuckStation",
    systems: ["PlayStation"],
    description: "Emulador de PlayStation 1",
    candidates: ["duckstation-qt", "duckstation"],
  }),
  retroarch: Object.freeze({
    id: "retroarch",
    name: "RetroArch",
    systems: ["Multi-sistema"],
    description: "Frontend para cores libretro",
    candidates: ["retroarch"],
    requiresCore: true,
  }),
  melonds: Object.freeze({
    id: "melonds",
    name: "melonDS",
    systems: ["Nintendo DS"],
    description: "Emulador de Nintendo DS",
    candidates: ["melonDS", "melonds"],
  }),
  desmume: Object.freeze({
    id: "desmume",
    name: "DeSmuME",
    systems: ["Nintendo DS"],
    description: "Emulador de Nintendo DS",
    candidates: ["desmume"],
  }),
})

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

function normalizeId(value) {
  const id = typeof value === "string" ? value.trim().toLowerCase() : ""
  return ID_RE.test(id) ? id : ""
}

function normalizeCommand(value) {
  if (typeof value !== "string") return ""
  const command = value.trim()
  if (!command || command.includes("\u0000")) return ""
  if (path.isAbsolute(command)) return path.normalize(command)
  return COMMAND_RE.test(command) ? command : ""
}

function normalizeDefinition(input) {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null
  const id = normalizeId(input.id)
  const name = typeof input.name === "string" ? input.name.trim() : ""
  const description =
    typeof input.description === "string" && input.description.trim()
      ? input.description.trim()
      : name
        ? `${name} (perfil customizado)`
        : ""
  const systems = Array.isArray(input.systems)
    ? input.systems
        .filter((item) => typeof item === "string" && item.trim())
        .map((item) => item.trim())
        .slice(0, 8)
    : []
  const candidates = Array.isArray(input.candidates)
    ? input.candidates.map(normalizeCommand).filter(Boolean).slice(0, 16)
    : []
  if (
    !id ||
    !name ||
    name.length > 120 ||
    !systems.length ||
    !description ||
    description.length > 300
  )
    return null
  return Object.freeze({
    id,
    name,
    systems,
    description,
    candidates: [...new Set(candidates)],
    ...(input.requiresCore === true ? { requiresCore: true } : {}),
  })
}

function normalizeAbsoluteFile(value, field = "path") {
  if (typeof value !== "string" || !value.trim() || value.includes("\u0000")) {
    return { ok: false, error: `${field}_invalido` }
  }
  const raw = value.trim()
  // ROM/core selectors return absolute paths. Refuse relative paths so a
  // renderer cannot make resolution depend on Electron's current directory.
  if (!path.isAbsolute(raw)) return { ok: false, error: `${field}_invalido` }
  const file = path.normalize(raw)
  if (!path.isAbsolute(file)) return { ok: false, error: `${field}_invalido` }
  return { ok: true, value: file }
}

function normalizeArgs(value) {
  if (value === undefined) return { ok: true, value: [] }
  if (!Array.isArray(value) || value.length > MAX_ARGS)
    return { ok: false, error: "args_invalidos" }
  const args = []
  for (const item of value) {
    if (typeof item !== "string" || item.includes("\u0000") || item.length > MAX_ARG_LENGTH) {
      return { ok: false, error: "args_invalidos" }
    }
    args.push(item)
  }
  return { ok: true, value: args }
}

function isRegular(file, fsImpl) {
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isFile() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function isExecutable(file, fsImpl) {
  if (!isRegular(file, fsImpl)) return false
  try {
    return (fsImpl.statSync(file).mode & 0o111) !== 0
  } catch {
    return false
  }
}

function findOnPath(command, fsImpl, envPath) {
  const normalized = normalizeCommand(command)
  if (!normalized) return ""
  if (path.isAbsolute(normalized)) return isExecutable(normalized, fsImpl) ? normalized : ""
  for (const directory of String(envPath || process.env.PATH || "").split(path.delimiter)) {
    if (!directory) continue
    const candidate = path.join(directory, normalized)
    if (isExecutable(candidate, fsImpl)) return candidate
  }
  return ""
}

function normalizeProfile(input, definition) {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, error: "perfil_invalido" }
  }
  const id = normalizeId(input.id)
  if (!id || !definition || id !== definition.id)
    return { ok: false, error: "emulador_desconhecido" }
  const executable = normalizeCommand(input.executable || input.command)
  if (!executable) return { ok: false, error: "executavel_invalido" }
  const args = normalizeArgs(input.args)
  if (!args.ok) return args
  let corePath = ""
  if (input.corePath !== undefined && input.corePath !== "") {
    const normalizedCore = normalizeAbsoluteFile(input.corePath, "corePath")
    if (!normalizedCore.ok) return normalizedCore
    corePath = normalizedCore.value
  }
  return {
    ok: true,
    profile: {
      id,
      executable,
      corePath,
      args: args.value,
      updatedAt: Number.isFinite(Number(input.updatedAt))
        ? Math.trunc(Number(input.updatedAt))
        : Date.now(),
    },
  }
}

function readState(file, fsImpl) {
  try {
    const stat = fsImpl.lstatSync(file)
    if (!stat.isFile() || stat.isSymbolicLink()) return { profiles: {} }
    const parsed = JSON.parse(fsImpl.readFileSync(file, "utf8"))
    if (!parsed || parsed.version !== REGISTRY_VERSION || typeof parsed.profiles !== "object")
      return { profiles: {} }
    return { profiles: parsed.profiles }
  } catch {
    return { profiles: {} }
  }
}

function atomicWrite(file, value, fsImpl = fsDefault) {
  const directory = path.dirname(file)
  fsImpl.mkdirSync(directory, { recursive: true, mode: 0o700 })
  try {
    const stat = fsImpl.lstatSync(directory)
    if (!stat.isDirectory() || stat.isSymbolicLink())
      throw new Error("diretorio_emuladores_invalido")
    // `mkdir` already applies 0700 to a new directory. Do not chmod through a
    // path after lstat: a concurrent symlink swap must never reach its target.
  } catch (error) {
    if (error?.message === "diretorio_emuladores_invalido") throw error
    throw new Error("diretorio_emuladores_invalido")
  }
  if (isRegular(file, fsImpl) === false) {
    try {
      if (fsImpl.lstatSync(file).isSymbolicLink()) throw new Error("arquivo_emuladores_symlink")
    } catch (error) {
      if (error?.message === "arquivo_emuladores_symlink") throw error
    }
  }
  const temporary = `${file}.${process.pid}.${Date.now()}.tmp`
  try {
    fsImpl.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, {
      encoding: "utf8",
      mode: 0o600,
      flag: "wx",
    })
    if (fsImpl.lstatSync(temporary).isSymbolicLink())
      throw new Error("temporario_emuladores_symlink")
    fsImpl.renameSync(temporary, file)
    // The temporary was created with 0600; rename preserves that mode without
    // a second path-based chmod that could follow a raced symlink.
  } finally {
    try {
      fsImpl.rmSync(temporary, { force: true })
    } catch {}
  }
}

// `extraDefinitions`/`definitions` permite que um integrador adicione um
// emulador sem alterar a política de argv ou habilitar execução arbitrária.
function createEmulatorRegistry({
  dataDir = path.join(
    process.env.ARCADIA_DATA_DIR || path.join(os.homedir(), ".local", "share", "arcadia"),
  ),
  profilesPath,
  fsImpl = fsDefault,
  envPath = process.env.PATH,
  extraDefinitions = [],
  definitions: definitionsOption,
} = {}) {
  const root = path.resolve(String(dataDir))
  const statePath = profilesPath ? path.resolve(profilesPath) : path.join(root, REGISTRY_FILENAME)
  const catalog = { ...DEFINITIONS }
  const configuredDefinitions =
    definitionsOption === undefined ? extraDefinitions : definitionsOption
  const extensions = Array.isArray(configuredDefinitions)
    ? configuredDefinitions
    : Object.values(configuredDefinitions || {})
  for (const extension of extensions) {
    const normalized = normalizeDefinition(extension)
    if (normalized) catalog[normalized.id] = normalized
  }

  function definitions() {
    return Object.values(catalog).map((item) => clone(item))
  }

  function readProfiles() {
    const state = readState(statePath, fsImpl)
    const profiles = {}
    for (const definition of Object.values(catalog)) {
      const candidate = state.profiles?.[definition.id]
      const normalized = normalizeProfile({ ...candidate, id: definition.id }, definition)
      if (normalized.ok) profiles[definition.id] = normalized.profile
    }
    return profiles
  }

  function profile(id) {
    return readProfiles()[normalizeId(id)] || null
  }

  function detectOne(definition, profiles) {
    const configured = profiles[definition.id]
    const configuredExecutable = configured
      ? findOnPath(configured.executable, fsImpl, envPath)
      : ""
    const detected = configured
      ? configuredExecutable
      : definition.candidates
          .map((candidate) => findOnPath(candidate, fsImpl, envPath))
          .find(Boolean) || ""
    return {
      ...clone(definition),
      // Keep the configured value visible for editing, but `available` only
      // becomes true after regular-file/executable validation.
      executable: detected || configured?.executable || "",
      available: Boolean(detected),
      source: configured ? "configured" : detected ? "detected" : "builtin",
      profile: configured ? { ...clone(configured), executable: configured.executable } : undefined,
    }
  }

  function list() {
    const profiles = readProfiles()
    return definitions().map((definition) => detectOne(definition, profiles))
  }

  function detect() {
    return { ok: true, emulators: list() }
  }

  function saveProfiles(profiles) {
    atomicWrite(statePath, { version: REGISTRY_VERSION, profiles }, fsImpl)
  }

  function setProfile(input) {
    const id = normalizeId(input?.id)
    const definition = catalog[id]
    const normalized = normalizeProfile(input, definition)
    if (!normalized.ok) return normalized
    const profiles = readProfiles()
    if (!profiles[id] && Object.keys(profiles).length >= MAX_PROFILES)
      return { ok: false, error: "perfis_emuladores_limite" }
    profiles[id] = normalized.profile
    try {
      saveProfiles(profiles)
      return {
        ok: true,
        profile: { ...clone(normalized.profile), executable: normalized.profile.executable },
      }
    } catch {
      return { ok: false, error: "emuladores_nao_gravavel" }
    }
  }

  function removeProfile(id) {
    const normalized = normalizeId(id)
    if (!catalog[normalized]) return { ok: false, error: "emulador_desconhecido" }
    const profiles = readProfiles()
    if (!profiles[normalized]) return { ok: true, removed: false }
    delete profiles[normalized]
    try {
      saveProfiles(profiles)
      return { ok: true, removed: true }
    } catch {
      return { ok: false, error: "emuladores_nao_gravavel" }
    }
  }

  function resolveLaunch({ emulatorId, romPath, extraArgs = [], corePath } = {}) {
    const id = normalizeId(emulatorId)
    const definition = catalog[id]
    if (!definition) return { ok: false, error: "emulador_desconhecido" }
    const rom = normalizeAbsoluteFile(romPath, "romPath")
    if (!rom.ok || !isRegular(rom.value, fsImpl)) return { ok: false, error: "rom_invalida" }
    const profiles = readProfiles()
    const configured = profiles[id]
    const configuredExecutable = configured
      ? findOnPath(configured.executable, fsImpl, envPath)
      : ""
    if (configured && !configuredExecutable)
      return { ok: false, error: "executavel_configurado_invalido" }
    const executable =
      configuredExecutable ||
      definition.candidates
        .map((candidate) => findOnPath(candidate, fsImpl, envPath))
        .find(Boolean) ||
      ""
    if (!executable) return { ok: false, error: "emulador_nao_encontrado" }
    const args = normalizeArgs(extraArgs)
    if (!args.ok) return args
    const baseArgs = configured?.args || []
    const selectedCore = corePath || configured?.corePath || ""
    const command = [executable, ...baseArgs]
    if (definition.requiresCore) {
      const core = normalizeAbsoluteFile(selectedCore, "corePath")
      if (!core.ok || !isRegular(core.value, fsImpl))
        return { ok: false, error: "retroarch_core_invalido" }
      command.push("-L", core.value)
    }
    command.push(rom.value, ...args.value)
    return { ok: true, emulatorId: id, system: definition.systems[0], cmd: command }
  }

  return Object.freeze({
    definitions,
    list,
    detect,
    profiles: () => clone(readProfiles()),
    getProfile: profile,
    setProfile,
    removeProfile,
    resolveLaunch,
    paths: () => ({ statePath }),
  })
}

module.exports = {
  REGISTRY_VERSION,
  REGISTRY_FILENAME,
  DEFINITIONS,
  normalizeId,
  normalizeCommand,
  normalizeDefinition,
  normalizeArgs,
  normalizeProfile,
  findOnPath,
  atomicWrite,
  createEmulatorRegistry,
}
