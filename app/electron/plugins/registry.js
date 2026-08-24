"use strict"

// Registro local de plugins. O registro guarda somente referências e estado de
// ativação; ele nunca baixa, descompacta ou executa código de terceiros.

const crypto = require("node:crypto")
const fsDefault = require("node:fs")
const path = require("node:path")
const {
  ENTRY_DIGEST_ALIASES,
  MANIFEST_FILE,
  MANIFEST_VERSION,
  normalizeId,
  validateId,
  validateManifest,
  readManifest,
  resolvePluginEntry,
  computeEntrySha256,
  publicManifest,
} = require("./manifest")
const {
  TRUST_STORE_FILENAME,
  canonicalPayload,
  verifySignature,
  createTrustStore,
} = require("./trust")

const REGISTRY_VERSION = 1
const REGISTRY_DIRNAME = "plugins"
const REGISTRY_FILENAME = "registry.json"
const LEGACY_REGISTRY_RELATIVE = path.join("bin", "plugins.json")
const MAX_REGISTRY_ENTRIES = 256

function nowMs() {
  return Date.now()
}

function clone(value) {
  return value && typeof value === "object" ? JSON.parse(JSON.stringify(value)) : value
}

function asTimestamp(value, fallback) {
  const number = Number(value)
  return Number.isFinite(number) && number >= 0 ? Math.trunc(number) : fallback
}

function normalizeStoredDigest(value) {
  if (typeof value !== "string") return ""
  const digest = value.trim().toLowerCase()
  return /^[0-9a-f]{64}$/.test(digest) ? digest : ""
}

function storedEntryDigest(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return ""
  for (const key of ENTRY_DIGEST_ALIASES) {
    const digest = normalizeStoredDigest(raw[key])
    if (digest) return digest
  }
  return ""
}

function digestMatches(expected, actual) {
  const left = normalizeStoredDigest(expected)
  const right = normalizeStoredDigest(actual)
  if (!left || !right) return false
  try {
    return crypto.timingSafeEqual(Buffer.from(left, "hex"), Buffer.from(right, "hex"))
  } catch {
    return false
  }
}

function safeStateRecord(id, raw, fallbackPath = "") {
  const normalized = normalizeId(id)
  if (!normalized || !raw || typeof raw !== "object" || Array.isArray(raw)) return null
  const pluginPath = typeof raw.path === "string" ? raw.path.trim() : fallbackPath
  // Built-ins intentionally have no package path. External package records
  // are required to carry one and are revalidated before any manifest read.
  // State is never allowed to become a path traversal primitive.
  if (pluginPath.includes("\u0000")) return null
  const updated = asTimestamp(raw.updatedAt, 0)
  const record = {
    id: normalized,
    path: pluginPath ? path.resolve(pluginPath) : "",
    enabled: raw.enabled === true,
    registeredAt: asTimestamp(raw.registeredAt, updated),
    updatedAt: updated,
  }
  const entrySha256 = storedEntryDigest(raw)
  if (entrySha256) record.entrySha256 = entrySha256
  return record
}

function emptyState() {
  return { version: REGISTRY_VERSION, plugins: {} }
}

function parseState(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return emptyState()
  // Version 1 has an explicit `plugins` envelope. Never interpret an unknown
  // version as a current state: this prevents future fields from being treated
  // as executable activation flags by an older host.
  if (value.version === REGISTRY_VERSION && value.plugins && typeof value.plugins === "object" && !Array.isArray(value.plugins)) {
    const state = emptyState()
    for (const [id, raw] of Object.entries(value.plugins).slice(0, MAX_REGISTRY_ENTRIES)) {
      const record = safeStateRecord(id, raw)
      if (record) state.plugins[record.id] = record
    }
    return state
  }
  return emptyState()
}

