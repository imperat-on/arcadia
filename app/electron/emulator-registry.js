"use strict"

// Catálogo, perfis, scanner local e resolvedor de emuladores. Este módulo só
// lê/valida arquivos e monta argv; nunca usa shell, nunca executa binários e
// não conhece Electron.
const fsDefault = require("node:fs")
const path = require("node:path")
const os = require("node:os")

const REGISTRY_VERSION = 1
const REGISTRY_FILENAME = "emulators.json"
const ROMS_FILENAME = "roms.json"
const ROMS_VERSION = 1
const MAX_PROFILES = 64
const MAX_ARGS = 32
const MAX_ARG_LENGTH = 1024
const MAX_SCAN_DEPTH = 8
const MAX_SCAN_RESULTS = 4096
const MAX_ROM_FOLDERS = 32
// Discovery deliberately stays bounded and read-only.  A PATH entry can point
// anywhere, so AppImage/Flatpak discovery never recursively walks a home tree
// or executes a candidate to identify it.
const MAX_APPIMAGE_RESULTS = 64
const MAX_DISCOVERY_ENTRIES = 512
const LINUX_STANDARD_PATHS = Object.freeze([
  "/usr/local/bin",
  "/usr/bin",
  "/bin",
  "/usr/local/sbin",
  "/usr/sbin",
  "/sbin",
  "/usr/local/games",
  "/usr/games",
  "/snap/bin",
])
const ID_RE = /^[a-z0-9][a-z0-9._-]{1,63}$/
const COMMAND_RE = /^[A-Za-z0-9][A-Za-z0-9._+@-]{0,127}$/
const FLATPAK_ID_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$/
const ROM_EXTENSION_RE = /^\.[a-z0-9][a-z0-9+_.-]{0,15}$/i

// File types accepted by the built-in scanners. These are deliberately an
// allowlist: scanning a folder must not turn every file in a user's home into
// a launch candidate. Sidecars (for example PS1 .bin files) are retained in
// the catalogue and grouped under their primary image by `scanRoms`.
const BUILTIN_ROM_EXTENSIONS = Object.freeze({
  pcsx2: Object.freeze([".iso", ".chd", ".cso", ".zso", ".gz", ".nrg", ".cue", ".bin", ".mds", ".mdf", ".m3u"]),
  rpcs3: Object.freeze([".iso", ".pkg", ".elf", ".self"]),
  dolphin: Object.freeze([".iso", ".rvz", ".wbfs", ".wia", ".gcz", ".ciso", ".nkit.iso", ".wad"]),
  ppsspp: Object.freeze([".iso", ".cso", ".chd", ".pbp", ".elf"]),
  duckstation: Object.freeze([".cue", ".bin", ".iso", ".chd", ".pbp", ".img", ".sub", ".ccd", ".mds", ".mdf", ".ecm", ".m3u"]),
  retroarch: Object.freeze([
    ".zip", ".7z", ".nes", ".unf", ".unif", ".gb", ".gbc", ".gba", ".gbx", ".nds", ".dsi",
    ".smc", ".sfc", ".fig", ".swc", ".n64", ".z64", ".v64", ".md", ".gen", ".smd", ".sms",
    ".gg", ".32x", ".pce", ".cue", ".bin", ".iso", ".chd", ".pbp", ".m3u", ".img", ".wbfs",
  ]),
  melonds: Object.freeze([".nds", ".dsi"]),
  desmume: Object.freeze([".nds", ".dsi"]),
})

// Sidecar files should not appear as duplicate games when a primary image is
// present in the same directory. A standalone .bin/.img remains a candidate.
const ROM_SIDECAR_RULES = Object.freeze({
  pcsx2: Object.freeze({ ".cue": Object.freeze([".bin"]), ".mds": Object.freeze([".mdf"]) }),
  duckstation: Object.freeze({
    ".cue": Object.freeze([".bin"]),
    ".ccd": Object.freeze([".img", ".sub"]),
    ".mds": Object.freeze([".mdf"]),
  }),
})

// Hydra's classic launch path uses deterministic, emulator-owned flags. They
// are selected by the main process through `launchMode: "hydra"`; the default
// resolver remains compatible with the original Arcadia argv contract.
const HYDRA_LAUNCH_PREFIX = Object.freeze({
  duckstation: Object.freeze(["-batch", "-fullscreen", "--"]),
  pcsx2: Object.freeze(["-batch", "-fullscreen", "--"]),
  rpcs3: Object.freeze(["--no-gui"]),
  retroarch: Object.freeze([]),
})
const HYDRA_LAUNCH_SUFFIX = Object.freeze({
  retroarch: Object.freeze(["-f"]),
})
const RPCS3_PKG_MAGIC = 0x7f504b47
const RPCS3_TITLE_ID_RE = /[A-Z]{4}\d{5}/
const LAUNCH_CODES = Object.freeze({
  DISC_SIDECAR_MISSING: "DISC_SIDECAR_MISSING",
  PACKAGE_TITLE_INVALID: "PACKAGE_TITLE_INVALID",
  PACKAGE_INSTALL_REQUIRED: "PACKAGE_INSTALL_REQUIRED",
})

