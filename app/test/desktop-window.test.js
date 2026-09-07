"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

const root = path.join(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")

// main importa Electron e inicia serviços no load; executamos só as funções
// de escala reais com a geometria fornecida pelo ambiente de teste.
function zoomFor({ width = 1600, height = 900, maximized = false, displaySize } = {}) {
  const size = displaySize || { width: 3840, height: 2160 }
  const display = { size, workAreaSize: size }
  const start = main.indexOf("function responsiveWindowScale(")
  const end = main.indexOf("let appliedZoomFactor", start)
  assert.ok(start >= 0 && end > start, "funções de escala presentes no main")
  const context = {
    win: {
      isDestroyed: () => false,
      getBounds: () => ({ width, height }),
      isMaximized: () => maximized,
    },
    screen: {
      getDisplayMatching: () => display,
      getPrimaryDisplay: () => display,
    },
  }
  vm.runInNewContext(main.slice(start, end), context)
  return context.zoomFactorFor
}

test("main empacotado usa JavaScript válido, sem anotações TypeScript", () => {
  assert.doesNotThrow(() => new vm.Script(main, { filename: "electron/main.js" }))
})

test("desktop mantém base legível mesmo com zoom legado reduzido", () => {
  const zoom = zoomFor()
  for (const legacy of [undefined, 0.7, 0.8, 0.9, 1, 1.1]) {
    assert.equal(zoom("desktop", legacy), 1.2)
  }
})

test("desktop acompanha a janela, não o tamanho físico do monitor", () => {
  const windowed = zoomFor()
  const resized = zoomFor({ width: 2560, height: 1440 })
  const maximized = zoomFor({ maximized: true })
  assert.equal(windowed("desktop", 1), 1.2)
  assert.ok(Math.abs(resized("desktop", 1) - 1.6) < 0.00001)
  assert.ok(maximized("desktop", 1) > resized("desktop", 1))
})

test("desktop não reduz controles em janelas menores nem exagera em ultrawide", () => {
  const smaller = zoomFor({ width: 1280, height: 720 })
  const ultrawide = zoomFor({ width: 3440, height: 1080 })
  assert.equal(smaller("desktop", 1), 1.2)
  assert.equal(ultrawide("desktop", 1), 1.2)
})

test("console preserva a preferência de zoom e o limite existente", () => {
  const fullHD = zoomFor({ displaySize: { width: 1920, height: 1080 } })
  assert.equal(fullHD("console", 0.9), 0.9)
  assert.equal(fullHD("console", 1.3), 1.3)
  assert.equal(zoomFor()("console", 1.3), 2)
})

test("desktop abre em janela normal e não oferece mais slider de zoom", () => {
  const start = main.indexOf("function createWindow()")
  const end = main.indexOf("win.loadFile(", start)
  assert.ok(start >= 0 && end > start)
  assert.doesNotMatch(main.slice(start, end), /win\.maximize\(\)/)
  assert.match(main.slice(start, end), /fullscreen: launcherMode === "console"/)
  const accessibility = fs.readFileSync(
    path.join(root, "src", "components", "desktop", "AccessibilityView.tsx"),
    "utf8",
  )
  assert.doesNotMatch(accessibility, /accessibility\.zoom|aplicarZoom|zoomDraft/)
  assert.match(accessibility, /accessibility\.fontes/)
})
