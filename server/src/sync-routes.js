"use strict"

// RPCs de conquistas e biblioteca. Espelha os SQLs v2-v6:
//   sync_achievements, pull_achievements, friend_achievements
//   push_library, pull_library

const { db, nowIso, nowEpochS } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { notifyLibraryChange, notifyAchievementsChange } = require("./realtime")

function requireAuth(req) {
  const v = verifyToken(extractToken(req) || "")
  return v.ok ? v.sub : null
}

// ---------------------------------------------------------------------------
// sync_achievements: upsert com earliest-wins, metadados coalesce, sem backdating
// ---------------------------------------------------------------------------
function rpcSyncAchievements(uid, p_items) {
  if (!Array.isArray(p_items)) return []
  const alteradas = []
  const RE = /^[0-9]+(\.[0-9]+)?$/
  const now = nowEpochS()
  const select = db.prepare(
    "SELECT unlocked_at FROM user_achievements WHERE user_id = ? AND appid = ? AND apiname = ?"
  )
  const insert = db.prepare(
    `INSERT INTO user_achievements (user_id, appid, apiname, unlocked_at, updated_at, title, icon, percent)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(user_id, appid, apiname)
     DO UPDATE SET unlocked_at = min(user_achievements.unlocked_at, excluded.unlocked_at),
                   updated_at = datetime('now'),
                   title = coalesce(excluded.title, user_achievements.title),
                   icon = coalesce(excluded.icon, user_achievements.icon),
                   percent = coalesce(excluded.percent, user_achievements.percent)
     WHERE user_achievements.unlocked_at > excluded.unlocked_at`
  )

  db.exec("BEGIN")
  try {
    for (const i of p_items) {
      const appid = i?.appid
      const apiname = i?.apiname
      const ts = i?.unlocked_at
      if (!appid || !apiname || typeof ts === "undefined" || !RE.test(String(ts))) continue

      const unlocked = Math.min(Number(ts), now) // sem backdating
      const title = i.title ? String(i.title) : null
      const icon = i.icon ? String(i.icon) : null
      const percent = i.percent != null ? Number(i.percent) : null

      const antes = select.get(uid, appid, apiname)
      insert.run(uid, appid, apiname, unlocked, nowIso(), title, icon, percent)

      const depois = select.get(uid, appid, apiname)
      // considerada "alterada" se nao existia antes ou o unlocked mudou
      if (!antes || antes.unlocked_at !== depois.unlocked_at) alteradas.push(depois)
    }
    db.exec("COMMIT")
  } catch (e) {
    db.exec("ROLLBACK")
    throw e
  }
  // Avisa outros dispositivos logados (canal achievements-<uid>) so quando
  // algo de fato desbloqueou — sem isto, uma conquista feita numa maquina so
  // aparecia na outra no proximo boot/login.
  if (alteradas.length) notifyAchievementsChange(uid)
  return alteradas
}

// ---------------------------------------------------------------------------
// pull_achievements: delta desde p_since
// ---------------------------------------------------------------------------
function rpcPullAchievements(uid, p_since) {
  const since = p_since ? String(p_since) : null
  const rows = since
    ? db
        .prepare(
          "SELECT * FROM user_achievements WHERE user_id = ? AND updated_at > ? ORDER BY updated_at"
        )
        .all(uid, since)
    : db
        .prepare("SELECT * FROM user_achievements WHERE user_id = ? ORDER BY updated_at")
        .all(uid)
  return rows
}

// ---------------------------------------------------------------------------
// friend_achievements: 4 guardas + limit 30
// ---------------------------------------------------------------------------
function rpcFriendAchievements(uid, p_friend) {
  if (!p_friend || p_friend === uid) return []

  // guarda 1: sao amigos aceitos?
  const amizade = db
    .prepare(
      "SELECT 1 FROM friendships WHERE status = 'accepted' AND " +
        "((user_a = ? AND user_b = ?) OR (user_a = ? AND user_b = ?))"
    )
    .get(uid, p_friend, p_friend, uid)
  if (!amizade) return []

  // guarda 2: algum bloqueio?
  const block = db
    .prepare(
      "SELECT 1 FROM blocks WHERE (blocker_id = ? AND blocked_id = ?) OR (blocker_id = ? AND blocked_id = ?)"
    )
    .get(uid, p_friend, p_friend, uid)
  if (block) return []

  // guarda 3: perfil privado?
  const perfil = db
    .prepare("SELECT profile_visibility FROM profiles WHERE id = ?")
    .get(p_friend)
  if (!perfil || perfil.profile_visibility === "private") return []

  return db
    .prepare(
      "SELECT * FROM user_achievements WHERE user_id = ? ORDER BY unlocked_at DESC LIMIT 30"
    )
    .all(p_friend)
}

