"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { EventEmitter } = require("node:events")
const fs = require("node:fs")
const path = require("node:path")
const { resolveLauncherMode, ignoreBrokenPipe } = require("../electron/startup")

const root = path.join(__dirname, "..")

test("AppImage inicia em desktop sem modo explícito e preserva console explícito", () => {
  assert.equal(resolveLauncherMode({}), "desktop")
  assert.equal(resolveLauncherMode({ PS5_FULLSCREEN: "1" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "console" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "desktop", PS5_FULLSCREEN: "1" }), "desktop")
})

test("stdout fechado não derruba o main, mas outros erros continuam visíveis", () => {
  const stream = new EventEmitter()
  ignoreBrokenPipe(stream)
  assert.doesNotThrow(() => stream.emit("error", Object.assign(new Error("closed"), { code: "EPIPE" })))
  assert.throws(() => stream.emit("error", Object.assign(new Error("disk"), { code: "EIO" })), /disk/)
})

test("preload e renderer mantêm desktop como fallback", () => {
  const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
  const renderer = fs.readFileSync(path.join(root, "src", "main.tsx"), "utf8")
  assert.match(preload, /ARCADIA_MODE \|\| "desktop"/)
  assert.match(renderer, /launcherMode as any\) \|\| "desktop"/)
})
