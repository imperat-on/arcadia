"use strict"

const asyncHandler = require("./async-handler")
const { sendError } = require("./api-observability")

// `checkDatabase` is injected so the route contract can be tested without a
// live PostgreSQL connection. Production passes db.query("SELECT 1").
function registerHealthRoutes(app, { checkDatabase, now = () => new Date().toISOString() } = {}) {
  if (typeof checkDatabase !== "function") {
    throw new TypeError("checkDatabase precisa ser uma funcao")
  }

  // Liveness/legacy health endpoint. Keep its existing response shape and
  // database probe so systemd/deploy clients do not need to change.
  app.get("/health", asyncHandler(async (req, res) => {
    await checkDatabase()
    res.json({ ok: true, name: "arcadia-server", time: now() })
  }))

  // Readiness is intentionally separate from liveness: orchestrators can
  // remove an instance from traffic when PostgreSQL is unavailable without
  // treating a still-running process as dead.
  app.get("/ready", asyncHandler(async (req, res) => {
    try {
      await checkDatabase()
    } catch {
      return sendError(req, res, 503, "servico_indisponivel")
    }
    return res.json({ ok: true, ready: true, name: "arcadia-server", time: now() })
  }))
}

module.exports = { registerHealthRoutes }
