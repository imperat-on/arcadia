"use strict"

const MIN_PRIORITY = -10
const MAX_PRIORITY = 10

function normalizePriority(value) {
  const parsed = Number(value)
  if (!Number.isFinite(parsed)) return 0
  return Math.max(MIN_PRIORITY, Math.min(MAX_PRIORITY, Math.trunc(parsed)))
}

function nextQueued(queue) {
  return queue
    .map((item, index) => ({ item, index }))
    .filter(({ item }) => item?.status === "queued")
    .sort((a, b) => normalizePriority(b.item.priority) - normalizePriority(a.item.priority) || a.index - b.index)[0]?.item
}

module.exports = { MIN_PRIORITY, MAX_PRIORITY, normalizePriority, nextQueued }
