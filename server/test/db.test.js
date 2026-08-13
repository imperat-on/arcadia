"use strict"

// Testes da Fase 1: schema PostgreSQL + health.
// Roda com `node --test`.

const test = require("node:test")
const assert = require("node:assert")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// DB temporario para nao poluir o de producao
process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-test-"))

test("db.js carrega e cria as tabelas do schema", async () => {
  const { db, initDb, RESERVED } = require("../src/db")

  await initDb()
  const tables = (await db.query(
    `SELECT table_name AS name
       FROM information_schema.tables
      WHERE table_schema = current_schema()
        AND table_type = 'BASE TABLE'
      ORDER BY table_name`,
  )).rows
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

test("reserved_usernames seed aplicado", async () => {
  const { db, initDb } = require("../src/db")
  await initDb()
  const row = (await db.query(
    "SELECT count(*)::integer AS n FROM reserved_usernames",
  )).rows[0]
  assert.ok(row.n >= 14, `seed >= 14 (got ${row.n})`)
})

test("helpers de tempo retornam valores validos", () => {
  const { nowIso, nowEpochS } = require("../src/db")
  const iso = nowIso()
  assert.ok(!isNaN(Date.parse(iso)), "nowIso e ISO valido")
  const epoch = nowEpochS()
  assert.ok(epoch > 1_600_000_000, "nowEpochS e epoch segundos")
})
