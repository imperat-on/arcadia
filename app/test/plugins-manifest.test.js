"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  MANIFEST_VERSION,
  PLUGIN_PERMISSIONS,
  validateManifest,
  parseManifest,
  resolvePluginEntry,
  computeEntrySha256,
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

test("manifest aceita digest SHA-256 opcional e canoniza aliases", () => {
  const digest = "AB".repeat(32)
  const result = validateManifest({
    manifestVersion: 1,
    id: "digest.demo",
    name: "Digest",
    version: "1.0.0",
    entry: "index.js",
    entryDigest: digest,
  })
  assert.equal(result.ok, true)
  assert.equal(result.manifest.entrySha256, digest.toLowerCase())
  assert.equal(result.manifest.entry_digest, digest.toLowerCase())
  assert.equal(Object.keys(result.manifest).includes("entryDigest"), false)

  for (const value of ["", "not-hex", "a".repeat(63), "g".repeat(64), 42, null]) {
    const invalid = validateManifest({
      manifestVersion: 1,
      id: "digest.demo",
      name: "Digest",
      version: "1.0.0",
      entry: "index.js",
      entrySha256: value,
    })
    assert.equal(invalid.ok, false, String(value))
    assert.ok(invalid.errors.includes("entry_digest_invalido"), String(value))
  }

  const duplicate = validateManifest({
    manifestVersion: 1,
    id: "digest.demo",
    name: "Digest",
    version: "1.0.0",
    entry: "index.js",
    entrySha256: "a".repeat(64),
    entryDigest: "b".repeat(64),
  })
  assert.equal(duplicate.ok, false)
  assert.ok(duplicate.errors.includes("campo_duplicado:entrySha256"))
})

test("computeEntrySha256 só lê bytes e rejeita symlink externo", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-hash-"))
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-hash-outside-"))
  try {
    const entry = path.join(root, "index.js")
    const source = "globalThis.__arcadia_hash_must_not_run = true\n"
    fs.writeFileSync(entry, source)
    const result = computeEntrySha256(entry, { root })
    assert.equal(result.ok, true)
    assert.equal(result.digest, crypto.createHash("sha256").update(source).digest("hex"))
    assert.equal(globalThis.__arcadia_hash_must_not_run, undefined)

    const external = path.join(outside, "index.js")
    fs.writeFileSync(external, source)
    fs.rmSync(entry)
    fs.symlinkSync(external, entry)
    const linked = computeEntrySha256(entry, { root })
    assert.equal(linked.ok, false)
    assert.equal(linked.error, "entry_fora_do_plugin")
  } finally {
    delete globalThis.__arcadia_hash_must_not_run
    fs.rmSync(root, { recursive: true, force: true })
    fs.rmSync(outside, { recursive: true, force: true })
  }
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
