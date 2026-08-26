"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { EventEmitter } = require("node:events")
const fs = require("node:fs")
const path = require("node:path")
const { resolveLauncherMode, ignoreBrokenPipe } = require("../electron/startup")

const root = path.join(__dirname, "..")

test("resolve modo respeita modo explícito antes dos fallbacks", () => {
  assert.equal(resolveLauncherMode({}), "desktop")
  assert.equal(resolveLauncherMode({ PS5_FULLSCREEN: "1" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "console" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "desktop", PS5_FULLSCREEN: "1" }), "desktop")
})

test("forçar desktop e preferência vencem o fullscreen legado", () => {
  assert.equal(resolveLauncherMode({ ARCADIA_FORCE_DESKTOP: "1", PS5_FULLSCREEN: "1" }), "desktop")
  assert.equal(resolveLauncherMode({ PS5_FULLSCREEN: "1" }, { start_in_console_mode: false }), "desktop")
  assert.equal(resolveLauncherMode({}, { start_in_console_mode: true }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "console", ARCADIA_FORCE_DESKTOP: "1" }), "console")
})

test("stdout fechado não derruba o main, mas outros erros continuam visíveis", () => {
  const stream = new EventEmitter()
  ignoreBrokenPipe(stream)
  assert.doesNotThrow(() => stream.emit("error", Object.assign(new Error("closed"), { code: "EPIPE" })))
  assert.throws(() => stream.emit("error", Object.assign(new Error("disk"), { code: "EIO" })), /disk/)
})

test("preload e contexto mantêm desktop como fallback e isolam a seed do modo", () => {
  const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
  const renderer = fs.readFileSync(path.join(root, "src", "main.tsx"), "utf8")
  const context = fs.readFileSync(path.join(root, "src", "components", "ModeContext.tsx"), "utf8")
  assert.match(preload, /ARCADIA_MODE \|\| "desktop"/)
  assert.match(renderer, /ModeProvider/)
  assert.match(renderer, /modeRef\.current === "console" \? "desktop" : "console"/)
  assert.match(context, /window\.launcherMode === "console" \? "console" : "desktop"/)
  assert.equal((renderer.match(/launcherMode/g) || []).length, 0)
})

test("main resolve a preferência antes de criar a janela e mantém o shell trocável", () => {
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  assert.match(main, /resolveLauncherMode\(process\.env, readConfig\(\)\)/)
  assert.match(main, /fullscreen: launcherMode === "console"/)
  assert.match(main, /frame: false/)
  assert.match(main, /ipcMain\.handle\("app:setMode"/)
})
