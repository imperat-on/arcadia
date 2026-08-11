// Autenticação do backend — MAIN PROCESS (nunca no renderer).
// Fluxo: cadastro com EMAIL + USERNAME + SENHA (sem verificação de email,
// projeto libertário); login com USERNAME + SENHA.
// Detalhe técnico: o backend autentica por EMAIL, então o login por username
// resolve o email via RPC login_email (security definer, chamável por anon).
"use strict"

const fs = require("fs")
const path = require("path")
const { getClient } = require("./client")

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/
const USERNAME_RE = /^[a-z0-9_]{3,20}$/
const SENHA_MIN = 6

const AVATAR_MAX = 5 * 1024 * 1024 // 5MB (limite do bucket — GIFs animados são pesados)
const AVATAR_DIM_MAX = 512 // dimensão máxima do avatar (qualquer formato)
const AVATAR_ENC = 256 // estáticos são re-encodados em ≤256px (economiza storage)
const AVATAR_EXT = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
}

/**
 * Lê as dimensões do cabeçalho da imagem (PNG/GIF/JPEG/WebP) SEM lib —
 * ~40 linhas puras; roda em Node puro (testável sem Electron).
 */
function dimensoesDeImagem(buf) {
  if (!buf || buf.length < 16) return null
  // PNG: "�PNG" + IHDR — w/h big-endian nos bytes 16/20
  if (buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4e && buf[3] === 0x47) {
    if (buf.length < 24) return null
    return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) }
  }
  // GIF: "GIF87a"/"GIF89a" — w/h little-endian nos bytes 6/8
  if (buf[0] === 0x47 && buf[1] === 0x49 && buf[2] === 0x46) {
    return { w: buf.readUInt16LE(6), h: buf.readUInt16LE(8) }
  }
  // JPEG: 0xFFD8 — varre marcadores até achar SOF (C0-CF, sem C4/C8/CC)
  if (buf[0] === 0xff && buf[1] === 0xd8) {
    let i = 2
    while (i + 9 < buf.length) {
      if (buf[i] !== 0xff) {
        i++
        continue
      }
      const m = buf[i + 1]
      if (m >= 0xc0 && m <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(m)) {
        return { h: buf.readUInt16BE(i + 5), w: buf.readUInt16BE(i + 7) }
      }
      i += 2 + buf.readUInt16BE(i + 2)
    }
    return null
  }
  // WebP: "RIFF....WEBP" com chunk VP8 / VP8L / VP8X
  if (buf[0] === 0x52 && buf[1] === 0x49 && buf[2] === 0x46 && buf[3] === 0x46 && buf[8] === 0x57 && buf[9] === 0x45) {
    const vp8 = buf.toString("ascii", 12, 16)
    if (vp8 === "VP8 ") return { w: buf.readUInt16LE(26) & 0x3fff, h: buf.readUInt16LE(28) & 0x3fff }
    if (vp8 === "VP8L") {
      const b = buf.readUInt32LE(21)
      return { w: (b & 0x3fff) + 1, h: ((b >> 14) & 0x3fff) + 1 }
    }
    if (vp8 === "VP8X") return { w: 1 + buf.readUIntLE(24, 3), h: 1 + buf.readUIntLE(27, 3) }
    return null
  }
  return null
}

/**
 * Prepara o avatar: valida dimensão (512 máx) e re-encoda estáticos em ≤256px
 * (nativeImage do Electron — só disponível no app real; GIF passa direto,
 * preservando a animação, apenas com o teto de dimensão).
 */
function processaAvatar(buf, ext) {
  const dims = dimensoesDeImagem(buf)
  if (dims && (dims.w > AVATAR_DIM_MAX || dims.h > AVATAR_DIM_MAX)) {
    return { erro: "avatar_dimensoes" }
  }
  if (ext === ".gif") return { buf, ext, mime: "image/gif" } // animação preservada
  if (dims && (dims.w > AVATAR_ENC || dims.h > AVATAR_ENC)) {
    try {
      const { nativeImage } = require("electron") // lazy: Node puro não tem
      const img = nativeImage.createFromBuffer(buf)
      if (!img.isEmpty()) {
        const escala = Math.min(AVATAR_ENC / dims.w, AVATAR_ENC / dims.h)
        const red = img.resize({
          width: Math.max(1, Math.round(dims.w * escala)),
          height: Math.max(1, Math.round(dims.h * escala)),
        })
        if (ext === ".jpg" || ext === ".jpeg") {
          const j = red.toJPEG(85)
          if (j && j.length) return { buf: j, ext: ".jpg", mime: "image/jpeg" }
        } else {
          const p = red.toPNG()
          if (p && p.length) return { buf: p, ext: ".png", mime: "image/png" }
        }
      }
    } catch {
      /* fallback: original */
    }
  }
  return { buf, ext, mime: AVATAR_EXT[ext] || "image/png" }
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
 * Pré-requisito: o servidor nao exige confirmacao de email
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
  // Colisão de username: o trigger handle_new_user pode ter renomeado o perfil
  // (joao → joao_1) SEM atualizar o user_metadata do auth. Busca o username
  // REAL criado pra UI exibir certo ("sua conta é joao_1", não "joao").
  let usernameReal = u
  try {
    const p = await myProfile()
    if (p?.ok && p.profile?.username) usernameReal = p.profile.username
  } catch {
    /* fallback: mantém o username pedido */
  }
  return { ok: true, session: data.session, user: data.user, usernameReal }
}

