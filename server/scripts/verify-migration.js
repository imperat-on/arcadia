"use strict"

const crypto = require("node:crypto")
const path = require("node:path")
const { DatabaseSync } = require("node:sqlite")
const { db } = require("../src/db")

const sqlitePath = path.resolve(process.argv[2] || path.join(__dirname, "..", "data", "arcadia.db"))

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`
  if (value && typeof value === "object" && !(value instanceof Date)) {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stable(value[key])}`).join(",")}}`
  }
  return JSON.stringify(value)
}

function timestamp(value) {
  if (value instanceof Date) return value.toISOString()
  const text = String(value)
  return new Date(/[z+-]\d*:?\d*$/i.test(text) ? text : `${text.replace(" ", "T")}Z`).toISOString()
}

function digest(rows) {
  return crypto.createHash("sha256").update(rows.map(stable).sort().join("\n")).digest("hex")
}

async function main() {
  const sqlite = new DatabaseSync(sqlitePath, { readOnly: true })
  try {
    const checks = [
      {
        name: "profiles",
        sqlite: sqlite.prepare(
          "SELECT id,email,password_hash,username,showcase,created_at FROM profiles ORDER BY id",
        ).all().map((row) => ({
          ...row,
          showcase: JSON.parse(row.showcase),
          created_at: timestamp(row.created_at),
        })),
        postgres: (await db.query(
          "SELECT id,email,password_hash,username,showcase,created_at FROM profiles ORDER BY id",
        )).rows.map((row) => ({ ...row, created_at: timestamp(row.created_at) })),
      },
      {
        name: "achievements",
        sqlite: sqlite.prepare(
          "SELECT user_id,appid,apiname,unlocked_at FROM user_achievements ORDER BY user_id,appid,apiname",
        ).all(),
        postgres: (await db.query(
          "SELECT user_id,appid,apiname,unlocked_at FROM user_achievements ORDER BY user_id,appid,apiname",
        )).rows,
      },
      {
        name: "refresh_tokens",
        sqlite: sqlite.prepare(
          "SELECT token,user_id,created_at,expires_at FROM refresh_tokens ORDER BY token",
        ).all().map((row) => ({
          ...row,
          created_at: timestamp(row.created_at),
          expires_at: timestamp(row.expires_at),
        })),
        postgres: (await db.query(
          "SELECT token,user_id,created_at,expires_at FROM refresh_tokens ORDER BY token",
        )).rows.map((row) => ({
          ...row,
          created_at: timestamp(row.created_at),
          expires_at: timestamp(row.expires_at),
        })),
      },
    ]

    for (const check of checks) {
      const source = digest(check.sqlite)
      const target = digest(check.postgres)
      console.log(`${check.name}: sqlite=${source} postgres=${target} ${source === target ? "OK" : "DIVERGENTE"}`)
      if (source !== target) throw new Error(`Hash divergente: ${check.name}`)
    }
    console.log("Hashes criticos validados com sucesso")
  } finally {
    sqlite.close()
    await db.end()
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
