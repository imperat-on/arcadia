"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

const root = path.join(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
const { fatorDeZoom } = require("../electron/ui-scale")

// main importa Electron e inicia serviços no load; executamos só a geometria
// real (responsiveWindowScale) com o ambiente de teste e compomos com a
// matemática da escala, que agora mora em electron/ui-scale.js — exatamente a
// mesma composição que o main faz.
function janela({ width = 1600, height = 900, maximized = false, displaySize } = {}) {
  const size = displaySize || { width: 3840, height: 2160 }
  const display = { size, workAreaSize: size }
  const start = main.indexOf("function responsiveWindowScale(")
  const end = main.indexOf("let appliedZoomFactor", start)
  assert.ok(start >= 0 && end > start, "responsiveWindowScale presente no main")
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
  return context.responsiveWindowScale
}

const zoomFor = (opts) => {
  const escala = janela(opts)
  return (mode, rel) => fatorDeZoom(mode, rel, escala(mode))
}

// Janela e tela em 1:1 com 1920×1080 → responsiveWindowScale() = 1, então o
// fator resultante é só base × preferência (o resto é testado à parte).
const NEUTRO = { width: 1920, height: 1080, displaySize: { width: 1920, height: 1080 } }

test("main empacotado usa JavaScript válido, sem anotações TypeScript", () => {
  assert.doesNotThrow(() => new vm.Script(main, { filename: "electron/main.js" }))
})

test("uma escala só: a preferência padrão (100%) usa a base de cada skin", () => {
  const zoom = zoomFor(NEUTRO)
  // 100% = base da skin — é o tamanho que o app já tinha antes da migração.
  assert.equal(zoom("desktop", 1), 1.2)
  assert.equal(zoom("console", 1), 1.3)
  // valor ausente/ inválido cai em 100%, nunca em zero
  assert.equal(zoom("desktop", undefined), 1.2)
  assert.equal(zoom("console", NaN), 1.3)
})

test("a MESMA chave passa a valer no desktop (antes o valor era ignorado)", () => {
  const zoom = zoomFor(NEUTRO)
  assert.ok(Math.abs(zoom("desktop", 0.85) - 1.02) < 1e-9)
  assert.ok(Math.abs(zoom("desktop", 1.15) - 1.38) < 1e-9)
  // e no console a preferência também manda
  assert.ok(Math.abs(zoom("console", 0.9) - 1.17) < 1e-9)
})

test("a preferência é presa na faixa 0.7–1.6", () => {
  const zoom = zoomFor(NEUTRO)
  assert.equal(zoom("desktop", 0.1), 0.84) // 1.2 × 0.7
  assert.equal(zoom("desktop", 99), 1.92) // 1.2 × 1.6
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

test("console acompanha a tela e respeita o teto de 2×", () => {
  const fullHD = zoomFor({ displaySize: { width: 1920, height: 1080 } })
  assert.equal(fullHD("console", 1), 1.3)
  assert.equal(zoomFor()("console", 1), 2) // 1.3 × 1.55 = 2.015 → teto
})

test("não existe mais um segundo knob de escala em lugar nenhum", () => {
  for (const chave of [
    "console_ui_scale",
    "desktop_scale_base_v2",
    "desktop_font_scale_v3",
    "big_picture_scale_defaults_v2",
    "big_picture_scale_defaults_v3",
  ]) {
    assert.doesNotMatch(
      main,
      new RegExp(`readConfig\\(\\)\\.${chave}|config\\.${chave}`),
      `${chave} não pode ser lida pelo main`,
    )
  }
  // o único caminho de zoom é o helper que delega para o módulo
  assert.match(main, /function uiScaleFactor\(mode\)/)
  assert.doesNotMatch(main, /function zoomFactorFor/)
})

test("desktop abre em janela normal e não oferece slider de zoom separado", () => {
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