function parseLegacyState(value) {
  const records = {}
  if (!value || typeof value !== "object" || Array.isArray(value)) return records
  for (const [id, raw] of Object.entries(value).slice(0, MAX_REGISTRY_ENTRIES)) {
    const normalized = normalizeId(id)
    if (!normalized || !raw || typeof raw !== "object" || Array.isArray(raw)) continue
    // Legacy records intentionally carry no path; they only preserve enabled
    // state for built-in integrations already known to the host.
    records[normalized] = {
      id: normalized,
      path: "",
      enabled: raw.enabled === true,
      registeredAt: asTimestamp(raw.updatedAt, 0),
      updatedAt: asTimestamp(raw.updatedAt, 0),
    }
  }
  return records
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

function atomicWrite(file, value, fsImpl) {
  const directory = path.dirname(file)
  fsImpl.mkdirSync(directory, { recursive: true, mode: 0o700 })
  try {
    const directoryStat = fsImpl.lstatSync(directory)
    if (!directoryStat.isDirectory() || directoryStat.isSymbolicLink()) {
      throw new Error("diretorio_de_registro_invalido")
    }
  } catch (error) {
    if (error?.message === "diretorio_de_registro_invalido") throw error
    throw new Error("diretorio_de_registro_invalido")
  }
  // Remove a stale temporary symlink/file before creating a fresh one. rmSync
  // removes a symlink itself (it does not follow it), then O_EXCL below closes
  // the race for normal local filesystems.
  const temporary = `${file}.${process.pid}.${Math.random().toString(16).slice(2)}.tmp`
  let fd = null
  try {
    const flags = fsImpl.constants?.O_WRONLY | fsImpl.constants?.O_CREAT | fsImpl.constants?.O_EXCL
    fd = fsImpl.openSync(temporary, flags || "wx", 0o600)
    const text = JSON.stringify(value, null, 2)
    fsImpl.writeFileSync(fd, `${text}\n`, "utf8")
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

function builtinManifest(definition) {
  if (!definition || typeof definition !== "object") return null
  const source = definition.manifest || definition
  const result = validateManifest(
    {
      manifestVersion: source.manifestVersion ?? MANIFEST_VERSION,
      apiVersion: source.apiVersion ?? 1,
      id: source.id ?? definition.id,
      name: source.name ?? definition.name ?? definition.id,
      version: source.version ?? "0.0.0",
      description: source.description ?? "",
      // Built-ins are host integrations, not packages loaded from disk. They
      // therefore intentionally have no executable entry.
      entry: source.entry,
      entrySha256: source.entrySha256 ?? source.entry_sha256 ?? source.entryDigest ?? source.entry_digest ?? source.digest ?? source.sha256,
      permissions: source.permissions || [],
    },
    { requireEntry: false },
  )
  return result.ok ? result.manifest : null
}

function createPluginRegistry({
  dataDir,
  fsImpl = fsDefault,
  now = nowMs,
  builtins = [],
  legacyRegistryPath,
  trustStorePath,
  trustStore,
} = {}) {
  if (typeof dataDir !== "string" || !dataDir.trim()) throw new Error("dataDir é obrigatório")
  const root = path.resolve(dataDir)
  const registryDir = path.join(root, REGISTRY_DIRNAME)
  const registryPath = path.join(registryDir, REGISTRY_FILENAME)
  const legacyPath = legacyRegistryPath ? path.resolve(legacyRegistryPath) : path.join(root, LEGACY_REGISTRY_RELATIVE)
  const resolvedTrustStorePath = trustStorePath
    ? path.resolve(trustStorePath)
    : path.join(registryDir, TRUST_STORE_FILENAME)
  // A caller may inject a small in-memory/fake store for tests or a host
  // policy. The normal path is the local atomic trusted-keys.json store.
  const trusted = trustStore && typeof trustStore === "object"
    ? trustStore
    : createTrustStore({ storePath: resolvedTrustStorePath, fsImpl })
  const builtinMap = new Map()

  const definitions = Array.isArray(builtins) ? builtins : Object.values(builtins || {})
  for (const definition of definitions) {
    const manifest = builtinManifest(definition)
    if (!manifest || builtinMap.has(manifest.id)) continue
    builtinMap.set(manifest.id, {
      manifest,
      installed: typeof definition.installed === "function" ? definition.installed : () => true,
      source: "builtin",
      // A built-in path is informational only and is never loaded as an entry.
      path: typeof definition.path === "string" ? definition.path : "",
    })
  }

  let stateCache = null
  function readState() {
    if (stateCache) return stateCache
    const canonical = parseState(readJson(registryPath, fsImpl))
    const legacy = parseLegacyState(readJson(legacyPath, fsImpl))
    // Existing Arcadia installations only have bin/plugins.json. Merge its
    // enable flags for known built-ins without allowing it to register paths.
    for (const [id, record] of Object.entries(legacy)) {
      if (!canonical.plugins[id] && builtinMap.has(id)) canonical.plugins[id] = record
      else if (canonical.plugins[id] && record.updatedAt > canonical.plugins[id].updatedAt) {
        canonical.plugins[id].enabled = record.enabled
        canonical.plugins[id].updatedAt = record.updatedAt
      }
    }
    stateCache = canonical
    return stateCache
  }

  function invalidate() {
    stateCache = null
  }

  function writeState() {
    const state = readState()
    atomicWrite(registryPath, { version: REGISTRY_VERSION, plugins: state.plugins }, fsImpl)
    // Keep the old file as a compatibility mirror. It intentionally omits
    // package paths/manifests; older Arcadia versions only know enabled flags.
    const legacy = {}
    for (const [id, record] of Object.entries(state.plugins)) {
      legacy[id] = { enabled: record.enabled, updatedAt: record.updatedAt }
    }
    try { atomicWrite(legacyPath, legacy, fsImpl) } catch {
      // Canonical state remains authoritative. A read-only legacy path must not
      // make registration/activation fail for new hosts.
    }
  }

  function readPackage(pluginPath) {
    if (typeof pluginPath !== "string" || !pluginPath.trim() || pluginPath.includes("\u0000")) {
      return { ok: false, manifest: null, path: "", entry: "", errors: ["caminho_invalido"] }
    }
    let resolved = path.resolve(pluginPath)
    try {
      const stat = fsImpl.lstatSync(resolved)
      // Accepting either a package directory or its plugin.json makes the
      // registration API convenient for file pickers, while still requiring
      // the package root itself to be a real (non-symlink) directory.
      if (stat.isFile() && path.basename(resolved) === MANIFEST_FILE) resolved = path.dirname(resolved)
      else if (!stat.isDirectory() || stat.isSymbolicLink()) {
        return { ok: false, manifest: null, path: resolved, entry: "", errors: ["diretorio_invalido"] }
      }
      const rootStat = fsImpl.lstatSync(resolved)
      if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) {
        return { ok: false, manifest: null, path: resolved, entry: "", errors: ["diretorio_invalido"] }
      }
    } catch {
      return { ok: false, manifest: null, path: resolved, entry: "", errors: ["diretorio_nao_encontrado"] }
    }
    const parsed = readManifest(path.join(resolved, MANIFEST_FILE), { fsImpl })
    if (!parsed.ok) return { ok: false, manifest: null, path: resolved, entry: "", errors: parsed.errors }
    const checkedEntry = resolvePluginEntry(resolved, parsed.manifest.entry, { fsImpl })
    if (!checkedEntry.ok) return { ok: false, manifest: null, path: resolved, entry: "", errors: [checkedEntry.error] }
    const calculated = computeEntrySha256(checkedEntry.path, { fsImpl, root: resolved })
    if (!calculated.ok) {
      return {
        ok: false,
        manifest: parsed.manifest,
        path: resolved,
        entry: checkedEntry.path,
        entrySha256: "",
        expectedEntrySha256: parsed.manifest.entrySha256 || "",
        errors: [calculated.error],
      }
    }
    const expectedEntrySha256 = parsed.manifest.entrySha256 || ""
    if (expectedEntrySha256 && !digestMatches(expectedEntrySha256, calculated.digest)) {
      return {
        ok: false,
        manifest: parsed.manifest,
        path: resolved,
        entry: checkedEntry.path,
        entrySha256: calculated.digest,
        expectedEntrySha256,
        signatureDeclared: Boolean(parsed.manifest.signingKeyId && parsed.manifest.signature),
        errors: ["entry_digest_mismatch"],
      }
    }

    const signatureDeclared = Boolean(parsed.manifest.signingKeyId && parsed.manifest.signature)
    let signatureValid = false
    let signatureError = ""
    if (signatureDeclared) {
      let publicKey = null
      try {
        if (typeof trusted.getPublicKey === "function") publicKey = trusted.getPublicKey(parsed.manifest.signingKeyId)
        else if (typeof trusted.get === "function") publicKey = trusted.get(parsed.manifest.signingKeyId)
      } catch {}
      if (!publicKey) {
        signatureError = "assinatura_chave_nao_confiavel"
      } else {
        // Signatures bind the canonical identity, version, and the bytes that
        // were actually read. A declared digest has already been checked above;
        // using the calculated value also makes signed manifests without the
        // optional legacy digest tamper-evident.
        signatureValid = verifySignature(
          canonicalPayload({
            id: parsed.manifest.id,
            version: parsed.manifest.version,
            entrySha256: calculated.digest,
          }),
          parsed.manifest.signature,
          publicKey,
        )
        if (!signatureValid) signatureError = "assinatura_invalida"
      }
      if (!signatureValid) {
        return {
          ok: false,
          manifest: parsed.manifest,
          path: resolved,
          entry: checkedEntry.path,
          entrySha256: calculated.digest,
          expectedEntrySha256,
          signatureDeclared,
          signatureValid: false,
          signatureError,
          errors: [signatureError || "assinatura_invalida"],
        }
      }
    }
    return {
      ok: true,
      manifest: parsed.manifest,
      path: resolved,
      entry: checkedEntry.path,
      entrySha256: calculated.digest,
      expectedEntrySha256,
      signatureDeclared,
      signatureValid,
      signatureError,
      errors: [],
    }
  }

  function recordFor(id) {
    const normalized = normalizeId(id)
    if (!normalized) return null
    return readState().plugins[normalized] || null
  }

  function packageDescriptor(id, record) {
    const normalized = normalizeId(id)
    if (!normalized || !record || !record.path) return null
    const packageInfo = readPackage(record.path)
    if (!packageInfo.ok) {
      return {
        id: normalized,
        manifest: null,
        installed: false,
        enabled: false,
        valid: false,
        error: packageInfo.errors[0] || "manifest_invalido",
        source: "local",
      }
    }
    // Registry key and package manifest id must agree. This prevents a path
    // from being switched to a different plugin by editing one JSON file.
    if (packageInfo.manifest.id !== normalized) {
      return {
        id: normalized,
        manifest: null,
        installed: false,
        enabled: false,
        valid: false,
        error: "id_divergente",
        source: "local",
      }
    }
    // A registration snapshots the entry digest. This catches tampering even
    // when an attacker edits the optional manifest field along with the code.
    if (record.entrySha256 && !digestMatches(record.entrySha256, packageInfo.entrySha256)) {
      return {
        id: normalized,
        manifest: publicManifest(packageInfo.manifest),
        installed: false,
        enabled: false,
        valid: false,
        error: "entry_digest_mismatch",
        source: "local",
      }
    }
    return {
      id: normalized,
      manifest: publicManifest(packageInfo.manifest),
      installed: true,
      enabled: record.enabled === true,
      valid: true,
      error: "",
      source: "local",
    }
  }

  function builtinDescriptor(id, definition) {
    let installed = false
    try { installed = Boolean(definition.installed()) } catch {}
    const record = recordFor(id)
    return {
      id,
      manifest: publicManifest(definition.manifest),
      installed,
      enabled: installed && (record == null ? true : record.enabled !== false),
      // Preserve the historical behavior for built-ins: detected SLSsteam was
      // visible as enabled until explicitly disabled. The old registry has no
      // record for a first-run installation, so `record == null` means enabled.
      valid: true,
      error: "",
      source: "builtin",
    }
  }

  function descriptor(id) {
    const normalized = normalizeId(id)
    if (!normalized) return null
    const builtin = builtinMap.get(normalized)
    if (builtin) return builtinDescriptor(normalized, builtin)
    const record = recordFor(normalized)
    return record ? packageDescriptor(normalized, record) : null
  }

  function listDetailed() {
    const output = []
    for (const [id, definition] of builtinMap) output.push(builtinDescriptor(id, definition))
    for (const [id, record] of Object.entries(readState().plugins)) {
      if (builtinMap.has(id)) continue
      const item = packageDescriptor(id, record)
      if (item) output.push(item)
    }
    return output
  }

  function register(pluginPath, { enabled = false } = {}) {
    const packageInfo = readPackage(pluginPath)
    if (!packageInfo.ok) return { ok: false, error: packageInfo.errors[0] || "manifest_invalido", errors: packageInfo.errors }
    const id = packageInfo.manifest.id
    if (builtinMap.has(id)) return { ok: false, error: "id_reservado" }
    const existing = recordFor(id)
    if (existing && existing.path !== packageInfo.path) return { ok: false, error: "id_ja_registrado" }
    const timestamp = asTimestamp(now(), Date.now())
    const record = {
      id,
      path: packageInfo.path,
      enabled: Boolean(enabled),
      registeredAt: existing?.registeredAt || timestamp,
      updatedAt: timestamp,
      // Snapshot the bytes at registration. Manifests without a declared
      // digest remain valid v1 packages, but are still tamper-detectable after
      // this registration through the private registry snapshot.
      entrySha256: packageInfo.entrySha256,
    }
    readState().plugins[id] = record
    try {
      writeState()
    } catch {
      invalidate()
      return { ok: false, error: "registro_nao_gravavel" }
    }
    invalidate()
    const plugin = packageDescriptor(id, readState().plugins[id])
    return { ok: true, plugin: plugin ? { ...plugin, manifest: publicManifest(plugin.manifest) } : null }
  }

  function setEnabled(id, enabled, { allowUndetected = false } = {}) {
    const normalized = normalizeId(id)
    if (!normalized) return { ok: false, error: "plugin_invalido" }
    const builtin = builtinMap.get(normalized)
    if (builtin && !allowUndetected) {
      let installed = false
      try { installed = Boolean(builtin.installed()) } catch {}
      if (!installed) return { ok: false, error: "not_detected" }
    }
    const record = recordFor(normalized)
    if (!builtin && !record) return { ok: false, error: "plugin_desconhecido" }
    if (record) {
      if (!builtin) {
        const item = packageDescriptor(normalized, record)
        if (!item?.valid || !item.installed) return { ok: false, error: item?.error || "manifest_invalido" }
      }
      record.enabled = Boolean(enabled)
      record.updatedAt = asTimestamp(now(), Date.now())
    } else {
      // First-run built-in activation gets a state record. This is what keeps
      // the old install/remove API's explicit choice durable.
      const timestamp = asTimestamp(now(), Date.now())
      readState().plugins[normalized] = {
        id: normalized,
        path: "",
        enabled: Boolean(enabled),
        registeredAt: timestamp,
        updatedAt: timestamp,
      }
    }
    try {
      writeState()
    } catch {
      invalidate()
      return { ok: false, error: "registro_nao_gravavel" }
    }
    invalidate()
    return { ok: true }
  }

  function unregister(id) {
    const normalized = normalizeId(id)
    if (!normalized) return { ok: false, error: "plugin_invalido" }
    if (builtinMap.has(normalized)) return { ok: false, error: "plugin_embutido" }
    const state = readState()
    if (!state.plugins[normalized]) return { ok: false, error: "plugin_desconhecido" }
    delete state.plugins[normalized]
    try {
      writeState()
    } catch {
      invalidate()
      return { ok: false, error: "registro_nao_gravavel" }
    }
    invalidate()
    return { ok: true }
  }

  function get(id) {
    const item = descriptor(id)
    if (!item) return null
    return {
      id: item.id,
      manifest: publicManifest(item.manifest),
      installed: Boolean(item.installed),
      enabled: Boolean(item.enabled),
      valid: item.valid !== false,
      error: item.error || "",
      source: item.source,
    }
  }

  function verificationResult(id, packageInfo, record = null) {
    const normalized = normalizeId(id) || ""
    const declaredDigest = packageInfo.manifest?.entrySha256 || ""
    const expectedDigest = record?.entrySha256 || declaredDigest
    const actualDigest = packageInfo.entrySha256 || ""
    const verified = Boolean(expectedDigest)
    const valid = packageInfo.ok && (!verified || digestMatches(expectedDigest, actualDigest))
    let error = ""
    if (!packageInfo.ok) error = packageInfo.errors?.[0] || "entry_nao_verificavel"
    else if (verified && !digestMatches(expectedDigest, actualDigest)) error = "entry_digest_mismatch"
    const result = {
      id: normalized,
      ok: valid,
      valid,
      verified,
      declared: Boolean(declaredDigest),
      algorithm: "sha256",
      expectedDigest,
      actualDigest,
      // `digest` is the stable shorthand used by callers that only need the
      // computed value. It is never a path and is safe to return to the UI.
      digest: actualDigest,
      source: record?.entrySha256 ? "registry" : (declaredDigest ? "manifest" : "none"),
      error,
    }
    // Keep the v1 response shape byte-for-byte compatible for unsigned legacy
    // manifests. Signature metadata is emitted only for a manifest that
    // already declared both fields, and contains no package/path information.
    const signedManifest = packageInfo.manifest
    if (signedManifest?.signingKeyId && signedManifest?.signature) {
      result.signingKeyId = signedManifest.signingKeyId
      result.signature = signedManifest.signature
      result.signatureAlgorithm = "ed25519"
      result.signatureDeclared = true
      result.signatureVerified = packageInfo.signatureValid === true && packageInfo.ok === true
      result.signatureValid = result.signatureVerified
      if (!result.signatureVerified && !result.error) result.error = packageInfo.signatureError || "assinatura_invalida"
      if (!result.signatureVerified) {
        result.ok = false
        result.valid = false
      }
    }
    return result
  }

  function verify(id) {
    const normalized = normalizeId(id)
    if (!normalized) return verificationResult("", { ok: false, errors: ["plugin_invalido"] })
    const builtin = builtinMap.get(normalized)
    if (builtin) {
      return {
        id: normalized,
        ok: true,
        valid: true,
        verified: false,
        declared: false,
        algorithm: "sha256",
        expectedDigest: "",
        actualDigest: "",
        digest: "",
        source: "builtin",
        error: "",
      }
    }
    const record = recordFor(normalized)
    if (!record) return verificationResult(normalized, { ok: false, errors: ["plugin_desconhecido"] })
    return verificationResult(normalized, readPackage(record.path), record)
  }

  // Direct package verification is intentionally read-only and never imports
  // the entry. It is useful before registration and keeps paths out of the
  // result returned to a caller.
  function verifyPackage(pluginPath) {
    const packageInfo = readPackage(pluginPath)
    return verificationResult(packageInfo.manifest?.id || "", packageInfo)
  }

  function hasPermission(id, permission) {
    const item = descriptor(id)
    return Boolean(item?.valid && item.enabled && item.manifest?.permissions?.includes(permission))
  }

  function paths() {
    // Internal diagnostics/testing only. Never return this value through IPC.
    return { root, registryPath, legacyPath, trustStorePath: resolvedTrustStorePath }
  }

  return {
    register,
    unregister,
    setEnabled,
    get,
    listDetailed,
    hasPermission,
    verify,
    verifyIntegrity: verify,
    verifyPlugin: verify,
    verifyPackage,
    readPackage,
    trustStore: trusted,
    paths,
    invalidate,
  }
}

module.exports = {
  REGISTRY_VERSION,
  REGISTRY_DIRNAME,
  REGISTRY_FILENAME,
  createPluginRegistry,
  parseState,
  parseLegacyState,
  atomicWrite,
}
