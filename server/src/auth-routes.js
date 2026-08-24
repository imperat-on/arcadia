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
const { db, nowIso, withTransaction } = require("./db")
const { issueTokens, verifyToken, extractToken, buildUser } = require("./jwt")
const asyncHandler = require("./async-handler")
const { createRateLimiter } = require("./rate-limit")

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
async function rpcLoginCheck(p_username, p_password) {
  const u = String(p_username || "").trim().toLowerCase()
  const p = String(p_password || "")

  const user = (await db.query("SELECT * FROM profiles WHERE username = $1 LIMIT 1", [u])).rows[0]
  if (!user) return { ok: false, error: "usuario_nao_existe" }

  // throttle: janela deslizante 5 falhas / 10min (usa o id como chave, como o SQL)
  if (!user.password_hash || user.password_hash === "!" || user.password_hash === "") {
    return { ok: false, error: "sem_senha" }
  }

  const senhaOk = bcrypt.compareSync(p, user.password_hash)
  return withTransaction(async (client) => {
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [user.id])
    const attempt = (
      await client.query("SELECT * FROM login_attempts WHERE username = $1", [user.id])
    ).rows[0]
    const now = Date.now()
    const active = attempt && now - Date.parse(attempt.window_start) <= LOGIN_WINDOW_MS
    if (active && attempt.attempts >= LOGIN_MAX_ATTEMPTS) {
      return { ok: false, error: "muitas_tentativas" }
    }
    if (!senhaOk) {
      await client.query(
        `INSERT INTO login_attempts (username, attempts, window_start) VALUES ($1, 1, $2)
         ON CONFLICT (username) DO UPDATE SET
           attempts = CASE
             WHEN login_attempts.window_start <= $3 THEN 1
             ELSE login_attempts.attempts + 1
           END,
           window_start = CASE
             WHEN login_attempts.window_start <= $3 THEN excluded.window_start
             ELSE login_attempts.window_start
           END`,
        [user.id, new Date(now).toISOString(), new Date(now - LOGIN_WINDOW_MS).toISOString()],
      )
      return { ok: false, error: "senha_errada" }
    }
    await client.query("DELETE FROM login_attempts WHERE username = $1", [user.id])
    return { ok: true, email: user.email }
  })
}

// ---------------------------------------------------------------------------
// username_available (RPC publica)
// ---------------------------------------------------------------------------
async function rpcUsernameAvailable(p_username) {
  const u = String(p_username || "").trim().toLowerCase()
  const [profiles, reserved] = await Promise.all([
    db.query("SELECT 1 FROM profiles WHERE username = $1", [u]),
    db.query("SELECT 1 FROM reserved_usernames WHERE username = $1", [u]),
  ])
  const emProfiles = profiles.rows[0]
  const emReserved = reserved.rows[0]
  return !emProfiles && !emReserved
}

// ---------------------------------------------------------------------------
// Signup, cria profile + devolve sessao (sem confirmacao de email)
// ---------------------------------------------------------------------------
async function signup(body) {
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
  if (!(await rpcUsernameAvailable(username)))
    return { status: 400, json: { error: "username_ocupado" } }

  const id = uuid()
  const hash = bcrypt.hashSync(password, 10)
  const now = nowIso()

  try {
    return await withTransaction(async (client) => {
      const profile = (
        await client.query(
          `INSERT INTO profiles (id, email, password_hash, username, created_at)
           VALUES ($1, $2, $3, $4, $5) RETURNING *`,
          [id, email, hash, username, now],
        )
      ).rows[0]
      const session = issueTokens(profile)

      await client.query(
        "INSERT INTO refresh_tokens (token, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
        [session.refresh_token, id, now, new Date(Date.now() + 30 * 24 * 3600 * 1000).toISOString()],
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
    })
  } catch (e) {
    if (e.code === "23505") {
      return { status: 400, json: { error: "username_ocupado" } }
    }
    throw e
  }

}

// Aplica a mesma janela de tentativas ao grant usado pelo client real. A
// operacao fica serializada por advisory lock para evitar duas tentativas
// concorrentes ultrapassarem o limite.
async function passwordLoginAllowed(userId, passwordOk) {
  return withTransaction(async (client) => {
    const key = `password:${userId}`
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [key])
    const row = (await client.query("SELECT * FROM login_attempts WHERE username = $1", [key])).rows[0]
    const now = Date.now()
    const expired = !row || now - Date.parse(row.window_start) > LOGIN_WINDOW_MS
    if (!expired && row.attempts >= LOGIN_MAX_ATTEMPTS) return false
    if (passwordOk) {
      await client.query("DELETE FROM login_attempts WHERE username = $1", [key])
      return true
    }
    await client.query(
      `INSERT INTO login_attempts (username, attempts, window_start) VALUES ($1, 1, $2)
       ON CONFLICT (username) DO UPDATE SET
         attempts = CASE WHEN login_attempts.window_start <= $3 THEN 1 ELSE login_attempts.attempts + 1 END,
         window_start = CASE WHEN login_attempts.window_start <= $3 THEN excluded.window_start ELSE login_attempts.window_start END`,
      [key, new Date(now).toISOString(), new Date(now - LOGIN_WINDOW_MS).toISOString()],
    )
    return false
  })
}

