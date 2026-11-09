// Gera a ponte launcherAPI a partir de app/electron/preload.js (congelado).
// Saídas:
//   - ui/src/launcher-api.gen.ts        (bridge Tauri, mesmas assinaturas)
//   - shim/ipc-manifest.json            (canais invoke/evento)
//   - crates/arcadia-ipc/src/channels.rs (listas p/ teste de cobertura)
"use strict"

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const preloadPath = path.join(root, "app", "electron", "preload.js")
const source = fs.readFileSync(preloadPath, "utf8")

if (!/^const \{ contextBridge, ipcRenderer \} = require\("electron"\)/m.test(source)) {
  throw new Error("preload.js: linha de require do electron não encontrada no formato esperado")
}
const extraRequires = source
  .split("\n")
  .filter((line) => /require\(/.test(line) && !/^\s*\/\//.test(line) && !line.includes('require("electron")'))
if (extraRequires.length) {
  throw new Error(`preload.js: require inesperado:\n${extraRequires.join("\n")}`)
}

const uniq = (values) => [...new Set(values)]
const matchAll = (re) => [...source.matchAll(re)].map((m) => m[1])

const invokeChannels = uniq(matchAll(/ipcRenderer\.invoke\(\s*"([^"]+)"/g))
const eventChannels = uniq(matchAll(/ipcRenderer\.on\(\s*"([^"]+)"/g))

if (invokeChannels.length < 100) {
  throw new Error(`preload.js: só ${invokeChannels.length} canais invoke; extração suspeita`)
}

const header = `// AUTO-GERADO por tools/gen-ipc.mjs a partir de app/electron/preload.js.
// Não editar à mão. O contrato é o preload congelado do Electron.
import { invoke as __tauriInvoke } from "@tauri-apps/api/core"
import { listen as __tauriListen } from "@tauri-apps/api/event"

type __Handler = (event: unknown, payload: unknown) => void

const __boot = ((globalThis as any).__ARCADIA_BOOT__ || {}) as {
  platform?: string
  home?: string
  localAppData?: string
  dataDir?: string
  mode?: string
}

const __listeners = new Map<__Handler, { channel: string; off: () => void }>()

const ipcRenderer = {
  // Tauri recebe (channel, args[]); o dispatcher Rust interpreta o array.
  invoke: (channel: string, ...args: unknown[]) => __tauriInvoke("arcadia_ipc", { channel, args }),
  on: (channel: string, handler: __Handler) => {
    const entry = { channel, off: () => {} }
    __listeners.set(handler, entry)
    void __tauriListen(channel, (event: { payload: unknown }) => handler({}, event.payload)).then((off: () => void) => {
      // Pode ter sido removido antes do listen resolver.
      if (__listeners.get(handler) === entry) entry.off = off
      else off()
    })
  },
  removeListener: (channel: string, handler: __Handler) => {
    const entry = __listeners.get(handler)
    if (!entry || entry.channel !== channel) return
    entry.off()
    __listeners.delete(handler)
  },
}

const contextBridge = {
  exposeInMainWorld: (name: string, value: unknown) => {
    ;(globalThis as any)[name] = value
  },
}

// Polyfill mínimo do preload (o preload sandboxed usava só estes campos).
const process = {
  platform: __boot.platform || "win32",
  env: {
    HOME: __boot.home || "",
    USERPROFILE: __boot.home || "",
    LOCALAPPDATA: __boot.localAppData || "",
    ARCADIA_DATA_DIR: __boot.dataDir || "",
    ARCADIA_MODE: __boot.mode || "desktop",
  } as Record<string, string | undefined>,
  versions: {} as Record<string, string | undefined>,
}
`

const body = source
  .replace(/^const \{ contextBridge, ipcRenderer \} = require\("electron"\)\r?\n/m, "")
  .replace(/^[ \t]*\/\/[^\n]*require\("os"\/"path"\)[^\n]*\r?\n/m, "")

const bridge = `${header}\n${body}\nexport {}\n`

const manifest = {
  generated_from: "app/electron/preload.js",
  invoke: invokeChannels,
  events: eventChannels,
}

const rustList = (name, items) =>
  `pub const ${name}: &[&str] = &[\n${items.map((item) => `    ${JSON.stringify(item)},`).join("\n")}\n];\n`

const channelsRs = `// AUTO-GERADO por tools/gen-ipc.mjs. Não editar à mão.\n\n${rustList(
  "INVOKE_CHANNELS",
  invokeChannels,
)}\n${rustList("EVENT_CHANNELS", eventChannels)}`

fs.mkdirSync(path.join(root, "shim"), { recursive: true })
fs.mkdirSync(path.join(root, "crates", "arcadia-ipc", "src"), { recursive: true })
fs.writeFileSync(path.join(root, "shim", "ipc-manifest.json"), JSON.stringify(manifest, null, 2) + "\n")
fs.writeFileSync(path.join(root, "ui", "src", "launcher-api.gen.ts"), bridge)
fs.writeFileSync(path.join(root, "crates", "arcadia-ipc", "src", "channels.rs"), channelsRs)

console.log(`invoke channels: ${invokeChannels.length}`)
console.log(`event channels:  ${eventChannels.length}`)
console.log("escrito: ui/src/launcher-api.gen.ts, shim/ipc-manifest.json, crates/arcadia-ipc/src/channels.rs")
