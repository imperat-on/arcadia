"use strict"

// Sistema de registro canônico para todas as plataformas retro suportadas.
// Este módulo define a ponte entre os aliases que aparecem nas sources Hydra,
// os nomes das databases/thumbnails do Libretro, e os emuladores locais.

const SYSTEMS = Object.freeze({
  "sony-playstation": {
    id: "sony-playstation",
    displayName: "PlayStation",
    libretroDatabase: "Sony - PlayStation",
    thumbnailCollection: "Sony - PlayStation",
    emulatorIds: ["duckstation"],
    aliases: ["ps1", "psx", "playstation", "playstation 1", "sony playstation"],
    mediaType: "disc",
    identityStrategy: ["serial", "sha1", "title"],
    serialPattern: /\b([A-Z]{4}[-_]?\d{5})\b/i,
  },
  "sony-playstation-2": {
    id: "sony-playstation-2",
    displayName: "PlayStation 2",
    libretroDatabase: "Sony - PlayStation 2",
    thumbnailCollection: "Sony - PlayStation 2",
    emulatorIds: ["pcsx2"],
    aliases: ["ps2", "playstation 2", "sony ps2"],
    mediaType: "disc",
    identityStrategy: ["serial", "sha1", "title"],
    serialPattern: /\b([A-Z]{4}[-_]?\d{5})\b/i,
  },
  "sony-playstation-3": {
    id: "sony-playstation-3",
    displayName: "PlayStation 3",
    libretroDatabase: "Sony - PlayStation 3",
    thumbnailCollection: "Sony - PlayStation 3",
    emulatorIds: ["rpcs3"],
    aliases: ["ps3", "playstation 3", "sony ps3"],
    mediaType: "mixed",
    identityStrategy: ["serial", "title"],
    serialPattern: /\b([A-Z]{4}\d{5})\b/i,
  },
  "sony-psp": {
    id: "sony-psp",
    displayName: "PlayStation Portable",
    libretroDatabase: "Sony - PlayStation Portable",
    thumbnailCollection: "Sony - PlayStation Portable",
    emulatorIds: ["ppsspp"],
    aliases: ["psp", "playstation portable", "sony psp"],
    mediaType: "mixed",
    identityStrategy: ["serial", "sha1", "title"],
    serialPattern: /\b([A-Z]{4}[-_]?\d{5})\b/i,
  },
  "nintendo-gamecube": {
    id: "nintendo-gamecube",
    displayName: "GameCube",
    libretroDatabase: "Nintendo - GameCube",
    thumbnailCollection: "Nintendo - GameCube",
    emulatorIds: ["dolphin"],
    aliases: ["gc", "gcn", "gamecube", "nintendo gamecube"],
    mediaType: "disc",
    identityStrategy: ["serial", "sha1", "title"],
    serialPattern: /\b(G[A-Z0-9]{3}[0-9]{2})\b/i,
  },
  "nintendo-wii": {
    id: "nintendo-wii",
    displayName: "Wii",
    libretroDatabase: "Nintendo - Wii",
    thumbnailCollection: "Nintendo - Wii",
    emulatorIds: ["dolphin"],
    aliases: ["wii", "nintendo wii"],
    mediaType: "disc",
    identityStrategy: ["serial", "sha1", "title"],
    serialPattern: /\b(R[A-Z0-9]{3}[0-9]{2})\b/i,
  },
  "nintendo-ds": {
    id: "nintendo-ds",
    displayName: "Nintendo DS",
    libretroDatabase: "Nintendo - Nintendo DS",
    thumbnailCollection: "Nintendo - Nintendo DS",
    emulatorIds: ["melonds", "desmume"],
    aliases: ["nds", "ds", "nintendo ds"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-dsi": {
    id: "nintendo-dsi",
    displayName: "Nintendo DSi",
    libretroDatabase: "Nintendo - Nintendo DSi",
    thumbnailCollection: "Nintendo - Nintendo DSi",
    emulatorIds: ["melonds", "desmume"],
    aliases: ["dsi", "nintendo dsi"],
    mediaType: "digital",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-nes": {
    id: "nintendo-nes",
    displayName: "NES",
    libretroDatabase: "Nintendo - Nintendo Entertainment System",
    thumbnailCollection: "Nintendo - Nintendo Entertainment System",
    emulatorIds: ["retroarch"],
    aliases: ["nes", "famicom", "nintendo entertainment system"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-snes": {
    id: "nintendo-snes",
    displayName: "SNES",
    libretroDatabase: "Nintendo - Super Nintendo Entertainment System",
    thumbnailCollection: "Nintendo - Super Nintendo Entertainment System",
    emulatorIds: ["retroarch"],
    aliases: ["snes", "sfc", "super famicom", "super nintendo"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-game-boy": {
    id: "nintendo-game-boy",
    displayName: "Game Boy",
    libretroDatabase: "Nintendo - Game Boy",
    thumbnailCollection: "Nintendo - Game Boy",
    emulatorIds: ["retroarch"],
    aliases: ["gb", "game boy", "gameboy"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-game-boy-color": {
    id: "nintendo-game-boy-color",
    displayName: "Game Boy Color",
    libretroDatabase: "Nintendo - Game Boy Color",
    thumbnailCollection: "Nintendo - Game Boy Color",
    emulatorIds: ["retroarch"],
    aliases: ["gbc", "game boy color", "gameboy color"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-game-boy-advance": {
    id: "nintendo-game-boy-advance",
    displayName: "Game Boy Advance",
    libretroDatabase: "Nintendo - Game Boy Advance",
    thumbnailCollection: "Nintendo - Game Boy Advance",
    emulatorIds: ["retroarch"],
    aliases: ["gba", "game boy advance", "gameboy advance"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
  "nintendo-64": {
    id: "nintendo-64",
    displayName: "Nintendo 64",
    libretroDatabase: "Nintendo - Nintendo 64",
    thumbnailCollection: "Nintendo - Nintendo 64",
    emulatorIds: ["retroarch"],
    aliases: ["n64", "nintendo 64"],
    mediaType: "cartridge",
    identityStrategy: ["crc32", "sha1", "title"],
  },
})

// Mapa invertido para resolução rápida de alias → systemId
const ALIAS_MAP = (() => {
  const map = new Map()
  for (const [id, system] of Object.entries(SYSTEMS)) {
    for (const alias of system.aliases) {
      const normalized = alias.toLowerCase().trim()
      if (!map.has(normalized)) {
        map.set(normalized, id)
      }
    }
  }
  return map
})()

/**
 * Resolve um alias de plataforma para o systemId canônico.
 * @param {string} alias - Alias da source (ex: "ps1", "snes")
 * @returns {string|null} - systemId canônico ou null se não reconhecido
 */
function resolveSystem(alias) {
  if (!alias || typeof alias !== "string") return null
  const normalized = alias.toLowerCase().trim()
  return ALIAS_MAP.get(normalized) || null
}

/**
 * Retorna a definição completa de um sistema.
 * @param {string} systemId - ID canônico do sistema
 * @returns {object|null} - Definição do sistema ou null
 */
function getSystem(systemId) {
  return SYSTEMS[systemId] || null
}

/**
 * Lista todos os sistemas registrados.
 * @returns {object[]} - Array de definições de sistemas
 */
function listSystems() {
  return Object.values(SYSTEMS).map(system => ({ ...system }))
}

/**
 * Extrai serial de um título ou descrição usando o pattern do sistema.
 * @param {string} text - Texto para buscar serial
 * @param {string} systemId - ID do sistema
 * @returns {string|null} - Serial encontrado ou null
 */
function extractSerial(text, systemId) {
  const system = SYSTEMS[systemId]
  if (!system || !system.serialPattern) return null
  const match = text.match(system.serialPattern)
  return match ? match[1].replace(/[-_]/g, "").toUpperCase() : null
}

/**
 * Normaliza um serial para comparação (remove traços, underscores, espaços).
 * @param {string} serial - Serial original
 * @returns {string} - Serial normalizado
 */
function normalizeSerial(serial) {
  return String(serial || "").replace(/[-_ ]/g, "").toUpperCase()
}

/**
 * Valida se um ID de sistema é built-in (não pode ser sobrescrito por plugins).
 * @param {string} systemId - ID para validar
 * @returns {boolean} - true se é built-in
 */
function isBuiltinSystem(systemId) {
  return Object.prototype.hasOwnProperty.call(SYSTEMS, systemId)
}

module.exports = {
  SYSTEMS,
  resolveSystem,
  getSystem,
  listSystems,
  extractSerial,
  normalizeSerial,
  isBuiltinSystem,
}
