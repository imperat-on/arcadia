"use strict"

// REST-lite para /rest/v1/:table, suporta o subconjunto que o shim do
// client.js usa (profiles/friendships). Nao e um parser PostgREST generico,
// e sim um switch restrito com as colunas permitidas por tabela.

const { db } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { notifyFriendshipInsert } = require("./realtime")

const COLUNAS_PROFILES = new Set([
  "id", "username", "avatar_url", "background_url", "steam_id", "display_name", "summary",
  "country", "city", "showcase", "profile_visibility", "show_location",
  "created_at", "email", "password_hash",
])
const COLUNAS_FRIENDSHIPS = new Set([
  "user_a", "user_b", "requester_id", "status", "created_at", "updated_at",
])

// Traduz os filtros do shim (?col=eq.val, ?col=ilike.pat, ?or=(...))
function parseFilters(req, permitidas) {
  const sql = []
  const params = []
  const q = req.query

  for (const [k, v] of Object.entries(q)) {
    if (k === "select" || k === "limit" || k === "or") continue
    if (!permitidas.has(k)) continue
    const [op, val] = String(v).split(".")
    if (op === "eq") {
      sql.push(`${k} = ?`)
      params.push(val)
    } else if (op === "ilike") {
      sql.push(`${k} LIKE ? COLLATE NOCASE`)
      params.push(val)
    }
  }

  // or: "a.eq.x,b.eq.y" ou "and(a.eq.x,b.eq.y),and(...)". O friends.js usa
  // accept/cancel com or contendo and(cond1,cond2)
  const orRaw = String(q.or || "")
  if (orRaw) {
    const clauses = []
    const re = /([a-z_]+)\.(eq|ilike)\.([^,)]+)/g
    let m
    while ((m = re.exec(orRaw))) {
      const [col, op, val] = [m[1], m[2], m[3]]
      if (!permitidas.has(col)) continue
      if (op === "eq") clauses.push(`${col} = ?`)
      else clauses.push(`${col} LIKE ? COLLATE NOCASE`)
      params.push(val)
    }
    if (clauses.length) sql.push(`(${clauses.join(" OR ")})`)
  }

  return { sql, params }
}

function requireAuth(req) {
  const token = extractToken(req)
  const v = verifyToken(token || "")
  return v.ok ? v.sub : null
}

// Parser do select para embeds do tipo pa:profiles!friendships_user_a_fkey(...)
// Devolve { colunas: string[], embeds: [{ nome, tabela, colunas }] }
// Atencao: o split por virgula nao pode quebrar DENTRO dos parenteses do embed
// (o friends.js manda "pa:profiles!...(id, username, ...)" numa linha so).
function parseSelect(sel) {
  const colunas = []
  const embeds = []
  const parts = []
  let cur = ""
  let depth = 0
  for (const ch of String(sel || "*")) {
    if (ch === "(") depth++
    if (ch === ")") depth--
    if (ch === "," && depth === 0) {
      parts.push(cur)
      cur = ""
    } else {
      cur += ch
    }
  }
  parts.push(cur)
  for (const p of parts) {
    const t = p.trim()
    if (!t) continue
    const m = t.match(/^([a-z_]+):profiles!([a-z_]+)\(([^)]*)\)$/i)
    if (m) {
      embeds.push({ nome: m[1], fk: m[2], colunas: m[3].split(",").map((c) => c.trim()) })
    } else {
      colunas.push(t)
    }
  }
  return { colunas, embeds }
}

// Perfil do "outro lado" do embed (pa/pb), resolvendo a FK
function embedProfile(colunas, profile) {
  if (!profile) return null
  const out = {}
  for (const c of colunas) if (c in profile) out[c] = profile[c]
  return out
}

