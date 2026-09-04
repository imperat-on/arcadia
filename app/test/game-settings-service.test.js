"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createGameSettingsService } = require("../electron/game-settings-service")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-game-settings-"))
  return { root, file: path.join(root, "game_settings.json") }
}

test("game settings lê, mescla, persiste e remove por id", () => {
  const f = fixture()
  try {
    const service = createGameSettingsService({ getPath: () => f.file })
    assert.deepEqual(service.get("steam:10"), {})

    assert.deepEqual(service.set("steam:10", { wineVersion: "proton", fsync: true }), {
      wineVersion: "proton",
      fsync: true,
    })
    assert.deepEqual(service.set("steam:10", { fsync: false, prefixPath: "/tmp/prefix" }), {
      wineVersion: "proton",
      fsync: false,
      prefixPath: "/tmp/prefix",
    })
    assert.deepEqual(JSON.parse(fs.readFileSync(f.file, "utf8")), {
      "steam:10": { wineVersion: "proton", fsync: false, prefixPath: "/tmp/prefix" },
    })

    service.remove("steam:10")
    assert.deepEqual(JSON.parse(fs.readFileSync(f.file, "utf8")), {})
    assert.deepEqual(service.get("steam:10"), {})
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("troca de conta invalida o cache mesmo com mtime igual", () => {
  const f = fixture()
  const other = path.join(f.root, "other.json")
  let active = f.file
  try {
    fs.writeFileSync(f.file, JSON.stringify({ "steam:10": { account: "first" } }))
    fs.writeFileSync(other, JSON.stringify({ "steam:10": { account: "second" } }))
    const mtime = new Date("2026-01-01T00:00:00.000Z")
    fs.utimesSync(f.file, mtime, mtime)
    fs.utimesSync(other, mtime, mtime)
    const service = createGameSettingsService({ getPath: () => active })

    assert.deepEqual(service.get("steam:10"), { account: "first" })
    active = other
    assert.deepEqual(service.get("steam:10"), { account: "second" })
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})


test("game settings escreve com permissão restrita e não segue symlink", () => {
  const f = fixture()
  const target = path.join(f.root, "target.json")
  try {
    const service = createGameSettingsService({ getPath: () => f.file })
    service.set("steam:20", { fsync: true })
    // NTFS nao tem bit POSIX; chmod 0600 e no-op no Windows.
    if (process.platform !== "win32") assert.equal(fs.statSync(f.file).mode & 0o777, 0o600)
    assert.deepEqual(fs.readdirSync(f.root).filter((name) => name.includes(".tmp-")), [])

    fs.rmSync(f.file)
    fs.writeFileSync(target, JSON.stringify({ untouched: true }))
    fs.symlinkSync(target, f.file)
    service.set("steam:20", { fsync: false })
    assert.deepEqual(JSON.parse(fs.readFileSync(target, "utf8")), { untouched: true })
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("Gamescope settings novas sobrevivem ao reload e preservam legado", () => {
  const f = fixture()
  const id = "custom:gamescope"
  const legacy = {
    gamescope: true,
    gsWidth: 2560,
    gsHeight: 1600,
    gsFps: 60,
    dxvkHud: "fps",
  }
  try {
    fs.writeFileSync(f.file, JSON.stringify({ [id]: legacy }), "utf8")
    const service = createGameSettingsService({ getPath: () => f.file })
    assert.deepEqual(service.get(id), legacy)

    const updated = service.set(id, {
      gsHdr: true,
      gsWindowMode: "borderless",
      gsFramerateLimit: 45,
    })
    assert.deepEqual(updated, {
      ...legacy,
      gsHdr: true,
      gsWindowMode: "borderless",
      gsFramerateLimit: 45,
    })

    // A fresh service proves these fields are on disk, not only in the cache.
    const reloaded = createGameSettingsService({ getPath: () => f.file })
    assert.deepEqual(reloaded.get(id), updated)

    // Disabling the wrapper must not silently discard its saved options.
    const disabled = reloaded.set(id, { gamescope: false })
    assert.deepEqual(disabled, { ...updated, gamescope: false })
    assert.deepEqual(createGameSettingsService({ getPath: () => f.file }).get(id), disabled)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})
