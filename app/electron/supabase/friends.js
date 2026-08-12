// Amigos (backend proprio) — MAIN PROCESS.
// Modelo: uma linha canônica por par (user_a < user_b) + requester_id + status.
"use strict"

const fs = require("node:fs")
const { getClient } = require("./client")
const { caminhoArquivoConta } = require("./conta")

// Cache local da lista (por conta): a 1ª pintura da aba amigos/perfil sai do
// disco (instantâneo) e o refresh em background atualiza via friends:changed.
// TTL curto, o cache é só para a primeira pintura, nunca fonte de verdade.
const CACHE_TTL_MS = 60_000
// TTL do cache de conquistas do amigo: se a reabertura do perfil bater no
// cache (sem fetch à Steam), a grade renderiza instantânea — sem o delay de
// título/ícone que antes acompanhava a abertura. O fetch revalida quando
// expira; TTL curto garante que os dados nunca envelhecem demais.
const FRIEND_ACH_TTL_MS = 5 * 60_000

/** Callback de atualização em background (registrado pelo ipc.js). */
let onAtualizadoCb = null
function onAtualizado(cb) {
  onAtualizadoCb = cb
}

function cacheFile() {
  return caminhoArquivoConta("friends_cache.json")
}

function gravarCache(data) {
  try {
    const file = cacheFile()
    const tmp = `${file}.tmp-${process.pid}-${Date.now()}`
    fs.writeFileSync(tmp, JSON.stringify({ ts: Date.now(), data }), { encoding: "utf8", mode: 0o600 })
    fs.renameSync(tmp, file)
    try {
      fs.chmodSync(file, 0o600)
    } catch {}
  } catch {
    /* cache é otimização, nunca requisito */
  }
}

function lerCache() {
  try {
    const c = JSON.parse(fs.readFileSync(cacheFile(), "utf8"))
    if (c && typeof c.ts === "number" && c.data && Date.now() - c.ts < CACHE_TTL_MS) return c.data
  } catch {}
  return null
}

function invalidarCache() {
  try {
    fs.rmSync(cacheFile(), { force: true })
  } catch {}
}

/** Par canônico: ordena os ids para (user_a, user_b) com user_a < user_b. */
function canonicalPair(a, b) {
  return a < b ? { user_a: a, user_b: b } : { user_a: b, user_b: a }
}

async function requireUserId() {
  const { data, error } = await getClient().auth.getUser()
  if (error || !data?.user) return null
  return data.user.id
}

/** Busca usuários por prefixo do username (exclui self; marca relação). */
async function search(query) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }

  const q = String(query || "").trim().toLowerCase()
  if (!q) return { ok: true, results: [] }

  const client = getClient()
  const { data: profiles, error } = await client
    .from("profiles")
    .select("id, username, avatar_url, display_name")
    .ilike("username", q + "%")
    .limit(20)
  if (error) return { ok: false, error: error.message }

  const encontrados = profiles.filter((p) => p.id !== me)
  if (!encontrados.length) return { ok: true, results: [] }

  // Relações existentes comigo (para marcar status na busca)
  const { data: rels } = await client
    .from("friendships")
    .select("user_a, user_b, status, requester_id")
    .or(`user_a.eq.${me},user_b.eq.${me}`)

  const relByUser = {}
  for (const r of rels || []) {
    const other = r.user_a === me ? r.user_b : r.user_a
    relByUser[other] = { status: r.status, requester_id: r.requester_id }
  }

  const results = encontrados.map((p) => ({
    id: p.id,
    username: p.username,
    display_name: p.display_name ?? null,
    avatar_url: p.avatar_url ?? null,
    status: relByUser[p.id]?.status ?? null,
    incoming: relByUser[p.id]?.requester_id === p.id,
  }))
  return { ok: true, results }
}

/** Envia pedido de amizade (RLS garante requester + pending). */
async function send(toUserId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }
  if (!toUserId || toUserId === me) return { ok: false, error: "destino_invalido" }

  const { user_a, user_b } = canonicalPair(me, toUserId)
  const { error } = await getClient().from("friendships").insert({
    user_a,
    user_b,
    requester_id: me,
    status: "pending",
  })
  if (error) return { ok: false, error: error.message }
  invalidarCache()
  return { ok: true }
}

