"use strict"

// Auth: espelha o Supabase GoTrue + as RPCs login_check/username_available.
// Rotas:
//   POST /auth/v1/signup
//   POST /auth/v1/token?grant_type=password|refresh_token
//   GET  /auth/v1/user
//   POST /auth/v1/logout
//   POST /rest/v1/rpc/login_check         (publico)
//   POST /rest/v1/rpc/username_available  (publico)
//
// O client.js (shim) monta essas chamadas. O shape de resposta segue o GoTrue.

const bcrypt = require("bcryptjs")
const crypto = require("node:crypto")
const { db, nowIso } = require("./db")
const { issueTokens, verifyToken, extractToken, buildUser } = require("./jwt")

// ---------------------------------------------------------------------------
// Validacoes (mesmas do auth.js do app)
// ---------------------------------------------------------------------------
const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/
const USERNAME_RE = /^[a-z0-9_]{3,20}$/
const SENHA_MIN = 6
const LOGIN_MAX_ATTEMPTS = 5
const LOGIN_WINDOW_MS = 10 * 60 * 1000 // 10 min

function uuid() {
  return crypto.randomUUID()
}

// ---------------------------------------------------------------------------
// login_check (RPC publica), espelha o SQL v6
// ---------------------------------------------------------------------------
function rpcLoginCheck(p_username, p_password) {
  const u = String(p_username || "").trim().toLowerCase()
  const p = String(p_password || "")

  const user = db
    .prepare("SELECT * FROM profiles WHERE username = ? LIMIT 1")
    .get(u)
  if (!user) return { ok: false, error: "usuario_nao_existe" }

  // throttle: janela deslizante 5 falhas / 10min (usa o id como chave, como o SQL)
  const now = Date.now()
  let attempt = db
    .prepare("SELECT * FROM login_attempts WHERE username = ?")
    .get(user.id)
  if (attempt) {
    const windowStart = Date.parse(attempt.window_start)
    const janelaExpirou = now - windowStart > LOGIN_WINDOW_MS
    if (!janelaExpirou && attempt.attempts >= LOGIN_MAX_ATTEMPTS) {
      return { ok: false, error: "muitas_tentativas" }
    }
  }

  if (!user.password_hash || user.password_hash === "!" || user.password_hash === "") {
    return { ok: false, error: "sem_senha" }
  }

  const senhaOk = bcrypt.compareSync(p, user.password_hash)
  if (!senhaOk) {
    // upsert do contador (reset se janela expirou, senao +1)
    if (!attempt || Date.parse(attempt.window_start) <= now - LOGIN_WINDOW_MS) {
      db.prepare(
        "INSERT INTO login_attempts (username, attempts, window_start) VALUES (?, 1, ?) " +
          "ON CONFLICT(username) DO UPDATE SET attempts = 1, window_start = excluded.window_start"
      ).run(user.id, new Date(now).toISOString())
    } else {
      db.prepare(
        "UPDATE login_attempts SET attempts = attempts + 1 WHERE username = ?"
      ).run(user.id)
    }
    return { ok: false, error: "senha_errada" }
  }

  // sucesso: limpa contador
  db.prepare("DELETE FROM login_attempts WHERE username = ?").run(user.id)
  return { ok: true, email: user.email }
}

// ---------------------------------------------------------------------------
// username_available (RPC publica)
// ---------------------------------------------------------------------------
function rpcUsernameAvailable(p_username) {
  const u = String(p_username || "").trim().toLowerCase()
  const emProfiles = db
    .prepare("SELECT 1 FROM profiles WHERE username = ?")
    .get(u)
  const emReserved = db
    .prepare("SELECT 1 FROM reserved_usernames WHERE username = ?")
    .get(u)
  return !emProfiles && !emReserved
}

// ---------------------------------------------------------------------------
// Signup, cria profile + devolve sessao (sem confirmacao de email)
// ---------------------------------------------------------------------------
function signup(body) {
  const email = String(body?.email || "").trim().toLowerCase()
  const username = String(body?.options?.data?.username || body?.username || "")
    .trim()
    .toLowerCase()
  const password = String(body?.password || "")

  if (!EMAIL_RE.test(email)) return { status: 400, json: { error: "email_invalido" } }
  if (!USERNAME_RE.test(username))
    return { status: 400, json: { error: "username_invalido" } }
  if (password.length < SENHA_MIN)
    return { status: 400, json: { error: "senha_curta" } }

  // username disponivel? (o app ja checa username_available antes. Se colidir
  // aqui, devolve erro claro. handle_new_user nao roda: perfis sao criados
  // na hora no signup, entao colisao real e rara)
  if (!rpcUsernameAvailable(username))
    return { status: 400, json: { error: "username_ocupado" } }

  const id = uuid()
  const hash = bcrypt.hashSync(password, 10)
  const now = nowIso()

  try {
    db.prepare(
      `INSERT INTO profiles (id, email, password_hash, username, created_at)
       VALUES (?, ?, ?, ?, ?)`
    ).run(id, email, hash, username, now)
  } catch (e) {
    if (/UNIQUE/.test(String(e.message))) {
      return { status: 400, json: { error: "username_ocupado" } }
    }
    throw e
  }

  const profile = db.prepare("SELECT * FROM profiles WHERE id = ?").get(id)
  const session = issueTokens(profile)

  // guarda refresh_token
  db.prepare(
    "INSERT INTO refresh_tokens (token, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)"
  ).run(
    session.refresh_token,
    id,
    now,
    new Date(Date.now() + 30 * 24 * 3600 * 1000).toISOString()
  )

  return {
    status: 200,
    json: {
      user: session.user,
      session: {
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        expires_in: session.expires_in,
        expires_at: session.expires_at,
        token_type: session.token_type,
        user: session.user,
      },
    },
  }
}