// ---------------------------------------------------------------------------
// Token (password e refresh), devolve sessao no shape GoTrue
// ---------------------------------------------------------------------------
async function tokenGrant(grantType, body) {
  if (grantType === "password") {
    const email = String(body?.email || "").trim().toLowerCase()
    const password = String(body?.password || "")
    const user = (await db.query("SELECT * FROM profiles WHERE email = $1", [email])).rows[0]
    const loginKey = user?.id || email || "unknown"
    if (!user || !user.password_hash) {
      await passwordLoginAllowed(loginKey, false)
      return { status: 400, json: { error: "Invalid login credentials" } }
    }
    const senhaOk = bcrypt.compareSync(password, user.password_hash)
    const permitida = await passwordLoginAllowed(loginKey, senhaOk)
    if (!senhaOk || !permitida) {
      return { status: 400, json: { error: "Invalid login credentials" } }
    }
    return issueSessionJson(user)
  }

  if (grantType === "refresh_token") {
    return rotateRefreshToken(String(body?.refresh_token || ""))
  }

  return { status: 400, json: { error: "invalid_grant" } }
}

async function rotateRefreshToken(token) {
  return withTransaction(async (client) => {
    // Lock the row before deleting it. Two concurrent refreshes must not both
    // accept the same token (rotation is also replay protection).
    const stored = (
      await client.query(
        "SELECT * FROM refresh_tokens WHERE token = $1 AND expires_at > NOW() FOR UPDATE",
        [token],
      )
    ).rows[0]
    if (!stored) return { status: 401, json: { error: "Invalid Refresh Token: Refresh Token Not Found" } }
    const user = (await client.query("SELECT * FROM profiles WHERE id = $1", [stored.user_id])).rows[0]
    if (!user) return { status: 401, json: { error: "Invalid Refresh Token: User Not Found" } }
    const session = issueTokens(user)
    const now = nowIso()
    await client.query("DELETE FROM refresh_tokens WHERE token = $1", [token])
    await client.query(
      "INSERT INTO refresh_tokens (token, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
      [session.refresh_token, user.id, now, new Date(Date.now() + 30 * 24 * 3600 * 1000).toISOString()],
    )
    return sessionResponse(session)
  })
}

function sessionResponse(session) {
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

async function issueSessionJson(user, replacedToken = "") {
  const session = issueTokens(user)
  const now = nowIso()
  await withTransaction(async (client) => {
    if (replacedToken) await client.query("DELETE FROM refresh_tokens WHERE token = $1", [replacedToken])
    await client.query(
      "INSERT INTO refresh_tokens (token, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
      [session.refresh_token, user.id, now, new Date(Date.now() + 30 * 24 * 3600 * 1000).toISOString()],
    )
  })
  return sessionResponse(session)
}

// ---------------------------------------------------------------------------
// Registro das rotas
// ---------------------------------------------------------------------------
function positiveEnvInt(name, fallback) {
  const value = Number(process.env[name])
  return Number.isSafeInteger(value) && value > 0 ? value : fallback
}

function registerAuthRoutes(app) {
  // Alem do throttle por conta usado no login, limite por IP os endpoints
  // publicos. Isso evita que um atacante distribua tentativas entre contas ou
  // force bcrypt/consultas de signup indefinidamente. O limite fica em memoria
  // por processo (adequado ao servidor unico) e pode ser ajustado no ambiente.
  const authRateLimit = createRateLimiter({
    windowMs: positiveEnvInt("AUTH_RATE_LIMIT_WINDOW_MS", 60 * 1000),
    max: positiveEnvInt("AUTH_RATE_LIMIT_MAX", 60),
  })
  const credentialRateLimit = createRateLimiter({
    windowMs: positiveEnvInt("AUTH_RATE_LIMIT_WINDOW_MS", 60 * 1000),
    max: positiveEnvInt("AUTH_CREDENTIAL_RATE_LIMIT_MAX", 30),
  })

  // POST /auth/v1/signup
  app.post("/auth/v1/signup", credentialRateLimit, asyncHandler(async (req, res) => {
    const r = await signup(req.body)
    res.status(r.status).json(r.json)
  }))

  // POST /auth/v1/token?grant_type=...
  app.post("/auth/v1/token", credentialRateLimit, asyncHandler(async (req, res) => {
    const grantType = String(req.query.grant_type || "")
    const r = await tokenGrant(grantType, req.body)
    res.status(r.status).json(r.json)
  }))

  // GET /auth/v1/user (exige Bearer)
  app.get("/auth/v1/user", asyncHandler(async (req, res) => {
    const token = extractToken(req)
    const v = verifyToken(token || "")
    if (!v.ok) return res.status(401).json({ error: "token invalido" })
    const user = (await db.query("SELECT * FROM profiles WHERE id = $1", [v.sub])).rows[0]
    if (!user) return res.status(401).json({ error: "usuario_nao_existe" })
    res.json({ user: buildUser(user) })
  }))

  // POST /auth/v1/logout
  app.post("/auth/v1/logout", asyncHandler(async (req, res) => {
    const token = extractToken(req)
    if (token) {
      const v = verifyToken(token)
      if (v.ok) await db.query("DELETE FROM refresh_tokens WHERE user_id = $1", [v.sub])
    }
    res.json({})
  }))

  // POST /rest/v1/rpc/login_check (publico)
  app.post("/rest/v1/rpc/login_check", credentialRateLimit, asyncHandler(async (req, res) => {
    const r = await rpcLoginCheck(req.body?.p_username, req.body?.p_password)
    res.json(r)
  }))

  // POST /rest/v1/rpc/username_available (publico)
  app.post("/rest/v1/rpc/username_available", authRateLimit, asyncHandler(async (req, res) => {
    res.json(await rpcUsernameAvailable(req.body?.p_username))
  }))
}

module.exports = { registerAuthRoutes, rpcLoginCheck, rpcUsernameAvailable }
