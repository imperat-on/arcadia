// Testes do sync de conquistas — partes puras (sem rede).
// normalizeTs, enqueue (dedupe earliest-wins), applyPulled (merge local) e
// isRetryable. Roda em Node puro; ARCADIA_DATA_DIR aponta pra pasta temporária.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// Precisa ser definido ANTES do require (DATA_DIR é lido no load do módulo).
const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-sync-"))
process.env.ARCADIA_DATA_DIR = DIR

const sync = require("../electron/supabase/sync.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

// ---------- normalizeTs ----------
test("normalizeTs: segundos (10 dígitos) passam direto", () => {
  assert.equal(sync.normalizeTs(1700000000), 1700000000)
  assert.equal(sync.normalizeTs("1700000000"), 1700000000)
})

test("normalizeTs: milissegundos (13 dígitos) viram segundos", () => {
  assert.equal(sync.normalizeTs(1700000000000), 1700000000)
  assert.equal(sync.normalizeTs(1785885386674), 1785885386)
})

test("normalizeTs: inválidos/null/zero devolvem null", () => {
  assert.equal(sync.normalizeTs(null), null)
  assert.equal(sync.normalizeTs(0), null)
  assert.equal(sync.normalizeTs(-5), null)
  assert.equal(sync.normalizeTs("abc"), null)
})

// ---------- enqueue (dedupe earliest-wins) ----------
test("enqueue: dedupe por (appid, apiname) mantém o MENOR unlocked_at", () => {
  sync.enqueue([{ appid: "730", apiname: "ach_01", unlocked_at: 1700000100 }])
  sync.enqueue([{ appid: "730", apiname: "ach_01", unlocked_at: 1700000050 }]) // mais cedo
  sync.enqueue([{ appid: "730", apiname: "ach_02", unlocked_at: 1700000200 }])
  const q = sync.enqueue([])
  assert.equal(q.length, 2)
  const a1 = q.find((i) => i.apiname === "ach_01")
  assert.equal(a1.unlocked_at, 1700000050, "deveria manter o desbloqueio mais cedo")
})

test("enqueue: itens sem apiname são ignorados", () => {
  sync.enqueue([{ appid: "730", unlocked_at: 1 }])
  const q = sync.enqueue([])
  assert.ok(!q.some((i) => !i.apiname))
})

test("enqueue: fila persiste em disco (sync_queue.json)", () => {
  const p = path.join(DIR, "sync_queue.json")
  assert.ok(fs.existsSync(p))
  const q = JSON.parse(fs.readFileSync(p, "utf8"))
  assert.ok(Array.isArray(q) && q.length > 0)
})

// ---------- applyPulled (merge local) ----------
function escreveAchievements(store) {
  fs.writeFileSync(path.join(DIR, "achievements.json"), JSON.stringify(store))
}

test("applyPulled: desbloqueado no servidor e não local → marca", () => {
  escreveAchievements({
    "730": { at: 1, items: [{ apiname: "ach_x", title: "X", achieved: false, unlock: 0 }] },
  })
  sync.applyPulled([{ appid: "730", apiname: "ach_x", unlocked_at: "2023-11-14T22:13:20Z" }])
  const store = JSON.parse(fs.readFileSync(path.join(DIR, "achievements.json"), "utf8"))
  const it = store["730"].items[0]
  assert.equal(it.achieved, true)
  assert.equal(it.unlock, 1700000000)
})

test("applyPulled: local já desbloqueado ANTES → mantém local (earliest wins)", () => {
  escreveAchievements({
    "730": { at: 1, items: [{ apiname: "ach_y", achieved: true, unlock: 1700000000 }] },
  })
  sync.applyPulled([{ appid: "730", apiname: "ach_y", unlocked_at: "2024-01-01T00:00:00Z" }])
  const store = JSON.parse(fs.readFileSync(path.join(DIR, "achievements.json"), "utf8"))
  assert.equal(store["730"].items[0].unlock, 1700000000, "não deve regredir o unlock local")
})

test("applyPulled: apiname desconhecido localmente → cria item mínimo", () => {
  escreveAchievements({})
  sync.applyPulled([{ appid: "999", apiname: "novo_ach", unlocked_at: "2023-11-14T22:13:20Z" }])
  const store = JSON.parse(fs.readFileSync(path.join(DIR, "achievements.json"), "utf8"))
  const it = store["999"]?.items?.[0]
  assert.ok(it, "item deveria existir")
  assert.equal(it.apiname, "novo_ach")
  assert.equal(it.achieved, true)
})

// ---------- isRetryable ----------
test("isRetryable: erro RLS/permission denied NÃO retry (bug de código)", () => {
  assert.equal(sync.isRetryable({ message: "permission denied for table profiles" }), false)
  assert.equal(sync.isRetryable({ message: "new row violates row-level security policy", code: "42501" }), false)
})

test("isRetryable: rede/timeout/429 retry", () => {
  assert.equal(sync.isRetryable({ message: "Failed to fetch" }), true)
  assert.equal(sync.isRetryable({ message: "fetch failed: ECONNREFUSED" }), true)
  assert.equal(sync.isRetryable({ status: 429 }), true)
})
