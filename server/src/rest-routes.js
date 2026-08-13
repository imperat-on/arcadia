"use strict"

const { db } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { notifyFriendshipInsert } = require("./realtime")
const asyncHandler = require("./async-handler")

const COLUNAS_PROFILES = new Set([
  "id", "username", "avatar_url", "background_url", "banner_url", "steam_id",
  "display_name", "summary", "country", "city", "showcase", "profile_visibility",
  "show_location", "created_at",
])
const COLUNAS_FRIENDSHIPS = new Set([
  "user_a", "user_b", "requester_id", "status", "created_at", "updated_at",
])

function bind(params, value) {
  params.push(value)
  return `$${params.length}`
}

function filterClause(raw, permitidas, params) {
  const match = String(raw).match(/^([a-z_]+)\.(eq|ilike)\.(.*)$/)
  if (!match || !permitidas.has(match[1])) return null
  const placeholder = bind(params, match[3])
  return match[2] === "eq"
    ? `${match[1]} = ${placeholder}`
    : `${match[1]} ILIKE ${placeholder}`
}

function splitTopLevel(raw) {
  const parts = []
  let current = ""
  let depth = 0
  for (const char of String(raw || "")) {
    if (char === "(") depth++
    if (char === ")") depth--
    if (char === "," && depth === 0) {
      parts.push(current)
      current = ""
    } else {
      current += char
    }
  }
  if (current) parts.push(current)
  return parts
}

function parseOr(raw, permitidas, params) {
  raw = String(raw || "").trim()
  if (raw.startsWith("(") && raw.endsWith(")")) raw = raw.slice(1, -1)
  const groups = []
  for (const part of splitTopLevel(raw)) {
    const and = part.match(/^and\((.*)\)$/)
    const atoms = and ? splitTopLevel(and[1]) : [part]
    const clauses = atoms.map((atom) => filterClause(atom, permitidas, params)).filter(Boolean)
    if (clauses.length) groups.push(`(${clauses.join(and ? " AND " : " OR ")})`)
  }
  return groups.length ? `(${groups.join(" OR ")})` : null
}

function parseFilters(req, permitidas) {
  const sql = []
  const params = []
  for (const [key, value] of Object.entries(req.query)) {
    if (key === "select" || key === "limit" || key === "or" || !permitidas.has(key)) continue
    const clause = filterClause(`${key}.${value}`, permitidas, params)
    if (clause) sql.push(clause)
  }
  const or = parseOr(req.query.or, permitidas, params)
  if (or) sql.push(or)
  return { sql, params }
}

function requireAuth(req) {
  const v = verifyToken(extractToken(req) || "")
  return v.ok ? v.sub : null
}

function parseSelect(select, permitidas) {
  const colunas = []
  const embeds = []
  for (const part of splitTopLevel(String(select || "*"))) {
    const item = part.trim()
    if (!item) continue
    const match = item.match(/^([a-z_]+):profiles!([a-z_]+)\(([^)]*)\)$/i)
    if (match) {
      const columns = match[3].split(",").map((column) => column.trim())
      embeds.push({
        nome: match[1],
        fk: match[2],
        colunas: columns.filter((column) => COLUNAS_PROFILES.has(column)),
      })
    } else if (item === "*") {
      colunas.length = 0
    } else if (permitidas.has(item)) {
      colunas.push(item)
    }
  }
  return { colunas, embeds }
}

function embedProfile(colunas, profile) {
  if (!profile) return null
  const out = {}
  for (const coluna of colunas) if (coluna in profile) out[coluna] = profile[coluna]
  return out
}

function limitOf(req) {
  return Math.max(1, Math.min(Number(req.query.limit) || 100, 1000))
}

function friendshipPair(raw, uid) {
  const ids = new Set()
  const regex = /user_[ab]\.eq\.([^,)]+)/g
  let match
  while ((match = regex.exec(String(raw || "")))) ids.add(match[1])
  ids.delete(uid)
  if (ids.size !== 1) return null
  const friendId = [...ids][0]
  return uid < friendId ? [uid, friendId] : [friendId, uid]
}

