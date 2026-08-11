"use strict"

// Emissao/validacao de JWT no shape GoTrue, para o shim do client.js e o
// session.json (que espera access_token + user.user_metadata.username).

const jwt = require("jsonwebtoken")

const SECRET = process.env.JWT_SECRET || "dev-secret-nao-use-em-prod"
const EXPIRES_IN_S = 3600 // 1h
const REFRESH_EXPIRES_IN_S = 60 * 60 * 24 * 30 // 30d

// Monta o objeto `user` no shape que o supabase-js devolve (getUser / signUp)
function buildUser(profile) {
  return {
    id: profile.id,
    email: profile.email,
    user_metadata: { username: profile.username },
    // campos opcionais que o renderer pode ler
    aud: "authenticated",
    role: "authenticated",
    app_metadata: {},
    created_at: profile.created_at,
  }
}

// Emite access_token (JWT) + refresh_token (opaco, guardado no servidor) no
// shape exato do GoTrue que o client.js espera.
function issueTokens(profile) {
  const access_token = jwt.sign(
    {
      sub: profile.id,
      email: profile.email,
      user_metadata: { username: profile.username },
      role: "authenticated",
    },
    SECRET,
    { expiresIn: EXPIRES_IN_S }
  )
  const refresh_token = require("node:crypto").randomBytes(32).toString("hex")

  return {
    access_token,
    refresh_token,
    expires_in: EXPIRES_IN_S,
    expires_at: Math.floor(Date.now() / 1000) + EXPIRES_IN_S,
    token_type: "bearer",
    user: buildUser(profile),
  }
}

// Valida e decodifica o access_token. Devolve { ok, sub, email, username } ou
// { ok:false, error }.
function verifyToken(token) {
  try {
    const payload = jwt.verify(token, SECRET)
    return {
      ok: true,
      sub: payload.sub,
      email: payload.email,
      username: payload.user_metadata?.username,
    }
  } catch (e) {
    return { ok: false, error: e.message }
  }
}

// Extrai o Bearer token de um header Authorization (aceita apikey/x-apikey
// como no Supabase, ignorando o valor deles).
function extractToken(req) {
  const auth = req.headers.authorization || ""
  if (auth.startsWith("Bearer ")) return auth.slice(7)
  return null
}

module.exports = { issueTokens, verifyToken, extractToken, buildUser, SECRET, EXPIRES_IN_S, REFRESH_EXPIRES_IN_S }
