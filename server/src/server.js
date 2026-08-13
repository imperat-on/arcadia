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
const { registerRealtime } = require("./realtime")

const PORT = process.env.PORT || 3000
const app = express()

// O renderer do Electron roda em file://; libera apenas leitura pública dos
// objetos e chamadas da API do launcher. Credenciais continuam protegidas
// pelos tokens nos endpoints autenticados.
app.use((req, res, next) => {
  const origin = req.headers.origin
  if (origin === "null" || origin === "file://" || !origin) {
    res.setHeader("Access-Control-Allow-Origin", origin || "*")
  }
  res.setHeader("Access-Control-Allow-Methods", "GET,POST,PATCH,DELETE,OPTIONS")
  res.setHeader("Access-Control-Allow-Headers", "content-type, authorization, apikey")
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

// Healthcheck (usado pelo systemd e pelo deploy)
app.get("/health", async (req, res, next) => {
  try {
    await db.query("SELECT 1")
    res.json({ ok: true, name: "arcadia-server", time: new Date().toISOString() })
  } catch (error) {
    next(error)
  }
})

// Middleware de erro padrao (JSON, nao HTML)
// eslint-disable-next-line no-unused-vars
app.use((err, req, res, next) => {
  console.error("[erro]", err)
  res.status(500).json({ error: "erro_interno" })
})

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
