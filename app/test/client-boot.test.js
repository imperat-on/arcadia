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

test("validação 401 no /user e no refresh derruba a sessão (SIGNED_OUT)", async () => {
  sessionStore.saveSession(SAVED_SESSION)
  global.fetch = async () => ({
    ok: false,
    status: 401,
    text: async () => JSON.stringify({ error: "token_invalido" }),
  })

  const eventos = []
  const subscription = getClient().auth.onAuthStateChange((event) => eventos.push(event))
  await restoreSession()
  await getClient().auth._validacaoSessao
  subscription.data.subscription.unsubscribe()

  assert.deepEqual(eventos, ["SIGNED_IN", "SIGNED_OUT"])
  assert.equal(getClient().auth._session, null)
  assert.equal(sessionStore.loadSession(), null)
})

test("falha de rede na validação não desloga (offline mantém sessão e session.json)", async () => {
  sessionStore.saveSession(SAVED_SESSION)
  global.fetch = async () => {
    throw new Error("ENETUNREACH")
  }

  const eventos = []
  const subscription = getClient().auth.onAuthStateChange((event) => eventos.push(event))
  await restoreSession()
  await getClient().auth._validacaoSessao
  subscription.data.subscription.unsubscribe()

  assert.deepEqual(eventos, ["SIGNED_IN"])
  assert.equal(getClient().auth._session?.user?.id, "user-boot")
  assert.equal(sessionStore.loadSession()?.user?.id, "user-boot")
})

test("corrida: validação antiga não derruba login de outra conta", async () => {
  sessionStore.saveSession(SAVED_SESSION)
  let responderValidacaoAntiga
  global.fetch = (url) => {
    const alvo = String(url)
    if (alvo.includes("/auth/v1/token?grant_type=password")) {
      return Promise.resolve({
        ok: true,
        status: 200,
        text: async () =>
          JSON.stringify({
            access_token: "novo.access.token",
            refresh_token: "novo.refresh.token",
            user: { id: "user-novo", user_metadata: { username: "novo_user" } },
          }),
      })
    }
    if (alvo.includes("/auth/v1/token?grant_type=refresh_token")) {
      return Promise.resolve({
        ok: false,
        status: 401,
        text: async () => JSON.stringify({ error: "refresh_invalido" }),
      })
    }
    // Validação da sessão antiga: fica pendente até o teste soltar.
    return new Promise((resolve) => {
      responderValidacaoAntiga = () =>
        resolve({
          ok: false,
          status: 401,
          text: async () => JSON.stringify({ error: "token_antigo_invalido" }),
        })
    })
  }

  const eventos = []
  const subscription = getClient().auth.onAuthStateChange((event) => eventos.push(event))
  await restoreSession()
  await getClient().auth.signInWithPassword({ email: "novo@arcadia.test", password: "segredo" })

  // A validação antiga responde 401 depois do login novo; a guarda de token
  // impede que ela derrube a sessão que já não é mais dela.
  responderValidacaoAntiga()
  await getClient().auth._validacaoSessao
  subscription.data.subscription.unsubscribe()

  assert.equal(getClient().auth._session?.user?.id, "user-novo")
  assert.ok(!eventos.includes("SIGNED_OUT"))
})
