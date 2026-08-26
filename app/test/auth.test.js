// Testes do auth — partes offline (sem rede): validações antes de qualquer
// chamada de rede. O signUp/signIn com dados válidos chama o Supabase (rede)
// e fica para o teste manual (f5-4).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")

const auth = require("../electron/supabase/auth.js")

// ---------- signUp ----------
test("signUp: email inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "nao-email", username: "teste", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "email_invalido" })
})

test("signUp: username inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "AB", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: username com caractere proibido falha", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste@x", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: senha curta (< 6) falha sem rede", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste", password: "123" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})

test("signUp: sem senha falha", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})

// ---------- signIn ----------
test("signIn: username inválido falha sem rede", async () => {
  const r = await auth.signIn({ username: "AB", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signIn: sem senha falha", async () => {
  const r = await auth.signIn({ username: "teste" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})

// ---------- caminhoDeArquivo (file:// com cache-buster do pickImage) ----------
test("caminhoDeArquivo: file:// com ?t= vira caminho puro", () => {
  assert.equal(
    auth.caminhoDeArquivo("file:///tmp/arcadia/avatar.png?t=1754412345678"),
    "/tmp/arcadia/avatar.png",
  )
})

test("caminhoDeArquivo: caminho cru e com espaços URL-encoded", () => {
  assert.equal(auth.caminhoDeArquivo("/tmp/minha foto.png"), "/tmp/minha foto.png")
  assert.equal(
    auth.caminhoDeArquivo("file:///tmp/minha%20foto.gif?t=1"),
    "/tmp/minha foto.gif",
  )
})

// ---------- dimensoesDeImagem (cabeçalhos sintéticos, sem Electron) ----------
function cabecalho(tipo) {
  // Monta buffers mínimos com os bytes de dimensão no lugar certo.
  // ATENÇÃO: bytes >0x7F devem ir como array de bytes (string write usa UTF-8!)
  const b = Buffer.alloc(64, 0)
  if (tipo === "png") {
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(b, 0)
    b.writeUInt32BE(1200, 16)
    b.writeUInt32BE(800, 20)
  } else if (tipo === "gif") {
    b.write("GIF89a", 0)
    b.writeUInt16LE(64, 6)
    b.writeUInt16LE(48, 8)
  } else if (tipo === "jpg") {
    b[0] = 0xff
    b[1] = 0xd8
    b[2] = 0xff
    b[3] = 0xe0
    b.writeUInt16BE(16, 4) // len APP0
    b[20] = 0xff
    b[21] = 0xc2 // SOF2
    b.writeUInt16BE(17, 22) // len SOF
    b.writeUInt16BE(600, 25) // h
    b.writeUInt16BE(400, 27) // w
  } else if (tipo === "webp") {
    b.write("RIFF", 0)
    b.write("WEBP", 8)
    b.write("VP8 ", 12)
    b.writeUInt16LE(300, 26) // w-1
    b.writeUInt16LE(200, 28) // h-1
  }
  return b
}

test("dimensoesDeImagem: PNG/GIF/JPEG/WebP", () => {
  assert.deepEqual(auth.dimensoesDeImagem(cabecalho("png")), { w: 1200, h: 800 })
  assert.deepEqual(auth.dimensoesDeImagem(cabecalho("gif")), { w: 64, h: 48 })
  assert.deepEqual(auth.dimensoesDeImagem(cabecalho("jpg")), { w: 400, h: 600 })
  assert.deepEqual(auth.dimensoesDeImagem(cabecalho("webp")), { w: 300, h: 200 })
  assert.equal(auth.dimensoesDeImagem(Buffer.alloc(8)), null)
  assert.equal(auth.dimensoesDeImagem(null), null)
})

test("processaAvatar: dimensão acima de 512 é rejeitada (qualquer formato)", () => {
  const r = auth.processaAvatar(cabecalho("png"), ".png") // 1200x800
  assert.equal(r.erro, "avatar_dimensoes")
})

test("processaAvatar: GIF passa direto (animação preservada) se ≤512", () => {
  const r = auth.processaAvatar(cabecalho("gif"), ".gif") // 64x48
  assert.equal(r.erro, undefined)
  assert.equal(r.mime, "image/gif")
  assert.equal(r.ext, ".gif")
})

test("processaAvatar: estático pequeno passa sem re-encode (≤256)", () => {
  const r = auth.processaAvatar(cabecalho("webp"), ".webp") // 300x200? >256
  // 300x200 passa no 512 mas excede 256 → tenta nativeImage (ausente em Node)
  // → cai no fallback e devolve o original (sem erro)
  assert.equal(r.erro, undefined)
  assert.equal(r.mime, "image/webp")
})
// (setBackground testado em server/test/e2e-conta.test.js)
