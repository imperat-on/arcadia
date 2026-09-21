"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.NODE_ENV = "test"
const DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-client-boot-"))
process.env.ARCADIA_DATA_DIR = DATA_DIR
process.env.ARCADIA_SUPABASE_URL = "https://arcadia.test"

const sessionStore = require("../electron/supabase/session")
const { getClient, restoreSession } = require("../electron/supabase/client")

const SAVED_SESSION = {
  access_token: "access.boot.token",
  refresh_token: "refresh.boot.token",
  expires_at: 4_000_000_000,
  user: {
    id: "user-boot",
    email: "boot@arcadia.test",
    user_metadata: { username: "boot_user" },
  },
}

const originalFetch = global.fetch

test.after(() => {
  global.fetch = originalFetch
  fs.rmSync(DATA_DIR, { recursive: true, force: true })
})

test("restoreSession emite SIGNED_IN na hora e valida a sessão em background", async () => {
  sessionStore.saveSession(SAVED_SESSION)
  const chamadas = []
  let responder
  global.fetch = (url) => {
    chamadas.push(String(url))
    // Segura a resposta: o boot NÃO pode depender dela.
    return new Promise((resolve) => {
      responder = () =>
        resolve({
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ user: SAVED_SESSION.user }),
        })
    })
  }

  const eventos = []
  const subscription = getClient().auth.onAuthStateChange((event) => eventos.push(event))
  const result = await restoreSession()

  // Resolve com a rede ainda pendente: a identidade sai do session.json.
  assert.equal(result.error, null)
  assert.equal(result.session.user.id, "user-boot")
  assert.deepEqual(eventos, ["SIGNED_IN"])
  assert.deepEqual(chamadas, ["https://arcadia.test/auth/v1/user"])

  // A validação conclui atrás e não muda a identidade quando o token vale.
  responder()
  await getClient().auth._validacaoSessao
  subscription.data.subscription.unsubscribe()
  assert.deepEqual(eventos, ["SIGNED_IN"])
})
