"use strict"

const fs = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const { Pool, types } = require("pg")

require("dotenv").config({ path: path.join(__dirname, "..", ".env"), quiet: true })

const DATA_DIR = process.env.DATA_DIR || path.join(__dirname, "..", "data")
const DB_PATH = path.join(DATA_DIR, "arcadia.db")
const IS_TEST = process.env.NODE_ENV === "test"
if (IS_TEST && !process.env.TEST_DATABASE_URL) {
  throw new Error("TEST_DATABASE_URL e obrigatorio em NODE_ENV=test")
}

const DATABASE_URL = IS_TEST
  ? process.env.TEST_DATABASE_URL
  : process.env.DATABASE_URL || "postgres://arcadia:arcadia@127.0.0.1:5432/arcadia"
const DATABASE_SCHEMA =
  process.env.DATABASE_SCHEMA ||
  (IS_TEST ? `arcadia_test_${process.pid}_${crypto.randomBytes(4).toString("hex")}` : "public")

if (!/^[a-z_][a-z0-9_]*$/.test(DATABASE_SCHEMA)) {
  throw new Error("DATABASE_SCHEMA invalido")
}

fs.mkdirSync(DATA_DIR, { recursive: true })

// Epochs e contagens do Arcadia cabem com seguranca em Number.
types.setTypeParser(20, Number)

const db = new Pool({
  connectionString: DATABASE_URL,
  max: Number(process.env.PGPOOL_MAX) || 10,
  idleTimeoutMillis: 30_000,
  connectionTimeoutMillis: 5_000,
  allowExitOnIdle: true,
  options: `-c search_path=${DATABASE_SCHEMA},public -c timezone=UTC`,
})

db.on("error", (error) => console.error("[postgres] erro no pool", error))
const poolQuery = db.query.bind(db)

const RESERVED = [
  "admin", "moderator", "support", "staff", "arcadia", "system",
  "official", "bot", "null", "undefined", "root", "test", "teste",
  "login", "signup", "auth", "api", "ws", "storage", "conta",
  "amigos", "sync", "biblioteca", "profile", "user",
]

let initPromise

function initDb() {
  if (!initPromise) {
    initPromise = (async () => {
      if (DATABASE_SCHEMA !== "public") {
        await poolQuery(`CREATE SCHEMA IF NOT EXISTS ${DATABASE_SCHEMA}`)
      }
      const schema = fs.readFileSync(path.join(__dirname, "..", "sql", "schema.sql"), "utf8")
      await poolQuery(schema)
      await poolQuery(
        `INSERT INTO reserved_usernames (username)
         SELECT unnest($1::text[])
         ON CONFLICT (username) DO NOTHING`,
        [RESERVED],
      )
      await poolQuery("SELECT 1")
    })().catch((error) => {
      initPromise = undefined
      throw error
    })
  }
  return initPromise
}

db.query = async (...args) => {
  await initDb()
  return poolQuery(...args)
}

async function withTransaction(fn) {
  await initDb()
  const client = await db.connect()
  try {
    await client.query("BEGIN")
    const result = await fn(client)
    await client.query("COMMIT")
    return result
  } catch (error) {
    await client.query("ROLLBACK")
    throw error
  } finally {
    client.release()
  }
}

function nowIso() {
  return new Date().toISOString()
}

function nowEpochS() {
  return Math.floor(Date.now() / 1000)
}

module.exports = {
  db,
  DB_PATH,
  DATA_DIR,
  DATABASE_URL,
  initDb,
  withTransaction,
  nowIso,
  nowEpochS,
  RESERVED,
}