/** Login com username + senha (verifica a senha no SQL; o email só sai
 *  DEPOIS da senha correta — login_email foi removido por vazar emails ao anon). */
async function signIn({ username, password } = {}) {
  const u = String(username || "").trim().toLowerCase()
  const p = String(password || "")
  if (!USERNAME_RE.test(u)) return { ok: false, error: "username_invalido" }
  if (!p) return { ok: false, error: "senha_curta" }

  const { data: check, error: rpcErr } = await getClient().rpc("login_check", {
    p_username: u,
    p_password: p,
  })
  if (rpcErr) return { ok: false, error: rpcErr.message }
  if (!check?.ok) return { ok: false, error: check?.error || "usuario_nao_existe" }
  const email = check.email
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
 * Nome ÚNICO por upload (<userid>/<ts>.<ext>): a URL muda a cada troca →
 * sem problema de cache no <img> (o antigo era sempre o mesmo nome e o
 * navegador mostrava a imagem velha). Remove o arquivo antigo depois.
 * SEGURANÇA (auditoria A-05): só sobe o que for IMAGEM de verdade — magic
 * bytes validados; nada de publicar arquivo arbitrário do disco em bucket público.
 */

// Magic bytes aceitos: PNG, JPEG, WebP, GIF (em ordem).
const MAGIC = [
  { sig: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], tipo: "png" },
  { sig: [0xff, 0xd8, 0xff], tipo: "jpg" },
  { sig: [0x52, 0x49, 0x46, 0x46], tipo: "webp" }, // RIFF....WEBP
  { sig: [0x47, 0x49, 0x46, 0x38], tipo: "gif" },
]

function magicDeImagem(buf) {
  if (!Buffer.isBuffer(buf) || buf.length < 12) return null
  for (const m of MAGIC) {
    if (buf.subarray(0, m.sig.length).equals(Buffer.from(m.sig))) {
      if (m.tipo === "webp") {
        // RIFF + "WEBP" nos bytes 8-11
        if (buf.toString("latin1", 8, 12) === "WEBP") return "webp"
        continue
      }
      return m.tipo
    }
  }
  return null
}

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
  // Só imagem de verdade (magic bytes) — senão o arquivo ia parar num bucket
  // público (ex.: ~/.ssh/id_rsa renomeado pra .png).
  if (!magicDeImagem(buf)) return { ok: false, error: "avatar_nao_imagem" }

  const ext = (path.extname(caminhoDeArquivo(filePath)) || "").toLowerCase()
  const extOk = AVATAR_EXT[ext]
  // Valida dimensão (512 máx) e re-encoda estáticos em ≤256px (GIF passa).
  const proc = processaAvatar(buf, extOk ? ext : ".png")
  if (proc.erro) return { ok: false, error: proc.erro }

  return uploadAvatarBytes(me, proc.buf, proc.mime, proc.ext)
}

/** Avatar vindo do RECORTE interativo (renderer): bytes prontos (PNG 256²). */
async function setAvatarBytes(buf, mime, ext) {
  const { data: ud, error: ue } = await getClient().auth.getUser()
  if (ue || !ud?.user) return { ok: false, error: "nao_logado" }
  if (!buf || !buf.length) return { ok: false, error: "avatar_ilegivel" }
  if (buf.length > AVATAR_MAX) return { ok: false, error: "avatar_grande" }
  // Magic bytes: o renderer pode mandar QUALQUER byte (mime/ext não provam
  // nada) — sem imagem de verdade, nada sobe pro bucket público.
  if (!magicDeImagem(Buffer.from(buf))) return { ok: false, error: "avatar_nao_imagem" }
  return uploadAvatarBytes(ud.user.id, Buffer.from(buf), mime || "image/png", ext || ".png")
}

/** Upload + gravação da URL (comum ao caminho de arquivo e bytes recortados). */
async function uploadAvatarBytes(me, buf, mime, extFinal) {
  if (buf.length > AVATAR_MAX) return { ok: false, error: "avatar_grande" }

  // Avatar atual (pra limpeza do arquivo antigo depois)
  const { data: atual } = await getClient()
    .from("profiles")
    .select("avatar_url")
    .eq("id", me)
    .maybeSingle()
  const urlAntiga = atual?.avatar_url || ""
  const nomeAntigo = urlAntiga.startsWith(`${getClient().supabaseUrl}/storage/v1/object/public/avatars/`)
    ? urlAntiga.split("/public/avatars/")[1]
    : null

  // Path <uid>/<arquivo>: a política do storage exige o uid no PREFIXO do
  // caminho (storage.foldername(name)[1] = auth.uid()) — nome plano antigo
  // (uid-ts.gif) quebraria o upload depois da migração v6.
  const nome = `${me}/${Date.now()}${extFinal}`
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

module.exports = { signUp, signIn, usernameAvailable, signOut, status, myProfile, updateProfile, setAvatar, setAvatarBytes, caminhoDeArquivo, dimensoesDeImagem, processaAvatar }
