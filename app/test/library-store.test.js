"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  LIBRARY_SCHEMA_VERSION,
  parseLibraryDocument,
  readLibraryFile,
  writeLibraryFile,
} = require("../electron/library-store")

test("library-store lê arrays legados sem migração destrutiva", () => {
  const result = parseLibraryDocument([
    { id: "steam:1", title: "Jogo", launcher: "steam", launch_cmd: ["steam"] },
  ])
  assert.equal(result.version, 0)
  assert.equal(result.legacy, true)
  assert.equal(result.games[0].id, "steam:1")
})

test("library-store grava e lê envelope versionado atomicamente", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-library-store-"))
  const file = path.join(dir, "nested", "library.json")
  try {
    const payload = writeLibraryFile(file, [{ id: "custom:1", title: "Local" }], {
      sources: { custom: 1 },
      generatedAt: 123,
    })
    assert.equal(payload.version, LIBRARY_SCHEMA_VERSION)
    assert.deepEqual(readLibraryFile(file), {
      version: 1,
      legacy: false,
      generatedAt: 123,
      sources: { custom: 1 },
      games: [{ id: "custom:1", title: "Local", launcher: "custom", launch_cmd: [] }],
    })
    assert.equal(fs.existsSync(`${file}.tmp`), false)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("library-store rejeita versões futuras sem expor payload desconhecido", () => {
  const result = parseLibraryDocument({ version: 99, games: [{ id: "x", title: "futuro" }] })
  assert.deepEqual(result, { version: 99, legacy: false, games: [], error: "versao_incompativel" })
})
