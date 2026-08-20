"use strict"

// O resolver não deve carregar Electron, filesystem ou rede. Estes testes
// exercitam convergência com os deltas chegando em ordens diferentes.
const test = require("node:test")
const assert = require("node:assert/strict")
const {
  normalizeSyncTimestamp,
  resolveAchievementConflict,
  resolveLibraryConflict,
  resolvePlaytimeConflict,
  resolveSyncConflict,
} = require("../../contracts")

test("resolver de sync normaliza datas sem depender do formato de origem", () => {
  assert.equal(normalizeSyncTimestamp("2024-01-01T00:00:00Z"), 1704067200)
  assert.equal(normalizeSyncTimestamp(1704067200000), 1704067200)
  assert.equal(normalizeSyncTimestamp("1704067200"), 1704067200)
  assert.equal(normalizeSyncTimestamp("nao-e-data"), null)
})

test("conquista: earliest-wins é comutativo e preserva metadata", () => {
  const a = { appid: "730", apiname: "ach", unlocked_at: 1700000200, title: "A" }
  const b = { appid: "730", apiname: "ach", unlocked_at: 1700000100, title: "B" }
  const ab = resolveAchievementConflict(a, b)
  const ba = resolveAchievementConflict(b, a)
  assert.deepEqual(ab, ba)
  assert.equal(ab.unlocked_at, 1700000100)
  assert.equal(ab.achieved, true)
})

test("biblioteca: revisão maior vence e empate de remoção não ressuscita jogo", () => {
  const add = { appid: "steam:10", title: "Portal", revision: 4 }
  const rename = { appid: "steam:10", title: "Portal 2", revision: 5 }
  assert.deepEqual(resolveLibraryConflict(add, rename), resolveLibraryConflict(rename, add))
  assert.equal(resolveLibraryConflict(add, rename).title, "Portal 2")

  const removed = { appid: "steam:10", removed: true, revision: 5 }
  const staleAdd = { appid: "steam:10", title: "Portal", revision: 5 }
  const merged = resolveLibraryConflict(removed, staleAdd)
  assert.equal(merged.removed, true)
  assert.equal(resolveSyncConflict("library", staleAdd, removed).removed, true)
})

test("biblioteca: título real vence placeholder quando backend antigo não tem versão", () => {
  const merged = resolveLibraryConflict(
    { appid: "steam:10", title: "Steam 10" },
    { appid: "steam:10", title: "Portal 2" },
  )
  assert.equal(merged.title, "Portal 2")
})

test("playtime: merge monotônico preserva o total maior", () => {
  assert.equal(resolvePlaytimeConflict({ minutes: 90 }, { minutes: "120" }), 120)
  assert.equal(resolvePlaytimeConflict({ minutes: 300 }, { minutes: 2 }), 300)
  assert.equal(resolveSyncConflict("playtime", 12, 30), 30)
  assert.equal(resolvePlaytimeConflict({ minutes: -1 }, { minutes: "ruim" }), 0)
})
