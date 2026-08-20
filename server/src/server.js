"use strict"

// Arcadia server - backend Node proprio (substitui o Supabase).
// Bootstrap express + healthcheck. Rotas de dominio sao registradas
// por fase (auth, rpc, friends, storage, realtime).

const express = require("express")
const { db, DATA_DIR, initDb } = require("./db")
const { registerAuthRoutes } = require("./auth-routes")
const { registerRestRoutes } = require("./rest-routes")
const { registerSyncRoutes } = require("./sync-routes")
const { registerStorageRoutes, startOrphanCleanup } = require("./storage-routes")
const { registerCatalogRoutes, warmUpCatalog, precarregarCatalogoCompleto } = require("./catalog-routes")
const { registerCommunityRoutes } = require("./community-routes")
const { registerRealtime } = require("./realtime")
const { registerHealthRoutes } = require("./health-routes")
const { requestContext, structuredErrors, sendError, handleError } = require("./api-observability")

const PORT = process.env.PORT || 3000
const app = express()

// Correlation is installed before parsers/routes so malformed requests and
// early failures still carry the same request id in headers and JSON errors.
app.use(requestContext)
app.use(structuredErrors)

// O renderer do Electron roda em file://; libera apenas leitura pública dos
// objetos e chamadas da API do launcher. Credenciais continuam protegidas
// pelos tokens nos endpoints autenticados.
app.use((req, res, next) => {
  const origin = req.headers.origin
  if (origin === "null" || origin === "file://" || !origin) {
    res.setHeader("Access-Control-Allow-Origin", origin || "*")
  }
  res.setHeader("Access-Control-Allow-Methods", "GET,POST,PATCH,DELETE,OPTIONS")
  res.setHeader("Access-Control-Allow-Headers", "content-type, authorization, apikey, x-request-id")
  res.setHeader("Access-Control-Expose-Headers", "X-Request-Id")
  if (req.method === "OPTIONS") return res.sendStatus(204)
  next()
})

app.use(express.json({ limit: "1mb" }))
app.use(require("compression")())

registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
registerStorageRoutes(app)
registerCatalogRoutes(app)
registerCommunityRoutes(app)
registerHealthRoutes(app, { checkDatabase: () => db.query("SELECT 1") })

// JSON para rotas desconhecidas; evita que proxies recebam a pagina HTML
// padrao do Express e mantém o mesmo contrato de erro das demais APIs.
app.use((req, res) => sendError(req, res, 404, "rota_nao_encontrada"))

// Middleware de erro padrao (JSON, nao HTML). A chave legada `error` permanece
// presente; `code`, `message` e `request_id` tornam o erro observavel.
// eslint-disable-next-line no-unused-vars
app.use(handleError)

let server
let realtime

async function startServer() {
  await initDb()
  startOrphanCleanup()
  server = app.listen(PORT, "0.0.0.0", () => {
    console.log(`arcadia-server ouvindo em http://0.0.0.0:${PORT} (postgres=ok)`)
    warmUpCatalog().catch((error) => console.error("[warmup]", error))
    precarregarCatalogoCompleto()
  })
  realtime = registerRealtime(server)
  return server
}

async function shutdown() {
  if (realtime) {
    for (const client of realtime.clients) client.terminate()
    await new Promise((resolve) => realtime.close(resolve))
  }
  if (server) {
    const closed = new Promise((resolve) => server.close(resolve))
    server.closeAllConnections?.()
    await closed
  }
  await db.end()
}

if (require.main === module) {
  startServer().catch((error) => {
    console.error("[startup]", error)
    process.exit(1)
  })
  process.on("SIGTERM", () => shutdown().then(() => process.exit(0)))
  process.on("SIGINT", () => shutdown().then(() => process.exit(0)))
}

module.exports = { app, startServer, shutdown, db, DATA_DIR }
