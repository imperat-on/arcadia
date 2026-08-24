"use strict"

// Limiter simples por janela fixa para endpoints publicos. O processo do
// servidor e unico, portanto um Map evita uma dependencia externa apenas
// para proteger o bootstrap de autenticacao. A chave padrao usa req.ip (o
// Express nao confia em X-Forwarded-For sem trust proxy configurado).

const DEFAULT_WINDOW_MS = 60 * 1000
const DEFAULT_MAX = 60
const DEFAULT_MAX_KEYS = 10_000

function positiveInteger(value, fallback) {
  return Number.isSafeInteger(value) && value > 0 ? value : fallback
}

function clientKey(req) {
  return String(req.ip || req.socket?.remoteAddress || "unknown")
}

function createRateLimiter(options = {}) {
  const windowMs = positiveInteger(options.windowMs, DEFAULT_WINDOW_MS)
  const max = positiveInteger(options.max, DEFAULT_MAX)
  const maxKeys = positiveInteger(options.maxKeys, DEFAULT_MAX_KEYS)
  const now = typeof options.now === "function" ? options.now : () => Date.now()
  const keyGenerator = typeof options.keyGenerator === "function" ? options.keyGenerator : clientKey
  const buckets = new Map()
  let nextCleanup = 0

  function cleanup(timestamp) {
    if (timestamp < nextCleanup && buckets.size < maxKeys) return
    for (const [key, bucket] of buckets) {
      if (bucket.resetAt <= timestamp) buckets.delete(key)
    }
    // Evita crescimento sem limite se muitas origens novas aparecerem durante
    // a mesma janela. A remocao e apenas do bucket mais antigo; isso nao altera
    // o limite dos demais clientes.
    while (buckets.size >= maxKeys) {
      const oldest = buckets.keys().next().value
      if (oldest === undefined) break
      buckets.delete(oldest)
    }
    nextCleanup = timestamp + windowMs
  }

  function limiter(req, res, next) {
    const timestamp = now()
    cleanup(timestamp)
    const generatedKey = keyGenerator(req)
    const key = String(generatedKey || "unknown")
    let bucket = buckets.get(key)
    if (!bucket || bucket.resetAt <= timestamp) {
      bucket = { count: 0, resetAt: timestamp + windowMs }
      buckets.set(key, bucket)
    }

    const resetIn = Math.max(0, Math.ceil((bucket.resetAt - timestamp) / 1000))
    const remaining = Math.max(0, max - bucket.count)
    res.setHeader("RateLimit-Limit", String(max))
    res.setHeader("RateLimit-Remaining", String(remaining))
    res.setHeader("RateLimit-Reset", String(resetIn))

    if (bucket.count >= max) {
      res.setHeader("Retry-After", String(Math.max(1, resetIn)))
      return res.status(429).json({ error: "muitas_requisicoes" })
    }

    bucket.count += 1
    res.setHeader("RateLimit-Remaining", String(Math.max(0, max - bucket.count)))
    return next()
  }

  // Exposto apenas para testes e para os testes de isolamento do servidor.
  // Nao e necessario ao uso normal do middleware.
  limiter.reset = () => buckets.clear()
  limiter.size = () => buckets.size

  return limiter
}

module.exports = { createRateLimiter, clientKey }
