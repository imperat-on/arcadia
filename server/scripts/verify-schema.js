"use strict"

const crypto = require("node:crypto")
const { db, initDb } = require("../src/db")
const { readMigrations } = require("../src/migrations")

async function main() {
  await initDb()
  const rows = (await db.query(
    "SELECT version, name, checksum, applied_at FROM schema_migrations ORDER BY version",
  )).rows
  const applied = new Map(rows.map((row) => [Number(row.version), row]))
  let invalid = false

  for (const migration of readMigrations()) {
    const row = applied.get(migration.version)
    const checksum = crypto.createHash("sha256").update(migration.sql).digest("hex")
    const ok = row && row.name === migration.name && row.checksum === checksum
    console.log(`${migration.version} ${migration.name}: ${ok ? "OK" : "AUSENTE/DIVERGENTE"}`)
    if (!ok) invalid = true
  }

  if (invalid) throw new Error("schema_migrations divergente")
  console.log(`Schema válido: ${rows.length} migration(s)`)
}

main()
  .catch((error) => {
    console.error("Falha ao validar schema:", error)
    process.exitCode = 1
  })
  .finally(() => db.end())
