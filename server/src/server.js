"use strict"

// Arcadia server - backend Node proprio (substitui o Supabase).
// Bootstrap express + healthcheck. Rotas de dominio sao registradas
// por fase (auth, rpc, friends, storage, realtime).

const express = require("express")
const { db, DATA_DIR } = require("./db")
const { registerAuthRoutes } = require("./auth-routes")
const { registerRestRoutes } = require("./rest-routes")
const { registerSyncRoutes } = require("./sync-routes")
const { registerStorageRoutes } = require("./storage-routes")
const { registerCatalogRoutes } = require("./catalog-routes")
const { registerRealtime } = require("./realtime")

const PORT = process.env.PORT || 3000
const app = express()

app.use(express.json({ limit: "1mb" }))
app.use(require("compression")())

registerAuthRoutes(app)
registerRestRoutes(app)
registerSyncRoutes(app)
registerStorageRoutes(app)
registerCatalogRoutes(app)

// Healthcheck (usado pelo systemd e pelo deploy)
app.get("/health", (req, res) => {
  res.json({ ok: true, name: "arcadia-server", time: new Date().toISOString() })
})

// Middleware de erro padrao (JSON, nao HTML)
// eslint-disable-next-line no-unused-vars
app.use((err, req, res, next) => {
  console.error("[erro]", err)
  res.status(500).json({ error: "erro_interno" })
})

const server = app.listen(PORT, "0.0.0.0", () => {
  console.log(`arcadia-server ouvindo em http://0.0.0.0:${PORT} (db=${db ? "ok" : "erro"})`)
})

registerRealtime(server)

process.on("SIGTERM", () => server.close(() => process.exit(0)))
process.on("SIGINT", () => server.close(() => process.exit(0)))

module.exports = { app, server, db, DATA_DIR }