function registerRestRoutes(app) {
  app.get("/rest/v1/:table", asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

    if (req.params.table === "profiles") {
      const { sql, params } = parseFilters(req, COLUNAS_PROFILES)
      const uidParam = bind(params, uid)
      sql.push(
        `(id = ${uidParam} OR profile_visibility = 'public' OR (profile_visibility = 'friends' AND EXISTS (` +
          `SELECT 1 FROM friendships f WHERE f.status = 'accepted' AND ` +
          `((f.user_a = profiles.id AND f.user_b = ${uidParam}) OR ` +
          `(f.user_b = profiles.id AND f.user_a = ${uidParam})))))`,
      )
      // Bloqueios anulam descoberta mutua, inclusive quando havia amizade.
      sql.push(
        `(id = ${uidParam} OR NOT EXISTS (` +
          `SELECT 1 FROM blocks b WHERE (b.blocker_id = ${uidParam} AND b.blocked_id = profiles.id) ` +
          `OR (b.blocker_id = profiles.id AND b.blocked_id = ${uidParam})))`,
      )
      const { colunas } = parseSelect(req.query.select, COLUNAS_PROFILES)
      const cols = colunas.length ? colunas.join(", ") : "id, username, avatar_url, display_name, summary, country, city, showcase, profile_visibility, show_location, created_at"
      const limit = bind(params, limitOf(req))
      const rows = (await db.query(
        `SELECT ${cols}, id AS __profile_id, show_location AS __show_location FROM profiles WHERE ${sql.join(" AND ")} LIMIT ${limit}`,
        params,
      )).rows
      for (const row of rows) {
        if (row.__profile_id !== uid && Number(row.__show_location) !== 1) {
          if (Object.prototype.hasOwnProperty.call(row, "country")) row.country = null
          if (Object.prototype.hasOwnProperty.call(row, "city")) row.city = null
        }
        delete row.__profile_id
        delete row.__show_location
      }
      return res.json(rows)
    }

    if (req.params.table === "friendships") {
      const { sql, params } = parseFilters(req, COLUNAS_FRIENDSHIPS)
      const uidParam = bind(params, uid)
      sql.push(`(user_a = ${uidParam} OR user_b = ${uidParam})`)
      const { colunas, embeds } = parseSelect(req.query.select, COLUNAS_FRIENDSHIPS)
      const cols = colunas.length ? colunas.join(", ") : "*"
      const limit = bind(params, limitOf(req))
      const rows = (await db.query(
        `SELECT ${cols} FROM friendships WHERE ${sql.join(" AND ")} LIMIT ${limit}`,
        params,
      )).rows

      if (embeds.length) {
        const profiles = (await db.query(
          "SELECT id, username, avatar_url, display_name FROM profiles",
        )).rows
        const byId = Object.fromEntries(profiles.map((profile) => [profile.id, profile]))
        for (const row of rows) {
          for (const embed of embeds) {
            const profile = embed.fk.includes("user_a") ? byId[row.user_a] : byId[row.user_b]
            row[embed.nome] = embedProfile(embed.colunas, profile)
          }
        }
      }
      return res.json(rows)
    }

    return res.status(404).json({ error: "tabela_nao_suportada" })
  }))

  app.post("/rest/v1/:table", asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    if (req.params.table === "friendships") {
      const { user_a, user_b, requester_id, status } = req.body || {}
      if (!user_a || !user_b || !requester_id) return res.status(400).json({ error: "campos_faltando" })
      if (
        requester_id !== uid ||
        (user_a !== uid && user_b !== uid) ||
        user_a === user_b ||
        (status && status !== "pending")
      ) {
        return res.status(403).json({ error: "permissao_negada" })
      }
      try {
        const blocked = await db.query(
          `SELECT 1 FROM blocks WHERE (blocker_id = $1 AND blocked_id = $2) OR (blocker_id = $2 AND blocked_id = $1)`,
          [user_a, user_b],
        )
        if (blocked.rows[0]) return res.status(403).json({ error: "usuario_bloqueado" })
        await db.query(
          "INSERT INTO friendships (user_a, user_b, requester_id, status) VALUES ($1, $2, $3, 'pending')",
          [user_a, user_b, requester_id],
        )
        notifyFriendshipInsert(user_a === uid ? user_b : user_a, requester_id)
        return res.status(201).json([{ user_a, user_b, requester_id, status: "pending" }])
      } catch (error) {
        if (["23503", "23505", "23514"].includes(error.code)) {
          return res.status(409).json({ error: "conflito" })
        }
        throw error
      }
    }
    if (req.params.table === "profiles") return res.status(403).json({ error: "permissao_negada" })
    return res.status(404).json({ error: "tabela_nao_suportada" })
  }))

  app.patch("/rest/v1/:table", asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

    if (req.params.table === "profiles") {
      const permitidas = [
        "display_name", "summary", "country", "city", "showcase", "profile_visibility",
        "show_location", "background_url", "avatar_url", "banner_url",
      ]
      const columns = Object.keys(req.body || {}).filter((key) => permitidas.includes(key))
      if (!columns.length) return res.status(400).json({ error: "sem_campos" })
      const params = columns.map((column) => {
        const value = req.body[column]
        return column === "showcase" && typeof value !== "string"
          ? JSON.stringify(value ?? [])
          : value
      })
      const sets = columns.map((column, index) => `${column} = $${index + 1}`)
      params.push(uid)
      await db.query(`UPDATE profiles SET ${sets.join(", ")} WHERE id = $${params.length}`, params)
      return res.json([])
    }

    if (req.params.table === "friendships") {
      if (req.body?.status === "accepted") {
        const pair = friendshipPair(req.query.or, uid)
        const requesterId = String(req.query.requester_id || "").replace(/^eq\./, "")
        if (!pair || !requesterId || requesterId === uid) {
          return res.status(400).json({ error: "filtro_invalido" })
        }
        const blocked = await db.query(
          `SELECT 1 FROM blocks WHERE (blocker_id = $1 AND blocked_id = $2) OR (blocker_id = $2 AND blocked_id = $1)`,
          pair,
        )
        if (blocked.rows[0]) return res.status(403).json({ error: "usuario_bloqueado" })
        await db.query(
          `UPDATE friendships SET status = 'accepted', updated_at = now()
           WHERE user_a = $1 AND user_b = $2 AND requester_id = $3 AND status = 'pending'`,
          [...pair, requesterId],
        )
      }
      return res.json([])
    }
    return res.status(404).json({ error: "tabela_nao_suportada" })
  }))

  app.delete("/rest/v1/:table", asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    if (req.params.table !== "friendships") {
      return res.status(404).json({ error: "tabela_nao_suportada" })
    }
    const pair = friendshipPair(req.query.or, uid)
    const status = String(req.query.status || "").replace(/^eq\./, "")
    const requesterId = String(req.query.requester_id || "").replace(/^eq\./, "")
    if (!pair || !["pending", "accepted"].includes(status)) {
      return res.status(400).json({ error: "filtro_invalido" })
    }
    if (status === "pending") {
      if (requesterId !== uid) return res.status(400).json({ error: "filtro_invalido" })
      await db.query(
        `DELETE FROM friendships
         WHERE user_a = $1 AND user_b = $2 AND requester_id = $3 AND status = 'pending'`,
        [...pair, requesterId],
      )
    } else {
      await db.query(
        "DELETE FROM friendships WHERE user_a = $1 AND user_b = $2 AND status = 'accepted'",
        pair,
      )
    }
    return res.json([])
  }))
}

module.exports = { registerRestRoutes }
