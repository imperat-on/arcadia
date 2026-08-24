"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { createRateLimiter } = require("../src/rate-limit")

function responseMock() {
  return {
    statusCode: 200,
    headers: {},
    body: null,
    setHeader(name, value) {
      this.headers[name] = value
    },
    status(code) {
      this.statusCode = code
      return this
    },
    json(body) {
      this.body = body
      return this
    },
  }
}

function request(ip) {
  return { ip }
}

test("rate limiter bloqueia por IP e publica janela de retry", () => {
  let now = 10_000
  const limiter = createRateLimiter({
    windowMs: 10_000,
    max: 2,
    now: () => now,
  })
  let passed = 0
  const call = (ip) => {
    const res = responseMock()
    limiter(request(ip), res, () => { passed++ })
    return res
  }

  const first = call("198.51.100.10")
  assert.equal(first.statusCode, 200)
  assert.equal(first.headers["RateLimit-Limit"], "2")
  assert.equal(first.headers["RateLimit-Remaining"], "1")

  const second = call("198.51.100.10")
  assert.equal(second.headers["RateLimit-Remaining"], "0")

  const blocked = call("198.51.100.10")
  assert.equal(blocked.statusCode, 429)
  assert.deepEqual(blocked.body, { error: "muitas_requisicoes" })
  assert.equal(blocked.headers["Retry-After"], "10")
  assert.equal(blocked.headers["RateLimit-Reset"], "10")
  assert.equal(passed, 2)

  // Um cliente diferente nao herda o contador do primeiro.
  const other = call("198.51.100.11")
  assert.equal(other.statusCode, 200)
  assert.equal(other.headers["RateLimit-Remaining"], "1")

  now += 10_000
  const afterWindow = call("198.51.100.10")
  assert.equal(afterWindow.statusCode, 200)
  assert.equal(afterWindow.headers["RateLimit-Remaining"], "1")
})

test("rate limiter mantem o numero de buckets limitado", () => {
  let now = 0
  const limiter = createRateLimiter({
    windowMs: 60_000,
    max: 1,
    maxKeys: 2,
    now: () => now,
  })
  const call = (ip) => limiter(request(ip), responseMock(), () => {})

  call("198.51.100.1")
  call("198.51.100.2")
  call("198.51.100.3")
  // O terceiro cliente substitui apenas o bucket mais antigo; nao ha
  // crescimento ilimitado com origens novas.
  assert.ok(limiter.size() <= 2)
  now = 60_000
  call("198.51.100.4")
  // A janela expirada e limpa antes de receber o novo bucket.
  assert.equal(limiter.size(), 1)
})
