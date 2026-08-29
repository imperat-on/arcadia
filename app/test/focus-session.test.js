"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { createFocusSession, createLaunchSession } = require("../electron/focus-session")

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

test("launch session mantém o token ocupado até a parada confirmada", () => {
  const events = []
  const lifecycle = createLaunchSession({
    onState: (state, token, meta) => events.push({ state, id: token.id, gameId: meta?.gameId }),
  })
  const first = lifecycle.begin({ gameId: "custom:one" })
  assert.ok(first)
  assert.equal(lifecycle.begin({ gameId: "custom:two" }), null)
  assert.equal(lifecycle.markRunning(first), true)
  const stopped = lifecycle.requestStop(first)
  assert.equal(stopped.ok, true)
  assert.equal(lifecycle.isBusy(), true)
  assert.equal(lifecycle.markRunning(first), false)
  assert.equal(lifecycle.finish(first), true)
  const second = lifecycle.begin({ gameId: "custom:two" })
  assert.ok(second)
  assert.notEqual(second.id, first.id)
  assert.equal(lifecycle.finish(first), false)
  assert.deepEqual(events.map((event) => event.state), ["starting", "running", "stopping", "idle", "starting"])
  assert.equal(events[0].gameId, "custom:one")
})

test("launch session ignora callbacks de uma geração antiga", () => {
  const lifecycle = createLaunchSession()
  const first = lifecycle.begin()
  assert.equal(lifecycle.finish(first), true)
  const second = lifecycle.begin()
  assert.equal(lifecycle.markRunning(first), false)
  assert.equal(lifecycle.requestStop(first).ok, false)
  assert.equal(lifecycle.isRunning(second), false)
  assert.equal(lifecycle.finish(second), true)
})
