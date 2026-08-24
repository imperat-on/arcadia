"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
const types = fs.readFileSync(path.join(root, "src", "global.d.ts"), "utf8")

test("IPC de plugins mantém canais legados e registra a superfície v1", () => {
  for (const channel of [
    "plugins:list",
    "plugins:install",
    "plugins:remove",
    "plugins:details",
    "plugins:get",
    "plugins:register",
    "plugins:unregister",
    "plugins:enable",
    "plugins:disable",
    "plugins:verify",
  ]) {
    assert.match(main, new RegExp(`plugins:${channel.split(":")[1]}`), channel)
  }
  assert.match(main, /pluginIdFromIpc/)
  assert.match(main, /pluginPathFromIpc/)
  assert.match(main, /notifyPluginsChanged/)
})

test("preload expõe só wrappers explícitos e normaliza tipos de plugin", () => {
  for (const method of [
    "pluginsList",
    "pluginsInstall",
    "pluginsRemove",
    "pluginsDetails",
    "pluginsGet",
    "pluginsRegister",
    "pluginsUnregister",
    "pluginsEnable",
    "pluginsDisable",
    "pluginsVerify",
  ]) assert.match(preload, new RegExp(`${method}\\s*:`), method)
  assert.match(preload, /typeof pluginPath === "string" \? pluginPath : ""/)
  assert.match(preload, /contextBridge\.exposeInMainWorld\("launcherAPI"/)
  assert.doesNotMatch(preload, /exposeInMainWorld\([^,]+,\s*ipcRenderer\s*\)/)
})

test("tipos do renderer descrevem manifest/permissões sem expor path do registro", () => {
  assert.match(types, /export type PluginPermission =/)
  assert.match(types, /export interface PluginManifest/)
  assert.match(types, /pluginsDetails:/)
  assert.match(types, /pluginsRegister:/)
  assert.match(types, /pluginsEnable:/)
  assert.match(types, /PluginVerification/)
  assert.match(types, /pluginsVerify:/)
  assert.match(types, /signingKeyId\?: string/)
  assert.match(types, /signatureVerified\?: boolean/)
})
