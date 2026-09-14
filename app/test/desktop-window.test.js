"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

const root = path.join(__dirname, "..")
const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
const { fatorDeZoom, escalaAutomatica, escalaDaCapa } = require("../electron/ui-scale")

// main importa Electron e inicia serviços no load; executamos só a geometria
// real (tamanhoDaTela) com o ambiente de teste e compomos com a matemática da
// escala, que mora em electron/ui-scale.js — a MESMA composição que o main faz.
function tela({ width = 1600, height = 900, maximized = false, displaySize } = {}) {
  const size = displaySize || { width: 3840, height: 2160 }
  const display = { size, workAreaSize: size }
  const start = main.indexOf("function tamanhoDaTela(")
  const end = main.indexOf("let appliedZoomFactor", start)
  assert.ok(start >= 0 && end > start, "tamanhoDaTela presente no main")
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
    REFERENCIA: { width: 1920, height: 1080 },
  }
  vm.runInNewContext(main.slice(start, end), context)
  return context.tamanhoDaTela
}

// Compõe exatamente como o main: tamanho da tela → escala automática → fator.
const zoomFor = (opts) => {
  const tamanho = tela(opts)
  return (mode) => fatorDeZoom(mode, escalaAutomatica(tamanho(mode)))
}

const NEUTRO = { width: 1920, height: 1080, displaySize: { width: 1920, height: 1080 } }

test("main empacotado usa JavaScript válido, sem anotações TypeScript", () => {
  assert.doesNotThrow(() => new vm.Script(main, { filename: "electron/main.js" }))
})

test("escala automática: a referência 1920x1080 dá o tamanho de sempre", () => {
  const zoom = zoomFor(NEUTRO)
  // 1.0 automático = base da skin — o tamanho que o app sempre teve nessa tela.
  assert.equal(zoom("desktop"), 1.2)
  assert.equal(zoom("console"), 1.3)
})

test("não existe mais preferência de escala: quem decide é a tela", () => {
  // nem chave lida, nem IPC de escala manual, nem knobs antigos
  for (const chave of [
    "ui_scale",
    "card_scale",
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
  assert.doesNotMatch(main, /app:setUiScale/, "o IPC de escala manual foi removido")
  assert.doesNotMatch(main, /function zoomFactorFor/)
  // o único caminho de zoom é o helper que delega para o módulo
  assert.match(main, /function uiScaleFactor\(mode\)/)
  // e a válvula de escape existe, com o nome combinado
  assert.match(main, /ARCADIA_UI_SCALE/)
})

test("desktop acompanha a janela, não o tamanho físico do monitor", () => {
  const windowed = zoomFor()
  const resized = zoomFor({ width: 2560, height: 1440 })
  const maximized = zoomFor({ maximized: true })
  assert.equal(windowed("desktop"), 1.2)
  assert.ok(Math.abs(resized("desktop") - 1.6) < 0.00001)
  assert.ok(maximized("desktop") > resized("desktop"))
})

test("desktop não reduz controles em janelas menores nem exagera em ultrawide", () => {
  const smaller = zoomFor({ width: 1280, height: 720 })
  const ultrawide = zoomFor({ width: 3440, height: 1080 })
  assert.equal(smaller("desktop"), 1.2, "janela pequena nunca encolhe")
  assert.equal(ultrawide("desktop"), 1.2, "ultrawide limita pela altura")
})

test("console acompanha a tela e respeita o teto do fator", () => {
  const fullHD = zoomFor({ displaySize: { width: 1920, height: 1080 } })
  assert.equal(fullHD("console"), 1.3)
  // 4K puro: 1.3 × 1.7 = 2.21 (o teto generoso é justamente o que faltava)
  assert.equal(zoomFor()("console").toFixed(2), "2.21")
  assert.ok(zoomFor()("console") < 2.4, "nunca passa do teto do fator")
})

test("a capa do trilho sai da mesma escala da tela", () => {
  const capa = (opts) => escalaDaCapa(escalaAutomatica(tela(opts)("console")))
  assert.equal(capa(NEUTRO), 1.6, "na referência, o tamanho de sempre")
  assert.equal(capa({ displaySize: { width: 3840, height: 2160 } }), 1.9, "4K: capa maior")
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