function registerRestRoutes(app) {
  const handler = (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    const tabela = req.params.table

    if (tabela === "profiles") {
      const { sql, params } = parseFilters(req, COLUNAS_PROFILES)
      // Sem RLS, filtro explicito: self, publicos e amigos aceitos.
      sql.push(
        "(id = ? OR profile_visibility = 'public' OR EXISTS (" +
          "SELECT 1 FROM friendships f WHERE f.status='accepted' " +
          "AND ((f.user_a = profiles.id AND f.user_b = ?) OR (f.user_b = profiles.id AND f.user_a = ?))))"
      )
      params.push(uid, uid, uid)

      const { colunas } = parseSelect(req.query.select)
      const cols = colunas.length
        ? colunas.join(", ")
        : "id, username, avatar_url, display_name, summary, country, city, showcase, profile_visibility, created_at"

      const rows = db
        .prepare(`SELECT ${cols} FROM profiles WHERE ${sql.join(" AND ")} LIMIT ${Number(req.query.limit) || 100}`)
        .all(...params)

      return res.json(rows)
    }

    if (tabela === "friendships") {
      const { sql, params } = parseFilters(req, COLUNAS_FRIENDSHIPS)
      sql.push("(user_a = ? OR user_b = ?)")
      params.push(uid, uid)

      const { colunas, embeds } = parseSelect(req.query.select)
      const cols = colunas.length ? colunas.join(", ") : "*"

      const rows = db
        .prepare(`SELECT ${cols} FROM friendships WHERE ${sql.join(" AND ")} LIMIT ${Number(req.query.limit) || 100}`)
        .all(...params)

      if (embeds.length) {
        const profiles = db.prepare("SELECT id, username, avatar_url, display_name FROM profiles").all()
        const byId = {}
        for (const p of profiles) byId[p.id] = p
        for (const row of rows) {
          for (const e of embeds) {
            const perfil = e.fk.includes("user_a") ? byId[row.user_a] : byId[row.user_b]
            row[e.nome] = embedProfile(e.colunas, perfil)
          }
        }
      }
      return res.json(rows)
    }

    return res.status(404).json({ error: "tabela_nao_suportada" })
  }

  app.get("/rest/v1/:table", handler)

  app.post("/rest/v1/:table", (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    const tabela = req.params.table
    const body = req.body || {}

    if (tabela === "friendships") {
      const { user_a, user_b, requester_id, status } = body
      if (!user_a || !user_b || !requester_id) {
        return res.status(400).json({ error: "campos_faltando" })
      }
      // RLS: so pode criar pedido para si (requester = uid) e pending
      if (requester_id !== uid || (status && status !== "pending")) {
        return res.status(403).json({ error: "permissao_negada" })
      }
      try {
        db.prepare(
          "INSERT INTO friendships (user_a, user_b, requester_id, status) VALUES (?, ?, ?, 'pending')"
        ).run(user_a, user_b, requester_id)
        // notifica o addressee (user_b) via realtime
        notifyFriendshipInsert(user_b, requester_id)
        return res.status(201).json([{ user_a, user_b, requester_id, status: "pending" }])
      } catch (e) {
        return res.status(409).json({ error: "conflito" })
      }
    }
    if (tabela === "profiles") {
      return res.status(403).json({ error: "permissao_negada" })
    }
    return res.status(404).json({ error: "tabela_nao_suportada" })
  })

  app.patch("/rest/v1/:table", (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    const tabela = req.params.table
    const body = req.body || {}
    const q = req.query

    if (tabela === "profiles") {
      const permitidas = ["display_name", "summary", "country", "city", "showcase", "profile_visibility", "show_location", "background_url"]
      const set = Object.keys(body).filter((k) => permitidas.includes(k))
      if (!set.length) return res.status(400).json({ error: "sem_campos" })
      const sets = set.map((k) => `${k} = ?`)
      const vals = set.map((k) => body[k])
      db.prepare(`UPDATE profiles SET ${sets.join(", ")} WHERE id = ?`).run(...vals, uid)
      return res.json([])
    }
    if (tabela === "friendships") {
      const status = body.status
      const orRaw = String(q.or || "")
      const re = /([a-z_]+)\.(eq)\.([^,)]+)/g
      let m
      const orClauses = []
      const params = []
      while ((m = re.exec(orRaw))) {
        const [col, , val] = m
        orClauses.push(`${col} = ?`)
        params.push(val)
      }
      const conds = orClauses.length ? [`(${orClauses.join(" AND ")})`] : []
      conds.push("(user_a = ? OR user_b = ?)")
      params.push(uid, uid)
      if (status && status === "accepted") {
        conds.push("status = 'pending'")
        db.prepare(`UPDATE friendships SET status = 'accepted', updated_at = datetime('now') WHERE ${conds.join(" AND ")}`).run(...params)
      }
      return res.json([])
    }
    return res.status(404).json({ error: "tabela_nao_suportada" })
  })

  app.delete("/rest/v1/:table", (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    const tabela = req.params.table
    const q = req.query
    if (tabela !== "friendships") return res.status(404).json({ error: "tabela_nao_suportada" })

    const orRaw = String(q.or || "")
    const re = /([a-z_]+)\.(eq)\.([^,)]+)/g
    let m
    const orClauses = []
    const params = []
    while ((m = re.exec(orRaw))) {
      const [col, , val] = m
      orClauses.push(`${col} = ?`)
      params.push(val)
    }
    const conds = orClauses.length ? [`(${orClauses.join(" AND ")})`] : []
    conds.push("(user_a = ? OR user_b = ?)")
    params.push(uid, uid)
    db.prepare(`DELETE FROM friendships WHERE ${conds.join(" AND ")}`).run(...params)
    return res.json([])
  })
}

module.exports = { registerRestRoutes }
