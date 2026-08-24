"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const ipc = fs.readFileSync(path.join(root, "electron", "supabase", "ipc.js"), "utf8")
const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
const types = fs.readFileSync(path.join(root, "src", "global.d.ts"), "utf8")

const channels = [
  "community:reviews",
  "community:review:create",
  "community:review:update",
  "community:review:remove",
  "community:review:report",
  "community:collections",
  "community:collection:get",
  "community:collection:create",
  "community:collection:update",
  "community:collection:remove",
  "community:collection:item:add",
  "community:collection:item:replace",
  "community:collection:item:remove",
  "community:collection:report",
]

test("IPC de comunidade registra operações aditivas no main", () => {
  for (const channel of channels) assert.ok(ipc.includes(`ipcMain.handle("${channel}"`), channel)
  assert.match(ipc, /garantirSessao\(\)/)
  assert.doesNotMatch(ipc, /community.*(?:access_token|refresh_token)/i)
})

test("preload e tipos expõem reviews/listas sem acesso direto ao ipcRenderer", () => {
  for (const method of [
    "communityReviews",
    "communityReviewCreate",
    "communityReviewUpdate",
    "communityReviewRemove",
    "communityCollections",
    "communityCollectionGet",
    "communityCollectionCreate",
    "communityCollectionUpdate",
    "communityCollectionRemove",
  ]) assert.match(preload, new RegExp(`${method}\\s*:`), method)
  assert.match(types, /CommunityReview/)
  assert.match(types, /CommunityCollection/)
  assert.match(types, /communityReviews:/)
  assert.match(types, /communityCollections:/)
})
