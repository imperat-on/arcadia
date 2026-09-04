"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createPluginRegistry } = require("../electron/plugins/registry")
const { createPluginSdk, PluginPermissionError } = require("../electron/plugins/sdk")

function tempPackage(root, manifest) {
  const directory = path.join(root, manifest.id)
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, "index.js"), "// test plugin\n")
  fs.writeFileSync(path.join(directory, "plugin.json"), JSON.stringify({ entry: "index.js", ...manifest }))
  return directory
}

function builtin(id, installed = () => true) {
  return {
    manifest: {
      manifestVersion: 1,
      apiVersion: 1,
      id,
      name: id,
      version: "1.0.0",
      permissions: ["library:read"],
    },
    installed,
  }
}

test("registro local valida, persiste e expõe só metadados seguros", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-registry-"))
  const packages = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-packages-"))
  try {
    const registry = createPluginRegistry({ dataDir, now: () => 1000 })
    const pluginPath = tempPackage(packages, {
      manifestVersion: 1,
      apiVersion: 1,
      id: "demo.plugin",
      name: "Demo",
      version: "1.0.0",
      permissions: ["library:read"],
    })
    const registered = registry.register(pluginPath)
    assert.equal(registered.ok, true)
    assert.equal(registered.plugin.path, undefined)
    assert.equal(registry.get("demo.plugin").enabled, false)
    assert.equal(registry.hasPermission("demo.plugin", "library:read"), false)
    assert.deepEqual(registry.setEnabled("demo.plugin", true), { ok: true })
    assert.equal(registry.hasPermission("demo.plugin", "library:read"), true)
    assert.equal(registry.hasPermission("demo.plugin", "filesystem:write"), false)
    const publicItem = registry.get("demo.plugin")
    assert.equal(publicItem.path, undefined)
    assert.deepEqual(publicItem.manifest.permissions, ["library:read"])

    const stateFile = path.join(dataDir, "plugins", "registry.json")
    const state = JSON.parse(fs.readFileSync(stateFile, "utf8"))
    assert.equal(state.version, 1)
    assert.equal(state.plugins["demo.plugin"].enabled, true)
    // NTFS nao tem bit POSIX; chmod 0600 e no-op no Windows.
    if (process.platform !== "win32") assert.equal((fs.statSync(stateFile).mode & 0o777), 0o600)

    const reopened = createPluginRegistry({ dataDir })
    assert.equal(reopened.get("demo.plugin").enabled, true)
    assert.equal(reopened.unregister("demo.plugin").ok, true)
    assert.equal(reopened.get("demo.plugin"), null)
    // Unregister never removes a user-owned package.
    assert.equal(fs.existsSync(path.join(pluginPath, "plugin.json")), true)
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true })
    fs.rmSync(packages, { recursive: true, force: true })
  }
})

test("registro migra flag legada apenas para built-ins", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-legacy-"))
  try {
    fs.mkdirSync(path.join(dataDir, "bin"), { recursive: true })
    fs.writeFileSync(
      path.join(dataDir, "bin", "plugins.json"),
      JSON.stringify({ built: { enabled: true, updatedAt: 20 } }),
    )
    const registry = createPluginRegistry({ dataDir, builtins: [builtin("built")] })
    const item = registry.get("built")
    assert.equal(item.enabled, true)
    assert.equal(item.installed, true)
    assert.equal(registry.paths().registryPath.endsWith(path.join("plugins", "registry.json")), true)
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true })
  }
})

test("registro rejeita pacote com manifesto inválido ou raiz symlink", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-invalid-"))
  const packages = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-invalid-pkg-"))
  try {
    const registry = createPluginRegistry({ dataDir })
    const bad = path.join(packages, "bad")
    fs.mkdirSync(bad)
    fs.writeFileSync(path.join(bad, "plugin.json"), JSON.stringify({
      manifestVersion: 1,
      apiVersion: 1,
      id: "bad",
      name: "Bad",
      version: "1.0.0",
      entry: "../outside.js",
    }))
    assert.equal(registry.register(bad).ok, false)
    const link = path.join(packages, "link")
    fs.symlinkSync(bad, link, "dir")
    const result = registry.register(link)
    assert.equal(result.ok, false)
    assert.equal(result.error, "diretorio_invalido")
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true })
    fs.rmSync(packages, { recursive: true, force: true })
  }
})

test("SDK verifica permissão no momento da chamada", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-sdk-"))
  const packages = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-plugin-sdk-pkg-"))
  try {
    const registry = createPluginRegistry({ dataDir })
    const pluginPath = tempPackage(packages, {
      manifestVersion: 1,
      apiVersion: 1,
      id: "sdk.demo",
      name: "SDK demo",
      version: "1.0.0",
      permissions: ["library:read"],
    })
    registry.register(pluginPath)
    const sdk = createPluginSdk({ registry, pluginId: "sdk.demo" })
    const read = sdk.capability("library:read", (value) => value + 1)
    assert.throws(() => read(1), (error) => error instanceof PluginPermissionError && error.code === "permission_denied")
    registry.setEnabled("sdk.demo", true)
    assert.equal(read(1), 2)
    assert.equal(sdk.hasPermission("filesystem:write"), false)
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true })
    fs.rmSync(packages, { recursive: true, force: true })
  }
})
