const test = require("node:test")
const assert = require("node:assert/strict")
const { addSessionTrophy } = require("../src/components/sessionTrophies.ts")

const trophy = { appid: "570", key: "0|1", apiname: "FIRST", title: "Primeira missão", unlock: 102, notice: "native" }

test("sessão registra apenas troféus daquele jogo e intervalo, sem repetir a mesma conquista", () => {
  const items = new Map()
  const add = event => addSessionTrophy(items, "steam:570", 100, 110, event)
  assert.equal(add({ ...trophy, appid: "440" }), false)
  assert.equal(add({ ...trophy, unlock: 99 }), false)
  assert.equal(add({ ...trophy, unlock: 111 }), false)
  assert.equal(add({ ...trophy, notice: null }), false)
  assert.equal(add(trophy), true)
  assert.equal(add({ ...trophy, apiname: "first", key: "5|1" }), false)
  assert.deepEqual([...items.values()], [trophy])
})
