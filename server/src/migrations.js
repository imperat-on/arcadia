"use strict"

// Descoberta determinística das migrations do backend.
// O schema.sql histórico vira a migration baseline (v1), preservando o fluxo
// manual documentado. Mudanças novas entram em sql/migrations/0002_*.sql e
// nunca dependem de CREATE TABLE IF NOT EXISTS para alterar produção.
const fs = require("node:fs")
const path = require("node:path")

const SCHEMA_FILE = path.join(__dirname, "..", "sql", "schema.sql")
const MIGRATIONS_DIR = path.join(__dirname, "..", "sql", "migrations")
const MIGRATION_RE = /^(\d+)_([a-z0-9][a-z0-9_-]*)\.sql$/i

function parseFileName(file) {
  const match = MIGRATION_RE.exec(file)
  if (!match) return null
  return { version: Number(match[1]), name: match[2], file }
}

function readMigrations() {
  const baseline = {
    version: 1,
    name: "initial-schema",
    file: path.basename(SCHEMA_FILE),
    sql: fs.readFileSync(SCHEMA_FILE, "utf8"),
  }
  const extras = fs.existsSync(MIGRATIONS_DIR)
    ? fs.readdirSync(MIGRATIONS_DIR).map(parseFileName).filter(Boolean)
    : []

  const all = [baseline, ...extras]
  const seen = new Set()
  for (const migration of all) {
    if (!Number.isSafeInteger(migration.version) || migration.version < 1) {
      throw new Error(`versao de migration invalida: ${migration.file}`)
    }
    if (seen.has(migration.version)) {
      throw new Error(`versao de migration duplicada: ${migration.version}`)
    }
    seen.add(migration.version)
  }

  return all
    .sort((a, b) => a.version - b.version)
    .map((migration) => ({
      ...migration,
      sql: migration.sql || fs.readFileSync(path.join(MIGRATIONS_DIR, migration.file), "utf8"),
    }))
}

function migrationTableSql() {
  return `
    CREATE TABLE IF NOT EXISTS schema_migrations (
      version INTEGER PRIMARY KEY,
      name TEXT NOT NULL,
      checksum TEXT NOT NULL,
      applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )
  `
}

module.exports = {
  SCHEMA_FILE,
  MIGRATIONS_DIR,
  MIGRATION_RE,
  parseFileName,
  readMigrations,
  migrationTableSql,
}
