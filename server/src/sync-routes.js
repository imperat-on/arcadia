"use strict"

const { db, nowEpochS, withTransaction } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { notifyLibraryChange, notifyAchievementsChange } = require("./realtime")
const asyncHandler = require("./async-handler")

function requireAuth(req) {
  const v = verifyToken(extractToken(req) || "")
  return v.ok ? v.sub : null
}

async function rpcSyncAchievements(uid, p_items) {
  if (!Array.isArray(p_items)) return []
  const changed = await withTransaction(async (client) => {
    const rows = []
    const numeric = /^[0-9]+$/
    const now = nowEpochS()
    for (const item of p_items.slice(0, 1000)) {
      const appid = item?.appid
      const apiname = item?.apiname
      const timestamp = item?.unlocked_at
      if (
        !appid || !apiname || timestamp === undefined || !numeric.test(String(timestamp)) ||
        !Number.isSafeInteger(Number(timestamp))
      ) continue
      const result = await client.query(
        `INSERT INTO user_achievements
           (user_id, appid, apiname, unlocked_at, title, icon, percent)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (user_id, appid, apiname) DO UPDATE SET
           unlocked_at = LEAST(user_achievements.unlocked_at, excluded.unlocked_at),
           updated_at = now(),
           title = COALESCE(excluded.title, user_achievements.title),
           icon = COALESCE(excluded.icon, user_achievements.icon),
           percent = COALESCE(excluded.percent, user_achievements.percent)
         WHERE user_achievements.unlocked_at > excluded.unlocked_at
         RETURNING unlocked_at`,
        [
          uid,
          appid,
          apiname,
          Math.min(Number(timestamp), now),
          item.title ? String(item.title) : null,
          item.icon ? String(item.icon) : null,
          item.percent != null ? Number(item.percent) : null,
        ],
      )
      if (result.rows[0]) rows.push(result.rows[0])
    }
    return rows
  })
  if (changed.length) notifyAchievementsChange(uid)
  return changed
}

async function rpcPullAchievements(uid, p_since) {
  const since = p_since ? String(p_since) : null
  const result = since
    ? await db.query(
        "SELECT * FROM user_achievements WHERE user_id = $1 AND updated_at > $2 ORDER BY updated_at",
        [uid, since],
      )
    : await db.query(
        "SELECT * FROM user_achievements WHERE user_id = $1 ORDER BY updated_at",
        [uid],
      )
  return result.rows
}

async function rpcFriendAchievements(uid, p_friend) {
  if (!p_friend || p_friend === uid) return []
  const friendship = await db.query(
    `SELECT 1 FROM friendships WHERE status = 'accepted' AND
     ((user_a = $1 AND user_b = $2) OR (user_a = $2 AND user_b = $1))`,
    [uid, p_friend],
  )
  if (!friendship.rows[0]) return []

  const block = await db.query(
    `SELECT 1 FROM blocks WHERE
     (blocker_id = $1 AND blocked_id = $2) OR (blocker_id = $2 AND blocked_id = $1)`,
    [uid, p_friend],
  )
  if (block.rows[0]) return []

  const profile = await db.query("SELECT profile_visibility FROM profiles WHERE id = $1", [p_friend])
  if (!profile.rows[0] || profile.rows[0].profile_visibility === "private") return []

  return (
    await db.query(
      "SELECT * FROM user_achievements WHERE user_id = $1 ORDER BY unlocked_at DESC LIMIT 30",
      [p_friend],
    )
  ).rows
}

async function rpcFriendProfile(uid, p_friend) {
  if (!p_friend || p_friend === uid) return { ok: false, error: "destino_invalido" }
  const friendship = await db.query(
    `SELECT 1 FROM friendships WHERE status = 'accepted' AND
     ((user_a = $1 AND user_b = $2) OR (user_a = $2 AND user_b = $1))`,
    [uid, p_friend],
  )
  if (!friendship.rows[0]) return { ok: false, error: "nao_sao_amigos" }
  const block = await db.query(
    `SELECT 1 FROM blocks WHERE
     (blocker_id = $1 AND blocked_id = $2) OR (blocker_id = $2 AND blocked_id = $1)`,
    [uid, p_friend],
  )
  if (block.rows[0]) return { ok: false, error: "perfil_indisponivel" }
  const profile = await db.query(
    `SELECT username, avatar_url, display_name, summary, country, city, showcase,
            background_url, banner_url, profile_visibility
     FROM profiles WHERE id = $1`,
    [p_friend],
  )
  if (!profile.rows[0] || profile.rows[0].profile_visibility === "private") return { ok: false, error: "perfil_privado" }
  const library = await db.query(
    `SELECT l.appid, l.title, l.platform, COALESCE(p.minutes, 0) AS minutes
     FROM user_library l LEFT JOIN user_playtime p ON p.user_id = l.user_id AND p.appid = l.appid
     WHERE l.user_id = $1 ORDER BY COALESCE(p.minutes, 0) DESC, l.title`,
    [p_friend],
  )
  const friendList = await db.query(
    `SELECT CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END AS id,
            p.username, p.display_name, p.avatar_url
     FROM friendships f
     JOIN profiles p ON p.id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
     WHERE f.status = 'accepted' AND (f.user_a = $1 OR f.user_b = $1)
     ORDER BY COALESCE(p.display_name, p.username)`,
    [p_friend],
  )
  const playtime = library.rows.reduce((total, game) => total + Number(game.minutes || 0), 0)
  return {
    ok: true,
    profile: profile.rows[0],
    games: library.rows,
    friends: friendList.rows,
    stats: { jogos: library.rows.length, playtime_hours: Math.round((playtime / 60) * 10) / 10 },
  }
}

