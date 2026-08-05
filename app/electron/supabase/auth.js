// Autenticação Supabase — MAIN PROCESS (nunca no renderer).
// Fluxo: cadastro com EMAIL + USERNAME + SENHA (sem verificação de email,
// projeto libertário); login com USERNAME + SENHA.
// Detalhe técnico: o Supabase autentica por EMAIL, então o login por username
// resolve o email via RPC login_email (security definer, chamável por anon).
"use strict"

const { getClient } = require("./client")

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/
const USERNAME_RE = /^[a-z0-9_]{3,20}$/
const SENHA_MIN = 6

/**
 * Cadastro: email + username + senha, SEM verificação de email.
 * Pré-requisito: "Confirm email" DESLIGADO no painel do Supabase
 * (Authentication → Sign In / Providers → Email). Sem isso o signUp não
 * devolve sessão e o app avisa.
 */
async function signUp({ email, username, password } = {}) {
  const e = String(email || "").trim().toLowerCase()
  if (!EMAIL_RE.test(e)) return { ok: false, error: "email_invalido" }

  const u = String(username || "").trim().toLowerCase()
  if (!USERNAME_RE.test(u)) return { ok: false, error: "username_invalido" }

  const p = String(password || "")
  if (p.length < SENHA_MIN) return { ok: false, error: "senha_curta" }

  const chk = await usernameAvailable(u)
  if (!chk.ok) return chk
  if (!chk.available) return { ok: false, error: "username_ocupado" }

  const { data, error } = await getClient().auth.signUp({
    email: e,
    password: p,
    options: { data: { username: u } },
  })
  if (error) return { ok: false, error: error.message }
  if (!data?.session) {
    return { ok: false, error: "confirmacao_necessaria" } // "Confirm email" ainda ligado
  }
  return { ok: true, session: data.session, user: data.user }
}

/** Login com username + senha (resolve o email da conta via RPC). */
async function signIn({ username, password } = {}) {
  const u = String(username || "").trim().toLowerCase()
  const p = String(password || "")
  if (!USERNAME_RE.test(u)) return { ok: false, error: "username_invalido" }
  if (!p) return { ok: false, error: "senha_curta" }

  const { data: email, error: rpcErr } = await getClient().rpc("login_email", {
    p_username: u,
  })
  if (rpcErr) return { ok: false, error: rpcErr.message }
  if (!email) return { ok: false, error: "usuario_nao_existe" }

  const { data, error } = await getClient().auth.signInWithPassword({
    email,
    password: p,
  })
  if (error) {
    const msg = String(error.message || "")
    if (/invalid login credentials/i.test(msg)) {
      return { ok: false, error: "credenciais_invalidas" }
    }
    return { ok: false, error: msg }
  }
  return { ok: true, session: data.session, user: data.user }
}

/** Pré-checagem via RPC (segura contra colisão de username). */
async function usernameAvailable(username) {
  const { data, error } = await getClient().rpc("username_available", {
    p_username: username,
  })
  if (error) return { ok: false, error: error.message }
  return { ok: true, available: !!data }
}

/** Encerra a sessão (session.js é limpo pelo attachAuthPersistence). */
async function signOut() {
  const { error } = await getClient().auth.signOut()
  return error ? { ok: false, error: error.message } : { ok: true }
}

/** Estado atual da sessão (para o boot do renderer). */
async function status() {
  const { data, error } = await getClient().auth.getSession()
  if (error) return { session: null, error: error.message }
  return { session: data.session, error: null }
}

module.exports = { signUp, signIn, usernameAvailable, signOut, status }
