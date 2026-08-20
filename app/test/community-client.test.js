"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const { createCommunityClient } = require("../electron/supabase/community")

function response(body, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async text() {
      return body === undefined ? "" : JSON.stringify(body)
    },
  }
}

function tempCache() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-community-"))
  return { dir, file: path.join(dir, "community-cache.json") }
}

test("community client normaliza reviews, envia auth no main e persiste cache atomico", async () => {
  const tmp = tempCache()
  const calls = []
  const client = createCommunityClient({
    baseUrl: "https://api.example.test/",
    cachePath: tmp.file,
    authHeaders: () => ({ apikey: "anon", authorization: "Bearer secret" }),
    fetchImpl: async (url, options) => {
      calls.push({ url, options })
      return response({
        ok: true,
        reviews: [{ id: 3, appid: "steam:10", text: "bom", rating: 5 }],
        pagination: { limit: 10, offset: 0, has_more: false },
      })
    },
  })

  const result = await client.listReviews("steam:10", { limit: 10 })
  assert.equal(result.ok, true)
  assert.equal(result.reviews[0].appid, "steam:10")
  assert.match(calls[0].url, /\/community\/v1\/reviews\?appid=steam%3A10&limit=10&offset=0/)
  assert.equal(calls[0].options.headers.authorization, "Bearer secret")
  assert.equal(JSON.parse(fs.readFileSync(tmp.file, "utf8")).reviews["reviews:steam:10"].items.length, 1)
  assert.doesNotMatch(JSON.stringify(result), /\/home\/|secret/)
  fs.rmSync(tmp.dir, { recursive: true, force: true })
})

test("community client devolve lista em cache quando a rede cai", async () => {
  const tmp = tempCache()
  let online = true
  const client = createCommunityClient({
    cachePath: tmp.file,
    fetchImpl: async () => {
      if (!online) throw new Error("network down")
      return response({ reviews: [{ id: 1, appid: "x", text: "cache", rating: 4 }] })
    },
  })
  await client.listReviews("x")
  online = false
  const result = await client.listReviews("x")
  assert.equal(result.ok, true)
  assert.equal(result.offline, true)
  assert.equal(result.reviews[0].text, "cache")
  fs.rmSync(tmp.dir, { recursive: true, force: true })
})

test("community client mantém envelopes de mutação e caminhos codificados", async () => {
  const tmp = tempCache()
  const calls = []
  const client = createCommunityClient({
    cachePath: tmp.file,
    fetchImpl: async (url, options) => {
      calls.push({ url, options })
      if (options.method === "DELETE") return response(undefined, 204)
      if (url.includes("/items")) return response({ ok: true, items: [{ appid: "steam:42", position: 0 }] }, 201)
      return response({ review: { id: 9, appid: "steam:42", text: "ok", rating: 5 } }, 201)
    },
  })
  const review = await client.createReview({ appid: "steam:42", text: "ok", rating: 5 })
  assert.equal(review.review.id, 9)
  await client.removeReview(9)
  const item = await client.addCollectionItem("id/with space", "steam:42")
  assert.equal(item.items[0].appid, "steam:42")
  assert.match(calls.at(-1).url, /collections\/id%2Fwith%20space\/items$/)
  assert.equal(JSON.parse(calls.at(-1).options.body).appid, "steam:42")
  fs.rmSync(tmp.dir, { recursive: true, force: true })
})
