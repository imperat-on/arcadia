// Amigos (Supabase) — MAIN PROCESS.
// Modelo: uma linha canônica por par (user_a < user_b) + requester_id + status.
"use strict"

const { getClient } = require("./client")

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
  return { ok: true }
}

/** Lista: amigos aceitos, pedidos recebidos e enviados (com username, avatar e created_at). */
async function list() {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }

  const client = getClient()
  const { data: rels, error } = await client
    .from("friendships")
    .select("user_a, user_b, status, requester_id, created_at")
    .or(`user_a.eq.${me},user_b.eq.${me}`)
  if (error) return { ok: false, error: error.message }

  const friends = []
  const incoming = []
  const outgoing = []
  const ids = new Set()
  for (const r of rels || []) {
    const other = r.user_a === me ? r.user_b : r.user_a
    ids.add(other)
    const info = { id: other, since: r.created_at ?? null }
    if (r.status === "accepted") friends.push(info)
    else if (r.requester_id === me) outgoing.push(info)
    else incoming.push(info)
  }

  const perfil = {}
  if (ids.size) {
    const { data: profs } = await client
      .from("profiles")
      .select("id, username, avatar_url, display_name")
      .in("id", [...ids])
    for (const p of profs || []) {
      perfil[p.id] = { username: p.username, avatar_url: p.avatar_url ?? null, display_name: p.display_name ?? null }
    }
  }
  const mk = (info) => ({
    id: info.id,
    username: perfil[info.id]?.username || "?",
    display_name: perfil[info.id]?.display_name ?? null,
    avatar_url: perfil[info.id]?.avatar_url ?? null,
    since: info.since ?? null,
  })

  return {
    ok: true,
    data: {
      friends: friends.map(mk),
      incoming: incoming.map(mk),
      outgoing: outgoing.map(mk),
    },
  }
}

/** Conquistas recentes do amigo (RPC security definer — só entre amigos). */
async function friendAchievements(friendId) {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado" }
  if (!friendId || friendId === me) return { ok: false, error: "destino_invalido" }

  const { data, error } = await getClient().rpc("friend_achievements", {
    p_friend: friendId,
  })
  if (error) return { ok: false, error: error.message }
  return { ok: true, achievements: data || [] }
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
  return { ok: true }
}

/**
 * Realtime: notifica quando ALGUÉM envia pedido pra mim (INSERT na friendships
 * com user_b = meu id). Devolve { start, stop }.
 */
function watchRequests(broadcast) {
  let channel = null

  const stop = () => {
    if (channel) {
      try {
        getClient().removeChannel(channel)
      } catch {
        /* ignore */
      }
      channel = null
    }
  }

  const start = async () => {
    stop()
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
  removeFriend,
  watchRequests,
}