const DEFINITIONS = Object.freeze({
  pcsx2: Object.freeze({
    id: "pcsx2",
    name: "PCSX2",
    systems: ["PlayStation 2"],
    description: "Emulador de PlayStation 2",
    candidates: ["pcsx2-qt", "pcsx2", "PCSX2"],
    flatpakIds: ["net.pcsx2.PCSX2"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.pcsx2,
    romDirectoryMarkers: [],
  }),
  rpcs3: Object.freeze({
    id: "rpcs3",
    name: "RPCS3",
    systems: ["PlayStation 3"],
    description: "Emulador de PlayStation 3",
    candidates: ["rpcs3"],
    flatpakIds: ["net.rpcs3.RPCS3"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.rpcs3,
    romDirectoryMarkers: ["PS3_GAME", "ps3_game"],
  }),
  dolphin: Object.freeze({
    id: "dolphin",
    name: "Dolphin",
    systems: ["GameCube", "Wii"],
    description: "Emulador de GameCube e Wii",
    // `dolphin` sozinho é o gerenciador de arquivos do KDE no Linux. O
    // emulador instala `dolphin-emu`; aceitar o nome curto gera falso positivo.
    candidates: ["dolphin-emu"],
    flatpakIds: ["org.DolphinEmu.dolphin-emu"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.dolphin,
    romDirectoryMarkers: [],
  }),
  ppsspp: Object.freeze({
    id: "ppsspp",
    name: "PPSSPP",
    systems: ["PlayStation Portable"],
    description: "Emulador de PSP",
    candidates: ["ppsspp", "PPSSPP"],
    flatpakIds: ["org.ppsspp.PPSSPP"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.ppsspp,
    romDirectoryMarkers: [],
  }),
  duckstation: Object.freeze({
    id: "duckstation",
    name: "DuckStation",
    systems: ["PlayStation"],
    description: "Emulador de PlayStation 1",
    candidates: ["duckstation-qt", "duckstation"],
    flatpakIds: ["org.duckstation.DuckStation"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.duckstation,
    romDirectoryMarkers: [],
  }),
  retroarch: Object.freeze({
    id: "retroarch",
    name: "RetroArch",
    systems: ["Multi-sistema"],
    description: "Frontend para cores libretro",
    candidates: ["retroarch"],
    flatpakIds: ["org.libretro.RetroArch"],
    requiresCore: true,
    romExtensions: BUILTIN_ROM_EXTENSIONS.retroarch,
    romDirectoryMarkers: [],
  }),
  melonds: Object.freeze({
    id: "melonds",
    name: "melonDS",
    systems: ["Nintendo DS"],
    description: "Emulador de Nintendo DS",
    candidates: ["melonDS", "melonds"],
    flatpakIds: ["net.kuribo64.melonDS"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.melonds,
    romDirectoryMarkers: [],
  }),
  desmume: Object.freeze({
    id: "desmume",
    name: "DeSmuME",
    systems: ["Nintendo DS"],
    description: "Emulador de Nintendo DS",
    candidates: ["desmume"],
    flatpakIds: ["org.desmume.DeSmuME"],
    romExtensions: BUILTIN_ROM_EXTENSIONS.desmume,
    romDirectoryMarkers: [],
  }),
})

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

function normalizeId(value) {
  const id = typeof value === "string" ? value.trim().toLowerCase() : ""
  if (["__proto__", "constructor", "prototype"].includes(id)) return ""
  return ID_RE.test(id) ? id : ""
}

function normalizeCommand(value) {
  if (typeof value !== "string") return ""
  const command = value.trim()
  if (!command || command.includes("\u0000")) return ""
  if (path.isAbsolute(command)) return path.normalize(command)
  return COMMAND_RE.test(command) ? command : ""
}

function normalizeFlatpakId(value) {
  if (typeof value !== "string" || value.includes("\u0000")) return ""
  const id = value.trim()
  return FLATPAK_ID_RE.test(id) ? id : ""
}

function normalizeFlatpakIds(value) {
  if (!Array.isArray(value)) return []
  return [...new Set(value.map(normalizeFlatpakId).filter(Boolean))].slice(0, 8)
}

function normalizeRomExtension(value) {
  if (typeof value !== "string" || value.includes("\u0000")) return ""
  const raw = value.trim().toLowerCase()
  const extension = raw.startsWith(".") ? raw : `.${raw}`
  return ROM_EXTENSION_RE.test(extension) ? extension : ""
}

function normalizeRomExtensions(value) {
  if (!Array.isArray(value)) return []
  return [...new Set(value.map(normalizeRomExtension).filter(Boolean))].slice(0, 64)
}

function normalizeRomDirectoryMarkers(value) {
  if (!Array.isArray(value)) return []
  return [
    ...new Set(
      value
        .filter((item) => typeof item === "string" && item.trim() && !item.includes("\u0000"))
        .map((item) => item.trim())
        .filter((item) => item !== "." && item !== ".." && !item.includes("/") && !item.includes("\\"))
        .slice(0, 16),
    ),
  ]
}

function normalizeRomFolders(value) {
  if (value === undefined) return []
  if (!Array.isArray(value)) return { ok: false, error: "rom_folders_invalidas" }
  const folders = []
  const seen = new Set()
  for (const item of value) {
    if (typeof item !== "string" && (!item || typeof item !== "object" || Array.isArray(item))) {
      return { ok: false, error: "rom_folder_invalida" }
    }
    const rawPath = typeof item === "string" ? item : item.path
    const normalized = normalizeAbsoluteFile(rawPath, "romFolder")
    if (!normalized.ok) return { ok: false, error: "rom_folder_invalida" }
    if (seen.has(normalized.value)) continue
    seen.add(normalized.value)
    folders.push({ path: normalized.value, recursive: typeof item === "object" && item.recursive === false ? false : true })
    if (folders.length >= MAX_ROM_FOLDERS) break
  }
  return folders
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
  const romExtensions = normalizeRomExtensions(input.romExtensions ?? input.extensions)
  const romDirectoryMarkers = normalizeRomDirectoryMarkers(input.romDirectoryMarkers)
  const flatpakIds = normalizeFlatpakIds(input.flatpakIds)
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
    ...(flatpakIds.length ? { flatpakIds } : {}),
    romExtensions,
    romDirectoryMarkers,
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
  // Checa os dois separadores: no Windows path.sep e backslash, mas um
  // caminho misto com barra e ".." passaria pelo crivo so-com-sep e o
  // normalize resolveria para fora da raiz: bypass de validacao.
  if (!path.isAbsolute(raw) || raw.split(/[\\/]/).includes("..")) {
    return { ok: false, error: `${field}_invalido` }
  }
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

function hasSymlinkComponent(file, fsImpl) {
  const absolute = path.resolve(file)
  const root = path.parse(absolute).root
  let current = root
  const relative = path.relative(root, absolute)
  for (const part of relative ? relative.split(path.sep) : []) {
    current = path.join(current, part)
    try {
      if (fsImpl.lstatSync(current).isSymbolicLink()) return true
    } catch {
      return false
    }
  }
  return false
}

function isRegular(file, fsImpl) {
  if (hasSymlinkComponent(file, fsImpl)) return false
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isFile() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function isDirectory(file, fsImpl) {
  if (hasSymlinkComponent(file, fsImpl)) return false
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isDirectory() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function isRomExtensionAllowed(name, definition) {
  const allowed = normalizeRomExtensions(definition?.romExtensions)
  if (!allowed.length) return true
  const lower = String(name || "").toLowerCase()
  return allowed.some((extension) => lower.endsWith(extension))
}

function isRomPathValid(file, definition, fsImpl) {
  if (isRegular(file, fsImpl)) return isRomExtensionAllowed(path.basename(file), definition)
  if (!isDirectory(file, fsImpl)) return false
  const markers = normalizeRomDirectoryMarkers(definition?.romDirectoryMarkers)
  return markers.includes(path.basename(file))
}

function isCoreFile(file, fsImpl) {
  if (!isRegular(file, fsImpl)) return false
  const lower = path.basename(file).toLowerCase()
  return lower.endsWith(".so") || lower.includes(".so.") || lower.endsWith(".dll") || lower.endsWith(".dylib")
}

function resolveHydraBootPath(emulatorId, romPath, fsImpl, executablePath, homeDir = os.homedir()) {
  if ((emulatorId === "pcsx2" || emulatorId === "duckstation") && path.extname(romPath).toLowerCase() === ".mds") {
    const stem = romPath.slice(0, -4)
    const mdf = [`${stem}.mdf`, `${stem}.MDF`].find((candidate) => isRegular(candidate, fsImpl))
    if (!mdf) {
      return {
        ok: false,
        error: "disc_sidecar_ausente",
        code: LAUNCH_CODES.DISC_SIDECAR_MISSING,
        expectedPath: `${stem}.mdf`,
      }
    }
    return { ok: true, path: mdf }
  }
  if (emulatorId !== "rpcs3" || path.extname(romPath).toLowerCase() !== ".pkg") {
    return { ok: true, path: romPath }
  }
  let handle
  try {
    handle = fsImpl.openSync(romPath, "r")
    const header = Buffer.alloc(0x80)
    const bytes = fsImpl.readSync(handle, header, 0, header.length, 0)
    if (bytes < 0x54 || header.readUInt32BE(0) !== RPCS3_PKG_MAGIC) {
      return { ok: false, error: "pkg_titulo_invalido", code: LAUNCH_CODES.PACKAGE_TITLE_INVALID }
    }
    const titleId = RPCS3_TITLE_ID_RE.exec(header.subarray(0, bytes).toString("latin1"))?.[0]
    if (!titleId) return { ok: false, error: "pkg_titulo_invalido", code: LAUNCH_CODES.PACKAGE_TITLE_INVALID }
    const roots = [
      path.join(homeDir, ".config", "rpcs3"),
      path.join(homeDir, ".var", "app", "net.rpcs3.RPCS3", "config", "rpcs3"),
    ]
    const executableValue = normalizeAbsoluteFile(executablePath, "executablePath")
    const executable = executableValue.ok ? executableValue.value : ""
    if (executable) roots.push(path.dirname(executable))
    for (const root of [...new Set(roots)]) {
      const eboot = path.join(root, "dev_hdd0", "game", titleId.toUpperCase(), "USRDIR", "EBOOT.BIN")
      if (isRegular(eboot, fsImpl)) return { ok: true, path: eboot, titleId: titleId.toUpperCase() }
    }
    return {
      ok: false,
      error: "pkg_instalacao_necessaria",
      code: LAUNCH_CODES.PACKAGE_INSTALL_REQUIRED,
      titleId: titleId.toUpperCase(),
    }
  } catch {
    return { ok: false, error: "pkg_titulo_invalido", code: LAUNCH_CODES.PACKAGE_TITLE_INVALID }
  } finally {
    try {
      if (handle !== undefined) fsImpl.closeSync(handle)
    } catch {}
  }
}

function isExecutable(file, fsImpl) {
  try {
    const stat = fsImpl.statSync(file)
    if (!stat.isFile()) return false
    // No Windows o bit de execução POSIX não existe no NTFS: chmodSync é no-op
    // e arquivos criados pelo usuário nascem com mode 0o666. Exigir 0o111 ali
    // quebrava toda detecção/resolução de emuladores no Windows. Arquivo
    // regular é tratável como executável; a permissão real é a ACL do NTFS.
    if (process.platform === "win32") return true
    return (stat.mode & 0o111) !== 0
  } catch {
    return false
  }
}

function splitPathValue(envPath) {
  const value = envPath === undefined ? process.env.PATH : envPath
  return String(value || "")
    .split(path.delimiter)
    .filter((entry) => entry && !entry.includes("\u0000"))
}

function normalizeDiscoveryPath(value) {
  if (typeof value !== "string" || !value.trim() || value.includes("\u0000")) return ""
  const raw = value.trim()
  return path.isAbsolute(raw) ? path.normalize(raw) : ""
}

function uniqueDiscoveryPaths(values) {
  const seen = new Set()
  const result = []
  for (const value of values || []) {
    const normalized = normalizeDiscoveryPath(value)
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    result.push(normalized)
  }
  return result
}

function buildLinuxSearchPath(envPath, platform, homeDir, includeStandardPaths = true) {
  // Keep relative PATH entries intact: shells commonly use `.` or a project
  // relative bin directory, and findOnPath historically accepted those. Only
  // the appended standard directories go through absolute-path normalization.
  const entries = splitPathValue(envPath)
  if (platform !== "linux") return entries.join(path.delimiter)
  const seen = new Set(entries)
  for (const value of uniqueDiscoveryPaths([
    homeDir ? path.join(homeDir, ".local", "bin") : "",
    ...(includeStandardPaths ? LINUX_STANDARD_PATHS : []),
  ])) {
    if (seen.has(value)) continue
    seen.add(value)
    entries.push(value)
  }
  return entries.join(path.delimiter)
}

function readDirectoryNames(directory, fsImpl) {
  if (!isDirectory(directory, fsImpl)) return []
  let entries
  try {
    entries = fsImpl.readdirSync(directory, { withFileTypes: true })
  } catch {
    try {
      entries = fsImpl.readdirSync(directory)
    } catch {
      return []
    }
  }
  return (entries || [])
    .map((entry) => (typeof entry === "string" ? entry : entry?.name))
    .filter((name) => typeof name === "string" && name && !name.includes("\u0000"))
    .sort((left, right) => left.localeCompare(right))
    .slice(0, MAX_DISCOVERY_ENTRIES)
}

function compactDiscoveryName(value) {
  return String(value || "").toLowerCase().replace(/[^a-z0-9]/g, "")
}

function isAppImageForDefinition(name, definition) {
  const lower = String(name || "").toLowerCase()
  if (!lower.endsWith(".appimage")) return false
  const aliases = [definition.id, ...(definition.candidates || [])]
    .map((candidate) => compactDiscoveryName(path.basename(candidate)))
    .filter(Boolean)
  const compact = compactDiscoveryName(lower.slice(0, -".appimage".length))
  return aliases.some((alias) => compact.includes(alias))
}

function buildAppImageDirectories({ platform, homeDir, envPath, appImageDirs = [] } = {}) {
  if (platform !== "linux") return []
  return uniqueDiscoveryPaths([
    ...(Array.isArray(appImageDirs) ? appImageDirs : []),
    homeDir ? path.join(homeDir, "Applications") : "",
    homeDir ? path.join(homeDir, "AppImages") : "",
    homeDir ? path.join(homeDir, "appimages") : "",
    homeDir ? path.join(homeDir, "Downloads") : "",
    homeDir ? path.join(homeDir, ".local", "bin") : "",
    "/opt",
    "/opt/Applications",
    "/opt/AppImages",
    "/opt/appimages",
    "/usr/local/bin",
    "/usr/bin",
  ])
}

function findAppImage(definition, fsImpl, directories) {
  let count = 0
  for (const directory of directories || []) {
    for (const name of readDirectoryNames(directory, fsImpl)) {
      if (++count > MAX_APPIMAGE_RESULTS * 8) return ""
      if (!isAppImageForDefinition(name, definition)) continue
      const candidate = path.join(directory, name)
      if (isExecutable(candidate, fsImpl)) return candidate
    }
  }
  return ""
}

function buildFlatpakRoots({ platform, homeDir, env = process.env, flatpakRoots = [] } = {}) {
  if (platform !== "linux") return []
  const xdgDataHome = typeof env?.XDG_DATA_HOME === "string" ? env.XDG_DATA_HOME : ""
  return uniqueDiscoveryPaths([
    ...(Array.isArray(flatpakRoots) ? flatpakRoots : []),
    xdgDataHome ? path.join(xdgDataHome, "flatpak") : "",
    homeDir ? path.join(homeDir, ".local", "share", "flatpak") : "",
    "/var/lib/flatpak",
    "/usr/local/share/flatpak",
  ])
}

// Flatpak's exported launcher is often a symlink. We inspect it only as an
// installation marker and launch the validated `flatpak` binary directly;
// never execute a wrapper or parse its shell script.
function isFlatpakExport(file, fsImpl) {
  const parent = path.dirname(file)
  if (hasSymlinkComponent(parent, fsImpl)) return false
  try {
    const stat = fsImpl.lstatSync(file)
    return stat.isFile() || stat.isSymbolicLink()
  } catch {
    return false
  }
}

function flatpakAppPresent(appId, fsImpl, roots) {
  for (const root of roots || []) {
    const appPaths = [
      path.join(root, "app", appId),
      path.join(root, appId),
    ]
    if (appPaths.some((candidate) => isDirectory(candidate, fsImpl))) return true
    const desktop = path.join(root, "exports", "share", "applications", `${appId}.desktop`)
    if (isFlatpakExport(desktop, fsImpl)) return true
    const wrapper = path.join(root, "exports", "bin", appId)
    if (isFlatpakExport(wrapper, fsImpl)) return true
  }
  return false
}

function findFlatpak(definition, fsImpl, envPath, roots) {
  const ids = normalizeFlatpakIds(definition.flatpakIds)
  if (!ids.length) return null
  const executable = findOnPath("flatpak", fsImpl, envPath)
  if (!executable) return null
  for (const appId of ids) {
    if (flatpakAppPresent(appId, fsImpl, roots)) {
      // The app id is a fixed, validated argument. Keeping it as a separate
      // argv item prevents shell metacharacters in paths or metadata from
      // becoming executable code.
      return { executable, args: ["run", appId] }
    }
  }
  return null
}

function detectDefinition(definition, { fsImpl, envPath, appImageDirs, flatpakRoots } = {}) {
  for (const candidate of definition.candidates || []) {
    const executable = findOnPath(candidate, fsImpl, envPath)
    if (executable) return { executable, args: [], source: "detected" }
  }
  const appImage = findAppImage(definition, fsImpl, appImageDirs)
  if (appImage) return { executable: appImage, args: [], source: "detected" }
  const flatpak = findFlatpak(definition, fsImpl, envPath, flatpakRoots)
  if (flatpak) return { ...flatpak, source: "detected" }
  return null
}

function findOnPath(command, fsImpl, envPath) {
  const normalized = normalizeCommand(command)
  if (!normalized) return ""
  if (path.isAbsolute(normalized)) return isExecutable(normalized, fsImpl) ? normalized : ""
  // Keep the historical empty-value fallback for callers using findOnPath
  // directly; the registry itself always supplies a concrete search path.
  const pathValue = envPath || process.env.PATH || ""
  for (const directory of String(pathValue).split(path.delimiter)) {
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
  let biosPath = ""
  if (input.biosPath !== undefined && input.biosPath !== "") {
    const normalizedBios = normalizeAbsoluteFile(input.biosPath, "biosPath")
    if (!normalizedBios.ok) return normalizedBios
    biosPath = normalizedBios.value
  }
  const romFolders = normalizeRomFolders(input.romFolders)
  if (!Array.isArray(romFolders)) return romFolders
  return {
    ok: true,
    profile: {
      id,
      executable,
      corePath,
      biosPath,
      romFolders,
      args: args.value,
      updatedAt: Number.isFinite(Number(input.updatedAt))
        ? Math.trunc(Number(input.updatedAt))
        : Date.now(),
    },
  }
}

function readState(file, fsImpl) {
  if (hasSymlinkComponent(file, fsImpl)) return { profiles: {} }
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

function readRomIndex(file, fsImpl) {
  if (hasSymlinkComponent(file, fsImpl)) return { version: ROMS_VERSION, emulators: {} }
  try {
    const stat = fsImpl.lstatSync(file)
    if (!stat.isFile() || stat.isSymbolicLink()) return { version: ROMS_VERSION, emulators: {} }
    const parsed = JSON.parse(fsImpl.readFileSync(file, "utf8"))
    if (!parsed || parsed.version !== ROMS_VERSION || !parsed.emulators || typeof parsed.emulators !== "object") {
      return { version: ROMS_VERSION, emulators: {} }
    }
    return { version: ROMS_VERSION, emulators: parsed.emulators }
  } catch {
    return { version: ROMS_VERSION, emulators: {} }
  }
}

function normalizeStoredRom(item) {
  if (!item || typeof item !== "object") return null
  const normalized = normalizeAbsoluteFile(item.path, "romPath")
  if (!normalized.ok) return null
  return {
    path: normalized.value,
    name: typeof item.name === "string" ? item.name.slice(0, 512) : path.basename(normalized.value),
    relativePath: typeof item.relativePath === "string" ? item.relativePath.slice(0, 2048) : path.basename(normalized.value),
    extension: typeof item.extension === "string" ? item.extension.slice(0, 32) : "",
    kind: item.kind === "directory" ? "directory" : "file",
    sizeBytes: Number.isFinite(Number(item.sizeBytes)) ? Math.max(0, Number(item.sizeBytes)) : 0,
    mtimeMs: Number.isFinite(Number(item.mtimeMs)) ? Math.max(0, Number(item.mtimeMs)) : 0,
    sidecars: Array.isArray(item.sidecars)
      ? item.sidecars.map((sidecar) => normalizeAbsoluteFile(sidecar, "sidecar")).filter((value) => value.ok).map((value) => value.value).slice(0, 16)
      : [],
  }
}

function atomicWrite(file, value, fsImpl = fsDefault) {
  const directory = path.dirname(file)
  if (hasSymlinkComponent(directory, fsImpl)) throw new Error("diretorio_emuladores_symlink")
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
  envPath,
  homeDir = os.homedir(),
  platform = process.platform,
  env = process.env,
  appImageDirs = [],
  flatpakRoots = [],
  extraDefinitions = [],
  definitions: definitionsOption,
} = {}) {
  const effectiveEnvPath = envPath === undefined ? env?.PATH : envPath
  const searchPath = buildLinuxSearchPath(
    effectiveEnvPath,
    platform,
    homeDir,
    envPath === undefined,
  )
  const appImageDirectories = buildAppImageDirectories({
    platform,
    homeDir,
    envPath: effectiveEnvPath,
    appImageDirs,
  })
  const flatpakDirectories = buildFlatpakRoots({
    platform,
    homeDir,
    env,
    flatpakRoots,
  })
  const root = path.resolve(String(dataDir))
  const statePath = profilesPath ? path.resolve(profilesPath) : path.join(root, REGISTRY_FILENAME)
  const romsPath = path.join(root, ROMS_FILENAME)
  const catalog = Object.assign(Object.create(null), DEFINITIONS)
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
      ? findOnPath(configured.executable, fsImpl, searchPath)
      : ""
    const detected = configured
      ? configuredExecutable
        ? { executable: configuredExecutable, args: configured.args || [], source: "configured" }
        : null
      : detectDefinition(definition, {
          fsImpl,
          envPath: searchPath,
          appImageDirs: appImageDirectories,
          flatpakRoots: flatpakDirectories,
        })
    return {
      ...clone(definition),
      // Keep the configured value visible for editing, but `available` only
      // becomes true after regular-file/executable validation.
      executable: detected?.executable || configured?.executable || "",
      available: Boolean(detected),
      source: configured ? "configured" : detected ? "detected" : "builtin",
      ...(!configured && detected?.args?.length ? { detectedArgs: [...detected.args] } : {}),
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

  function saveRomScan(emulatorId, result) {
    const state = readRomIndex(romsPath, fsImpl)
    const roms = (result.roms || []).map(normalizeStoredRom).filter(Boolean).slice(0, MAX_SCAN_RESULTS)
    state.emulators[emulatorId] = {
      updatedAt: Date.now(),
      directory: result.directory || "",
      folders: Array.isArray(result.folders) && result.folders.length
        ? result.folders
        : profile(emulatorId)?.romFolders || [],
      truncated: Boolean(result.truncated),
      roms,
    }
    try {
      atomicWrite(romsPath, state, fsImpl)
      return true
    } catch {
      return false
    }
  }

  function romIndex() {
    const state = readRomIndex(romsPath, fsImpl)
    const result = {}
    for (const [id, entry] of Object.entries(state.emulators || {})) {
      if (!catalog[id] || !entry || typeof entry !== "object") continue
      const folders = normalizeRomFolders(entry.folders)
      result[id] = {
        updatedAt: Number.isFinite(Number(entry.updatedAt)) ? Number(entry.updatedAt) : 0,
        directory: normalizeAbsoluteFile(entry.directory, "directory").value || "",
        folders: Array.isArray(folders) ? folders : [],
        truncated: Boolean(entry.truncated),
        roms: Array.isArray(entry.roms)
          ? entry.roms.map(normalizeStoredRom).filter(Boolean).slice(0, MAX_SCAN_RESULTS)
          : [],
      }
    }
    return result
  }

  function scanRoms({
    emulatorId,
    directory,
    rootPath,
    folderPath,
    recursive = true,
    maxDepth = MAX_SCAN_DEPTH,
    maxResults = MAX_SCAN_RESULTS,
  } = {}) {
    const id = normalizeId(emulatorId)
    const definition = catalog[id]
    if (!definition) return { ok: false, error: "emulador_desconhecido" }
    if (typeof recursive !== "boolean") return { ok: false, error: "opcoes_scan_invalidas" }
    if (!Number.isInteger(maxDepth) || maxDepth < 0 || maxDepth > MAX_SCAN_DEPTH) {
      return { ok: false, error: "opcoes_scan_invalidas" }
    }
    if (!Number.isInteger(maxResults) || maxResults < 1 || maxResults > MAX_SCAN_RESULTS) {
      return { ok: false, error: "opcoes_scan_invalidas" }
    }
    if (!directory && !rootPath && !folderPath) {
      return scanConfiguredRoms({ emulatorId: id, recursive, maxDepth, maxResults })
    }
    const normalizedDirectory = normalizeAbsoluteFile(directory || rootPath || folderPath, "directory")
    if (!normalizedDirectory.ok || !isDirectory(normalizedDirectory.value, fsImpl)) {
      return { ok: false, error: "diretorio_rom_invalido" }
    }
    const allowed = normalizeRomExtensions(definition.romExtensions)
    const candidates = []
    const queue = [{ directory: normalizedDirectory.value, depth: 0 }]
    const seenDirectories = new Set()
    const candidateLimit = Math.min(
      MAX_SCAN_RESULTS,
      Math.max(maxResults + 32, maxResults * 4),
    )
    let capped = false

    while (queue.length && candidates.length < candidateLimit) {
      const current = queue.shift()
      if (!current || seenDirectories.has(current.directory)) continue
      seenDirectories.add(current.directory)
      let entries
      try {
        entries = fsImpl.readdirSync(current.directory, { withFileTypes: true })
      } catch {
        continue
      }
      entries = Array.from(entries || []).sort((a, b) => {
        const an = typeof a === "string" ? a : a.name
        const bn = typeof b === "string" ? b : b.name
        return String(an).localeCompare(String(bn))
      })
      for (const entry of entries) {
        if (candidates.length >= candidateLimit) {
          capped = true
          break
        }
        const name = typeof entry === "string" ? entry : entry?.name
        if (typeof name !== "string" || !name || name === "." || name === "..") continue
        const file = path.join(current.directory, name)
        let link = false
        try {
          link = typeof entry !== "string" && entry.isSymbolicLink?.() === true
        } catch {}
        if (link || hasSymlinkComponent(file, fsImpl)) continue
        let stat
        try {
          stat = fsImpl.lstatSync(file)
        } catch {
          continue
        }
        if (stat.isDirectory()) {
          const markers = normalizeRomDirectoryMarkers(definition.romDirectoryMarkers)
          if (markers.includes(name)) {
            const sizeBytes = (() => {
              let total = 0
              const pending = [file]
              const visited = new Set()
              let seenEntries = 0
              while (pending.length && seenEntries < 100000) {
                const dir = pending.shift()
                if (!dir || visited.has(dir) || hasSymlinkComponent(dir, fsImpl)) continue
                visited.add(dir)
                let children
                try {
                  children = fsImpl.readdirSync(dir, { withFileTypes: true })
                } catch {
                  continue
                }
                for (const child of children || []) {
                  if (++seenEntries > 100000) break
                  if (!child?.name || child.isSymbolicLink?.()) continue
                  const nested = path.join(dir, child.name)
                  if (child.isDirectory?.()) pending.push(nested)
                  else if (child.isFile?.() && !hasSymlinkComponent(nested, fsImpl)) {
                    try {
                      total += Math.max(0, Number(fsImpl.lstatSync(nested).size) || 0)
                    } catch {}
                  }
                }
              }
              return total
            })()
            candidates.push({
              path: path.normalize(file),
              name,
              relativePath: path.relative(normalizedDirectory.value, file),
              extension: "",
              sizeBytes,
              mtimeMs: Number(stat.mtimeMs) || 0,
              directory: current.directory,
              isDirectoryMarker: true,
            })
            continue
          }
          if (recursive && current.depth < maxDepth) {
            queue.push({ directory: file, depth: current.depth + 1 })
          }
          continue
        }
        if (!stat.isFile() || stat.isSymbolicLink()) continue
        const lowerName = name.toLowerCase()
        const extension = allowed
          .slice()
          .sort((a, b) => b.length - a.length)
          .find((item) => lowerName.endsWith(item))
        if (!extension) continue
        const sizeBytes = Number.isFinite(Number(stat.size)) ? Math.max(0, Number(stat.size)) : 0
        const mtimeMs = Number.isFinite(Number(stat.mtimeMs)) ? Number(stat.mtimeMs) : 0
        candidates.push({
          path: path.normalize(file),
          name,
          relativePath: path.relative(normalizedDirectory.value, file),
          extension,
          sizeBytes,
          mtimeMs,
          directory: current.directory,
        })
      }
    }
    if (candidates.length >= candidateLimit) capped = true

    const rules = ROM_SIDECAR_RULES[id] || {}
    const suppressed = new Set()
    const sidecars = new Map()
    for (const primary of candidates) {
      const primaryExtension = path.extname(primary.name).toLowerCase()
      const sidecarExtensions = rules[primaryExtension]
      if (!sidecarExtensions) continue
      const primaryStem = path.basename(primary.name, path.extname(primary.name)).toLowerCase()
      for (const sidecar of candidates) {
        if (sidecar === primary || sidecar.directory !== primary.directory) continue
        if (path.basename(sidecar.name, path.extname(sidecar.name)).toLowerCase() !== primaryStem) continue
        if (!sidecarExtensions.includes(path.extname(sidecar.name).toLowerCase())) continue
        suppressed.add(sidecar.path)
        const attached = sidecars.get(primary.path) || []
        attached.push(sidecar)
        sidecars.set(primary.path, attached)
      }
    }

    // Hydra treats a playlist as the canonical entry for a directory: the
    // individual discs remain launchable through the playlist and should not
    // appear as duplicate games in the scan result.
    const playlistDirectories = new Set(
      candidates
        .filter((candidate) => candidate.extension === ".m3u")
        .map((candidate) => candidate.directory),
    )
    const roms = candidates
      .filter((candidate) => {
        if (suppressed.has(candidate.path)) return false
        return !(
          playlistDirectories.has(candidate.directory) &&
          candidate.extension !== ".m3u" &&
          !candidate.isDirectoryMarker
        )
      })
      .sort((a, b) => a.path.localeCompare(b.path))
      .map((candidate) => {
        const attached = sidecars.get(candidate.path) || []
        return {
          path: candidate.path,
          name: candidate.name,
          relativePath: candidate.relativePath,
          extension: candidate.extension,
          kind: candidate.isDirectoryMarker ? "directory" : "file",
          sizeBytes: candidate.sizeBytes + attached.reduce((total, item) => total + item.sizeBytes, 0),
          mtimeMs: candidate.mtimeMs,
          sidecars: attached.map((item) => item.path).sort(),
        }
      })
    const truncated = capped || roms.length > maxResults
    const result = {
      ok: true,
      emulatorId: id,
      directory: normalizedDirectory.value,
      recursive,
      maxDepth,
      romExtensions: allowed,
      romDirectoryMarkers: normalizeRomDirectoryMarkers(definition.romDirectoryMarkers),
      truncated,
      roms: roms.slice(0, maxResults),
    }
    result.persisted = saveRomScan(id, result)
    return result
  }

  function scanConfiguredRoms({ emulatorId, recursive = true, maxDepth = MAX_SCAN_DEPTH, maxResults = MAX_SCAN_RESULTS } = {}) {
    const id = normalizeId(emulatorId)
    const configured = profile(id)?.romFolders || []
    const merged = new Map()
    let truncated = false
    let romExtensions = []
    for (const folder of configured) {
      const result = scanRoms({
        emulatorId: id,
        directory: folder.path,
        recursive: folder.recursive && recursive,
        maxDepth,
        maxResults,
      })
      if (!result.ok) continue
      romExtensions = result.romExtensions || romExtensions
      truncated = truncated || Boolean(result.truncated)
      for (const rom of result.roms || []) {
        if (!merged.has(rom.path)) merged.set(rom.path, rom)
      }
      if (merged.size >= maxResults) {
        truncated = true
        break
      }
    }
    const result = {
      ok: true,
      emulatorId: id,
      recursive,
      maxDepth,
      romExtensions,
      romDirectoryMarkers: normalizeRomDirectoryMarkers(catalog[id]?.romDirectoryMarkers),
      truncated,
      folders: configured.map((folder) => ({ ...folder })),
      roms: [...merged.values()].sort((a, b) => a.path.localeCompare(b.path)).slice(0, maxResults),
    }
    result.persisted = saveRomScan(id, result)
    return result
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

  function resolveLaunch({ emulatorId, romPath, extraArgs = [], corePath, launchMode } = {}) {
    const id = normalizeId(emulatorId)
    if (launchMode !== undefined && launchMode !== "default" && launchMode !== "hydra") {
      return { ok: false, error: "launch_mode_invalido" }
    }
    const definition = catalog[id]
    if (!definition) return { ok: false, error: "emulador_desconhecido" }
    const rom = normalizeAbsoluteFile(romPath, "romPath")
    if (!rom.ok || !isRomPathValid(rom.value, definition, fsImpl)) return { ok: false, error: "rom_invalida" }
    const profiles = readProfiles()
    const configured = profiles[id]
    const configuredExecutable = configured
      ? findOnPath(configured.executable, fsImpl, searchPath)
      : ""
    if (configured && !configuredExecutable)
      return { ok: false, error: "executavel_configurado_invalido" }
    const detected = configured
      ? { executable: configuredExecutable, args: configured.args || [] }
      : detectDefinition(definition, {
          fsImpl,
          envPath: searchPath,
          appImageDirs: appImageDirectories,
          flatpakRoots: flatpakDirectories,
        })
    const executable = detected?.executable || ""
    if (!executable) return { ok: false, error: "emulador_nao_encontrado" }
    const args = normalizeArgs(extraArgs)
    if (!args.ok) return args
    const baseArgs = configured?.args || detected?.args || []
    const selectedCore = corePath || configured?.corePath || ""
    const hydraMode = launchMode === "hydra"
    const command = [executable, ...baseArgs]
    if (hydraMode) command.push(...(HYDRA_LAUNCH_PREFIX[id] || []))
    if (definition.requiresCore) {
      const core = normalizeAbsoluteFile(selectedCore, "corePath")
      if (!core.ok || !isCoreFile(core.value, fsImpl))
        return { ok: false, error: "retroarch_core_invalido" }
      command.push("-L", core.value)
    }
    const boot = hydraMode
      ? resolveHydraBootPath(id, rom.value, fsImpl, executable, homeDir)
      : { ok: true, path: rom.value }
    if (!boot.ok) return boot
    command.push(boot.path, ...args.value)
    if (hydraMode) command.push(...(HYDRA_LAUNCH_SUFFIX[id] || []))
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
    scanRoms,
    scanConfiguredRoms,
    roms: romIndex,
    resolveLaunch,
    paths: () => ({ statePath }),
    romPaths: () => ({ romsPath }),
  })
}

module.exports = {
  REGISTRY_VERSION,
  REGISTRY_FILENAME,
  ROMS_FILENAME,
  ROMS_VERSION,
  MAX_SCAN_DEPTH,
  MAX_SCAN_RESULTS,
  LAUNCH_CODES,
  DEFINITIONS,
  normalizeId,
  normalizeCommand,
  normalizeFlatpakId,
  normalizeFlatpakIds,
  normalizeRomExtension,
  normalizeRomExtensions,
  normalizeRomDirectoryMarkers,
  normalizeRomFolders,
  normalizeDefinition,
  normalizeArgs,
  normalizeProfile,
  findOnPath,
  atomicWrite,
  createEmulatorRegistry,
}
