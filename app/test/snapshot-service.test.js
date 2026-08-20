"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createSnapshotService } = require("../electron/snapshot-service")

test("snapshot cria, lista e restaura dados com backup", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-snapshots-"))
  const source = path.join(root, "source")
  const snapshots = path.join(root, "snapshots")
  const target = path.join(root, "target")
  fs.mkdirSync(source, { recursive: true })
  fs.writeFileSync(path.join(source, "save.dat"), "v1")
  try {
    const service = createSnapshotService({ snapshotsDir: snapshots, now: () => new Date("2026-01-01T00:00:00.000Z") })
    const created = service.create({ gameId: "steam:440", sourceDir: source, label: "Antes" })
    assert.equal(created.ok, true)
    assert.equal(created.snapshot.label, "Antes")
    assert.equal(service.list("steam:440").length, 1)
    fs.mkdirSync(target, { recursive: true })
    fs.writeFileSync(path.join(target, "old.dat"), "old")
    const restored = service.restore({ gameId: "steam:440", snapshotId: created.snapshot.id, targetDir: target })
    assert.equal(restored.ok, true)
    assert.equal(fs.readFileSync(path.join(target, "save.dat"), "utf8"), "v1")
    assert.equal(fs.existsSync(restored.backupPath), true)
    assert.equal(fs.readFileSync(path.join(restored.backupPath, "old.dat"), "utf8"), "old")
    assert.equal(service.remove({ gameId: "steam:440", snapshotId: created.snapshot.id }).ok, true)
    assert.equal(service.list("steam:440").length, 0)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test("snapshot rejeita origem, versão e traversal inválidos", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-snapshots-"))
  try {
    const service = createSnapshotService({ snapshotsDir: path.join(root, "snapshots") })
    assert.deepEqual(service.create({ gameId: "steam:1", sourceDir: "/does/not/exist" }), { ok: false, error: "origem_invalida" })
    assert.deepEqual(service.restore({ gameId: "steam:1", snapshotId: "../../etc", targetDir: path.join(root, "target") }), { ok: false, error: "snapshot_invalido" })
    assert.deepEqual(service.remove({ gameId: "steam:1", snapshotId: "missing" }), { ok: false, error: "snapshot_invalido" })
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
