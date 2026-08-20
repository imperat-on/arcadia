"use strict"

/**
 * Returns the next item for a horizontal roving-tabindex group.
 *
 * Keeping this policy independent from React makes the edge behaviour
 * explicit: the rail clamps at both ends and Home/End jump to the edges.
 */
function nextRovingIndex(current, total, key) {
  if (!Number.isFinite(total) || total <= 0) return null
  const last = Math.max(0, Math.floor(total) - 1)
  const index = Math.min(last, Math.max(0, Math.floor(Number(current) || 0)))

  if (key === "ArrowLeft") return Math.max(0, index - 1)
  if (key === "ArrowRight") return Math.min(last, index + 1)
  if (key === "Home") return 0
  if (key === "End") return last
  return null
}

function isRovingKey(key) {
  return key === "ArrowLeft" || key === "ArrowRight" || key === "Home" || key === "End"
}

module.exports = { isRovingKey, nextRovingIndex }
