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
const { fetchRede } = require("../httpfetch")
const dns = require("node:dns").promises
const https = require("node:https")
const WebSocket = require("ws")

let client = null
const ipv4Cache = new Map()
const ipv4InFlight = new Map()
const ipv4Preferred = new Map()
const ipv4Agents = new Map()
const USER_CACHE_TTL_MS = 60_000
const SYSTEM_DNS_TIMEOUT_MS = 500

// O fallback IPv4 é usado no backend .ts.net. Sem um Agent persistente, cada
// RPC fazia um novo handshake TLS; o login tem pelo menos dois RPCs seguidos.
// Reaproveitar a conexão reduz bastante a latência sem alterar a segurança TLS.
function ipv4Agent(url) {
  const key = `${url.hostname}:${url.port || 443}`
  let agent = ipv4Agents.get(key)
  if (!agent) {
    agent = new https.Agent({
      keepAlive: true,
      maxSockets: 6,
      maxFreeSockets: 2,
      scheduling: "lifo",
    })
    ipv4Agents.set(key, agent)
  }
  return agent
}

function orderedIpv4Addresses(hostname, addresses) {
  const preferred = ipv4Preferred.get(hostname)
  if (!preferred || !addresses.includes(preferred)) return addresses
  return [preferred, ...addresses.filter((address) => address !== preferred)]
}

async function lookupSystemIpv4(hostname) {
  // O resolvedor do sistema é normalmente mais rápido que DoH. O limite curto
  // mantém o fallback DoH para redes com DNS local quebrado.
  let timer = null
  try {
    const localLookup = dns.lookup(hostname, { all: true, family: 4, verbatim: true })
    const localTimeout = new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("DNS local timeout")), SYSTEM_DNS_TIMEOUT_MS)
    })
    const result = await Promise.race([localLookup, localTimeout])
    const addresses = [...new Set((result || [])
      .map((item) => String(item?.address || ""))
      .filter((address) => /^\d{1,3}(?:\.\d{1,3}){3}$/.test(address)))]
    if (addresses.length) return addresses
  } catch {
    /* DoH abaixo cobre resolvedores locais indisponíveis. */
  } finally {
    if (timer) clearTimeout(timer)
  }
  return null
}

function waitWithAbort(promise, signal) {
  if (!signal) return promise
  if (signal.aborted) return Promise.reject(signal.reason || new Error("aborted"))
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason || new Error("aborted"))
    signal.addEventListener("abort", abort, { once: true })
    promise.then(
      (value) => { signal.removeEventListener("abort", abort); resolve(value) },
      (error) => { signal.removeEventListener("abort", abort); reject(error) },
    )
  })
}

async function resolveBackendIpv4(hostname, signal) {
  const cached = ipv4Cache.get(hostname)
  if (cached?.expires > Date.now() && cached.addresses.length) return cached.addresses

  // O pre-warm do backend pode estar rodando ao mesmo tempo que o primeiro
  // clique em Entrar. Compartilhar o DNS evita duas consultas DoH concorrentes.
  const ongoing = ipv4InFlight.get(hostname)
  if (ongoing) return waitWithAbort(ongoing, signal)

  const request = (async () => {
    const localAddresses = await lookupSystemIpv4(hostname)
    if (localAddresses?.length) {
      ipv4Cache.set(hostname, { addresses: localAddresses, expires: Date.now() + 5 * 60 * 1000 })
      return localAddresses
    }

    const endpoint = new URL("https://cloudflare-dns.com/dns-query")
    endpoint.searchParams.set("name", hostname)
    endpoint.searchParams.set("type", "A")
    const response = await fetchRede(endpoint.href, {
      headers: { Accept: "application/dns-json", "User-Agent": "arcadia" },
      signal: AbortSignal.timeout(8000),
    })
    if (!response.ok) throw new Error(`DNS HTTP ${response.status}`)
    const payload = await response.json()
    const addresses = [...new Set((payload?.Answer || [])
      .filter((answer) => answer?.type === 1 && /^\d{1,3}(?:\.\d{1,3}){3}$/.test(String(answer.data || "")))
      .map((answer) => String(answer.data)))]
    if (!addresses.length) throw new Error("DNS sem IPv4")
    ipv4Cache.set(hostname, { addresses, expires: Date.now() + 5 * 60 * 1000 })
    return addresses
  })()
  ipv4InFlight.set(hostname, request)
  // A entrada só sai quando a consulta real acaba. Um waiter pode abortar a
  // própria espera sem abrir uma segunda consulta enquanto o DNS continua.
  const limpar = () => {
    if (ipv4InFlight.get(hostname) === request) ipv4InFlight.delete(hostname)
  }
  request.then(limpar, limpar)
  return waitWithAbort(request, signal)
}

