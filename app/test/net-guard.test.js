import test from "node:test"
import assert from "node:assert/strict"
import { createRequire } from "node:module"

const require = createRequire(import.meta.url)
const { sanear } = require("../electron/net-guard.js")

// O crash de producao: header com code point > 255 (U+FFFD de dado corrompido).
// O undici recusa: "Cannot convert argument to a ByteString ... 65533 > 255".
// Esses valores chegam como OBJETO (o proprio `new Headers()` ja recusaria na
// construcao, que e' o bug: nada chega ao bus sem passar por aqui).
test("valor com U+FFFD e' filtrado (o caso do crash)", () => {
  const s = sanear({ Authorization: "Bearer abc\uFFFDdef" })
  assert.equal(s.Authorization, "Bearer abcdef")
})

test("titulo com (R) e demais latin-1 passam (<= 255)", () => {
  const s = sanear({ "X-Game": "Call of Duty\u00AE: WWII" })
  assert.equal(s["X-Game"], "Call of Duty\u00AE: WWII")
})

test("emoji e CJK sao removidos do valor", () => {
  const s = sanear({ "X-T": "ok \u{1F600}\u4E2D" })
  assert.equal(s["X-T"], "ok ")
})

test("nome do header tambem e' saneado", () => {
  const s = sanear({ "X-N\u00E1ome": "v" })
  assert.equal(Object.keys(s)[0], "X-Nome")
})

test("Headers do bus (entries) validos sao aceitos", () => {
  const h = new Headers({ a: "1", b: "2" })
  const s = sanear(h)
  assert.equal(s.a, "1")
  assert.equal(s.b, "2")
})

test("valores nulos sao ignorados e o objeto vazio nao quebra", () => {
  assert.deepEqual(sanear({ a: null, b: undefined, c: "ok" }), { c: "ok" })
  assert.deepEqual(sanear({}), {})
})

test("sem headers devolve o mesmo valor", () => {
  assert.equal(sanear(null), null)
  assert.equal(sanear(undefined), undefined)
})