/** Aceita um pedido recebido (só o addressee, só se pending). */
async function accept(friendId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }

  const { error } = await getClient()
    .from("friendships")
    .update({ status: "accepted" })
    .or(`and(user_a.eq.${me},user_b.eq.${friendId}),and(user_a.eq.${friendId},user_b.eq.${me})`)
    .eq("status", "pending")
    .eq("requester_id", friendId)
  if (error) return { ok: false, error: error.message }
  invalidarCache()
  return { ok: true }
}

/** Cancela um pedido pendente (só o requester). */
async function cancel(friendId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }

  const { error } = await getClient()
    .from("friendships")
    .delete()
    .or(`and(user_a.eq.${me},user_b.eq.${friendId}),and(user_a.eq.${friendId},user_b.eq.${me})`)
    .eq("status", "pending")
    .eq("requester_id", me)
  if (error) return { ok: false, error: error.message }
  invalidarCache()
  return { ok: true }
}

/**
 * Busca a lista NO SERVIDOR e grava o cache. UMA query com os perfis embutidos
 * (embedded resources — o "outro lado" é user_a ou user_b conforme quem sou eu);
 * ~0.6s no total (era 2 queries sequenciais, ~1.4s).
 * Separado do list() para poder rodar em background quando há cache.
 */
async function buscarFresco() {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }

  const client = getClient()
  const { data: rels, error } = await client
    .from("friendships")
    .select(
      `user_a, user_b, status, requester_id, created_at,
       pa:profiles!friendships_user_a_fkey(id, username, avatar_url, display_name),
       pb:profiles!friendships_user_b_fkey(id, username, avatar_url, display_name)`,
    )
    .or(`user_a.eq.${me},user_b.eq.${me}`)
  if (error) return { ok: false, error: error.message }

  const friends = []
  const incoming = []
  const outgoing = []
  const perfil = {}
  for (const r of rels || []) {
    const other = r.user_a === me ? r.user_b : r.user_a
    // PostgREST devolve to-one ora como objeto, ora como array [obj] — normaliza.
    const pa = Array.isArray(r.pa) ? r.pa?.[0] : r.pa
    const pb = Array.isArray(r.pb) ? r.pb?.[0] : r.pb
    const p = (r.user_a === me ? pb : pa) || null
    perfil[other] = p
      ? { username: p.username, avatar_url: p.avatar_url ?? null, display_name: p.display_name ?? null }
      : null
    const info = { id: other, since: r.created_at ?? null }
    if (r.status === "accepted") friends.push(info)
    else if (r.requester_id === me) outgoing.push(info)
    else incoming.push(info)
  }
  const mk = (info) => ({
    id: info.id,
    username: perfil[info.id]?.username || "?",
    display_name: perfil[info.id]?.display_name ?? null,
    avatar_url: perfil[info.id]?.avatar_url ?? null,
    since: info.since ?? null,
  })

  const data = {
    friends: friends.map(mk),
    incoming: incoming.map(mk),
    outgoing: outgoing.map(mk),
  }
  gravarCache(data)
  return { ok: true, data }
}

/**
 * Lista: amigos aceitos, pedidos recebidos e enviados (com username, avatar e
 * created_at). Com cache válido (< 60s) e sem forcar: retorna o cache NA HORA
 * e atualiza em background (avisa via onAtualizado → friends:changed).
 */
async function list({ forcar } = {}) {
  if (!forcar) {
    const cache = lerCache()
    if (cache) {
      // Pintura instantânea; o background substitui pelo fresco em ~0.6s.
      buscarFresco()
        .then((r) => {
          if (r.ok && onAtualizadoCb) onAtualizadoCb(r.data)
        })
        .catch(() => {})
      return { ok: true, data: cache, deCache: true }
    }
  }
  const r = await buscarFresco()
  return { ok: r.ok, data: r.data, deCache: false, error: r.error }
}

// Cache das conquistas de AMIGOS (por par conta+amigo): abrir o mesmo perfil
// duas vezes em seguida nao repete o round-trip ao servidor (~1.2s no tunel).
// TTL curto — os dados servem so para a pintura imediata; o que for fresco o
// usuario ve logo, e a aba reabre sempre rapido.
function achCacheFile() {
  return caminhoArquivoConta("friend_achievements_cache.json")
}

function lerAchCache(friendId, ttl = CACHE_TTL_MS) {
  try {
    const c = JSON.parse(fs.readFileSync(achCacheFile(), "utf8"))
    const e = c?.[friendId]
    if (e && typeof e.ts === "number" && Date.now() - e.ts < ttl) return e.data
  } catch {}
  return null
}

