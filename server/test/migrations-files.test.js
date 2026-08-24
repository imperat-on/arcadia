"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { readMigrations, parseFileName, MIGRATIONS_DIR } = require("../src/migrations")


test("migrations inclui baseline e arquivos versionados em ordem", () => {
  const migrations = readMigrations()
  assert.ok(migrations.length >= 2)
  assert.equal(migrations[0].version, 1)
  assert.equal(migrations[0].name, "initial-schema")
  assert.equal(migrations[0].file, "schema.sql")
  assert.deepEqual(
    migrations.map((migration) => migration.version),
    [...migrations].sort((a, b) => a.version - b.version).map((migration) => migration.version),
  )
  assert.ok(migrations.every((migration) => migration.sql.trim().length > 0))
  assert.ok(migrations.some((migration) => migration.file === "0002_performance_indexes.sql"))
  assert.ok(migrations.some((migration) => migration.file === "0003_community.sql"))
})

test("parseFileName aceita nomes seguros e rejeita arquivos arbitrários", () => {
  assert.deepEqual(parseFileName("0007_add_reviews.sql"), {
    version: 7,
    name: "add_reviews",
    file: "0007_add_reviews.sql",
  })
  assert.equal(parseFileName("README.md"), null)
  assert.equal(parseFileName("../0007_escape.sql"), null)
  assert.deepEqual(parseFileName("0000_invalid.sql"), {
    version: 0,
    name: "invalid",
    file: "0000_invalid.sql",
  })
  assert.ok(MIGRATIONS_DIR.endsWith("server/sql/migrations"))
})
