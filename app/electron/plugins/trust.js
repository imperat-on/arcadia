"use strict"

// Chaves confiáveis de plugins locais. Este módulo só manipula metadados e
// verifica assinaturas; nunca baixa, importa ou executa o entry de um plugin.

const crypto = require("node:crypto")
const fsDefault = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const TRUST_STORE_VERSION = 1
const TRUST_STORE_FILENAME = "trusted-keys.json"
const MAX_TRUSTED_KEYS = 256
const MAX_KEY_ID_LENGTH = 128
const KEY_ID_RE = /^[A-Za-z0-9](?:[A-Za-z0-9._:-]{0,126}[A-Za-z0-9])?$/
const SIGNATURE_LENGTH = 64
const BASE64_RE = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/

function own(value, key) {
  return Object.prototype.hasOwnProperty.call(value, key)
}

function normalizeKeyId(value) {
  if (typeof value !== "string") return ""
  const id = value.trim()
  if (!id || id.length > MAX_KEY_ID_LENGTH || id.includes("\u0000")) return ""
  return KEY_ID_RE.test(id) ? id : ""
}

function decodeSignature(value) {
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
    const bytes = Buffer.from(value)
    return bytes.length === SIGNATURE_LENGTH ? bytes : null
  }
  if (typeof value !== "string" || !value || value.length % 4 !== 0 || !BASE64_RE.test(value)) return null
  try {
    const bytes = Buffer.from(value, "base64")
    // Re-encoding closes Buffer.from's permissive handling of malformed input.
    if (bytes.length !== SIGNATURE_LENGTH || bytes.toString("base64") !== value) return null
    return bytes
  } catch {
    return null
  }
}

function importPublicKey(value) {
  try {
    if (value && typeof value === "object" && value.type === "public" && value.asymmetricKeyType === "ed25519") {
      return value
    }
    let source = value
    if (typeof source === "string" && !source.includes("BEGIN")) {
      // Accept a base64-encoded SPKI DER key as a convenience for callers that
      // persist keys without PEM armor. The trust store itself writes PEM.
      if (!BASE64_RE.test(source) || source.length % 4 !== 0) return null
      source = Buffer.from(source, "base64")
    }
    const key = crypto.createPublicKey(source)
    return key.asymmetricKeyType === "ed25519" ? key : null
  } catch {
    return null
  }
}

function exportPublicKey(value) {
  const key = importPublicKey(value)
  if (!key) return ""
  try {
    return key.export({ type: "spki", format: "pem" }).toString()
  } catch {
    return ""
  }
}

/**
 * Return the exact bytes covered by a plugin signature.
 *
 * Property order and the explicit empty digest are intentional. They make the
 * payload independent of manifest field order and keep signatures over an
 * unsigned/legacy digest unambiguous. Callers should pass a validated manifest
 * (the registry passes its canonical manifest plus the computed digest).
 */
function canonicalPayload(value, version, entrySha256) {
  let source
  if (value && typeof value === "object" && !Buffer.isBuffer(value)) {
    source = value
  } else {
    source = { id: value, version, entrySha256 }
  }
  const payload = {
    id: typeof source.id === "string" ? source.id : "",
    version: typeof source.version === "string" ? source.version : "",
    entrySha256:
      typeof source.entrySha256 === "string"
        ? source.entrySha256
        : typeof source.entry_digest === "string"
          ? source.entry_digest
          : "",
  }
  return JSON.stringify(payload)
}

function looksLikeSignature(value) {
  return Boolean(decodeSignature(value))
}

/**
 * Verify an Ed25519 signature. Supported forms are:
 *   verifySignature(manifest, signature, publicKey)
 *   verifySignature(manifest, publicKey, signature)
 *   verifySignature({ manifest, signature, publicKey })
 *   verifySignature({ payload, signature, publicKey })
 *
 * The return value is deliberately boolean so callers cannot accidentally
 * expose key material or filesystem details through a verification response.
 */
