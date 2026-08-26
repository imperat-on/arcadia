"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const {
  createProtocolHandler,
  normalizeAssetPath,
  resolveAssetPath,
  isServable,
  getMimeType,
} = require("../electron/themes/protocol")

// --- Helpers ---

function createFakeFs(files = {}) {
  return {
    readFileSync(p) {
      const key = String(p)
      if (!(key in files)) throw new Error(`ENOENT: ${key}`)
      return files[key]
    },
    lstatSync(p) {
      const key = String(p)
      if (!(key in files)) throw new Error(`ENOENT: ${key}`)
      return {
        isSymbolicLink: () => false,
        isFile: () => true,
        isDirectory: () => false,
      }
    },
  }
}

// --- normalizeAssetPath ---

test("protocol normalizeAssetPath aceita paths válidos", () => {
  assert.equal(normalizeAssetPath("assets/bg.webp"), "assets/bg.webp")
  assert.equal(normalizeAssetPath("assets/sub/image.png"), "assets/sub/image.png")
  assert.equal(normalizeAssetPath("./assets/bg.webp"), "assets/bg.webp")
})

test("protocol normalizeAssetPath rejeita traversal", () => {
  assert.equal(normalizeAssetPath("../../etc/passwd"), null)
  assert.equal(normalizeAssetPath("assets/../../secret"), null)
  assert.equal(normalizeAssetPath("../outside"), null)
})

test("protocol normalizeAssetPath rejeita absolutos e vazios", () => {
  assert.equal(normalizeAssetPath("/etc/passwd"), null)
  assert.equal(normalizeAssetPath(""), null)
  assert.equal(normalizeAssetPath(null), null)
  assert.equal(normalizeAssetPath(42), null)
})

test("protocol normalizeAssetPath rejeita backslash", () => {
  assert.equal(normalizeAssetPath("assets\\evil"), null)
})

// --- resolveAssetPath ---

test("protocol resolveAssetPath resolve dentro do tema", () => {
  const result = resolveAssetPath("/themes/test", "assets/bg.webp")
  assert.equal(result.ok, true)
  assert.ok(result.resolved.endsWith(path.join("assets", "bg.webp")))
})

test("protocol resolveAssetPath rejeita saída do tema", () => {
  const result = resolveAssetPath("/themes/test", "../../etc/passwd")
  assert.equal(result.ok, false)
  assert.equal(result.error, "path_invalido")
})

// --- isServable / getMimeType ---

test("protocol isServable aceita imagens, vídeos e fontes", () => {
  assert.equal(isServable("bg.webp"), true)
  assert.equal(isServable("image.png"), true)
  assert.equal(isServable("video.mp4"), true)
  assert.equal(isServable("font.woff2"), true)
})

test("protocol isServable rejeita CSS, JSON, JS", () => {
  assert.equal(isServable("theme.css"), false)
  assert.equal(isServable("data.json"), false)
  assert.equal(isServable("script.js"), false)
  assert.equal(isServable("page.html"), false)
})

test("protocol getMimeType retorna MIME correto", () => {
  assert.equal(getMimeType("image.png"), "image/png")
  assert.equal(getMimeType("photo.jpg"), "image/jpeg")
  assert.equal(getMimeType("photo.jpeg"), "image/jpeg")
  assert.equal(getMimeType("anim.webp"), "image/webp")
  assert.equal(getMimeType("anim.gif"), "image/gif")
  assert.equal(getMimeType("font.woff2"), "font/woff2")
  assert.equal(getMimeType("video.mp4"), "video/mp4")
  assert.equal(getMimeType("video.webm"), "video/webm")
  assert.equal(getMimeType("unknown.xyz"), null)
})

// --- createProtocolHandler ---

test("protocol handler registra tema e serve asset", () => {
  const themeDir = "/themes/arcadia.aurora"
  const fs = createFakeFs({
    [path.join(themeDir, "assets", "bg.webp")]: Buffer.from("fake-image"),
  })
  const handler = createProtocolHandler({ fsImpl: fs })
  handler.registerTheme("arcadia.aurora", themeDir)

  const result = handler.handleRequest("arcadia-theme://arcadia.aurora/assets/bg.webp")
  assert.equal(result.ok, true)
  assert.equal(result.mime, "image/webp")
  assert.ok(Buffer.isBuffer(result.data))
})

test("protocol handler rejeita tema não registrado", () => {
  const handler = createProtocolHandler({ fsImpl: createFakeFs() })
  const result = handler.handleRequest("arcadia-theme://unknown/assets/bg.webp")
  assert.equal(result.ok, false)
  assert.equal(result.error, "tema_nao_registrado")
})

test("protocol handler rejeita extensão não servível", () => {
  const themeDir = "/themes/test"
  const fs = createFakeFs({
    [path.join(themeDir, "theme.css")]: "body { color: red; }",
  })
  const handler = createProtocolHandler({ fsImpl: fs })
  handler.registerTheme("test", themeDir)

  const result = handler.handleRequest("arcadia-theme://test/theme.css")
  assert.equal(result.ok, false)
  assert.equal(result.error, "extensao_nao_permitida")
})

test("protocol handler rejeita traversal na URL", () => {
  const themeDir = "/themes/test"
  const fs = createFakeFs()
  const handler = createProtocolHandler({ fsImpl: fs })
  handler.registerTheme("test", themeDir)

  const result = handler.handleRequest("arcadia-theme://test/../../etc/passwd")
  assert.equal(result.ok, false)
  assert.equal(result.error, "path_invalido")
})

test("protocol handler rejeita URL com protocolo errado", () => {
  const handler = createProtocolHandler({ fsImpl: createFakeFs() })
  assert.equal(handler.handleRequest("https://evil.com").ok, false)
  assert.equal(handler.handleRequest("").ok, false)
  assert.equal(handler.handleRequest(null).ok, false)
})

test("protocol handler rejeita arquivo não encontrado", () => {
  const themeDir = "/themes/test"
  const fs = createFakeFs()
  const handler = createProtocolHandler({ fsImpl: fs })
  handler.registerTheme("test", themeDir)

  const result = handler.handleRequest("arcadia-theme://test/assets/missing.webp")
  assert.equal(result.ok, false)
  assert.equal(result.error, "arquivo_nao_encontrado")
})

test("protocol handler unregisterTheme remove acesso", () => {
  const themeDir = "/themes/test"
  const fs = createFakeFs({
    [path.join(themeDir, "assets", "bg.webp")]: Buffer.from("data"),
  })
  const handler = createProtocolHandler({ fsImpl: fs })
  handler.registerTheme("test", themeDir)
  assert.equal(handler.handleRequest("arcadia-theme://test/assets/bg.webp").ok, true)

  handler.unregisterTheme("test")
  assert.equal(handler.handleRequest("arcadia-theme://test/assets/bg.webp").ok, false)
})