async function bodyBuffer(body) {
  if (body == null) return null
  if (typeof body === "string" || Buffer.isBuffer(body) || body instanceof Uint8Array) return body
  if (typeof body.arrayBuffer === "function") return Buffer.from(await body.arrayBuffer())
  throw new TypeError("corpo HTTP não suportado")
}

async function fetchBackendIpv4(value, options = {}) {
  const url = new URL(value)
  if (url.protocol !== "https:") throw new Error("fallback IPv4 exige HTTPS")
  const addresses = orderedIpv4Addresses(url.hostname, await resolveBackendIpv4(url.hostname, options.signal))
  const body = await bodyBuffer(options.body)
  let lastError = null

  for (const address of addresses) {
    try {
      const response = await new Promise((resolve, reject) => {
        const headers = { ...(options.headers || {}), Host: url.host }
        const request = https.request({
          protocol: "https:",
          hostname: address,
          port: url.port || 443,
          servername: url.hostname,
          method: options.method || "GET",
          path: `${url.pathname}${url.search}`,
          headers,
          agent: ipv4Agent(url),
          rejectUnauthorized: true,
        }, (response) => {
          const chunks = []
          response.on("data", (chunk) => chunks.push(chunk))
          response.on("end", () => {
            const buffer = Buffer.concat(chunks)
            const status = Number(response.statusCode || 0)
            resolve({
              ok: status >= 200 && status < 300,
              status,
              text: async () => buffer.toString("utf8"),
              json: async () => JSON.parse(buffer.toString("utf8")),
            })
          })
        })
        request.setTimeout(10_000, () => request.destroy(new Error("connect timeout")))
        request.on("error", reject)
        if (options.signal) {
          if (options.signal.aborted) request.destroy(options.signal.reason)
          else options.signal.addEventListener("abort", () => request.destroy(options.signal.reason), { once: true })
        }
        if (body != null) request.write(body)
        request.end()
      })
      ipv4Preferred.set(url.hostname, address)
      return response
    } catch (error) {
      lastError = error
      if (ipv4Preferred.get(url.hostname) === address) ipv4Preferred.delete(url.hostname)
    }
  }
  throw lastError || new Error("IPv4 indisponível")
}

// O fetch global do Node/Undici pode ficar preso no IPv6 em redes sem rota
// funcional. No Electron usamos net.fetch (via fetchRede), que compartilha o
// resolvedor/Happy Eyeballs do Chromium. Falha de conexão vira uma resposta
// tratável para nunca rejeitar um handler IPC de conta.
function boundedSignal(signal, timeoutMs) {
  const timeout = AbortSignal.timeout(timeoutMs)
  if (!signal) return timeout
  if (typeof AbortSignal.any === "function") return AbortSignal.any([signal, timeout])
  // Electron/Node atuais têm AbortSignal.any; este fallback mantém
  // compatibilidade com runtimes antigos usados em instalações legadas.
  const controller = new AbortController()
  const abort = (event) => controller.abort(event?.target?.reason)
  if (signal.aborted) controller.abort(signal.reason)
  else signal.addEventListener("abort", abort, { once: true })
  timeout.addEventListener("abort", abort, { once: true })
  return controller.signal
}

async function backendFetch(url, options, fetchImpl = fetchRede, fallbackImpl = fetchBackendIpv4) {
  const hostname = (() => {
    try { return new URL(url).hostname.toLowerCase() } catch { return "" }
  })()
  // Uma única deadline evita somar 15s do fallback + 15s da pilha normal
  // quando o servidor está fora. Cada tentativa recebe um sinal derivado novo:
  // se a primeira falhar cedo, a segunda ainda usa o tempo que restou.
  const caminhoTs = hostname.endsWith(".ts.net") && fetchImpl === fetchRede
  const totalTimeout = caminhoTs ? 15_000 : 30_000
  const iniciouEm = Date.now()
  const deadlineSignal = boundedSignal(options?.signal, totalTimeout)
  const requestOptions = () => {
    const restante = Math.max(1, totalTimeout - (Date.now() - iniciouEm))
    return { ...options, signal: boundedSignal(deadlineSignal, restante) }
  }

  // Tailscale Funnel possui A e AAAA públicos, mas alguns resolvedores locais
  // entregam apenas AAAA. Nessa situação esperar o IPv6 falhar acrescenta 30s
  // ao boot e mantém o splash preto. Para .ts.net, IPv4 resolvido por DoH é a
  // rota primária; a pilha normal continua como fallback.
  if (caminhoTs) {
    try {
      return await fallbackImpl(url, requestOptions())
    } catch {
      try { return await fetchImpl(url, requestOptions()) } catch {}
    }
  } else {
    try {
      return await fetchImpl(url, requestOptions())
    } catch {
      try { return await fallbackImpl(url, requestOptions()) } catch {}
    }
  }

  return {
    ok: false,
    status: 0,
    text: async () => JSON.stringify({ error: "rede_indisponivel" }),
  }
}