function verifySignature(first, second, third) {
  let payload
  let signature
  let publicKey

  if (first && typeof first === "object" && !Buffer.isBuffer(first)) {
    if (own(first, "payload")) payload = String(first.payload)
    else if (own(first, "manifest")) payload = canonicalPayload(first.manifest)
    else payload = canonicalPayload(first)
    signature = first.signature
    publicKey = first.publicKey ?? first.key
  } else {
    payload = typeof first === "string" && first.trim().startsWith("{") ? first : canonicalPayload(first)
    if (looksLikeSignature(second) && !looksLikeSignature(third)) {
      signature = second
      publicKey = third
    } else if (looksLikeSignature(third) && !looksLikeSignature(second)) {
      publicKey = second
      signature = third
    } else {
      // The common `(manifest, signature, publicKey)` form wins when both
      // values are malformed; it still returns false without throwing.
      signature = second
      publicKey = third
    }
  }

  const bytes = decodeSignature(signature)
  const key = importPublicKey(publicKey)
  if (!bytes || !key || typeof payload !== "string") return false
  try {
    return crypto.verify(null, Buffer.from(payload, "utf8"), key, bytes)
  } catch {
    return false
  }
}

function parseStore(value) {
  const state = { version: TRUST_STORE_VERSION, keys: {} }
  if (!value || typeof value !== "object" || Array.isArray(value)) return state
  if (value.version !== undefined && value.version !== TRUST_STORE_VERSION && value.version !== String(TRUST_STORE_VERSION)) {
    return state
  }
  let source = value.keys ?? value.trustedKeys ?? value.trusted_keys
  if (Array.isArray(source)) {
    source = Object.fromEntries(
      source
        .filter((item) => item && typeof item === "object" && !Array.isArray(item))
        .map((item) => [item.keyId ?? item.id, item.publicKey ?? item.key ?? item.pem]),
    )
  }
  if (!source || typeof source !== "object" || Array.isArray(source)) return state
  for (const [rawId, rawValue] of Object.entries(source).slice(0, MAX_TRUSTED_KEYS)) {
    const keyId = normalizeKeyId(rawId)
    const publicKey = exportPublicKey(
      typeof rawValue === "string"
        ? rawValue
        : rawValue && typeof rawValue === "object"
          ? rawValue.publicKey ?? rawValue.key ?? rawValue.pem
          : null,
    )
    if (keyId && publicKey) state.keys[keyId] = publicKey
  }
  return state
}

function readJson(file, fsImpl) {
  try {
    const stat = fsImpl.lstatSync(file)
    if (!stat.isFile() || stat.isSymbolicLink()) return null
    return JSON.parse(fsImpl.readFileSync(file, "utf8"))
  } catch {
    return null
  }
}

function atomicWrite(file, value, fsImpl = fsDefault) {
  const directory = path.dirname(file)
  fsImpl.mkdirSync(directory, { recursive: true, mode: 0o700 })
  try {
    const stat = fsImpl.lstatSync(directory)
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error("trust_store_diretorio_invalido")
  } catch (error) {
    if (error?.message === "trust_store_diretorio_invalido") throw error
    throw new Error("trust_store_diretorio_invalido")
  }
  const temporary = `${file}.${process.pid}.${Math.random().toString(16).slice(2)}.tmp`
  let fd = null
  try {
    const flags = fsImpl.constants?.O_WRONLY | fsImpl.constants?.O_CREAT | fsImpl.constants?.O_EXCL
    fd = fsImpl.openSync(temporary, flags || "wx", 0o600)
    fsImpl.writeFileSync(fd, `${JSON.stringify(value, null, 2)}\n`, "utf8")
    try { fsImpl.fsyncSync(fd) } catch {}
    fsImpl.closeSync(fd)
    fd = null
    fsImpl.renameSync(temporary, file)
    try { fsImpl.chmodSync(file, 0o600) } catch {}
  } catch (error) {
    if (fd !== null) {
      try { fsImpl.closeSync(fd) } catch {}
    }
    try { fsImpl.rmSync(temporary, { force: true }) } catch {}
    throw error
  }
}

function resolveStorePath(options = {}) {
  if (typeof options === "string") return path.resolve(options)
  const explicit = options.storePath ?? options.trustStorePath ?? options.filePath ?? options.file ?? options.path
  if (typeof explicit === "string" && explicit.trim()) return path.resolve(explicit)
  if (typeof options.dataDir === "string" && options.dataDir.trim()) {
    return path.join(path.resolve(options.dataDir), "plugins", TRUST_STORE_FILENAME)
  }
  const dataDir = process.env.ARCADIA_DATA_DIR
  if (typeof dataDir === "string" && dataDir.trim()) {
    return path.join(path.resolve(dataDir), "plugins", TRUST_STORE_FILENAME)
  }
  return path.join(os.homedir(), ".local", "share", "arcadia", "plugins", TRUST_STORE_FILENAME)
}

