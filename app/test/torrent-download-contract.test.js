"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// torrent.js resolves its state path at require time. Keep this contract test
// isolated from a developer's real queue.
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-torrent-contract-"))
process.env.ARCADIA_DATA_DIR = dataDir
const { normalizeDownloadUri, normalizeTorrentId, start, list } = require("../electron/torrent")

test.after(() => fs.rmSync(dataDir, { recursive: true, force: true }))

test("torrent:start accepts only BitTorrent magnets and public HTTP(S) URIs", () => {
  assert.equal(
    normalizeDownloadUri("magnet:?xt=urn:btih:ABC123&dn=Classic%20Game"),
    "magnet:?xt=urn:btih:ABC123&dn=Classic%20Game",
  )
  assert.equal(normalizeDownloadUri("MAGNET:?xt=urn:btih:ABC123"), "magnet:?xt=urn:btih:ABC123")
  assert.equal(
    normalizeDownloadUri("https://downloads.example/game.zip"),
    "https://downloads.example/game.zip",
  )
  // Existing Sources feeds can still use ordinary HTTP hosters; credentials,
  // local targets and non-download protocols never cross the IPC boundary.
  assert.equal(
    normalizeDownloadUri("http://downloads.example/game.zip"),
    "http://downloads.example/game.zip",
  )
  for (const uri of [
    "javascript:alert(1)",
    "file:///etc/passwd",
    "data:text/plain,owned",
    "magnet:?dn=missing-xt",
    "magnet:?xt=urn:sha1:wrong-kind",
    "https://user:pass@example.test/game.zip",
    "http://127.0.0.1:8080/private.zip",
    "https://localhost/private.zip",
  ])
    assert.equal(normalizeDownloadUri(uri), "", uri)
})

test("torrent IDs are stable and normalized exactly once", () => {
  assert.equal(normalizeTorrentId("classics-source:17"), "tor:classics-source:17")
  assert.equal(normalizeTorrentId("tor:classics-source:17"), "tor:classics-source:17")
  assert.equal(normalizeTorrentId(17), "tor:17")
  assert.equal(normalizeTorrentId("tor:tor:17"), "")
  assert.equal(normalizeTorrentId("../outside"), "")
})

test("invalid Retro URI is rejected before a torrent state entry is created", async () => {
  const result = await start({
    gameId: "classic-source:17",
    url: "file:///etc/passwd",
    title: "Should not queue",
  })
  assert.deepEqual(result, { ok: false, error: "invalid_download_uri" })
  assert.deepEqual(list(), [])
})

test("HTTP Retro start persists a visible queue item before the host responds", async () => {
  let release
  const responseReady = new Promise((resolve) => {
    release = resolve
  })
  const originalFetch = global.fetch
  global.fetch = async () => responseReady
  let pending
  try {
    pending = start({
      gameId: "classic-source:18",
      url: "https://downloads.example/classic.zip",
      title: "Classic queue item",
    })
    // start() has reached the first await in the resolver/fetch path, while
    // the state placeholder is already durable and listable.
    await new Promise((resolve) => setImmediate(resolve))
    assert.deepEqual(
      list().map(({ gameId, title, engine }) => ({ gameId, title, engine })),
      [{ gameId: "tor:classic-source:18", title: "Classic queue item", engine: "http" }],
    )

    release(
      new Response("abc", {
        status: 200,
        headers: { "content-type": "application/octet-stream", "content-length": "3" },
      }),
    )
    assert.deepEqual(await pending, { ok: true })
  } finally {
    global.fetch = originalFetch
    // Keep this test from leaving the polling interval or a partial file.
    await require("../electron/torrent").cancel("classic-source:18")
  }
})

test("HTTP Retro não segue redirecionamento para host privado", async () => {
  const originalFetch = global.fetch
  global.fetch = async () =>
    new Response(null, {
      status: 302,
      headers: { location: "http://127.0.0.1/private.zip" },
    })
  try {
    const result = await start({
      gameId: "classic-source:19",
      url: "https://downloads.example/redirect.zip",
      title: "Unsafe redirect",
    })
    assert.equal(result.ok, false)
    assert.equal(result.queued, true)
    assert.match(result.error, /redirecionamento inseguro/)
    assert.equal(
      list().find((item) => item.gameId === "tor:classic-source:19")?.erro,
      "redirecionamento inseguro",
    )
  } finally {
    global.fetch = originalFetch
    await require("../electron/torrent").cancel("classic-source:19")
  }
})
