"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
const types = fs.readFileSync(path.join(root, "src", "global.d.ts"), "utf8")

test("IPC de emuladores monta catálogo e perfis sem shell", () => {
  for (const channel of [
    "emulators:list",
    "emulators:detect",
    "emulators:profiles",
    "emulators:profile:set",
    "emulators:profile:remove",
    "emulators:resolve",
  ])
    assert.ok(main.includes(`ipcMain.handle("${channel}"`), channel)
  assert.match(main, /emulatorRegistry\.resolveLaunch/)
  const registry = fs.readFileSync(path.join(root, "electron", "emulator-registry.js"), "utf8")
  assert.doesNotMatch(main, /emulators:.*exec|emulators:.*spawn/i)
  assert.doesNotMatch(registry, /child_process|execFile|spawn\s*\(/)
})

test("preload/types expõem ponte explícita de emuladores", () => {
  for (const method of [
    "emulatorsList",
    "emulatorsDetect",
    "emulatorsProfiles",
    "emulatorProfileSet",
    "emulatorProfileRemove",
    "emulatorsResolve",
  ])
    assert.match(preload, new RegExp(`${method}\\s*:`), method)
  assert.match(types, /export interface EmulatorProfile/)
  assert.match(types, /emulatorsList:/)
  assert.match(types, /emulatorsResolve:/)
})