function publicKeys(state) {
  return Object.entries(state.keys).map(([keyId, publicKey]) => ({ keyId, publicKey }))
}

function createTrustStore(options = {}) {
  const fsImpl = options.fsImpl || fsDefault
  const storePath = resolveStorePath(options)
  if (!storePath || storePath.includes("\u0000")) throw new TypeError("trustStorePath inválido")
  let cache = null

  function state() {
    // Read on every operation so a trust change made by another process is
    // observed without requiring an Electron restart.
    const parsed = parseStore(readJson(storePath, fsImpl))
    cache = parsed
    return parsed
  }

  function write(next) {
    atomicWrite(storePath, { version: TRUST_STORE_VERSION, keys: next.keys }, fsImpl)
    cache = next
  }

  function add(keyIdOrValue, publicKeyValue) {
    const value = keyIdOrValue && typeof keyIdOrValue === "object" && !Array.isArray(keyIdOrValue)
      ? keyIdOrValue
      : null
    const keyId = normalizeKeyId(value ? value.keyId ?? value.id : keyIdOrValue)
    const publicKey = exportPublicKey(value ? value.publicKey ?? value.key ?? value.pem : publicKeyValue)
    if (!keyId || !publicKey) return { ok: false, error: "chave_invalida" }
    const next = state()
    if (!next.keys[keyId] && Object.keys(next.keys).length >= MAX_TRUSTED_KEYS) {
      return { ok: false, error: "trust_store_cheio" }
    }
    next.keys[keyId] = publicKey
    try {
      write(next)
      return { ok: true, keyId }
    } catch {
      cache = null
      return { ok: false, error: "trust_store_nao_gravavel" }
    }
  }

  function remove(keyIdOrValue) {
    const keyId = normalizeKeyId(
      keyIdOrValue && typeof keyIdOrValue === "object" ? keyIdOrValue.keyId ?? keyIdOrValue.id : keyIdOrValue,
    )
    if (!keyId) return { ok: false, error: "chave_invalida" }
    const next = state()
    if (!own(next.keys, keyId)) return { ok: false, error: "chave_desconhecida" }
    delete next.keys[keyId]
    try {
      write(next)
      return { ok: true, keyId }
    } catch {
      cache = null
      return { ok: false, error: "trust_store_nao_gravavel" }
    }
  }

  function list() {
    return publicKeys(state()).map((item) => ({ ...item }))
  }

  function get(keyId) {
    const normalized = normalizeKeyId(keyId)
    if (!normalized) return null
    return state().keys[normalized] || null
  }

  function has(keyId) {
    return Boolean(get(keyId))
  }

  function verify(keyId, manifestOrPayload, signature) {
    const publicKey = get(keyId)
    if (!publicKey) return false
    return verifySignature(manifestOrPayload, signature, publicKey)
  }

  return Object.freeze({
    add,
    addKey: add,
    remove,
    removeKey: remove,
    list,
    get,
    getPublicKey: get,
    has,
    verify,
    verifySignature,
    // This is intentionally not exposed through list/add/remove responses;
    // it is only useful to registry construction and focused tests.
    invalidate: () => { cache = null },
  })
}

module.exports = {
  TRUST_STORE_VERSION,
  TRUST_STORE_FILENAME,
  TRUST_STORE_FILE: TRUST_STORE_FILENAME,
  MAX_TRUSTED_KEYS,
  MAX_KEY_ID_LENGTH,
  normalizeKeyId,
  canonicalPayload,
  verifySignature,
  verifySignatureDetailed: (manifestOrOptions, signature, publicKey) => ({
    ok: verifySignature(manifestOrOptions, signature, publicKey),
    valid: verifySignature(manifestOrOptions, signature, publicKey),
  }),
  importPublicKey,
  exportPublicKey,
  parseStore,
  atomicWrite,
  resolveStorePath,
  createTrustStore,
  TrustStore: createTrustStore,
}