let backendWarmup = null

// Abre DNS/TLS enquanto o launcher pinta a tela. O resultado não é requisito
// para o boot; a única finalidade é deixar a conexão reutilizável para o
// primeiro login. Mesmo uma resposta 404 aquece o socket corretamente.
function warmBackend() {
  if (backendWarmup) return backendWarmup
  backendWarmup = backendFetch(`${config.url}/auth/v1/health`, {
    method: "GET",
    headers: { apikey: config.anonKey },
    signal: AbortSignal.timeout(5_000),
  }).catch(() => null)
  return backendWarmup
}

function backendLookup(hostname, _options, callback) {
  resolveBackendIpv4(hostname)
    .then((addresses) => callback(null, orderedIpv4Addresses(hostname, addresses)[0], 4))
    .catch((error) => callback(error))
}

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
    // Após emitir um token, o usuário já foi autenticado pelo backend. Os
    // módulos de sync/profile chamavam /auth/v1/user repetidamente no mesmo
    // instante do login; uma janela curta de cache evita essas viagens extras.
    this._userValidatedAt = 0
    this._userInFlight = null
    this._refreshInFlight = null
  }

  _resetUserCache() {
    this._userValidatedAt = 0
    this._userInFlight = null
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
    const res = await backendFetch(config.url + path, {
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
      return {
        data: null,
        error: {
          message: data?.error || data?.msg || `HTTP ${res.status}`,
          status: Number(res.status) || 0,
          code: data?.code,
        },
      }
    }
    return { data, error: null }
  }

  async _sessionResponse(data) {
    // data ja tem o shape GoTrue {access_token, refresh_token, user, ...}
    this._session = data
    this._resetUserCache()
    return { data: { session: data }, error: null }
  }

  async getSession() {
    return { data: { session: this._session }, error: null }
  }

  // Access tokens expiram enquanto o app permanece aberto. O client anterior
  // só renovava a sessão durante o boot; depois disso qualquer sync devolvia
  // 401 e a fila ficava presa sem feedback. Compartilhe um único refresh para
  // chamadas concorrentes e emita TOKEN_REFRESHED para persistir a sessão.
  async refreshSession() {
    const refreshToken = this._session?.refresh_token
    if (!refreshToken) {
      return {
        data: { session: null },
        error: { message: "sessao sem refresh_token", status: 401 },
      }
    }
    if (this._refreshInFlight?.refreshToken === refreshToken) {
      return this._refreshInFlight.promise
    }

    const promise = (async () => {
      const result = await this._request("POST", "/auth/v1/token?grant_type=refresh_token", {
        refresh_token: refreshToken,
      })
      if (result.error || !result.data?.access_token) {
        return {
          data: { session: null },
          error: result.error || { message: "resposta de refresh invalida", status: 401 },
        }
      }
      // Uma resposta antiga não pode substituir uma sessão que mudou durante
      // o refresh (logout/login rápido ou troca de conta).
      if (this._session?.refresh_token !== refreshToken) {
        return { data: { session: null }, error: { message: "sessao_trocada", status: 409 } }
      }
      const session = {
        ...result.data,
        refresh_token: result.data.refresh_token || refreshToken,
        user: result.data.user || this._session.user,
      }
      this._session = session
      this._resetUserCache()
      this._userValidatedAt = Date.now()
      this.emitter.emit("TOKEN_REFRESHED", session)
      return { data: { session }, error: null }
    })()
    this._refreshInFlight = { refreshToken, promise }
    try {
      return await promise
    } finally {
      if (this._refreshInFlight?.promise === promise) this._refreshInFlight = null
    }
  }

  async setSession(sessaoSalva, { emitSignedIn = false } = {}) {
    // Valida o access_token e devolve o usuario. Se expirou, faz refresh.
    this._session = sessaoSalva
    this._resetUserCache()
    const { error } = await this._request("GET", "/auth/v1/user", null, {
      json: true,
    })
    if (error) {
      const refreshed = await this.refreshSession()
      if (!refreshed.error) return refreshed
      this._session = null
      return { data: { session: null }, error: { message: "sessao invalida", status: 401 } }
    }
    this._userValidatedAt = Date.now()
    if (emitSignedIn) this.emitter.emit("SIGNED_IN", this._session)
    return { data: { session: this._session }, error: null }
  }

  async signUp({ email, password, options } = {}) {
    const { data, error } = await this._request("POST", "/auth/v1/signup", {
      email,
      password,
      options,
    })
    if (error) return { data: null, error }
    this._session = data.session
    this._resetUserCache()
    this._userValidatedAt = Date.now()
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
    this._resetUserCache()
    this._userValidatedAt = Date.now()
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
      this._resetUserCache()
      sessionStore.clearSession()
      this.emitter.emit("SIGNED_OUT", null)
    }
    return { error }
  }

  async getUser() {
    const localUser = this._session?.user
    const token = this._session?.access_token
    if (
      localUser?.id &&
      Date.now() - this._userValidatedAt < USER_CACHE_TTL_MS
    ) {
      return { data: { user: localUser }, error: null }
    }
    if (token && this._userInFlight?.token === token) {
      return this._userInFlight.promise
    }

    const promise = (async () => {
      const { data, error } = await this._request("GET", "/auth/v1/user", null, {
        json: true,
      })
      if (error) {
        // Sessão salva pode expirar enquanto o launcher continua aberto. Tenta
        // renovar somente para 401; falha de rede não deve invalidar a conta.
        if (error.status === 401 && this._session?.refresh_token) {
          const refreshed = await this.refreshSession()
          if (!refreshed.error && refreshed.data?.session?.user) {
            return { data: { user: refreshed.data.session.user }, error: null }
          }
        }
        return { data: { user: null }, error }
      }
      // Não deixe uma resposta de uma sessão antiga validar a sessão nova.
      if (this._session?.access_token === token) this._userValidatedAt = Date.now()
      return { data: { user: data.user }, error: null }
    })()
    const entry = token ? { token, promise } : null
    if (entry) this._userInFlight = entry
    try {
      return await promise
    } finally {
      if (entry && this._userInFlight === entry) this._userInFlight = null
    }
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
      const res = await backendFetch(url, {
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
    const res = await backendFetch(url, {
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
        const res = await backendFetch(`${config.url}/storage/v1/object/${bucket}/${path}`, {
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
        const res = await backendFetch(`${config.url}/storage/v1/object/${bucket}`, {
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
    const request = () =>
      backendFetch(`${config.url}/rest/v1/rpc/${fn}`, {
        method: "POST",
        headers: { ...this._authHeaders(), "content-type": "application/json" },
        body: JSON.stringify(args || {}),
        signal: AbortSignal.timeout(30_000),
      })
    const tokenAtRequest = this.auth._session?.access_token
    let res = await request()
    // O token pode expirar entre getUser() e o RPC. Renova uma vez e repete a
    // chamada original, mantendo a fila intacta quando a renovação falhar.
    if (res.status === 401 && this.auth._session?.refresh_token) {
      if (tokenAtRequest !== this.auth._session.access_token) {
        res = await request()
      } else {
        const refreshed = await this.auth.refreshSession()
        if (!refreshed.error) res = await request()
      }
    }
    const text = await res.text()
    // Nunca lancar em resposta nao-JSON (404 HTML de rota ausente virava
    // excecao aqui e o perfil do amigo ficava "carregando" para sempre).
    let data = null
    try {
      data = text ? JSON.parse(text) : null
    } catch {
      data = null
    }
    if (!res.ok) {
      return {
        data: null,
        error: {
          message: data?.error || `HTTP ${res.status}`,
          status: Number(res.status) || 0,
          code: data?.code,
        },
      }
    }
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
      ws = new WebSocket(wsUrl, { lookup: backendLookup })
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
      const { error } = await auth.setSession(saved, { emitSignedIn: true })
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

module.exports = { getClient, restoreSession, attachAuthPersistence, backendFetch, fetchBackendIpv4, resolveBackendIpv4, warmBackend }
