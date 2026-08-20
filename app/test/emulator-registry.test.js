"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  DEFINITIONS,
  normalizeId,
  normalizeArgs,
  findOnPath,
  normalizeDefinition,
  createEmulatorRegistry,
} = require("../electron/emulator-registry")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-emulators-"))
  const bin = path.join(root, "bin")
  fs.mkdirSync(bin)
  const rom = path.join(root, "games", "demo.iso")
  fs.mkdirSync(path.dirname(rom), { recursive: true })
  fs.writeFileSync(rom, "rom bytes")
  const exe = path.join(bin, "pcsx2-qt")
  fs.writeFileSync(exe, "#!/bin/sh\n")
  fs.chmodSync(exe, 0o755)
  return { root, bin, rom, exe }
}

test("catálogo inclui emuladores recomendados e sistemas", () => {
  for (const id of ["pcsx2", "rpcs3", "dolphin", "ppsspp", "duckstation", "retroarch"]) {
    assert.ok(DEFINITIONS[id], id)
    assert.ok(DEFINITIONS[id].systems.length)
  }
})

test("extensões de catálogo são normalizadas sem transformar perfil em código", () => {
  const definition = normalizeDefinition({
    id: "myemu",
    name: "Meu Emulador",
    description: "Emulador local",
    systems: ["Sistema próprio"],
    candidates: ["myemu", "myemu;rm -rf /"],
  })
  assert.deepEqual(definition.candidates, ["myemu"])
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, extraDefinitions: [definition] })
    assert.equal(
      registry.definitions().some((item) => item.id === "myemu"),
      true,
    )
    assert.equal(registry.setProfile({ id: "myemu", executable: "myemu" }).ok, true)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("ids, args e PATH são normalizados sem shell", () => {
  assert.equal(normalizeId(" PCSX2 "), "pcsx2")
  assert.equal(normalizeId("../bad"), "")
  assert.equal(normalizeArgs(["--fullscreen", "arquivo com espaço"]).ok, true)
  assert.equal(normalizeArgs(["a\u0000b"]).ok, false)
  const f = fixture()
  try {
    assert.equal(findOnPath("pcsx2-qt", fs, f.bin), f.exe)
    assert.equal(findOnPath("missing", fs, f.bin), "")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("setProfile persiste atomicamente e detect lista disponibilidade", () => {
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const saved = registry.setProfile({ id: "pcsx2", executable: f.exe, args: ["--fullscreen"] })
    assert.equal(saved.ok, true)
    assert.equal(fs.statSync(path.join(f.root, "emulators.json")).mode & 0o777, 0o600)
    assert.deepEqual(
      fs.readdirSync(f.root).filter((name) => name.includes(".tmp-")),
      [],
    )
    const item = registry.list().find((entry) => entry.id === "pcsx2")
    assert.equal(item.available, true)
    assert.equal(item.source, "configured")
    assert.deepEqual(item.profile.args, ["--fullscreen"])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("resolveLaunch monta argv e nunca serializa comando em shell", () => {
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const result = registry.resolveLaunch({
      emulatorId: "pcsx2",
      romPath: f.rom,
      extraArgs: ["--fullscreen"],
    })
    assert.equal(result.ok, true)
    assert.deepEqual(result.cmd, [f.exe, f.rom, "--fullscreen"])
    assert.equal(
      result.cmd.some((arg) => arg.includes(" && ")),
      false,
    )
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("perfil configurado inválido não cai silenciosamente em outro candidato", () => {
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.equal(
      registry.setProfile({ id: "pcsx2", executable: path.join(f.bin, "missing") }).ok,
      true,
    )
    assert.equal(registry.list().find((item) => item.id === "pcsx2").available, false)
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: f.rom }).error,
      "executavel_configurado_invalido",
    )
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("retroarch exige core regular e mantém o core fora de payloads arbitrários", () => {
  const f = fixture()
  try {
    const retro = path.join(f.bin, "retroarch")
    const core = path.join(f.root, "cores", "pcsx2_libretro.so")
    fs.writeFileSync(retro, "bin")
    fs.chmodSync(retro, 0o755)
    fs.mkdirSync(path.dirname(core), { recursive: true })
    fs.writeFileSync(core, "core")
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const missing = registry.resolveLaunch({ emulatorId: "retroarch", romPath: f.rom })
    assert.equal(missing.error, "retroarch_core_invalido")
    const result = registry.resolveLaunch({
      emulatorId: "retroarch",
      romPath: f.rom,
      corePath: core,
    })
    assert.deepEqual(result.cmd, [retro, "-L", core, f.rom])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("resolveLaunch rejeita emulador, ROM e symlink inválidos", () => {
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.equal(
      registry.resolveLaunch({ emulatorId: "unknown", romPath: f.rom }).error,
      "emulador_desconhecido",
    )
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: "/tmp/does-not-exist" }).error,
      "rom_invalida",
    )
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: "../demo.iso" }).error,
      "rom_invalida",
    )
    assert.equal(
      registry.resolveLaunch({
        emulatorId: "pcsx2",
        romPath: `${f.root}/games/../games/demo.iso`,
      }).error,
      "rom_invalida",
    )
    const link = path.join(f.root, "link.iso")
    fs.symlinkSync(f.rom, link)
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: link }).error,
      "rom_invalida",
    )
    const linkedDir = path.join(f.root, "linked-games")
    fs.symlinkSync(path.dirname(f.rom), linkedDir)
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: path.join(linkedDir, "demo.iso") })
        .error,
      "rom_invalida",
    )
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("persistência rejeita symlink do estado e não segue o destino", () => {
  const f = fixture()
  const outside = path.join(f.root, "outside.json")
  const state = path.join(f.root, "emulators.json")
  try {
    fs.writeFileSync(outside, "outside")
    fs.symlinkSync(outside, state)
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const result = registry.setProfile({ id: "pcsx2", executable: f.exe })
    assert.equal(result.ok, false)
    assert.equal(fs.readFileSync(outside, "utf8"), "outside")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("removeProfile é idempotente e caminhos privados ficam apenas em paths interno", () => {
  const f = fixture()
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    registry.setProfile({ id: "pcsx2", executable: f.exe })
    assert.equal(registry.removeProfile("pcsx2").removed, true)
    assert.equal(registry.removeProfile("pcsx2").removed, false)
    assert.equal(registry.profiles().pcsx2, undefined)
    assert.equal(registry.paths().statePath, path.join(f.root, "emulators.json"))
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})
