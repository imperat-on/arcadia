"use strict"

const fs = require("node:fs")
const path = require("node:path")
const { DatabaseSync } = require("node:sqlite")

const sqlitePath = path.resolve(process.argv[2] || path.join(__dirname, "..", "data", "arcadia.db"))
if (!fs.existsSync(sqlitePath)) throw new Error(`SQLite nao encontrado: ${sqlitePath}`)

const { db, initDb, withTransaction } = require("../src/db")

const TABLES = [
  { name: "profiles", pk: ["id"] },
  { name: "reserved_usernames", pk: ["username"] },
  { name: "friendships", pk: ["user_a", "user_b"] },
  { name: "user_achievements", pk: ["user_id", "appid", "apiname"] },
  { name: "user_library", pk: ["user_id", "appid"] },
  { name: "user_playtime", pk: ["user_id", "appid"] },
  { name: "refresh_tokens", pk: ["token"] },
  { name: "login_attempts", pk: ["username"] },
  { name: "user_sources", pk: ["user_id", "source_id"] },
  { name: "blocks", pk: ["blocker_id", "blocked_id"] },
  { name: "catalog_cache", pk: ["key"] },
  { name: "user_reviews", pk: ["id"] },
]

function quote(identifier) {
  return `"${identifier.replaceAll('"', '""')}"`
}

async function migrateTable(client, sqlite, table) {
  const rows = sqlite.prepare(`SELECT * FROM ${quote(table.name)}`).all()
  for (const row of rows) {
    const columns = Object.keys(row)
    if (table.name === "profiles") row.showcase = JSON.stringify(JSON.parse(row.showcase || "[]"))
    await client.query(
      `INSERT INTO ${quote(table.name)} (${columns.map(quote).join(", ")})
       VALUES (${columns.map((_, index) => `$${index + 1}`).join(", ")})
       ON CONFLICT (${table.pk.map(quote).join(", ")}) DO NOTHING`,
      columns.map((column) => row[column]),
    )
  }
  return rows.length
}

async function main() {
  const sqlite = new DatabaseSync(sqlitePath, { readOnly: true })
  try {
    const integrity = sqlite.prepare("PRAGMA integrity_check").get().integrity_check
    if (integrity !== "ok") throw new Error(`SQLite inconsistente: ${integrity}`)
    await initDb()

    const sourceCounts = await withTransaction(async (client) => {
      await client.query("SET LOCAL TIME ZONE 'UTC'")
      await client.query("SET CONSTRAINTS ALL DEFERRED")
      const occupied = []
      for (const table of TABLES.filter((item) => item.name !== "reserved_usernames")) {
        const count = Number(
          (await client.query(`SELECT count(*) AS count FROM ${quote(table.name)}`)).rows[0].count,
        )
        if (count) occupied.push(`${table.name}=${count}`)
      }
      if (occupied.length) {
        throw new Error(`PostgreSQL de destino nao esta vazio: ${occupied.join(", ")}`)
      }

      const counts = {}
      for (const table of TABLES) counts[table.name] = await migrateTable(client, sqlite, table)
      await client.query(
        `SELECT setval(
           pg_get_serial_sequence('user_reviews', 'id'),
           GREATEST(COALESCE((SELECT MAX(id) FROM user_reviews), 0), 1),
           EXISTS (SELECT 1 FROM user_reviews)
         )`,
      )
      let failed = false
      for (const table of TABLES) {
        const target = Number(
          (await client.query(`SELECT count(*) AS count FROM ${quote(table.name)}`)).rows[0].count,
        )
        const source = counts[table.name]
        const valid = table.name === "reserved_usernames" ? target >= source : target === source
        console.log(`${table.name}: sqlite=${source} postgres=${target} ${valid ? "OK" : "DIVERGENTE"}`)
        if (!valid) failed = true
      }
      const critical = await client.query(
        `SELECT count(*) FILTER (
                  WHERE password_hash IS NULL OR showcase IS NULL OR jsonb_typeof(showcase) <> 'array'
                ) AS invalid_profiles,
                (SELECT count(*) FROM user_achievements WHERE unlocked_at IS NULL) AS invalid_achievements
         FROM profiles`,
      )
      if (Number(critical.rows[0].invalid_profiles) || Number(critical.rows[0].invalid_achievements)) {
        failed = true
      }
      if (failed) throw new Error("Validacao da migracao falhou")
      return counts
    })

    void sourceCounts
    console.log("Migracao validada com sucesso")
  } finally {
    sqlite.close()
    await db.end()
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
