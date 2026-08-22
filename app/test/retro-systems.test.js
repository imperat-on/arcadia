"use strict"

const test = require("node:test")
const assert = require("node:assert")
const {
  resolveSystem,
  getSystem,
  listSystems,
  extractSerial,
  normalizeSerial,
  isBuiltinSystem,
} = require("../electron/retro-systems.js")

test("resolveSystem recognizes all PlayStation aliases", () => {
  assert.equal(resolveSystem("ps1"), "sony-playstation")
  assert.equal(resolveSystem("psx"), "sony-playstation")
  assert.equal(resolveSystem("playstation"), "sony-playstation")
  assert.equal(resolveSystem("PS1"), "sony-playstation")
  assert.equal(resolveSystem("  psx  "), "sony-playstation")

  assert.equal(resolveSystem("ps2"), "sony-playstation-2")
  assert.equal(resolveSystem("playstation 2"), "sony-playstation-2")

  assert.equal(resolveSystem("ps3"), "sony-playstation-3")
  assert.equal(resolveSystem("playstation 3"), "sony-playstation-3")

  assert.equal(resolveSystem("psp"), "sony-psp")
  assert.equal(resolveSystem("playstation portable"), "sony-psp")
})

test("resolveSystem recognizes Nintendo aliases", () => {
  assert.equal(resolveSystem("nes"), "nintendo-nes")
  assert.equal(resolveSystem("famicom"), "nintendo-nes")

  assert.equal(resolveSystem("snes"), "nintendo-snes")
  assert.equal(resolveSystem("sfc"), "nintendo-snes")
  assert.equal(resolveSystem("super famicom"), "nintendo-snes")

  assert.equal(resolveSystem("n64"), "nintendo-64")
  assert.equal(resolveSystem("nintendo 64"), "nintendo-64")

  assert.equal(resolveSystem("gb"), "nintendo-game-boy")
  assert.equal(resolveSystem("gbc"), "nintendo-game-boy-color")
  assert.equal(resolveSystem("gba"), "nintendo-game-boy-advance")

  assert.equal(resolveSystem("gc"), "nintendo-gamecube")
  assert.equal(resolveSystem("gcn"), "nintendo-gamecube")
  assert.equal(resolveSystem("gamecube"), "nintendo-gamecube")

  assert.equal(resolveSystem("wii"), "nintendo-wii")

  assert.equal(resolveSystem("nds"), "nintendo-ds")
  assert.equal(resolveSystem("ds"), "nintendo-ds")
  assert.equal(resolveSystem("dsi"), "nintendo-dsi")
})

test("resolveSystem returns null for unknown aliases", () => {
  assert.equal(resolveSystem("unknown"), null)
  assert.equal(resolveSystem("xbox"), null)
  assert.equal(resolveSystem(""), null)
  assert.equal(resolveSystem(null), null)
  assert.equal(resolveSystem(undefined), null)
  assert.equal(resolveSystem(123), null)
})

test("getSystem returns complete system definition", () => {
  const ps1 = getSystem("sony-playstation")
  assert.ok(ps1)
  assert.equal(ps1.id, "sony-playstation")
  assert.equal(ps1.displayName, "PlayStation")
  assert.equal(ps1.libretroDatabase, "Sony - PlayStation")
  assert.equal(ps1.thumbnailCollection, "Sony - PlayStation")
  assert.deepEqual(ps1.emulatorIds, ["duckstation"])
  assert.ok(ps1.aliases.includes("ps1"))
  assert.equal(ps1.mediaType, "disc")
  assert.deepEqual(ps1.identityStrategy, ["serial", "sha1", "title"])
  assert.ok(ps1.serialPattern instanceof RegExp)
})

test("getSystem returns null for unknown system", () => {
  assert.equal(getSystem("unknown-system"), null)
  assert.equal(getSystem(""), null)
  assert.equal(getSystem(null), null)
})

test("listSystems returns all systems", () => {
  const systems = listSystems()
  assert.ok(Array.isArray(systems))
  assert.ok(systems.length >= 14) // At least PS1-3, PSP, GC, Wii, DS, DSi, NES, SNES, GB, GBC, GBA, N64

  const ids = systems.map(s => s.id)
  assert.ok(ids.includes("sony-playstation"))
  assert.ok(ids.includes("nintendo-64"))
  assert.ok(ids.includes("nintendo-gamecube"))
})

test("extractSerial finds PlayStation serials", () => {
  assert.equal(extractSerial("SLUS-20312", "sony-playstation"), "SLUS20312")
  assert.equal(extractSerial("SCUS_94900", "sony-playstation"), "SCUS94900")
  assert.equal(extractSerial("Game Title [SLUS-20312] [USA]", "sony-playstation"), "SLUS20312")
  assert.equal(extractSerial("SLES-12345 Region", "sony-playstation"), "SLES12345")
})

test("extractSerial finds PS2 serials", () => {
  assert.equal(extractSerial("SLUS-20312", "sony-playstation-2"), "SLUS20312")
  assert.equal(extractSerial("SCUS_94900", "sony-playstation-2"), "SCUS94900")
})

test("extractSerial finds PS3 serials", () => {
  assert.equal(extractSerial("BLUS30100", "sony-playstation-3"), "BLUS30100")
  assert.equal(extractSerial("NPUB30001", "sony-playstation-3"), "NPUB30001")
  assert.equal(extractSerial("Game [BLUS30100] Region", "sony-playstation-3"), "BLUS30100")
})

