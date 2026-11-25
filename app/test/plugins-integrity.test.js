"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { validateManifest } = require("../electron/plugins/manifest")
const { createPluginRegistry } = require("../electron/plugins/registry")
const pluginFacade = require("../electron/plugins")

function digest(value) {
  return crypto.createHash("sha256").update(value).digest("hex")
}

function packageAt(root, { id, source = "// safe plugin\n", entrySha256, entryDigest } = {}) {
  const directory = path.join(root, id)
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, "index.js"), source)
  const manifest = {
    manifestVersion: 1,
    apiVersion: 1,
    id,
    name: id,
    version: "1.0.0",
    entry: "index.js",
  }
  if (entrySha256 !== undefined) manifest.entrySha256 = entrySha256
  if (entryDigest !== undefined) manifest.entryDigest = entryDigest
  fs.writeFileSync(path.join(directory, "plugin.json"), JSON.stringify(manifest))
  return directory
}

function temporaryRoots() {
  return {
    dataDir: fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-integrity-data-")),
    packages: fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-integrity-pkg-")),
  }
}

function cleanup({ dataDir, packages }) {
  fs.rmSync(dataDir, { recursive: true, force: true })
  fs.rmSync(packages, { recursive: true, force: true })
}

test("fachada expõe verify sem vazar caminhos privados", () => {
  assert.equal(typeof pluginFacade.verify, "function")
  assert.equal(typeof pluginFacade.verifyPackage, "function")
  const unknown = pluginFacade.verify("integrity.unknown")
  assert.equal(unknown.ok, false)
  assert.equal(unknown.error, "plugin_desconhecido")
  assert.equal(unknown.path, undefined)
  const invalid = pluginFacade.verifyPackage(path.join(os.tmpdir(), "arcadia-no-such-plugin"))
  assert.equal(invalid.ok, false)
  assert.equal(invalid.path, undefined)
})

test("digest do manifest é opcional, mas alias é canonizado", () => {
  const value = "entry bytes"
  const result = validateManifest({
    manifestVersion: 1,
    id: "integrity.alias",
    name: "Integrity alias",
    version: "1.0.0",
    entry: "index.js",
    entryDigest: digest(value).toUpperCase(),
  })
  assert.equal(result.ok, true)
  assert.equal(result.manifest.entrySha256, digest(value))
  assert.equal(result.manifest.entryDigest, digest(value))

  const legacy = validateManifest({
    manifestVersion: 1,
    id: "integrity.legacy",
    name: "Legacy",
    version: "1.0.0",
    entry: "index.js",
  })
  assert.equal(legacy.ok, true)
  assert.equal(Object.hasOwn(legacy.manifest, "entrySha256"), false)
})

test("registry registra digest, verifica bytes intactos e nunca executa entry", () => {
  const roots = temporaryRoots()
  try {
    const source = "globalThis.__arcadia_plugin_executed = true\n"
    const pluginPath = packageAt(roots.packages, { id: "integrity.intact", source, entrySha256: digest(source) })
    const registry = createPluginRegistry({ dataDir: roots.dataDir, now: () => 5000 })

    const registered = registry.register(pluginPath)
    assert.equal(registered.ok, true)
    assert.equal(globalThis.__arcadia_plugin_executed, undefined)
    const checked = registry.verify("integrity.intact")
    assert.equal(checked.ok, true)
    assert.equal(checked.valid, true)
    assert.equal(checked.verified, true)
    assert.equal(checked.algorithm, "sha256")
    assert.equal(checked.expectedDigest, digest(source))
    assert.equal(checked.actualDigest, digest(source))
    assert.equal(checked.source, "registry")

    const state = JSON.parse(fs.readFileSync(path.join(roots.dataDir, "plugins", "registry.json"), "utf8"))
    assert.equal(state.plugins["integrity.intact"].entrySha256, digest(source))
  } finally {
    delete globalThis.__arcadia_plugin_executed
    cleanup(roots)
  }
})

test("registry detecta tamper, não habilita entry divergente e bloqueia symlink externo", () => {
  const roots = temporaryRoots()
  try {
    const source = "// original\n"
    const pluginPath = packageAt(roots.packages, { id: "integrity.tamper", source, entryDigest: digest(source) })
    const registry = createPluginRegistry({ dataDir: roots.dataDir })
    assert.equal(registry.register(pluginPath).ok, true)
    assert.equal(registry.get("integrity.tamper").enabled, false)
    assert.deepEqual(registry.setEnabled("integrity.tamper", true), { ok: true })
    assert.equal(registry.get("integrity.tamper").enabled, true)

    fs.writeFileSync(path.join(pluginPath, "index.js"), "// tampered\n")
    const tampered = registry.verify("integrity.tamper")
    assert.equal(tampered.ok, false)
    assert.equal(tampered.valid, false)
    assert.equal(tampered.error, "entry_digest_mismatch")
    assert.equal(registry.get("integrity.tamper").valid, false)
    assert.deepEqual(registry.setEnabled("integrity.tamper", true), { ok: false, error: "entry_digest_mismatch" })

    const outside = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-integrity-outside-"))
    try {
      const external = path.join(outside, "entry.js")
      fs.writeFileSync(external, source)
      fs.rmSync(path.join(pluginPath, "index.js"))
      fs.symlinkSync(external, path.join(pluginPath, "index.js"))
      const linked = registry.verify("integrity.tamper")
      assert.equal(linked.ok, false)
      assert.equal(linked.valid, false)
      assert.ok(["entry_fora_do_plugin", "entry_invalido"].includes(linked.error))
    } finally {
      fs.rmSync(outside, { recursive: true, force: true })
    }
  } finally {
    cleanup(roots)
  }
})

test("registry preserva manifest v1 sem digest, mas rejeita digest declarado incorreto", () => {
  const roots = temporaryRoots()
  try {
    const source = "// v1 sem digest\n"
    const legacyPath = packageAt(roots.packages, { id: "integrity.v1", source })
    const registry = createPluginRegistry({ dataDir: roots.dataDir })
    assert.equal(registry.register(legacyPath).ok, true)
    const legacy = registry.verify("integrity.v1")
    assert.equal(legacy.ok, true)
    assert.equal(legacy.valid, true)
    assert.equal(legacy.declared, false)
    assert.equal(registry.get("integrity.v1").enabled, false)

    const wrongPath = packageAt(roots.packages, { id: "integrity.wrong", source, entrySha256: "0".repeat(64) })
    const rejected = registry.register(wrongPath)
    assert.equal(rejected.ok, false)
    assert.equal(rejected.error, "entry_digest_mismatch")
    assert.equal(registry.get("integrity.wrong"), null)
  } finally {
    cleanup(roots)
  }
})
