"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { isRovingKey, nextRovingIndex } = require("../src/components/ps5-launcher/rovingTab.cjs")

test("roving rail moves horizontally and clamps at the edges", () => {
  assert.equal(nextRovingIndex(2, 4, "ArrowLeft"), 1)
  assert.equal(nextRovingIndex(2, 4, "ArrowRight"), 3)
  assert.equal(nextRovingIndex(0, 4, "ArrowLeft"), 0)
  assert.equal(nextRovingIndex(3, 4, "ArrowRight"), 3)
})

test("Home and End provide fast keyboard access to rail edges", () => {
  assert.equal(nextRovingIndex(2, 4, "Home"), 0)
  assert.equal(nextRovingIndex(0, 4, "End"), 3)
  assert.equal(isRovingKey("Home"), true)
  assert.equal(isRovingKey("PageDown"), false)
})

test("empty rails and unrelated keys do not select an item", () => {
  assert.equal(nextRovingIndex(0, 0, "ArrowRight"), null)
  assert.equal(nextRovingIndex(0, 3, "ArrowDown"), null)
  assert.equal(nextRovingIndex(99, 3, "ArrowLeft"), 1)
})
