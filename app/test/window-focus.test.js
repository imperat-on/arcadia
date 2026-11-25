"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { restoreWindowFocus } = require("../electron/window-focus")

function fakeWindow({ minimized = false, visible = true } = {}) {
  const calls = []
  let isMin = minimized
  let isVisible = visible
  const value = {
    isDestroyed: () => false,
    isMinimized: () => isMin,
    isVisible: () => isVisible,
    restore: () => { calls.push("restore"); isMin = false },
    show: () => { calls.push("show"); isVisible = true },
    focus: (options) => calls.push(["focus", options]),
    webContents: { focus: () => calls.push("webContents.focus") },
  }
  return { value, calls }
}

test("restoreWindowFocus reativa janela minimizada/escondida e o renderer", () => {
  const { value, calls } = fakeWindow({ minimized: true, visible: false })
  let notified = 0

  assert.equal(restoreWindowFocus(value, { onFocused: () => notified++ }), true)
  assert.deepEqual(calls, [
    "restore",
    "show",
    ["focus", { steal: true }],
    "webContents.focus",
  ])
  assert.equal(notified, 1)
})

test("restoreWindowFocus não toca numa janela destruída", () => {
  const calls = []
  const value = { isDestroyed: () => true, focus: () => calls.push("focus") }
  assert.equal(restoreWindowFocus(value, { onFocused: () => calls.push("notify") }), false)
  assert.deepEqual(calls, [])
})

test("restoreWindowFocus também funciona quando a janela só perdeu foco", () => {
  const { value, calls } = fakeWindow()
  assert.equal(restoreWindowFocus(value), true)
  assert.deepEqual(calls, [["focus", { steal: true }], "webContents.focus"])
})

test("restoreWindowFocus respeita um guard de término confirmado", () => {
  const { value, calls } = fakeWindow()
  let confirmado = false
  assert.equal(restoreWindowFocus(value, { canRestore: () => confirmado }), false)
  assert.deepEqual(calls, [])
  confirmado = true
  assert.equal(restoreWindowFocus(value, { canRestore: () => confirmado }), true)
  assert.deepEqual(calls, [["focus", { steal: true }], "webContents.focus"])
})
