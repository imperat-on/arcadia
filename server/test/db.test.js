"use strict"

// Testes da Fase 1: schema SQLite + health.
// Roda com `node --test`. Usa DB temporario (DATA_DIR=/tmp).

const test = require("node:test")
const assert = require("node:assert")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// DB temporario para nao poluir o de producao
process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-test-"))

test("db.js carrega e cria as tabelas do schema", () => {
  const { db, RESERVED } = require("../src/db")

  const tables = db
    .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    .all()
    .map((r) => r.name)

  for (const t of [
    "profiles", "friendships", "user_achievements", "user_library",
    "user_playtime", "login_attempts", "reserved_usernames", "blocks",
  ]) {
    assert.ok(tables.includes(t), `tabela ${t} existe`)
  }

  assert.ok(RESERVED.includes("admin"), "reserved inclui admin")
  assert.ok(RESERVED.includes("arcadia"), "reserved inclui arcadia")
})

test("reserved_usernames seed aplicado", () => {
  const { db } = require("../src/db")
  const row = db
    .prepare("SELECT count(*) AS n FROM reserved_usernames")
    .get()
  assert.ok(row.n >= 14, `seed >= 14 (got ${row.n})`)
})

test("helpers de tempo retornam valores validos", () => {
  const { nowIso, nowEpochS } = require("../src/db")
  const iso = nowIso()
  assert.ok(!isNaN(Date.parse(iso)), "nowIso e ISO valido")
  const epoch = nowEpochS()
  assert.ok(epoch > 1_600_000_000, "nowEpochS e epoch segundos")
})
