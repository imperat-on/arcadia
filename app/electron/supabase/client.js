"use strict"

// Cliente do backend do Arcadia (shim), roda NO MAIN PROCESS.
// Substitui o supabase-js por um fetch REST contra o servidor Node proprio.
// Mantem a MESMA superficie publica que o antigo createClient, para que
// auth.js, friends.js, sync.js, biblioteca.js, ipc.js e main.js nao mudem.
//
//  - getClient(): { auth, from(), rpc(), storage.from(), channel(), removeChannel(), supabaseUrl }
//  - restoreSession(): recupera a sessao salva no boot
//  - attachAuthPersistence(): espelha onAuthStateChange para session.json
//
// O formato da sessao (access_token JWT + refresh_token + user.user_metadata)
// segue o shape do supabase-js, entao session.js e o main.js:1773 continuam
// consumindo do mesmo jeito.

const config = require("./config")
const sessionStore = require("./session")
const WebSocket = require("ws")

let client = null

// ---------------------------------------------------------------------------
// Mini onAuthStateChange: registra listeners e emite eventos
// ---------------------------------------------------------------------------
class AuthEmitter {
  constructor() {
    this.listeners = new Set()
  }
  subscribe(cb) {
    this.listeners.add(cb)
    return { data: { subscription: { unsubscribe: () => this.listeners.delete(cb) } } }
  }
  emit(event, session) {
    for (const cb of this.listeners) {
      try {
        cb(event, session)
      } catch {
        /* listener nao deve quebrar o fluxo */
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Auth client (shape supabase-js)
// ---------------------------------------------------------------------------
class AuthClient {
  constructor() {
    this.emitter = new AuthEmitter()
    this._session = null
  }

  onAuthStateChange(cb) {
    return this.emitter.subscribe(cb)
  }

  async _request(method, path, body, { json = true } = {}) {
    const headers = { apikey: config.anonKey }
    // endpoints autenticados precisam do access_token (signup/token sao publicos)
    if (this._session?.access_token) {
      headers.authorization = `Bearer ${this._session.access_token}`
    }
    let payload
    if (body) {
      headers["content-type"] = "application/json"
      payload = JSON.stringify(body)
    }
    // Timeout de rede: servidor fora do ar nao pode segurar o handler IPC
    // para sempre (a UI ficava em "carregando" indefinidamente).
    const res = await fetch(config.url + path, {
      method,
      headers,
      body: payload,
      signal: AbortSignal.timeout(30_000),
    })
    const text = await res.text()
    // Resposta nao-JSON (ex.: 404 HTML de rota inexistente) NAO pode lancar:
    // vira erro tratavel em vez de rejeicao que trava a tela.
    let data
    try {
      data = json && text ? JSON.parse(text) : text
    } catch {
      data = text
    }
    if (!res.ok) {
      return { data: null, error: { message: data?.error || data?.msg || `HTTP ${res.status}` } }
    }
    return { data, error: null }
  }

  async _sessionResponse(data) {
    // data ja tem o shape GoTrue {access_token, refresh_token, user, ...}
    this._session = data
    return { data: { session: data }, error: null }
  }

  async getSession() {
    return { data: { session: this._session }, error: null }
  }

  async setSession(sessaoSalva) {
    // Valida o access_token e devolve o usuario. Se expirou, faz refresh.
    this._session = sessaoSalva
    const { error } = await this._request("GET", "/auth/v1/user", null, {
      json: true,
    })
    if (error) {
      // tenta refresh
      const rt = sessaoSalva?.refresh_token
      if (rt) {
        const r = await this._request("POST", "/auth/v1/token?grant_type=refresh_token", {
          refresh_token: rt,
        })
        if (!r.error) {
          this._session = r.data
          this.emitter.emit("TOKEN_REFRESHED", r.data)
          return { data: { session: r.data }, error: null }
        }
      }
      this._session = null
      return { data: { session: null }, error: { message: "sessao invalida" } }
    }
    return { data: { session: sessaoSalva }, error: null }
  }

  async signUp({ email, password, options } = {}) {
    const { data, error } = await this._request("POST", "/auth/v1/signup", {
      email,
      password,
      options,
    })
    if (error) return { data: null, error }
    this._session = data.session
    this.emitter.emit("SIGNED_IN", data.session)
    return { data: { user: data.user, session: data.session }, error: null }
  }

  async signInWithPassword({ email, password }) {
    const { data, error } = await this._request(
      "POST",
      "/auth/v1/token?grant_type=password",
      { email, password }
    )
    if (error) return { data: null, error }
    this._session = data
    this.emitter.emit("SIGNED_IN", data)
    return { data: { user: data.user, session: data }, error: null }
  }

  async signOut() {
    let error = null
    try {
      ;({ error } = await this._request("POST", "/auth/v1/logout", null, {
        json: false,
      }))
    } catch (e) {
      // Logout local nao pode depender da rede. O token expira em 1h e a
      // revogacao remota volta a funcionar no proximo login/logout online.
      error = { message: String(e?.message || e) }
    } finally {
      this._session = null
      sessionStore.clearSession()
      this.emitter.emit("SIGNED_OUT", null)
    }
    return { error }
  }

  async getUser() {
    const { data, error } = await this._request("GET", "/auth/v1/user", null, {
      json: true,
    })
    if (error) return { data: { user: null }, error }
    return { data: { user: data.user }, error: null }
  }
}

// ---------------------------------------------------------------------------
// REST-lite builder: from("tabela").select().eq().or().ilike().limit()...
// ---------------------------------------------------------------------------
class QueryBuilder {
  constructor(client, table) {
    this.client = client
    this.table = table
    this._select = "*"
    this._filters = []
    this._or = null
    this._limit = null
    this._single = false
  }

  select(cols) {
    this._select = cols
    return this
  }
  eq(col, val) {
    this._filters.push([col, "eq", val])
    return this
  }
  or(cond) {
    this._or = cond
    return this
  }
  ilike(col, pat) {
    this._filters.push([col, "ilike", pat])
    return this
  }
  limit(n) {
    this._limit = n
    return this
  }
  single() {
    this._single = true
    return this
  }
  maybeSingle() {
    this._single = true
    return this
  }

  async _run(method, body) {
    const params = new URLSearchParams()
    params.set("select", this._select)
    for (const [c, op, v] of this._filters) params.set(c, `${op}.${v}`)
    if (this._or) params.set("or", `(${this._or})`)
    if (this._limit) params.set("limit", String(this._limit))
    if (method === "GET") {
      const qs = params.toString()
      const url = `${config.url}/rest/v1/${this.table}${qs ? "?" + qs : ""}`
      const res = await fetch(url, {
        method: "GET",
        headers: this.client._authHeaders(),
      })
      const text = await res.text()
      const data = text ? JSON.parse(text) : []
      if (!res.ok) return { data: null, error: { message: data?.error || `HTTP ${res.status}` } }
      if (this._single) {
        return { data: Array.isArray(data) ? (data[0] ?? null) : data, error: null }
      }
      return { data, error: null }
    }
    // POST/PATCH/DELETE
    const qs = params.toString()
    const url = `${config.url}/rest/v1/${this.table}${qs ? "?" + qs : ""}`
    const res = await fetch(url, {
      method,
      headers: { ...this.client._authHeaders(), "content-type": "application/json" },
      body: body ? JSON.stringify(body) : undefined,
    })
    const text = await res.text()
    const data = text ? JSON.parse(text) : []
    if (!res.ok) return { data: null, error: { message: data?.error || `HTTP ${res.status}` } }
    return { data, error: null }
  }

  insert(obj) {
    this._method = "POST"
    this._body = obj
    return this
  }
  update(obj) {
    this._method = "PATCH"
    this._body = obj
    return this
  }
  delete() {
    this._method = "DELETE"
    return this
  }
  // Tornar o builder thenable: `await builder` executa a query (como o
  // supabase-js). Necessario para maybeSingle()/single() e para o padrao
  // `await client.from(...).select(...).eq(...).eq(...)`.
  then(resolve, reject) {
    const method = this._method || "GET"
    const body = this._body
    return this._run(method, body).then(resolve, reject)
  }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------
class StorageClient {
  constructor(client) {
    this.client = client
  }
  from(bucket) {
    const headers = this.client._authHeaders()
    return {
      upload: async (path, file, _opts) => {
        const res = await fetch(`${config.url}/storage/v1/object/${bucket}/${path}`, {
          method: "POST",
          headers,
          body: file,
        })
        const text = await res.text()
        const data = text ? JSON.parse(text) : {}
        if (!res.ok) return { data: null, error: { message: data?.error || `HTTP ${res.status}` } }
        return { data, error: null }
      },
      remove: async (paths) => {
        const res = await fetch(`${config.url}/storage/v1/object/${bucket}`, {
          method: "DELETE",
          headers: { ...headers, "content-type": "application/json" },
          body: JSON.stringify({ paths }),
        })
        const text = await res.text()
        const data = text ? JSON.parse(text) : []
        if (!res.ok) return { data: null, error: { message: `HTTP ${res.status}` } }
        return { data, error: null }
      },
    }
  }
}

// ---------------------------------------------------------------------------
// Cliente principal (shim)
// ---------------------------------------------------------------------------
class ArcadiaClient {
  constructor() {
    this.auth = new AuthClient()
    this.supabaseUrl = config.url
    this._channels = new Map()
  }

  _authHeaders() {
    const token = this.auth._session?.access_token
    const headers = { apikey: config.anonKey }
    if (token) headers.authorization = `Bearer ${token}`
    return headers
  }

  from(table) {
    return new QueryBuilder(this, table)
  }

  async rpc(fn, args) {
    const res = await fetch(`${config.url}/rest/v1/rpc/${fn}`, {
      method: "POST",
      headers: { ...this._authHeaders(), "content-type": "application/json" },
      body: JSON.stringify(args || {}),
      signal: AbortSignal.timeout(30_000),
    })
    const text = await res.text()
    // Nunca lancar em resposta nao-JSON (404 HTML de rota ausente virava
    // excecao aqui e o perfil do amigo ficava "carregando" para sempre).
    let data = null
    try {
      data = text ? JSON.parse(text) : null
    } catch {
      data = null
    }
    if (!res.ok) return { data: null, error: { message: data?.error || `HTTP ${res.status}` } }
    return { data, error: null }
  }

  get storage() {
    return new StorageClient(this)
  }

  channel(name) {
    // Realtime WS (Phoenix-lite): conecta no socket, faz phx_join e entrega
    // eventos `postgres_changes` aos callbacks registrados via .on().
    const self = this
    let ws = null
    let _subscribed = false
    const _handlers = []

    const conn = () => {
      if (ws && ws.readyState === 1) return
      const wsUrl = config.url.replace(/^http/, "ws") + "/realtime/v1/websocket"
      ws = new WebSocket(wsUrl)
      ws.on("open", () => {
        // handshake do canal friends-<me> + token
        ws.send(
          JSON.stringify({
            topic: name,
            event: "phx_join",
            payload: {
              access_token: self.auth._session?.access_token || "",
              config: { postgres_changes: [] },
            },
            ref: "1",
          })
        )
      })
      ws.on("message", (raw) => {
        let msg
        try {
          msg = JSON.parse(raw.toString())
        } catch {
          return
        }
        if (msg.event === "phx_reply") {
          // join ok/erro
          if (msg.payload?.status !== "ok") ws.close()
          return
        }
        if (msg.event === "postgres_changes") {
          for (const h of _handlers) {
            try {
              h.cb(msg.payload)
            } catch {
              /* handler nao deve derrubar */
            }
          }
        }
      })
      ws.on("error", () => {})
      ws.on("close", () => {
        if (_subscribed) setTimeout(conn, 3000) // reconecta
      })
    }

    const ch = {
      name,
      _handlers,
      on(event, _cfg, cb) {
        if (event === "postgres_changes") _handlers.push({ cb })
        return ch
      },
      subscribe() {
        _subscribed = true
        conn()
        return ch
      },
      unsubscribe() {
        _subscribed = false
        if (ws) ws.close()
        return ch
      },
    }
    this._channels.set(name, ch)
    return ch
  }

  removeChannel(ch) {
    if (ch?.unsubscribe) ch.unsubscribe()
    this._channels.delete(ch?.name)
  }
}

function getClient() {
  if (!client) client = new ArcadiaClient()
  return client
}

/** Boot: restaura a sessao salva (se houver) e devolve o estado atual. */
async function restoreSession() {
  const saved = sessionStore.loadSession()
  const auth = getClient().auth
  if (saved) {
    try {
      const { error } = await auth.setSession(saved)
      if (error) sessionStore.clearSession()
    } catch {
      sessionStore.clearSession()
    }
  }
  const { data, error } = await auth.getSession()
  return error ? { session: null, error } : { session: data.session, error: null }
}

/**
 * Espelha mudancas de auth no session.json:
 *  - SIGNED_IN / TOKEN_REFRESHED → salva
 *  - SIGNED_OUT / USER_DELETED   → limpa
 * Devolve a funcao de unsubscribe.
 */
function attachAuthPersistence() {
  const { data } = getClient().auth.onAuthStateChange((event, session) => {
    if (event === "SIGNED_IN" || event === "TOKEN_REFRESHED") {
      if (session) sessionStore.saveSession(session)
    } else if (event === "SIGNED_OUT" || event === "USER_DELETED") {
      sessionStore.clearSession()
    }
  })
  return () => data?.subscription?.unsubscribe()
}

module.exports = { getClient, restoreSession, attachAuthPersistence }
