"use strict"

// Testes da Fase 2: auth (signup, login, throttle, refresh, user, logout).
// Roda contra o app Express em memoria (sem porta), DB temporario.

const test = require("node:test")
const assert = require("node:assert")
const { after } = require("node:test")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-auth-"))

const express = require("express")
const { registerAuthRoutes } = require("../src/auth-routes")

const app = express()
app.use(express.json())
registerAuthRoutes(app)

// sobe o app numa porta efemera p/ o fetch (que precisa de URL real)
const listener = app.listen(0)
const base = `http://127.0.0.1:${listener.address().port}`
after(() => listener.close())

async function post(url, body, headers = {}) {
  const r = await fetch(base + url, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  })
  return { status: r.status, body: await r.json() }
}
async function get(url, headers = {}) {
  const r = await fetch(base + url, { headers })
  return { status: r.status, body: await r.json() }
}

test("signup cria usuario e devolve sessao", async () => {
  const r = await post("/auth/v1/signup", {
    email: "a@b.com",
    password: "senha123",
    options: { data: { username: "alice" } },
  })
  assert.strictEqual(r.status, 200)
  assert.ok(r.body.session.access_token, "tem access_token")
  assert.ok(r.body.session.refresh_token, "tem refresh_token")
  assert.strictEqual(r.body.session.user.user_metadata.username, "alice")
  assert.ok(r.body.session.user.id, "tem user.id")
})

test("signup rejeita username invalido e ocupado", async () => {
  const invalido = await post("/auth/v1/signup", {
    email: "x@y.com",
    password: "senha123",
    options: { data: { username: "!!" } },
  })
  assert.strictEqual(invalido.status, 400)
  assert.strictEqual(invalido.body.error, "username_invalido")

  const ocupado = await post("/auth/v1/signup", {
    email: "y@z.com",
    password: "senha123",
    options: { data: { username: "alice" } },
  })
  assert.strictEqual(ocupado.status, 400)
  assert.strictEqual(ocupado.body.error, "username_ocupado")
})

test("login_check com senha certa devolve email", async () => {
  const r = await post("/rest/v1/rpc/login_check", {
    p_username: "alice",
    p_password: "senha123",
  })
  assert.strictEqual(r.body.ok, true)
  assert.strictEqual(r.body.email, "a@b.com")
})

test("login_check errado devolve senha_errada e depois muitas_tentativas", async () => {
  // cria o throttling: 5 falhas
  for (let i = 0; i < 5; i++) {
    const r = await post("/rest/v1/rpc/login_check", {
      p_username: "alice",
      p_password: "errada",
    })
    assert.strictEqual(r.body.error, "senha_errada")
  }
  const bloqueado = await post("/rest/v1/rpc/login_check", {
    p_username: "alice",
    p_password: "senha123",
  })
  assert.strictEqual(bloqueado.body.error, "muitas_tentativas")
})

test("login_check serializa falhas concorrentes", async () => {
  await post("/auth/v1/signup", {
    email: "parallel@b.com",
    password: "senha123",
    options: { data: { username: "parallel" } },
  })
  const failures = await Promise.all(
    Array.from({ length: 5 }, () => post("/rest/v1/rpc/login_check", {
      p_username: "parallel",
      p_password: "errada",
    })),
  )
  assert.ok(failures.every((result) => result.body.error === "senha_errada"))
  const blocked = await post("/rest/v1/rpc/login_check", {
    p_username: "parallel",
    p_password: "senha123",
  })
  assert.strictEqual(blocked.body.error, "muitas_tentativas")
})

test("username_available espelha profiles e reserved", async () => {
  const alice = await post("/rest/v1/rpc/username_available", {
    p_username: "alice",
  })
  assert.strictEqual(alice.body, false, "alice ocupado")
  const livre = await post("/rest/v1/rpc/username_available", {
    p_username: "bob_fresh",
  })
  assert.strictEqual(livre.body, true, "bob_fresh livre")
  const admin = await post("/rest/v1/rpc/username_available", {
    p_username: "admin",
  })
  assert.strictEqual(admin.body, false, "admin reservado")
})

test("token grant_type=password devolve sessao", async () => {
  const r = await post("/auth/v1/token?grant_type=password", {
    email: "a@b.com",
    password: "senha123",
  })
  assert.strictEqual(r.status, 200)
  assert.ok(r.body.access_token)
})

test("token grant_type=refresh_token troca a sessao", async () => {
  const login = await post("/auth/v1/token?grant_type=password", {
    email: "a@b.com",
    password: "senha123",
  })
  const r = await post("/auth/v1/token?grant_type=refresh_token", {
    refresh_token: login.body.refresh_token,
  })
  assert.strictEqual(r.status, 200)
  assert.ok(r.body.access_token)
})

test("GET /auth/v1/user valida token e devolve perfil", async () => {
  const login = await post("/auth/v1/token?grant_type=password", {
    email: "a@b.com",
    password: "senha123",
  })
  const r = await get("/auth/v1/user", {
    authorization: `Bearer ${login.body.access_token}`,
  })
  assert.strictEqual(r.status, 200)
  assert.strictEqual(r.body.user.user_metadata.username, "alice")
})

test("GET /auth/v1/user sem token devolve 401", async () => {
  const r = await get("/auth/v1/user")
  assert.strictEqual(r.status, 401)
})
