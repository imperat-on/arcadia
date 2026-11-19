// Testa a ponte gerada sem Rust: empacota ui/src/launcher-api.gen.ts com
// esbuild, substitui @tauri-apps/api por mocks e executa em Node.
//
// Prova:
//  - window.launcherAPI/frMode/frPlatform/frPaths existem após o import
//  - invoke("library:get") chega como arcadia_ipc { channel, args }
//  - onUiEscala assina "ui:escala" e o unsubscribe desliga
"use strict"

import assert from "node:assert/strict"
import { build } from "../ui/node_modules/esbuild/lib/main.js"
import { readFileSync, mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { pathToFileURL } from "node:url"

const root = path.resolve(import.meta.dirname, "..")
const manifest = JSON.parse(readFileSync(path.join(root, "shim", "ipc-manifest.json"), "utf8"))

const calls = []
const listened = []
const unlistened = []

const mockModules = {
  "@tauri-apps/api/core": `
    export function invoke(cmd, payload) {
      globalThis.__calls.push({ cmd, payload })
      return Promise.resolve(null)
    }
  `,
  "@tauri-apps/api/event": `
    export function listen(channel, handler) {
      globalThis.__listened.push(channel)
      globalThis.__handler = handler
      return Promise.resolve(() => { globalThis.__unlistened.push(channel) })
    }
  `,
}

const outdir = mkdtempSync(path.join(tmpdir(), "arcadia-bridge-"))
const outfile = path.join(outdir, "bundle.mjs")

await build({
  entryPoints: [path.join(root, "ui", "src", "launcher-api.gen.ts")],
  bundle: true,
  format: "esm",
  platform: "neutral",
  outfile,
  plugins: [
    {
      name: "mock-tauri",
      setup(buildApi) {
        buildApi.onResolve({ filter: /^@tauri-apps\/api\/(core|event)$/ }, (args) => ({
          path: args.path,
          namespace: "mock",
        }))
        buildApi.onLoad({ filter: /.*/, namespace: "mock" }, (args) => ({
          contents: mockModules[args.path],
          loader: "js",
        }))
      },
    },
  ],
})

globalThis.window = globalThis
globalThis.__ARCADIA_BOOT__ = {
  platform: "win32",
  home: "C:\\Users\\zes",
  localAppData: "C:\\Users\\zes\\AppData\\Local",
  dataDir: "C:\\Users\\zes\\AppData\\Local\\arcadia",
  mode: "desktop",
}
globalThis.__calls = calls
globalThis.__listened = listened
globalThis.__unlistened = unlistened

await import(pathToFileURL(outfile).href)

const api = globalThis.launcherAPI
assert.ok(api, "launcherAPI não foi exposta")

const methods = Object.entries(api).filter(([, value]) => typeof value === "function")
const invokeMethods = methods.filter(([name]) => !name.startsWith("on"))
const eventMethods = methods.filter(([name]) => name.startsWith("on"))

assert.equal(invokeMethods.length, manifest.invoke.length, "número de métodos invoke diverge do preload")
assert.equal(eventMethods.length, manifest.events.length, "número de métodos de evento diverge do preload")

await api.getLibrary()
assert.equal(calls.at(-1).cmd, "arcadia_ipc")
assert.deepEqual(calls.at(-1).payload, { channel: "library:get", args: [] })

await api.launch(["steam", "run", "1"], "1", "desktop")
assert.deepEqual(calls.at(-1).payload, {
  channel: "game:launch",
  args: [{ cmd: ["steam", "run", "1"], gameId: "1", mode: "desktop" }],
})

const off = api.onUiEscala(() => {})
assert.equal(listened.at(-1), "ui:escala")
// listen() resolve no próximo microtask
await new Promise((resolve) => setTimeout(resolve, 0))
off()
await new Promise((resolve) => setTimeout(resolve, 0))
assert.equal(unlistened.at(-1), "ui:escala")

assert.equal(globalThis.launcherPlatform, "win32")
assert.equal(globalThis.launcherPaths.dataDir, "C:\\Users\\zes\\AppData\\Local\\arcadia")
assert.equal(globalThis.launcherMode, "desktop")

console.log(`bridge ok: ${invokeMethods.length} invoke + ${eventMethods.length} eventos`)
