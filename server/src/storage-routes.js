"use strict"

// Storage generico por bucket. Espelha os buckets do Supabase:
//   POST   /storage/v1/object/:bucket/:uid/:file   (upload, owner-scoped)
//   GET    /storage/v1/object/public/:bucket/:uid/:file (serve)
//   DELETE /storage/v1/object/:bucket              (remove via { paths })
// Path: <uid>/<ts><ext>. Valida magic bytes + limite de tamanho por bucket.
//
// Buckets:
//   avatars     - so imagem, 5MB, serve como image/*
//   backgrounds - imagem ou video (webm/mp4/m4v/mov), 25MB

const fs = require("node:fs")
const path = require("node:path")
const { verifyToken, extractToken } = require("./jwt")

// Magic bytes de imagem (mesmos do auth.js do app)
const MAGIC = [
  { sig: [0x89, 0x50, 0x4e, 0x47], mime: "image/png", ext: ".png" },
  { sig: [0xff, 0xd8, 0xff], mime: "image/jpeg", ext: ".jpg" },
  { sig: [0x47, 0x49, 0x46, 0x38], mime: "image/gif", ext: ".gif" },
  { sig: [0x52, 0x49, 0x46, 0x46], mime: "image/webp", ext: ".webp" },
]

// Box `ftyp` do ISO BMFF (mp4/m4v/mov): 3 bytes zero + "ftyp" nos bytes 4-7
function ehMp4(buf) {
  return (
    buf.length >= 8 &&
    buf[0] === 0x00 &&
    buf[1] === 0x00 &&
    buf[2] === 0x00 &&
    buf[4] === 0x66 &&
    buf[5] === 0x74 &&
    buf[6] === 0x79 &&
    buf[7] === 0x70
  )
}

// Imagem + video (webm via EBML, mp4/m4v/mov via ftyp)
const MAGIC_BG = [
  ...MAGIC,
  { sig: [0x1a, 0x45, 0xdf, 0xa3], mime: "video/webm", ext: ".webm" },
  { test: ehMp4, mime: "video/mp4", ext: ".mp4" },
]

function matchMagic(lista, buf) {
  for (const m of lista) {
    if (m.test ? m.test(buf) : buf.length >= m.sig.length && m.sig.every((b, i) => buf[i] === b)) {
      return m
    }
  }
  return null
}

function magicDeImagem(buf) {
  return matchMagic(MAGIC, buf)
}

const BUCKETS = {
  avatars: {
    max: 5 * 1024 * 1024, // 5MB
    nomeRe: /^[0-9]+\.(png|jpe?g|webp|gif)$/i,
    magic: MAGIC,
    erroTipo: "avatar_nao_imagem",
    erroTamanho: "avatar_grande",
    mimeFor: () => "image/*",
  },
  backgrounds: {
    max: 25 * 1024 * 1024, // 25MB
    nomeRe: /^[0-9]+\.(png|jpe?g|webp|gif|webm|mp4|m4v|mov)$/i,
    magic: MAGIC_BG,
    erroTipo: "background_nao_midia",
    erroTamanho: "background_grande",
    mimeFor: (file) => {
      if (/\.webm$/i.test(file)) return "video/webm"
      if (/\.(mp4|m4v|mov)$/i.test(file)) return "video/mp4"
      return "image/*"
    },
  },
}

function requireAuth(req) {
  const v = verifyToken(extractToken(req) || "")
  return v.ok ? v.sub : null
}

function registerStorageRoutes(app) {
  const dataDir = process.env.DATA_DIR || path.join(__dirname, "..", "data")
  const rootDe = (bucket) => path.join(dataDir, "..", bucket)
  for (const bucket of Object.keys(BUCKETS)) fs.mkdirSync(rootDe(bucket), { recursive: true })

  // Upload (owner-scoped: :uid precisa ser o do token)
  app.post("/storage/v1/object/:bucket/:uid/:file", (req, res) => {
    const cfg = BUCKETS[req.params.bucket]
    if (!cfg) return res.status(404).json({ error: "bucket_invalido" })

    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

    const chunks = []
    req.on("data", (c) => chunks.push(c))
    req.on("end", () => {
      const buf = Buffer.concat(chunks)
      if (!buf.length) return res.status(400).json({ error: "arquivo_vazio" })
      if (buf.length > cfg.max) return res.status(400).json({ error: cfg.erroTamanho })

      if (!matchMagic(cfg.magic, buf)) return res.status(400).json({ error: cfg.erroTipo })

      if (req.params.uid !== uid) {
        return res.status(403).json({ error: "permissao_negada" })
      }

      const file = req.params.file
      if (!cfg.nomeRe.test(file)) {
        return res.status(400).json({ error: "nome_invalido" })
      }

      const dir = path.join(rootDe(req.params.bucket), uid)
      fs.mkdirSync(dir, { recursive: true })
      fs.writeFileSync(path.join(dir, file), buf)
      res.status(200).json({ Key: `${uid}/${file}` })
    })
  })

  // Serve (publico)
  app.get("/storage/v1/object/public/:bucket/:uid/:file", (req, res) => {
    const cfg = BUCKETS[req.params.bucket]
    if (!cfg) return res.status(404).json({ error: "bucket_invalido" })

    const dest = path.join(rootDe(req.params.bucket), req.params.uid, req.params.file)
    if (!fs.existsSync(dest)) return res.status(404).json({ error: "nao_encontrado" })
    res.set("content-type", cfg.mimeFor(req.params.file))
    res.sendFile(dest)
  })

  // Remove (owner-scoped). O shim manda DELETE /storage/v1/object/:bucket
  // com { paths: ["uid/ts.ext"] }, entao parseia o caminho da lista.
  app.delete(["/storage/v1/object/:bucket", "/storage/v1/object/:bucket/:uid/:file"], (req, res) => {
    const cfg = BUCKETS[req.params.bucket]
    if (!cfg) return res.status(404).json({ error: "bucket_invalido" })

    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

    const root = rootDe(req.params.bucket)
    const paths = (req.body?.paths || [])
      .map((p) => String(p).split("/"))
      .filter((s) => s.length === 2)
    if (req.params.uid) paths.push([req.params.uid, req.params.file])

    const removidos = []
    for (const [owner, file] of paths) {
      if (owner !== uid) continue // owner-scoped
      const dest = path.join(root, owner, file)
      if (fs.existsSync(dest)) fs.rmSync(dest)
      removidos.push(`${owner}/${file}`)
    }
    res.json(removidos.map((n) => ({ name: n })))
  })
}

module.exports = { registerStorageRoutes, magicDeImagem }