// ---------------------------------------------------------------------------
// Token (password e refresh), devolve sessao no shape GoTrue
// ---------------------------------------------------------------------------
function tokenGrant(grantType, body) {
  if (grantType === "password") {
    const email = String(body?.email || "").trim().toLowerCase()
    const password = String(body?.password || "")
    const user = db.prepare("SELECT * FROM profiles WHERE email = ?").get(email)
    if (!user || !user.password_hash) {
      return { status: 400, json: { error: "Invalid login credentials" } }
    }
    if (!bcrypt.compareSync(password, user.password_hash)) {
      return { status: 400, json: { error: "Invalid login credentials" } }
    }
    return issueSessionJson(user)
  }

  if (grantType === "refresh_token") {
    const rt = String(body?.refresh_token || "")
    const stored = db
      .prepare("SELECT * FROM refresh_tokens WHERE token = ?")
      .get(rt)
    if (!stored || Date.parse(stored.expires_at) < Date.now()) {
      return { status: 401, json: { error: "Invalid Refresh Token: Refresh Token Not Found" } }
    }
    const user = db.prepare("SELECT * FROM profiles WHERE id = ?").get(stored.user_id)
    if (!user) return { status: 401, json: { error: "Invalid Refresh Token: User Not Found" } }
    return issueSessionJson(user)
  }

  return { status: 400, json: { error: "invalid_grant" } }
}

function issueSessionJson(user) {
  const session = issueTokens(user)
  const now = nowIso()
  db.prepare(
    "INSERT INTO refresh_tokens (token, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)"
  ).run(
    session.refresh_token,
    user.id,
    now,
    new Date(Date.now() + 30 * 24 * 3600 * 1000).toISOString()
  )
  return {
    status: 200,
    json: {
      access_token: session.access_token,
      refresh_token: session.refresh_token,
      expires_in: session.expires_in,
      expires_at: session.expires_at,
      token_type: session.token_type,
      user: session.user,
    },
  }
}

// ---------------------------------------------------------------------------
// Registro das rotas
// ---------------------------------------------------------------------------
function registerAuthRoutes(app) {
  // POST /auth/v1/signup
  app.post("/auth/v1/signup", (req, res) => {
    const r = signup(req.body)
    res.status(r.status).json(r.json)
  })

  // POST /auth/v1/token?grant_type=...
  app.post("/auth/v1/token", (req, res) => {
    const grantType = String(req.query.grant_type || "")
    const r = tokenGrant(grantType, req.body)
    res.status(r.status).json(r.json)
  })

  // GET /auth/v1/user (exige Bearer)
  app.get("/auth/v1/user", (req, res) => {
    const token = extractToken(req)
    const v = verifyToken(token || "")
    if (!v.ok) return res.status(401).json({ error: "token invalido" })
    const user = db.prepare("SELECT * FROM profiles WHERE id = ?").get(v.sub)
    if (!user) return res.status(401).json({ error: "usuario_nao_existe" })
    res.json({ user: buildUser(user) })
  })

  // POST /auth/v1/logout
  app.post("/auth/v1/logout", (req, res) => {
    const token = extractToken(req)
    if (token) {
      const v = verifyToken(token)
      if (v.ok) db.prepare("DELETE FROM refresh_tokens WHERE user_id = ?").run(v.sub)
    }
    res.json({})
  })

  // POST /rest/v1/rpc/login_check (publico)
  app.post("/rest/v1/rpc/login_check", (req, res) => {
    const r = rpcLoginCheck(req.body?.p_username, req.body?.p_password)
    res.json(r)
  })

  // POST /rest/v1/rpc/username_available (publico)
  app.post("/rest/v1/rpc/username_available", (req, res) => {
    res.json(rpcUsernameAvailable(req.body?.p_username))
  })
}

module.exports = { registerAuthRoutes, rpcLoginCheck, rpcUsernameAvailable }
