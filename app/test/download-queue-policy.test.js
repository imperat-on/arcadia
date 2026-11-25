"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { nextQueued, normalizePriority } = require("../electron/download-queue-policy")

test("prioridade da fila é limitada e normalizada", () => {
  assert.equal(normalizePriority("4.9"), 4)
  assert.equal(normalizePriority(99), 10)
  assert.equal(normalizePriority(-99), -10)
  assert.equal(normalizePriority("nan"), 0)
})

test("fila escolhe prioridade maior e preserva FIFO em empate", () => {
  const queue = [
    { appid: "a", status: "queued", priority: 0 },
    { appid: "b", status: "paused", priority: 10 },
    { appid: "c", status: "queued", priority: 5 },
    { appid: "d", status: "queued", priority: 5 },
  ]
  assert.equal(nextQueued(queue).appid, "c")
  assert.equal(nextQueued([{ appid: "x", status: "done" }]), undefined)
})
