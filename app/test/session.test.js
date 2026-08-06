// Testes da sessão (node --test, zero dependências).
// Roda em Node puro → safeStorage indisponível → cobre o caminho de fallback.
// Roda: cd app && npm test
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// Diretório isolado por teste (injetado via ARCADIA_DATA_DIR)
function tempDataDir(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-session-"))
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  return dir
}

const SESSION = {
  access_token: "fake.access.token",
  refresh_token: "fake.refresh.token",
  expires_at: 9999999999,
  user: { id: "uuid-teste", email: "a@b.com" },
}

test("save → load roundtrip preserva a sessão", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")

  store.saveSession(SESSION)
  const loaded = store.loadSession()
  assert.deepEqual(loaded, SESSION)
})

test("arquivo gravado com permissão 0600", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")

  store.saveSession(SESSION)
  const mode = fs.statSync(store.sessionPath()).mode & 0o777
  assert.equal(mode, 0o600, `esperado 0600, veio ${mode.toString(8)}`)
})

test("load sem arquivo devolve null", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")
  assert.equal(store.loadSession(), null)
})

test("clearSession remove o arquivo", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")

  store.saveSession(SESSION)
  assert.ok(fs.existsSync(store.sessionPath()))
  store.clearSession()
  assert.equal(fs.existsSync(store.sessionPath()), false)
  assert.equal(store.loadSession(), null)
})

test("arquivo corrompido é APAGADO (sem .bak com tokens) e load devolve null", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")

  fs.writeFileSync(store.sessionPath(), "{corrompido!!!", "utf8")
  assert.equal(store.loadSession(), null)
  assert.equal(fs.existsSync(store.sessionPath()), false, "corrompido deveria ser apagado (auditoria A-13)")
  assert.equal(fs.existsSync(store.sessionPath() + ".bak"), false, "não pode sobrar .bak com tokens")
})

test("saveSession(null) não cria arquivo", (t) => {
  const dir = tempDataDir(t)
  process.env.ARCADIA_DATA_DIR = dir
  const store = require("../electron/supabase/session.js")

  store.saveSession(null)
  assert.equal(fs.existsSync(store.sessionPath()), false)
})