test("extractSerial finds GameCube serials", () => {
  assert.equal(extractSerial("GALE01", "nintendo-gamecube"), "GALE01")
  assert.equal(extractSerial("Game (GALE01)", "nintendo-gamecube"), "GALE01")
})

test("extractSerial finds Wii serials", () => {
  assert.equal(extractSerial("RMCE01", "nintendo-wii"), "RMCE01")
  assert.equal(extractSerial("Game [RMCE01]", "nintendo-wii"), "RMCE01")
})

test("extractSerial returns null when no serial found", () => {
  assert.equal(extractSerial("No Serial Here", "sony-playstation"), null)
  assert.equal(extractSerial("", "sony-playstation"), null)
  assert.equal(extractSerial("Game Title", "nintendo-nes"), null)
})

test("extractSerial returns null for systems without serial patterns", () => {
  assert.equal(extractSerial("Some Text", "nintendo-nes"), null)
  assert.equal(extractSerial("SLUS-20312", "nintendo-64"), null)
})

test("normalizeSerial removes separators and uppercases", () => {
  assert.equal(normalizeSerial("SLUS-20312"), "SLUS20312")
  assert.equal(normalizeSerial("SCUS_94900"), "SCUS94900")
  assert.equal(normalizeSerial("slus 20312"), "SLUS20312")
  assert.equal(normalizeSerial("slus-20312"), "SLUS20312")
  assert.equal(normalizeSerial(""), "")
  assert.equal(normalizeSerial(null), "")
})

test("isBuiltinSystem identifies built-in systems", () => {
  assert.equal(isBuiltinSystem("sony-playstation"), true)
  assert.equal(isBuiltinSystem("nintendo-64"), true)
  assert.equal(isBuiltinSystem("nintendo-gamecube"), true)
  assert.equal(isBuiltinSystem("unknown-system"), false)
  assert.equal(isBuiltinSystem("custom-plugin-system"), false)
  assert.equal(isBuiltinSystem(""), false)
})

test("case insensitivity in alias resolution", () => {
  assert.equal(resolveSystem("PS1"), "sony-playstation")
  assert.equal(resolveSystem("Ps1"), "sony-playstation")
  assert.equal(resolveSystem("pS1"), "sony-playstation")
  assert.equal(resolveSystem("SNES"), "nintendo-snes")
  assert.equal(resolveSystem("N64"), "nintendo-64")
})

test("whitespace handling in alias resolution", () => {
  assert.equal(resolveSystem("  ps1  "), "sony-playstation")
  assert.equal(resolveSystem("\tgba\t"), "nintendo-game-boy-advance")
  assert.equal(resolveSystem("  playstation 2  "), "sony-playstation-2")
})

test("all systems have required fields", () => {
  const systems = listSystems()
  for (const system of systems) {
    assert.ok(system.id, `System missing id: ${JSON.stringify(system)}`)
    assert.ok(system.displayName, `System ${system.id} missing displayName`)
    assert.ok(system.libretroDatabase, `System ${system.id} missing libretroDatabase`)
    assert.ok(system.thumbnailCollection, `System ${system.id} missing thumbnailCollection`)
    assert.ok(Array.isArray(system.emulatorIds), `System ${system.id} emulatorIds not array`)
    assert.ok(system.emulatorIds.length > 0, `System ${system.id} has no emulators`)
    assert.ok(Array.isArray(system.aliases), `System ${system.id} aliases not array`)
    assert.ok(system.aliases.length > 0, `System ${system.id} has no aliases`)
    assert.ok(system.mediaType, `System ${system.id} missing mediaType`)
    assert.ok(Array.isArray(system.identityStrategy), `System ${system.id} identityStrategy not array`)
    assert.ok(system.identityStrategy.length > 0, `System ${system.id} has empty identityStrategy`)
  }
})

test("no duplicate aliases across systems", () => {
  const systems = listSystems()
  const seen = new Map()

  for (const system of systems) {
    for (const alias of system.aliases) {
      const normalized = alias.toLowerCase().trim()
      if (seen.has(normalized)) {
        assert.fail(`Duplicate alias "${alias}" in systems ${seen.get(normalized)} and ${system.id}`)
      }
      seen.set(normalized, system.id)
    }
  }
})

test("identity strategies are valid", () => {
  const validStrategies = ["serial", "sha1", "md5", "crc32", "title"]
  const systems = listSystems()

  for (const system of systems) {
    for (const strategy of system.identityStrategy) {
      assert.ok(
        validStrategies.includes(strategy),
        `System ${system.id} has invalid strategy: ${strategy}`
      )
    }
  }
})

test("media types are valid", () => {
  const validTypes = ["cartridge", "disc", "digital", "mixed"]
  const systems = listSystems()

  for (const system of systems) {
    assert.ok(
      validTypes.includes(system.mediaType),
      `System ${system.id} has invalid mediaType: ${system.mediaType}`
    )
  }
})

test("serial patterns capture correctly", () => {
  const testCases = [
    { systemId: "sony-playstation", text: "SLUS-20312", expected: "SLUS20312" },
    { systemId: "sony-playstation", text: "Game [SCUS_94900]", expected: "SCUS94900" },
    { systemId: "sony-playstation-3", text: "BLUS30100", expected: "BLUS30100" },
    { systemId: "nintendo-gamecube", text: "GALE01", expected: "GALE01" },
    { systemId: "nintendo-wii", text: "RMCE01", expected: "RMCE01" },
  ]

  for (const { systemId, text, expected } of testCases) {
    const result = extractSerial(text, systemId)
    assert.equal(result, expected, `Failed to extract serial from "${text}" for ${systemId}`)
  }
})
