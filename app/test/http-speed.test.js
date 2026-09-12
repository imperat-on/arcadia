"use strict"

// Velocidade do download HTTP (bytes/s por amostra de tempo).
// Regressão do bug "419 MiB/s": a velocidade exibida era o TOTAL baixado
// porque o delta usava `_b`, que a projeção do tick não persiste — e o
// intervalo do tick não é 1s (delta por tick nunca foi bytes/s).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-hs-"))
process.env.ARCADIA_DATA_DIR = dataDir
const { httpBps } = require("../electron/torrent")

test.after(() => fs.rmSync(dataDir, { recursive: true, force: true }))

test("httpBps: primeira amostra é 0 (sem referência)", () => {
  assert.equal(httpBps(undefined, 5 * 1024 ** 2, 1000), 0)
  assert.equal(httpBps(null, 5 * 1024 ** 2, 1000), 0)
})

test("httpBps: delta de bytes dividido pelo tempo REAL decorrido", () => {
  // +10 MiB em 2s = 5 MiB/s
  assert.equal(httpBps({ bytes: 0, ts: 1000 }, 10 * 1024 ** 2, 3000), 5242880)
  // +1 MiB em 500ms = 2 MiB/s
  assert.equal(httpBps({ bytes: 0, ts: 1000 }, 1024 ** 2, 1500), 2097152)
})

test("httpBps: guardas — tempo parado e bytes regredindo (restart/pause)", () => {
  assert.equal(httpBps({ bytes: 100, ts: 5000 }, 200, 5000), 0)
  assert.equal(httpBps({ bytes: 5000, ts: 1000 }, 100, 2000), 0)
})

test("regressão 419 MiB/s: velocidade NUNCA é o total baixado", () => {
  const total = 419 * 1024 ** 2
  // 1º tick com o item já em 419 MiB: velocidade 0 (não 419 MiB/s).
  assert.equal(httpBps(undefined, total, 1), 0)
  // 2º tick: só o delta DESTE intervalo conta (50 MiB em 2s = 25 MiB/s).
  assert.equal(httpBps({ bytes: total, ts: 1000 }, total + 50 * 1024 ** 2, 3000), 25 * 1024 ** 2)
})
