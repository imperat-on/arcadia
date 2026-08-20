"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const paths = require("../electron/runtime-paths")

test("resolveDataDir usa o diretório padrão quando não há override", () => {
  const resolved = paths.resolveDataDir("")
  assert.equal(resolved, path.resolve(paths.DEFAULT_DATA_DIR))
  assert.ok(path.isAbsolute(resolved))
})

test("resolveDataDir normaliza override relativo e expande caminhos", () => {
  const resolved = paths.resolveDataDir("./tmp/arcadia-runtime")
  assert.ok(path.isAbsolute(resolved))
  assert.ok(resolved.endsWith(path.join("tmp", "arcadia-runtime")))
})

test("getDataDir e dataPath respeitam ARCADIA_DATA_DIR", () => {
  const anterior = process.env.ARCADIA_DATA_DIR
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-runtime-"))
  try {
    process.env.ARCADIA_DATA_DIR = dir
    assert.equal(paths.getDataDir(), path.resolve(dir))
    assert.equal(paths.dataPath("library.json"), path.join(dir, "library.json"))
    assert.equal(
      paths.accountPath("alice", "achievements.json"),
      path.join(dir, "contas", "alice", "achievements.json"),
    )
    assert.equal(paths.accountPath("", "library.json"), path.join(dir, "library.json"))
  } finally {
    if (anterior === undefined) delete process.env.ARCADIA_DATA_DIR
    else process.env.ARCADIA_DATA_DIR = anterior
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
