"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const read = (...parts) => fs.readFileSync(path.join(root, ...parts), "utf8")

test("ponte aditiva do catálogo Retro permanece no main/preload/tipos", () => {
  const main = read("electron", "main.js")
  const preload = read("electron", "preload.js")
  const types = read("src", "global.d.ts")
  const store = read("src", "components", "desktop", "StoreView.tsx")
  const retro = read("src", "components", "desktop", "RetroStoreView.tsx")
  const artwork = read("src", "components", "desktop", "retroArtwork.ts")
  const launcher = read("src", "components", "desktop", "DesktopLauncher.tsx")
  assert.match(main, /ipcMain\.handle\("retro:list"/)
  assert.match(main, /ipcMain\.handle\("retro:game"/)
  assert.match(preload, /retroList:.*retro:list/)
  assert.match(preload, /retroGame:.*retro:game/)
  assert.match(preload, /retroOffer:.*retro:offer/)
  assert.match(types, /retroList:/)
  assert.match(types, /retroGame:/)
  assert.match(store, /RetroStoreView/)
  assert.match(store, /"steam" \| "retro"/)
  assert.match(retro, /retroList/)
  assert.match(retro, /retroGame/)
  assert.match(retro, /torrentStart/)
  assert.match(retro, /onOpenDownloads/)
  assert.match(artwork, /loadRetroCovers/)
  assert.doesNotMatch(artwork, /searchArt/)
  assert.match(artwork, /protocol !== "https:"/)
  assert.match(launcher, /onOpenDownloads=.*setView\("downloads"\)/)
})

test("downloads Retro usam o diálogo e a pasta compartilhados", () => {
  const retro = read("src", "components", "desktop", "RetroStoreView.tsx")
  assert.match(retro, /MetodoDownloadDialog/)
  assert.match(retro, /makeRetroDownloadChoice/)
  assert.match(retro, /depotDisponivel=\{false\}/)
  assert.match(retro, /opcoes: OpcaoTorrent\[\]/)
  assert.match(retro, /savePath,/) // selected folder is forwarded to torrent:start
  assert.match(retro, /magnet: uri/)
  assert.match(retro, /sourceTitle \|\| game\.sourceId/)
  assert.match(retro, /cover: getRetroCover\(game\)/)
})

test("detalhe Retro expõe remoção da biblioteca", () => {
  const retro = read("src", "components", "desktop", "RetroStoreView.tsx")
  const preload = read("electron", "preload.js")
  const main = read("electron", "main.js")
  assert.match(preload, /retroLibraryRemove:.*retro:libraryRemove/)
  assert.match(main, /ipcMain\.handle\("retro:libraryRemove"/)
  assert.match(retro, /retroLibraryRemove/)
  assert.match(retro, /onLibraryChanged/)
})

test("remoção Retro aceita entradas legadas salvas como launcher custom", () => {
  const main = read("electron", "main.js")
  const start = main.indexOf('ipcMain.handle("retro:libraryRemove"')
  const handler = main.slice(start, start + 1400)
  assert.ok(start >= 0)
  assert.match(handler, /\^retro:/)
  assert.match(handler, /game\.id !== value/)
  assert.doesNotMatch(handler, /game\.launcher === "retro"/)
})

test("detalhe Retro aberto pela Library não busca a grade", () => {
  const retro = read("src", "components", "desktop", "RetroStoreView.tsx")
  // A guarda de 4.1: com initialGameId, retro:list não é chamado.
  assert.match(retro, /if \(initialGameId\) return/)
})