async function rpcPushLibrary(uid, p_lib, p_playtime) {
  const library = Array.isArray(p_lib) ? p_lib.slice(0, 1000) : []
  const playtime = Array.isArray(p_playtime) ? p_playtime.slice(0, 1000) : []
  await withTransaction(async (client) => {
    for (const game of library) {
      if (!game?.appid) continue
      if (game.removed) {
        await client.query("DELETE FROM user_library WHERE user_id = $1 AND appid = $2", [uid, game.appid])
        await client.query("DELETE FROM user_playtime WHERE user_id = $1 AND appid = $2", [uid, game.appid])
      } else {
        await client.query(
          `INSERT INTO user_library (user_id, appid, title, platform) VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id, appid) DO UPDATE SET
             title = excluded.title, platform = excluded.platform, updated_at = now()`,
          [uid, game.appid, game.title || game.appid, game.platform || "windows"],
        )
      }
    }

    const integer = /^-?[0-9]+$/
    for (const item of playtime) {
      if (!item?.appid || !integer.test(String(item.minutes))) continue
      const minutes = Number(item.minutes)
      if (!Number.isSafeInteger(minutes) || minutes <= 0 || minutes > 999999) continue
      await client.query(
        `INSERT INTO user_playtime (user_id, appid, minutes) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, appid) DO UPDATE SET
           minutes = LEAST(999999, GREATEST(0, user_playtime.minutes + excluded.minutes)),
           updated_at = now()`,
        [uid, item.appid, minutes],
      )
    }
  })
  if (library.length || playtime.length) notifyLibraryChange(uid)
}

async function rpcPullLibrary(uid) {
  return (
    await db.query(
      `SELECT l.appid, l.title, l.platform, COALESCE(p.minutes, 0) AS minutes
       FROM user_library l
       LEFT JOIN user_playtime p ON p.user_id = l.user_id AND p.appid = l.appid
       WHERE l.user_id = $1 ORDER BY l.added_at`,
      [uid],
    )
  ).rows
}

const RE_SOURCE_ID = /^[0-9a-f]{12}$/
const RE_URL = /^https?:\/\//

async function rpcPushSources(uid, p_sources) {
  if (!Array.isArray(p_sources)) return
  await withTransaction(async (client) => {
    for (const source of p_sources) {
      const sourceId = source?.source_id
      if (!sourceId || !RE_SOURCE_ID.test(sourceId)) continue
      if (source.removed) {
        await client.query(
          "UPDATE user_sources SET removed_at = now() WHERE user_id = $1 AND source_id = $2",
          [uid, sourceId],
        )
      } else if (source.url && RE_URL.test(source.url)) {
        await client.query(
          `INSERT INTO user_sources (user_id, source_id, url, name) VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id, source_id) DO UPDATE SET
             url = excluded.url, name = excluded.name, removed_at = NULL`,
          [uid, sourceId, source.url, source.name || ""],
        )
      }
    }
  })
}

async function rpcPullSources(uid) {
  return (
    await db.query(
      `SELECT source_id, url, name FROM user_sources
       WHERE user_id = $1 AND removed_at IS NULL ORDER BY added_at`,
      [uid],
    )
  ).rows
}

function registerSyncRoutes(app) {
  const authed = (fn) => asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    res.json(await fn(uid, req.body || {}))
  })

  app.post("/rest/v1/rpc/sync_achievements", authed((uid, body) => rpcSyncAchievements(uid, body.p_items)))
  app.post("/rest/v1/rpc/pull_achievements", authed((uid, body) => rpcPullAchievements(uid, body.p_since)))
  app.post("/rest/v1/rpc/friend_achievements", authed((uid, body) => rpcFriendAchievements(uid, body.p_friend)))
  app.post("/rest/v1/rpc/friend_profile", authed((uid, body) => rpcFriendProfile(uid, body.p_friend)))
  app.post("/rest/v1/rpc/push_library", authed((uid, body) => rpcPushLibrary(uid, body.p_lib, body.p_playtime)))
  app.post("/rest/v1/rpc/pull_library", authed((uid) => rpcPullLibrary(uid)))
  app.post("/rest/v1/rpc/push_sources", authed((uid, body) => rpcPushSources(uid, body.p_sources)))
  app.post("/rest/v1/rpc/pull_sources", authed((uid) => rpcPullSources(uid)))
}

module.exports = {
  registerSyncRoutes,
  rpcSyncAchievements,
  rpcPullAchievements,
  rpcFriendAchievements,
  rpcFriendProfile,
  rpcPushLibrary,
  rpcPullLibrary,
  rpcPushSources,
  rpcPullSources,
}
