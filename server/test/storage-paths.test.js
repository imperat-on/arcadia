"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
process.env.JWT_SECRET = process.env.JWT_SECRET || "storage-path-tests-secret-012345678901234567890123"
const { safeFile, safeObjectPath, magicDeImagem } = require("../src/storage-routes")

const UUID = "11111111-1111-4111-8111-111111111111"

test("storage valida nomes e mantém objetos dentro do owner", () => {
  const root = "/tmp/arcadia-storage"
  assert.equal(safeFile({ nomeRe: /^[0-9]+\.png$/i }, "1.png"), true)
  assert.equal(safeFile({ nomeRe: /^[0-9]+\.png$/i }, "../1.png"), false)
  assert.equal(safeObjectPath(root, UUID, "1.png"), `${root}/${UUID}/1.png`)
  assert.equal(safeObjectPath(root, UUID, "../1.png"), null)
  assert.equal(safeObjectPath(root, "not-a-uuid", "1.png"), null)
})

test("storage reconhece magic bytes de imagens sem confiar na extensão", () => {
  assert.equal(magicDeImagem(Buffer.from([0x89, 0x50, 0x4e, 0x47])).mime, "image/png")
  assert.equal(magicDeImagem(Buffer.from("not an image")), null)
})
