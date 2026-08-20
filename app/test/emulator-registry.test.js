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
  for (const id of ["pcsx2", "rpcs3", "dolphin", "ppsspp", "duckstation", "retroarch", "melonds", "desmume"]) {
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
  assert.equal(normalizeId("__proto__"), "")
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
    const saved = registry.setProfile({
      id: "pcsx2",
      executable: f.exe,
      biosPath: path.join(f.root, "bios"),
      romFolders: [{ path: path.join(f.root, "roms"), recursive: false }],
      args: ["--fullscreen"],
    })
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
    assert.equal(item.profile.biosPath, path.join(f.root, "bios"))
    assert.deepEqual(item.profile.romFolders, [{ path: path.join(f.root, "roms"), recursive: false }])
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
      registry.resolveLaunch({ emulatorId: "toString", romPath: f.rom }).error,
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


test("scanRoms usa allowlist, recursão segura e agrupa sidecars", () => {
  const f = fixture()
  const romDir = path.join(f.root, "roms")
  const nested = path.join(romDir, "nested")
  fs.mkdirSync(nested, { recursive: true })
  const cue = path.join(romDir, "Game.cue")
  const bin = path.join(romDir, "Game.bin")
  const iso = path.join(nested, "Other.ISO")
  fs.writeFileSync(cue, "cue")
  fs.writeFileSync(bin, "bin")
  fs.writeFileSync(iso, "iso")
  fs.writeFileSync(path.join(romDir, "readme.txt"), "ignore")
  const link = path.join(romDir, "linked.iso")
  fs.symlinkSync(iso, link)
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const result = registry.scanRoms({ emulatorId: "pcsx2", directory: romDir })
    assert.equal(result.ok, true)
    assert.deepEqual(result.roms.map((item) => item.relativePath), ["Game.cue", "nested/Other.ISO"])
    assert.deepEqual(result.roms[0].sidecars, [bin])
    assert.equal(result.roms[0].sizeBytes, 6)
    assert.deepEqual(result.romExtensions, [
      ".iso", ".chd", ".cso", ".zso", ".gz", ".nrg", ".cue", ".bin", ".mds", ".mdf", ".m3u",
    ])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("scanRoms limita profundidade/resultados e normaliza extensões customizadas", () => {
  const f = fixture()
  const root = path.join(f.root, "roms")
  const nested = path.join(root, "one", "two")
  fs.mkdirSync(nested, { recursive: true })
  fs.writeFileSync(path.join(root, "a.foo"), "a")
  fs.writeFileSync(path.join(root, "b.FOO"), "b")
  fs.writeFileSync(path.join(nested, "deep.foo"), "deep")
  try {
    const registry = createEmulatorRegistry({
      dataDir: f.root,
      definitions: [{
        id: "customscan",
        name: "Scanner",
        systems: ["Sistema"],
        candidates: ["scanner"],
        romExtensions: ["foo", ".foo", "bad/ext", "*"],
        romDirectoryMarkers: ["DISC", "../bad", "nested/name"],
      }],
    })
    assert.deepEqual(registry.definitions().find((item) => item.id === "customscan").romExtensions, [".foo"])
    assert.deepEqual(registry.definitions().find((item) => item.id === "customscan").romDirectoryMarkers, ["DISC"])
    const shallow = registry.scanRoms({ emulatorId: "customscan", directory: root, maxDepth: 0 })
    assert.deepEqual(shallow.roms.map((item) => item.name), ["a.foo", "b.FOO"])
    const limited = registry.scanRoms({ emulatorId: "customscan", directory: root, maxResults: 1 })
    assert.equal(limited.roms.length, 1)
    assert.equal(limited.truncated, true)
    assert.equal(registry.scanRoms({ emulatorId: "customscan", directory: root, maxDepth: 99 }).error, "opcoes_scan_invalidas")
    assert.equal(registry.scanRoms({ emulatorId: "customscan", directory: path.join(f.root, "missing") }).error, "diretorio_rom_invalido")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("resolveLaunch oferece template de argv compatível com Hydra sem shell", () => {
  const f = fixture()
  try {
    const rpcs3 = path.join(f.bin, "rpcs3")
    const retroarch = path.join(f.bin, "retroarch")
    const core = path.join(f.root, "cores", "core.so")
    fs.writeFileSync(rpcs3, "bin")
    fs.writeFileSync(retroarch, "bin")
    fs.chmodSync(rpcs3, 0o755)
    fs.chmodSync(retroarch, 0o755)
    fs.mkdirSync(path.dirname(core), { recursive: true })
    fs.writeFileSync(core, "core")
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.deepEqual(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: f.rom, launchMode: "hydra" }).cmd,
      [f.exe, "-batch", "-fullscreen", "--", f.rom],
    )
    assert.deepEqual(
      registry.resolveLaunch({ emulatorId: "rpcs3", romPath: f.rom, launchMode: "hydra" }).cmd,
      [rpcs3, "--no-gui", f.rom],
    )
    assert.deepEqual(
      registry.resolveLaunch({ emulatorId: "retroarch", romPath: f.rom, corePath: core, launchMode: "hydra" }).cmd,
      [retroarch, "-L", core, f.rom, "-f"],
    )
    assert.equal(registry.resolveLaunch({ emulatorId: "pcsx2", romPath: f.rom, launchMode: "sh" }).error, "launch_mode_invalido")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("resolveLaunch rejeita extensão incompatível com o catálogo", () => {
  const f = fixture()
  const bad = path.join(f.root, "games", "demo.txt")
  fs.writeFileSync(bad, "not a rom")
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.equal(registry.resolveLaunch({ emulatorId: "pcsx2", romPath: bad }).error, "rom_invalida")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("scanRoms reconhece PS3_GAME e prioriza playlists M3U", () => {
  const f = fixture()
  const rootDir = path.join(f.root, "roms")
  const ps3 = path.join(rootDir, "Title", "PS3_GAME")
  const playlistDir = path.join(rootDir, "multi")
  fs.mkdirSync(ps3, { recursive: true })
  fs.mkdirSync(playlistDir, { recursive: true })
  fs.writeFileSync(path.join(ps3, "PARAM.SFO"), "metadata")
  fs.writeFileSync(path.join(rootDir, "Title", "EBOOT.BIN"), "internal")
  fs.writeFileSync(path.join(playlistDir, "game.m3u"), "disc1.iso\ndisc2.iso")
  fs.writeFileSync(path.join(playlistDir, "disc1.iso"), "disc")
  fs.writeFileSync(path.join(playlistDir, "disc2.iso"), "disc")
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const result = registry.scanRoms({ emulatorId: "rpcs3", directory: rootDir })
    assert.equal(result.ok, true)
    assert.equal(result.roms.some((item) => item.path === ps3 && item.kind === "directory"), true)
    assert.equal(result.roms.some((item) => item.name === "EBOOT.BIN"), false)
    const playlist = registry.scanRoms({ emulatorId: "pcsx2", directory: playlistDir })
    assert.deepEqual(playlist.roms.map((item) => item.name), ["game.m3u"])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("modo Hydra troca MDS por MDF sidecar quando disponível", () => {
  const f = fixture()
  const mds = path.join(f.root, "games", "disc.mds")
  const mdf = path.join(f.root, "games", "disc.mdf")
  fs.writeFileSync(mds, "descriptor")
  fs.writeFileSync(mdf, "image")
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.equal(
      registry.resolveLaunch({ emulatorId: "pcsx2", romPath: mds, launchMode: "hydra" }).cmd.at(-1),
      mdf,
    )
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("modo Hydra não passa PKG cru e usa EBOOT instalado quando existe", () => {
  const f = fixture()
  const rpcs3 = path.join(f.bin, "rpcs3")
  fs.writeFileSync(rpcs3, "bin")
  fs.chmodSync(rpcs3, 0o755)
  const pkg = path.join(f.root, "games", "INSTALL.pkg")
  const header = Buffer.alloc(0x80)
  header.writeUInt32BE(0x7f504b47, 0)
  Buffer.from("UP0001-NPAB12345_00-GAME000000000000").copy(header, 0x30)
  fs.writeFileSync(pkg, header)
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin, homeDir: path.join(f.root, "home") })
    const missing = registry.resolveLaunch({ emulatorId: "rpcs3", romPath: pkg, launchMode: "hydra" })
    assert.equal(missing.error, "pkg_instalacao_necessaria")
    assert.equal(missing.code, "PACKAGE_INSTALL_REQUIRED")
    const eboot = path.join(f.root, "home", ".config", "rpcs3", "dev_hdd0", "game", "NPAB12345", "USRDIR", "EBOOT.BIN")
    fs.mkdirSync(path.dirname(eboot), { recursive: true })
    fs.writeFileSync(eboot, "eboot")
    const result = registry.resolveLaunch({ emulatorId: "rpcs3", romPath: pkg, launchMode: "hydra" })
    assert.deepEqual(result.cmd, [rpcs3, "--no-gui", eboot])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("scanRoms sem diretório usa pastas ROM persistidas e deduplica caminhos", () => {
  const f = fixture()
  const folder = path.join(f.root, "roms")
  fs.mkdirSync(folder, { recursive: true })
  const rom = path.join(folder, "game.iso")
  fs.writeFileSync(rom, "iso")
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    assert.equal(registry.setProfile({ id: "pcsx2", executable: f.exe, romFolders: [folder, folder] }).ok, true)
    assert.equal(registry.setProfile({ id: "pcsx2", executable: f.exe, romFolders: [`${f.root}/../outside`] }).error, "rom_folder_invalida")
    const result = registry.scanRoms({ emulatorId: "pcsx2", maxResults: 32 })
    assert.equal(result.ok, true)
    assert.deepEqual(result.folders, [{ path: folder, recursive: true }])
    assert.equal(result.persisted, true)
    assert.deepEqual(result.roms.map((item) => item.path), [rom])
    assert.deepEqual(registry.roms().pcsx2.roms.map((item) => item.path), [rom])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("modo Hydra bloqueia MDS sem MDF sidecar com código estruturado", () => {
  const f = fixture()
  const mds = path.join(f.root, "games", "missing.mds")
  fs.writeFileSync(mds, "descriptor")
  try {
    const registry = createEmulatorRegistry({ dataDir: f.root, envPath: f.bin })
    const result = registry.resolveLaunch({ emulatorId: "pcsx2", romPath: mds, launchMode: "hydra" })
    assert.equal(result.error, "disc_sidecar_ausente")
    assert.equal(result.code, "DISC_SIDECAR_MISSING")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})
