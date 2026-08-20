"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { getEmulatorStatus, preflightEmulator, PS1_MIN, PS2_MIN } = require("../electron/emulator-status")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bios-"))
  const home = path.join(root, "home")
  const bin = path.join(root, "bin")
  fs.mkdirSync(home, { recursive: true })
  fs.mkdirSync(bin, { recursive: true })
  const exe = path.join(bin, "pcsx2")
  fs.writeFileSync(exe, "exe")
  return { root, home, bin, exe }
}

test("detecta BIOS PS1 pelo tamanho e assinatura, sem extensão obrigatória", () => {
  const f = fixture()
  const bios = path.join(f.root, "duck", "bios")
  fs.mkdirSync(bios, { recursive: true })
  const dump = Buffer.alloc(PS1_MIN)
  Buffer.from("Sony Computer Entertainment").copy(dump, 32)
  fs.writeFileSync(path.join(bios, "scph-no-extension"), dump)
  try {
    const status = getEmulatorStatus({ emulatorId: "duckstation", executablePath: f.exe, biosPath: bios, home: f.home })
    assert.equal(status.ok, true)
    assert.equal(status.installed, true)
    assert.equal(status.detectedPath, bios)
    assert.equal(preflightEmulator({ emulatorId: "duckstation", executablePath: f.exe, biosPath: bios, home: f.home }).ok, true)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("detecta BIOS PS2 por RESET/ROMVER e bloqueia ausência", () => {
  const f = fixture()
  const bios = path.join(f.root, "pcsx2", "bios")
  fs.mkdirSync(bios, { recursive: true })
  const dump = Buffer.alloc(PS2_MIN)
  Buffer.from("RESET").copy(dump, 64)
  Buffer.from("ROMVER").copy(dump, 128)
  fs.writeFileSync(path.join(bios, "bios.bin"), dump)
  try {
    const installed = getEmulatorStatus({ emulatorId: "pcsx2", executablePath: f.exe, biosPath: bios, home: f.home })
    assert.equal(installed.installed, true)
    const missing = preflightEmulator({ emulatorId: "pcsx2", executablePath: f.exe, biosPath: path.join(f.root, "missing"), home: f.home })
    assert.equal(missing.ok, false)
    assert.equal(missing.error, "BIOS_NOT_CONFIGURED")
    assert.equal(missing.code, "BIOS_NOT_CONFIGURED")
    assert.equal(missing.legacyError, "bios_nao_configurado")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("firmware RPCS3 é reportado sem bloquear status genérico e symlink não é seguido", () => {
  const f = fixture()
  const external = path.join(f.home, ".config", "rpcs3", "dev_flash", "sys", "external")
  fs.mkdirSync(external, { recursive: true })
  fs.writeFileSync(path.join(external, "firmware.dat"), "firmware")
  const outside = path.join(f.root, "outside")
  fs.writeFileSync(outside, "outside")
  const link = path.join(f.root, "linked")
  fs.symlinkSync(outside, link)
  try {
    const status = getEmulatorStatus({ emulatorId: "rpcs3", executablePath: f.exe, home: f.home })
    assert.equal(status.kind, "firmware")
    assert.equal(status.installed, true)
    const no = getEmulatorStatus({ emulatorId: "pcsx2", executablePath: f.exe, biosPath: link, home: f.home })
    assert.equal(no.installed, false)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("preflight usa códigos estruturados para entradas inválidas", () => {
  const result = getEmulatorStatus({ emulatorId: "" })
  assert.equal(result.ok, false)
  assert.equal(result.code, "EMULATOR_UNKNOWN")
  assert.equal(result.error, "EMULATOR_UNKNOWN")
})
