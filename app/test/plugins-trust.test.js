"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  canonicalPayload,
  verifySignature,
  createTrustStore,
} = require("../electron/plugins/trust")
const { validateManifest } = require("../electron/plugins/manifest")
const { createPluginRegistry } = require("../electron/plugins/registry")

function keyPair() {
  return crypto.generateKeyPairSync("ed25519")
}

function sign(manifest, privateKey) {
  return crypto.sign(null, Buffer.from(canonicalPayload(manifest), "utf8"), privateKey).toString("base64")
}

function packageAt(root, { id, source = "// local plugin\n", signingKeyId, signature, entrySha256 } = {}) {
  const directory = path.join(root, id)
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, "index.js"), source)
  const digest = crypto.createHash("sha256").update(source).digest("hex")
  const manifest = {
    manifestVersion: 1,
    apiVersion: 1,
    id,
    name: id,
    version: "1.0.0",
    entry: "index.js",
    entrySha256: entrySha256 === undefined ? digest : entrySha256,
  }
  if (signingKeyId !== undefined) manifest.signingKeyId = signingKeyId
  if (signature !== undefined) manifest.signature = signature
  fs.writeFileSync(path.join(directory, "plugin.json"), JSON.stringify(manifest))
  return { directory, manifest, digest }
}

function cleanup(...roots) {
  for (const root of roots) fs.rmSync(root, { recursive: true, force: true })
}

test("canonicalPayload fixa id/version/entrySha256 e verifica Ed25519", () => {
  const { privateKey, publicKey } = keyPair()
  const manifest = { id: "demo.plugin", version: "1.2.3", entrySha256: "ab".repeat(32) }
  const payload = canonicalPayload(manifest)
  assert.equal(payload, JSON.stringify(manifest))
  const signature = sign(manifest, privateKey)
  const pem = publicKey.export({ type: "spki", format: "pem" })
  assert.equal(verifySignature(payload, signature, pem), true)
  assert.equal(verifySignature(payload, signature.slice(0, -1) + "A", pem), false)
  assert.equal(verifySignature({ ...manifest, version: "9.9.9" }, signature, pem), false)
})

test("manifest mantém legado e valida par signingKeyId/signature estritamente", () => {
  const legacy = validateManifest({
    manifestVersion: 1,
    id: "legacy.plugin",
    name: "Legacy",
    version: "1.0.0",
    entry: "index.js",
  })
  assert.equal(legacy.ok, true)
  assert.equal(Object.hasOwn(legacy.manifest, "signature"), false)

  const bad = validateManifest({
    manifestVersion: 1,
    id: "signed.plugin",
    name: "Signed",
    version: "1.0.0",
    entry: "index.js",
    signingKeyId: "vendor/key",
    signature: "not-a-signature",
  })
  assert.equal(bad.ok, false)
  assert.ok(bad.errors.includes("signing_key_id_invalido"))
  assert.ok(bad.errors.includes("signature_invalida"))

  const incomplete = validateManifest({
    manifestVersion: 1,
    id: "signed.plugin",
    name: "Signed",
    version: "1.0.0",
    entry: "index.js",
    signingKeyId: "vendor-key",
  })
  assert.equal(incomplete.ok, false)
  assert.ok(incomplete.errors.includes("assinatura_incompleta"))
})

test("trust store é atômico, 0600 e nunca retorna paths", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-trust-"))
  try {
    const { publicKey } = keyPair()
    const store = createTrustStore({ dataDir })
    const added = store.add("vendor-key", publicKey)
    assert.deepEqual(added, { ok: true, keyId: "vendor-key" })
    assert.equal(Object.hasOwn(added, "path"), false)
    const keys = store.list()
    assert.equal(keys.length, 1)
    assert.equal(keys[0].keyId, "vendor-key")
    assert.equal(Object.hasOwn(keys[0], "path"), false)
    const file = path.join(dataDir, "plugins", "trusted-keys.json")
    // NTFS nao tem bit POSIX; chmod 0600 e no-op no Windows.
    if (process.platform !== "win32") assert.equal((fs.statSync(file).mode & 0o777), 0o600)
    assert.equal(store.remove("vendor-key").ok, true)
    assert.deepEqual(store.list(), [])
  } finally {
    cleanup(dataDir)
  }
})

test("registry exige assinatura declarada, detecta tamper e preserva legacy", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-trust-data-"))
  const packages = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-trust-pkg-"))
  try {
    const { privateKey, publicKey } = keyPair()
    const source = "globalThis.__arcadia_trust_entry_ran = true\n"
    const unsigned = packageAt(packages, { id: "legacy.trust", source })
    const store = createTrustStore({ dataDir })
    assert.equal(store.add("vendor-key", publicKey).ok, true)

    const signedBase = packageAt(packages, { id: "signed.trust", source: "// signed\n" })
    signedBase.manifest.signingKeyId = "vendor-key"
    signedBase.manifest.signature = sign(signedBase.manifest, privateKey)
    fs.writeFileSync(path.join(signedBase.directory, "plugin.json"), JSON.stringify(signedBase.manifest))

    const registry = createPluginRegistry({ dataDir })
    assert.equal(registry.register(unsigned.directory).ok, true)
    const legacy = registry.verify("legacy.trust")
    assert.equal(legacy.ok, true)
    assert.equal(Object.hasOwn(legacy, "signature"), false)

    const registered = registry.register(signedBase.directory)
    assert.equal(registered.ok, true)
    assert.equal(globalThis.__arcadia_trust_entry_ran, undefined)
    const verified = registry.verify("signed.trust")
    assert.equal(verified.ok, true)
    assert.equal(verified.signatureAlgorithm, "ed25519")
    assert.equal(verified.signatureVerified, true)
    assert.equal(verified.path, undefined)

    fs.writeFileSync(path.join(signedBase.directory, "index.js"), "// tampered\n")
    const tampered = registry.verify("signed.trust")
    assert.equal(tampered.ok, false)
    assert.equal(tampered.path, undefined)
    assert.ok(["entry_digest_mismatch", "assinatura_invalida"].includes(tampered.error))
  } finally {
    delete globalThis.__arcadia_trust_entry_ran
    cleanup(dataDir, packages)
  }
})
