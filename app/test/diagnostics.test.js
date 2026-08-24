"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createDiagnosticsService } = require("../electron/diagnostics")

test("diagnósticos agregam estado local sem expor paths ou tokens", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-diagnostics-"))
  const previous = process.env.ARCADIA_DATA_DIR
  process.env.ARCADIA_DATA_DIR = dir
  try {
    fs.writeFileSync(path.join(dir, "library.json"), "[]")
    fs.writeFileSync(path.join(dir, "downloads.json"), "[]")
    fs.mkdirSync(path.join(dir, "snapshots", "steam_1", "one"), { recursive: true })
    const service = createDiagnosticsService({
      dataDir: dir,
      appVersion: "1.2.3",
      now: () => new Date("2026-01-01T00:00:00.000Z"),
      getQueue: () => [{ status: "queued" }, { status: "error" }],
      getLibrary: () => [{ launcher: "steam" }, { launcher: "steam" }, { launcher: "lutris" }],
      osImpl: { platform: () => "linux", release: () => "test", arch: () => "x64" },
    })
    const value = service.collect()
    assert.equal(value.version, 1)
    assert.equal(value.app.version, "1.2.3")
    assert.equal(value.storage.writable, true)
    assert.equal(value.storage.snapshots, 1)
    assert.deepEqual(value.downloads.by_status, { queued: 1, error: 1 })
    assert.deepEqual(value.library.by_launcher, { steam: 2, lutris: 1 })
    assert.equal(JSON.stringify(value).includes(dir), false)
    assert.equal(JSON.stringify(value).includes("token"), false)
  } finally {
    if (previous === undefined) delete process.env.ARCADIA_DATA_DIR
    else process.env.ARCADIA_DATA_DIR = previous
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
