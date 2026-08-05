// Autenticação Supabase — MAIN PROCESS (nunca no renderer).
// Fluxo: cadastro com EMAIL + USERNAME + SENHA (sem verificação de email,
// projeto libertário); login com USERNAME + SENHA.
// Detalhe técnico: o Supabase autentica por EMAIL, então o login por username
// resolve o email via RPC login_email (security definer, chamável por anon).
"use strict"

const fs = require("fs")
const path = require("path")
const { getClient } = require("./client")

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/
const USERNAME_RE = /^[a-z0-9_]{3,20}$/
const SENHA_MIN = 6

const AVATAR_MAX = 5 * 1024 * 1024 // 5MB (limite do bucket — GIFs animados são pesados)
const AVATAR_EXT = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
}

/**
 * O pickImage do launcher devolve "file:///caminho/avatar.png?t=123" (URL com
 * cache-buster). O fs não lê isso — normaliza pra caminho puro.
 */
function caminhoDeArquivo(p) {
  let s = String(p || "")
  if (s.startsWith("file://")) s = s.slice("file://".length)
  s = s.split("?")[0].split("#")[0]
  try {
    s = decodeURIComponent(s)
  } catch {
    /* caminho já era cru */
  }
  return s
}

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

/** Perfil online do usuário logado (username + avatar + campos do perfil). */
async function myProfile() {
  const { data: ud, error: ue } = await getClient().auth.getUser()
  if (ue || !ud?.user) return { ok: false, error: "nao_logado" }
  const me = ud.user.id

  const { data, error } = await getClient()
    .from("profiles")
    .select("username, avatar_url, display_name, summary, country, city, showcase")
    .eq("id", me)
    .maybeSingle()
  if (error) return { ok: false, error: error.message }
  return {
    ok: true,
    profile: {
      username: data?.username ?? null,
      avatar_url: data?.avatar_url ?? null,
      display_name: data?.display_name ?? null,
      summary: data?.summary ?? null,
      country: data?.country ?? null,
      city: data?.city ?? null,
      showcase: Array.isArray(data?.showcase) ? data.showcase : [],
    },
  }
}

// Whitelist de campos editáveis do perfil online (nunca username/email/id).
const CAMPOS_PERFIL = ["display_name", "summary", "country", "city", "showcase"]

/** Grava campos do perfil online (whitelist). RLS: só a própria linha. */
async function updateProfile(campos) {
  const { data: ud, error: ue } = await getClient().auth.getUser()
  if (ue || !ud?.user) return { ok: false, error: "nao_logado" }
  const me = ud.user.id

  const alvo = {}
  for (const k of CAMPOS_PERFIL) {
    if (campos && campos[k] !== undefined) alvo[k] = campos[k]
  }
  if (!Object.keys(alvo).length) return { ok: false, error: "sem_campos" }

  const { error } = await getClient()
    .from("profiles")
    .update(alvo)
    .eq("id", me)
  if (error) return { ok: false, error: error.message }
  return { ok: true }
}

/**
 * Sobe o avatar (arquivo local) pro Storage público e grava a URL em
 * profiles.avatar_url — assim amigos veem a foto. Exige o bucket "avatars"
 * (migracao-2026-08-05-avatar-bucket-gif.sql + policies v2).
 * Nome ÚNICO por upload (<userid>-<ts>.<ext>): a URL muda a cada troca →
 * sem problema de cache no <img> (o antigo era sempre o mesmo nome e o
 * navegador mostrava a imagem velha). Remove o arquivo antigo depois.
 */
async function setAvatar(filePath) {
  const { data: ud, error: ue } = await getClient().auth.getUser()
  if (ue || !ud?.user) return { ok: false, error: "nao_logado" }
  const me = ud.user.id

  let buf
  try {
    buf = fs.readFileSync(caminhoDeArquivo(filePath))
  } catch {
    return { ok: false, error: "avatar_ilegivel" }
  }
  if (buf.length > AVATAR_MAX) return { ok: false, error: "avatar_grande" }

  const ext = (path.extname(caminhoDeArquivo(filePath)) || "").toLowerCase()
  const extOk = AVATAR_EXT[ext]
  const mime = extOk || "image/png"

  // Avatar atual (pra limpeza do arquivo antigo depois)
  const { data: atual } = await getClient()
    .from("profiles")
    .select("avatar_url")
    .eq("id", me)
    .maybeSingle()
  const urlAntiga = atual?.avatar_url || ""
  const nomeAntigo = urlAntiga.startsWith(`${getClient().supabaseUrl}/storage/v1/object/public/avatars/`)
    ? urlAntiga.split("/").pop()
    : null

  const nome = `${me}-${Date.now()}${extOk ? ext : ".png"}`
  const { error: upErr } = await getClient()
    .storage.from("avatars")
    .upload(nome, buf, { upsert: false, contentType: mime })
  if (upErr) return { ok: false, error: upErr.message }

  const avatarUrl = `${getClient().supabaseUrl}/storage/v1/object/public/avatars/${nome}`
  const { error: dbErr } = await getClient()
    .from("profiles")
    .update({ avatar_url: avatarUrl })
    .eq("id", me)
  if (dbErr) return { ok: false, error: dbErr.message }

  // Limpeza do arquivo antigo (best effort — nunca derruba o upload)
  if (nomeAntigo && nomeAntigo !== nome) {
    try {
      await getClient().storage.from("avatars").remove([nomeAntigo])
    } catch {
      /* ignore */
    }
  }

  return { ok: true, avatar_url: avatarUrl }
}

module.exports = { signUp, signIn, usernameAvailable, signOut, status, myProfile, updateProfile, setAvatar, caminhoDeArquivo }
