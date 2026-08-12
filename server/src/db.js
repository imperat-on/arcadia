"use strict"

// Banco SQLite do Arcadia. Espelha os schemas Postgres do Supabase
// (docs/sql/schema.sql + migracoes v2-v7). node:sqlite (Node >= 22).
//
// Nota: `node:sqlite` e experimental no Node 22. Se falhar em runtime,
// trocar por `better-sqlite3` sem tocar nas rotas (mesma API sync).

const { DatabaseSync } = require("node:sqlite")
const path = require("node:path")
const fs = require("node:fs")

const DATA_DIR = process.env.DATA_DIR || path.join(__dirname, "..", "data")
const DB_PATH = path.join(DATA_DIR, "arcadia.db")

fs.mkdirSync(DATA_DIR, { recursive: true })

const db = new DatabaseSync(DB_PATH)

// WAL: leituras nao bloqueiam escritas e o backup via .backup e seguro.
db.exec("PRAGMA journal_mode = WAL")
db.exec("PRAGMA foreign_keys = ON")

// ---------------------------------------------------------------------------
// Schema (espelha o Postgres, adaptado p/ SQLite)
// ---------------------------------------------------------------------------
db.exec(`
CREATE TABLE IF NOT EXISTS profiles (
  id                 TEXT PRIMARY KEY,
  email              TEXT UNIQUE NOT NULL,
  password_hash      TEXT NOT NULL,
  username           TEXT UNIQUE NOT NULL CHECK (username GLOB '[a-z0-9_]*' AND length(username) BETWEEN 3 AND 20),
  avatar_url         TEXT,
  background_url     TEXT,
  banner_url         TEXT,
  steam_id           TEXT,
  display_name       TEXT,
  summary            TEXT,
  country            TEXT,
  city               TEXT,
  showcase           TEXT NOT NULL DEFAULT '[]',
  profile_visibility TEXT NOT NULL DEFAULT 'public' CHECK (profile_visibility IN ('public','friends','private')),
  show_location      INTEGER NOT NULL DEFAULT 0,
  created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_profiles_username ON profiles (username);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS friendships (
  user_a       TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  user_b       TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  requester_id TEXT NOT NULL REFERENCES profiles(id),
  status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','accepted','blocked')),
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (user_a, user_b),
  CHECK (user_a < user_b)
);
CREATE INDEX IF NOT EXISTS idx_friendships_a ON friendships (user_a, status);
CREATE INDEX IF NOT EXISTS idx_friendships_b ON friendships (user_b, status);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS user_achievements (
  user_id     TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  appid       TEXT NOT NULL,
  apiname     TEXT NOT NULL,
  unlocked_at INTEGER NOT NULL,
  updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
  title       TEXT,
  icon        TEXT,
  percent     REAL,
  PRIMARY KEY (user_id, appid, apiname)
);
CREATE INDEX IF NOT EXISTS idx_achievements_since ON user_achievements (user_id, updated_at);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS user_library (
  user_id    TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  appid      TEXT NOT NULL,
  title      TEXT NOT NULL DEFAULT '',
  platform   TEXT NOT NULL DEFAULT 'windows',
  added_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (user_id, appid)
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS user_playtime (
  user_id    TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  appid      TEXT NOT NULL,
  minutes    INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (user_id, appid)
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS refresh_tokens (
  token      TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT NOT NULL
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS login_attempts (
  username     TEXT PRIMARY KEY,
  attempts     INTEGER NOT NULL DEFAULT 1,
  window_start TEXT NOT NULL DEFAULT (datetime('now'))
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS reserved_usernames (
  username TEXT PRIMARY KEY
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS user_sources (
  user_id    TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  source_id  TEXT NOT NULL,
  url        TEXT NOT NULL,
  name       TEXT NOT NULL DEFAULT '',
  added_at   TEXT NOT NULL DEFAULT (datetime('now')),
  removed_at TEXT,
  PRIMARY KEY (user_id, source_id)
);
`)

db.exec(`
CREATE TABLE IF NOT EXISTS blocks (
  blocker_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  blocked_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (blocker_id, blocked_id),
  CHECK (blocker_id <> blocked_id)
);

-- Cache de catalogo da loja (proxy): o servidor busca a fonte externa uma
-- vez por TTL e guarda aqui. key e a identidade do cache (allowlist no
-- catalog-fetch), data o JSON pronto, at o epoch-s da busca.
CREATE TABLE IF NOT EXISTS catalog_cache (
  key TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  at   INTEGER NOT NULL
);

-- Reviews da comunidade: quem usa o projeto escreve reviews que ficam no
-- servidor (fonte de verdade propria, nao dependendo da Steam). O app mostra
-- estas reviews com fallback para a Steam quando nao ha nenhuma.
CREATE TABLE IF NOT EXISTS user_reviews (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  appid TEXT NOT NULL,
  text TEXT NOT NULL,
  positive INTEGER NOT NULL DEFAULT 1,
  hours REAL NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_user_reviews_appid ON user_reviews(appid);
`)

// Seeds de reserved_usernames (14 do SQL v6 + nomes de rota reservados)
const RESERVED = [
  "admin", "moderator", "support", "staff", "arcadia", "system",
  "official", "bot", "null", "undefined", "root", "test", "teste",
  "login", "signup", "auth", "api", "ws", "storage", "conta",
  "amigos", "sync", "biblioteca", "profile", "user",
]

const insertReserved = db.prepare(
  "INSERT OR IGNORE INTO reserved_usernames (username) VALUES (?)"
)
for (const u of RESERVED) insertReserved.run(u)

// ---------------------------------------------------------------------------
// Helpers de tempo: o app manda epoch segundos (Steam) e o DB usa datetime
// ISO para updated_at/created_at, com INTEGER epoch-s em unlocked_at.
// ---------------------------------------------------------------------------
function nowIso() {
  return new Date().toISOString()
}

function nowEpochS() {
  return Math.floor(Date.now() / 1000)
}

// ---------------------------------------------------------------------------
module.exports = { db, DB_PATH, DATA_DIR, nowIso, nowEpochS, RESERVED }
