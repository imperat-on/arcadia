"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  createSteamNewsImageResolver,
  extractSteamNewsImage,
  isSteamNewsUrl,
  normalizeSteamImageUrl,
} = require("../electron/steam-news")

test("normaliza imagens do Steam Clan e extrai metadados da página", () => {
  assert.equal(
    normalizeSteamImageUrl("{STEAM_CLAN_IMAGE}/35190511/capa.png"),
    "https://clan.cloudflare.steamstatic.com/images/35190511/capa.png",
  )
  assert.equal(
    extractSteamNewsImage('<meta property="og:image" content="https://clan.akamai.steamstatic.com/images/1/news.jpg">'),
    "https://clan.akamai.steamstatic.com/images/1/news.jpg",
  )
  assert.equal(
    extractSteamNewsImage('[img src="{STEAM_CLAN_IMAGE}/22/post.webp"][/img]'),
    "https://clan.cloudflare.steamstatic.com/images/22/post.webp",
  )
})

test("resolver busca a capa da notícia uma vez e reutiliza o cache", async () => {
  let calls = 0
  const resolve = createSteamNewsImageResolver({
    fetchImpl: async () => {
      calls += 1
      return {
        ok: true,
        text: async () => '<link rel="image_src" href="https://clan.akamai.steamstatic.com/images/7/post.png">',
      }
    },
  })
  const item = { url: "https://steamstore-a.akamaihd.net/news/externalpost/steam_community_announcements/123" }
  assert.equal(await resolve(item), "https://clan.akamai.steamstatic.com/images/7/post.png")
  assert.equal(await resolve(item), "https://clan.akamai.steamstatic.com/images/7/post.png")
  assert.equal(calls, 1)
})

test("resolver não acessa URLs externas à Steam", async () => {
  let called = false
  const resolve = createSteamNewsImageResolver({ fetchImpl: async () => { called = true } })
  assert.equal(isSteamNewsUrl("https://evil.example/post"), false)
  assert.equal(await resolve({ url: "https://evil.example/post" }), "")
  assert.equal(called, false)
})
