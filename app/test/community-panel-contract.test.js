"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const panel = fs.readFileSync(
  path.join(__dirname, "..", "src", "components", "CommunityPanel.tsx"),
  "utf8",
)

test("painel de comunidade usa apenas a bridge e mantém o estado offline", () => {
  for (const method of [
    "communityReviews",
    "communityReviewCreate",
    "communityReviewUpdate",
    "communityReviewRemove",
    "communityReviewReport",
    "communityCollections",
    "communityCollectionCreate",
    "communityCollectionUpdate",
    "communityCollectionRemove",
    "communityCollectionReport",
  ]) assert.match(panel, new RegExp(method), method)
  assert.match(panel, /offline/i)
  assert.match(panel, /aria-label=/)
  assert.match(panel, /Página/)
})

test("painel não acessa ipcRenderer nem transporta credenciais ou paths locais", () => {
  assert.doesNotMatch(panel, /ipcRenderer/)
  assert.doesNotMatch(panel, /access_token|refresh_token|authorization/i)
  assert.doesNotMatch(panel, /file:\/\/|process\.env|\/home\//)
  assert.match(panel, /window\.confirm/)
  assert.match(panel, /window\.prompt/)
})
