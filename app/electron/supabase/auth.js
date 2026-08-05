// Autenticação Supabase — MAIN PROCESS (nunca no renderer).
// Fluxo do MVP: email + código OTP de 6 dígitos (sem senha).
//  - requestCode(): envia o código; se o email ainda não tem conta, cria a
//    conta na hora (shouldCreateUser) com o username no metadata (o trigger
//    handle_new_user cria o profile).
//  - verifyCode(): valida o código e completa o login (sessão → session.js
//    via attachAuthPersistence no client).
//  - Recuperação de acesso = pedir um código novo (mesmo fluxo).
"use strict"

const crypto = require("crypto")
const { getClient } = require("./client")

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/
const USERNAME_RE = /^[a-z0-9_]{3,20}$/

/**
 * Cadastro instantâneo (libertário: SEM verificação de email).
 * Pré-requisito: "Confirm email" DESLIGADO no painel do Supabase
 * (Authentication → Providers → Email). Sem isso o signUp não devolve
 * sessão e o app avisa.
 * Senha aleatória gerada internamente — o usuário nunca digita senha; a
 * sessão vem pronta e é persistida em session.json.
 */
async function signUp({ email, username } = {}) {
  const e = String(email || "").trim().toLowerCase()
  if (!EMAIL_RE.test(e)) return { ok: false, error: "email_invalido" }

  const u = String(username || "").trim().toLowerCase()
  if (!USERNAME_RE.test(u)) return { ok: false, error: "username_invalido" }

  const chk = await usernameAvailable(u)
  if (!chk.ok) return chk
  if (!chk.available) return { ok: false, error: "username_ocupado" }

  const password = crypto.randomBytes(24).toString("base64url")
  const { data, error } = await getClient().auth.signUp({
    email: e,
    password,
    options: { data: { username: u } },
  })
  if (error) return { ok: false, error: error.message }
  if (!data?.session) {
    return { ok: false, error: "confirmacao_necessaria" } // "Confirm email" ainda ligado
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

/** Envia o código OTP por email (cria a conta se o email for novo). */
async function requestCode({ email, username } = {}) {
  const e = String(email || "").trim().toLowerCase()
  if (!EMAIL_RE.test(e)) return { ok: false, error: "email_invalido" }

  const u = String(username || "").trim().toLowerCase()
  if (u && !USERNAME_RE.test(u)) return { ok: false, error: "username_invalido" }
  if (u) {
    const chk = await usernameAvailable(u)
    if (!chk.ok) return chk
    if (!chk.available) return { ok: false, error: "username_ocupado" }
  }

  const options = { shouldCreateUser: true }
  if (u) options.data = { username: u }

  const { error } = await getClient().auth.signInWithOtp({ email: e, options })
  if (error) return { ok: false, error: error.message }
  return { ok: true }
}

/** Valida o código e completa o login. */
async function verifyCode({ email, token } = {}) {
  const e = String(email || "").trim().toLowerCase()
  const t = String(token || "").trim()
  if (!EMAIL_RE.test(e) || !/^\d{6}$/.test(t)) {
    return { ok: false, error: "codigo_invalido" }
  }
  const { data, error } = await getClient().auth.verifyOtp({
    email: e,
    token: t,
    type: "email",
  })
  if (error) return { ok: false, error: error.message }
  return { ok: true, session: data.session, user: data.user }
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

module.exports = { signUp, usernameAvailable, requestCode, verifyCode, signOut, status }
