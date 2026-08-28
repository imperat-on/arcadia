"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { createFocusSession } = require("../electron/focus-session")

test("sessão publica false no launch e um único true no finalizer", () => {
  const events = []
  const gate = createFocusSession({ onFocus: (focused, forced) => events.push({ focused, forced }) })

  assert.equal(gate.begin(), true)
  assert.deepEqual(events, [{ focused: false, forced: true }])
  // O foco transitório do wrapper não pode liberar o renderer.
  assert.equal(gate.nativeFocus(true), false)
  assert.deepEqual(events, [{ focused: false, forced: true }])

  assert.equal(gate.finish(), true)
  assert.equal(gate.finish(), false)
  assert.deepEqual(events, [
    { focused: false, forced: true },
    { focused: true, forced: true },
  ])
  assert.equal(gate.isActive(), false)
})

test("foco nativo normal continua funcionando fora da sessão", () => {
  const events = []
  const gate = createFocusSession({ onFocus: (focused) => events.push(focused) })
  assert.equal(gate.nativeFocus(false), true)
  assert.equal(gate.nativeFocus(true), true)
  assert.deepEqual(events, [false, true])
})

test("segunda sessão pode começar depois do término", () => {
  const events = []
  const gate = createFocusSession({ onFocus: (focused) => events.push(focused) })
  assert.equal(gate.begin(), true)
  assert.equal(gate.finish(), true)
  assert.equal(gate.begin(), true)
  assert.deepEqual(events, [false, true, false])
})