function gravarAchCache(friendId, data) {
  try {
    const file = achCacheFile()
    let all = {}
    try {
      all = JSON.parse(fs.readFileSync(file, "utf8")) || {}
    } catch {
      /* primeira gravacao: arquivo ainda nao existe */
    }
    all[friendId] = { ts: Date.now(), data }
    const tmp = `${file}.tmp-${process.pid}-${Date.now()}`
    fs.writeFileSync(tmp, JSON.stringify(all), { encoding: "utf8", mode: 0o600 })
    fs.renameSync(tmp, file)
    try {
      fs.chmodSync(file, 0o600)
    } catch {}
  } catch {
    /* cache e otimizacao, nunca requisito */
  }
}

/** Conquistas recentes do amigo (RPC security definer — so entre amigos). */
async function friendAchievements(friendId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }
  if (!friendId || friendId === me) return { ok: false, error: "destino_invalido" }

  const cache = lerAchCache(friendId)
  if (cache) return { ok: true, achievements: cache, deCache: true }

  const { data, error } = await getClient().rpc("friend_achievements", {
    p_friend: friendId,
  })
  if (error) return { ok: false, error: error.message }
  gravarAchCache(friendId, data || [])
  return { ok: true, achievements: data || [] }
}

/** Conquistas do amigo COM fallback de cache — a reabertura do perfil mostra
 * os dados da última vez (5 min) sem refetch à Steam, renderizando imediato. */
async function friendAchievementsCached(friendId) {
  const cache = lerAchCache(friendId, FRIEND_ACH_TTL_MS)
  if (cache) return { ok: true, achievements: cache, deCache: true }
  return friendAchievements(friendId)
}

/** Remove amigo aceito (policy friends_delete_accepted — qualquer membro do par). */
async function removeFriend(friendId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }
  if (!friendId || friendId === me) return { ok: false, error: "destino_invalido" }

  const { error } = await getClient()
    .from("friendships")
    .delete()
    .or(`and(user_a.eq.${me},user_b.eq.${friendId}),and(user_a.eq.${friendId},user_b.eq.${me})`)
    .eq("status", "accepted")
  if (error) return { ok: false, error: error.message }
  invalidarCache()
  try {
    const file = achCacheFile()
    const all = JSON.parse(fs.readFileSync(file, "utf8")) || {}
    if (all[friendId]) {
      delete all[friendId]
      fs.writeFileSync(file, JSON.stringify(all), { encoding: "utf8", mode: 0o600 })
    }
  } catch {}
  return { ok: true }
}

/**
 * Realtime: notifica quando ALGUÉM envia pedido pra mim (INSERT na friendships
 * com user_b = meu id). Devolve { start, stop }.
 */
function watchRequests(broadcast) {
  let channel = null
  let iniciando = null // serializa starts concorrentes (SIGNED_IN duplo do boot)

  const stop = async () => {
    if (channel) {
      try {
        await getClient().removeChannel(channel)
      } catch {
        /* ignore */
      }
      channel = null
    }
  }

  const start = () => {
    // SIGNED_IN pode disparar 2x no boot (restauração de sessão em 2 pontos);
    // sem serialização, o 2º start adiciona .on() DEPOIS do subscribe() →
    // "cannot add postgres_changes callbacks after subscribe()".
    if (iniciando) return iniciando
    iniciando = (async () => {
      await stop()
      const me = await requireUserId()
      if (!me) return
      channel = getClient().channel(`friends-${me}`)
      channel.on(
        "postgres_changes",
        {
          event: "INSERT",
          schema: "public",
          table: "friendships",
          filter: `user_b=eq.${me}`,
        },
        (payload) => broadcast("friends:request", { from: payload.new?.requester_id }),
      )
      channel.subscribe()
    })().finally(() => {
      iniciando = null
    })
    return iniciando
  }

  return { start, stop }
}

module.exports = {
  canonicalPair,
  search,
  send,
  accept,
  cancel,
  list,
  friendAchievements,
  friendAchievementsCached,
  removeFriend,
  watchRequests,
  onAtualizado,
  gravarCache,
  lerCache,
  invalidarCache,
  achCacheFile,
  lerAchCache,
  gravarAchCache,
}
