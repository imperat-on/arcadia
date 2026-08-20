"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  MANIFEST_VERSION,
  PLUGIN_PERMISSIONS,
  validateManifest,
  parseManifest,
  resolvePluginEntry,
} = require("../electron/plugins/manifest")

test("manifest v1 canoniza aliases e permissões declarativas", () => {
  const result = validateManifest({
    manifest_version: MANIFEST_VERSION,
    api_version: "1",
    id: "com.arcadia.demo",
    name: "Demo",
    version: "1.2.3",
    entrypoint: "index.js",
    permissions: ["library:read", "events:subscribe"],
  })
  assert.equal(result.ok, true)
  assert.deepEqual(result.manifest, {
    manifestVersion: 1,
    apiVersion: 1,
    id: "com.arcadia.demo",
    name: "Demo",
    version: "1.2.3",
    description: "",
    entry: "index.js",
    permissions: ["library:read", "events:subscribe"],
  })
})

test("manifest rejeita versão, permissão e campos desconhecidos", () => {
  const result = validateManifest({
    manifestVersion: 2,
    apiVersion: 1,
    id: "demo",
    name: "Demo",
    version: "1.0.0",
    entry: "index.js",
    permissions: ["filesystem:*", PLUGIN_PERMISSIONS[0], PLUGIN_PERMISSIONS[0]],
    execute: "node index.js",
  })
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("manifest_version_invalida"))
  assert.ok(result.errors.some((error) => error.startsWith("permission_nao_suportada:")))
  assert.ok(result.errors.some((error) => error.startsWith("permission_duplicada:")))
  assert.ok(result.errors.some((error) => error.startsWith("campo_desconhecido:")))
})

test("manifest não aceita ids/path traversal e entry absoluto", () => {
  for (const id of ["../plugin", "Plugin", "", ".", "a".repeat(65)]) {
    const result = validateManifest({
      manifestVersion: 1,
      apiVersion: 1,
      id,
      name: "Demo",
      version: "1.0.0",
      entry: "index.js",
    })
    assert.equal(result.ok, false, id)
  }
  const traversal = validateManifest({
    manifestVersion: 1,
    apiVersion: 1,
    id: "demo",
    name: "Demo",
    version: "1.0.0",
    entry: "../index.js",
  })
  assert.equal(traversal.ok, false)
  assert.ok(traversal.errors.includes("entry_invalido"))
  assert.equal(parseManifest("not-json").ok, false)
})

test("entry não pode seguir symlink para fora do pacote", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-entry-"))
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-outside-"))
  try {
    fs.mkdirSync(path.join(root, "plugin"))
    fs.writeFileSync(path.join(outside, "entry.js"), "")
    fs.symlinkSync(path.join(outside, "entry.js"), path.join(root, "plugin", "entry.js"))
    const result = resolvePluginEntry(path.join(root, "plugin"), "entry.js")
    assert.equal(result.ok, false)
    assert.ok(["entry_invalido", "entry_fora_do_plugin"].includes(result.error))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
    fs.rmSync(outside, { recursive: true, force: true })
  }
})
