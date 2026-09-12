"use strict"

// Contratos da injeção de trackers do engine aria2 (Windows). Magnets sem
// tracker dependem do DHT; em redes que bloqueiam UDP o DHT pode estar morto,
// então o engine sempre anexa a lista pública verificada.

const test = require("node:test")
const assert = require("node:assert/strict")
const { TRACKERS_PADRAO, injetarTrackers } = require("../electron/aria2")

test("aria2: trackerless magnets receive every default public tracker", () => {
  const magnet = "magnet:?xt=urn:btih:481b6e3617be4c88f96cb25e47c9d8272130071e&dn=repack"
  const out = injetarTrackers(magnet)
  assert.ok(out.startsWith(magnet), "original magnet text must be preserved")
  for (const t of TRACKERS_PADRAO) {
    assert.ok(out.includes("tr=" + encodeURIComponent(t)), `missing tracker: ${t}`)
  }
})

test("aria2: existing trackers are never duplicated (raw or URL-encoded)", () => {
  const t = TRACKERS_PADRAO[0]
  for (const forma of [t, encodeURIComponent(t)]) {
    const magnet = `magnet:?xt=urn:btih:${"a".repeat(40)}&tr=${forma}`
    const out = injetarTrackers(magnet)
    // A forma presente no magnet entra 1x (a outra forma não aparece, pois
    // "%2F" não contém "/"); o total entre as duas formas deve ser 1.
    const total = out.split(t).length - 1 + out.split(encodeURIComponent(t)).length - 1
    assert.equal(total, 1, `duplicated or dropped: ${t}`)
  }
})

test("aria2: default tracker list is well-formed and non-trivial", () => {
  assert.ok(TRACKERS_PADRAO.length >= 4)
  for (const t of TRACKERS_PADRAO) {
    assert.match(t, /^(https?|udp):\/\/[^/\s]+:\d+\/announce$/, `malformed: ${t}`)
  }
  // Pelo menos metade em HTTP(S): UDP pode estar bloqueado na rede do usuário.
  const http = TRACKERS_PADRAO.filter((t) => /^https?:/.test(t))
  assert.ok(http.length >= TRACKERS_PADRAO.length / 2)
})

test("aria2: non-magnet input passes through untouched", () => {
  assert.equal(injetarTrackers("https://example.test/file.zip"), "https://example.test/file.zip")
  assert.equal(injetarTrackers(""), "")
  assert.equal(injetarTrackers(null), "")
})