// ---------------------------------------------------------------------------
// push_library: upsert lib + playtime acumula com clamp
// ---------------------------------------------------------------------------
function rpcPushLibrary(uid, p_lib, p_playtime) {
  db.exec("BEGIN")
  try {
    const lib = Array.isArray(p_lib) ? p_lib : []
    for (const g of lib) {
      if (!g?.appid) continue
      if (g.removed) {
        db.prepare("DELETE FROM user_library WHERE user_id = ? AND appid = ?").run(uid, g.appid)
        db.prepare("DELETE FROM user_playtime WHERE user_id = ? AND appid = ?").run(uid, g.appid)
      } else {
        db.prepare(
          `INSERT INTO user_library (user_id, appid, title, platform) VALUES (?, ?, ?, ?)
           ON CONFLICT(user_id, appid) DO UPDATE SET
             title = excluded.title, platform = excluded.platform, updated_at = datetime('now')`
        ).run(uid, g.appid, g.title || g.appid, g.platform || "windows")
      }
    }

    const pt = Array.isArray(p_playtime) ? p_playtime : []
    const RE = /^-?[0-9]+$/
    for (const p of pt) {
      if (!p?.appid || !RE.test(String(p.minutes))) continue
      const minutos = Number(p.minutes)
      if (minutos <= 0) continue
      // acumula: soma ao existente, clamp [0, 999999]
      db.prepare(
        `INSERT INTO user_playtime (user_id, appid, minutes) VALUES (?, ?, ?)
         ON CONFLICT(user_id, appid) DO UPDATE SET
           minutes = min(999999, max(0, user_playtime.minutes + excluded.minutes)),
           updated_at = datetime('now')`
      ).run(uid, p.appid, minutos)
    }
    db.exec("COMMIT")
  } catch (e) {
    db.exec("ROLLBACK")
    throw e
  }
  // Avisa outros dispositivos logados (canal library-<uid>) so quando algo de
  // biblioteca de fato mudou — playtime sozinho nao justifica um pull instantaneo
  // nas outras maquinas, ele ja sobe no proximo pull normal.
  if (Array.isArray(p_lib) && p_lib.length) notifyLibraryChange(uid)
}

// ---------------------------------------------------------------------------
// pull_library: join library + playtime
// ---------------------------------------------------------------------------
function rpcPullLibrary(uid) {
  return db
    .prepare(
      "SELECT l.appid, l.title, l.platform, coalesce(p.minutes, 0) AS minutes " +
        "FROM user_library l LEFT JOIN user_playtime p ON p.user_id = l.user_id AND p.appid = l.appid " +
        "WHERE l.user_id = ? ORDER BY l.added_at"
    )
    .all(uid)
}

// ---------------------------------------------------------------------------
// push_sources / pull_sources: registro de fontes publicas (source_id, url,
// name). Nunca sincroniza etag/lastMod/count nem fontes com API key: esses
// dados sao estado local e nao entram aqui.
// ---------------------------------------------------------------------------
const RE_SOURCE_ID = /^[0-9a-f]{12}$/
const RE_URL = /^https?:\/\//

function rpcPushSources(uid, p_sources) {
  if (!Array.isArray(p_sources)) return
  const remove = db.prepare(
    "UPDATE user_sources SET removed_at = datetime('now') WHERE user_id = ? AND source_id = ?"
  )
  const upsert = db.prepare(
    `INSERT INTO user_sources (user_id, source_id, url, name) VALUES (?, ?, ?, ?)
     ON CONFLICT(user_id, source_id) DO UPDATE SET
       url = excluded.url, name = excluded.name, removed_at = NULL`
  )

  db.exec("BEGIN")
  try {
    for (const s of p_sources) {
      const sourceId = s?.source_id
      if (!sourceId || !RE_SOURCE_ID.test(sourceId)) continue

      if (s.removed) {
        remove.run(uid, sourceId)
      } else {
        if (!s.url || !RE_URL.test(s.url)) continue
        upsert.run(uid, sourceId, s.url, s.name || "")
      }
    }
    db.exec("COMMIT")
  } catch (e) {
    db.exec("ROLLBACK")
    throw e
  }
}

function rpcPullSources(uid) {
  return db
    .prepare(
      "SELECT source_id, url, name FROM user_sources WHERE user_id = ? AND removed_at IS NULL ORDER BY added_at"
    )
    .all(uid)
}

// ---------------------------------------------------------------------------
// Registro das rotas (todas exigem auth)
// ---------------------------------------------------------------------------
function registerSyncRoutes(app) {
  const authed = (fn) => (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    try {
      res.json(fn(uid, req.body || {}))
    } catch (e) {
      res.status(400).json({ error: String(e.message || e) })
    }
  }

  app.post("/rest/v1/rpc/sync_achievements", authed((uid, b) => rpcSyncAchievements(uid, b.p_items)))
  app.post("/rest/v1/rpc/pull_achievements", authed((uid, b) => rpcPullAchievements(uid, b.p_since)))
  app.post("/rest/v1/rpc/friend_achievements", authed((uid, b) => rpcFriendAchievements(uid, b.p_friend)))
  app.post("/rest/v1/rpc/push_library", authed((uid, b) => rpcPushLibrary(uid, b.p_lib, b.p_playtime)))
  app.post("/rest/v1/rpc/pull_library", authed((uid) => rpcPullLibrary(uid)))
  app.post("/rest/v1/rpc/push_sources", authed((uid, b) => rpcPushSources(uid, b.p_sources)))
  app.post("/rest/v1/rpc/pull_sources", authed((uid) => rpcPullSources(uid)))
}

module.exports = {
  registerSyncRoutes,
  rpcSyncAchievements,
  rpcPullAchievements,
  rpcFriendAchievements,
  rpcPushLibrary,
  rpcPullLibrary,
  rpcPushSources,
  rpcPullSources,
}
