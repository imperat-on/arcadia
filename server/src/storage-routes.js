"use strict"

// Storage de avatares. Espelha o bucket `avatars` do Supabase:
//   POST   /storage/v1/object/avatars/:uid/:file   (upload, owner-scoped)
//   GET    /storage/v1/object/public/avatars/:uid/:file (serve)
//   DELETE /storage/v1/object/avatars              (remove via { paths })
// Path: <uid>/<ts><ext>. Valida magic bytes + limite de 5MB (como no app).

const fs = require("node:fs")
const path = require("node:path")
const { verifyToken, extractToken } = require("./jwt")

const AVATAR_MAX = 5 * 1024 * 1024 // 5MB

// Magic bytes por formato (mesmos do auth.js do app)
const MAGIC = [
  { sig: [0x89, 0x50, 0x4e, 0x47], mime: "image/png", ext: ".png" },
  { sig: [0xff, 0xd8, 0xff], mime: "image/jpeg", ext: ".jpg" },
  { sig: [0x47, 0x49, 0x46, 0x38], mime: "image/gif", ext: ".gif" },
  { sig: [0x52, 0x49, 0x46, 0x46], mime: "image/webp", ext: ".webp" },
]

function magicDeImagem(buf) {
  for (const m of MAGIC) {
    if (buf.length >= m.sig.length && m.sig.every((b, i) => buf[i] === b)) return m
  }
  return null
}

function requireAuth(req) {
  const v = verifyToken(extractToken(req) || "")
  return v.ok ? v.sub : null
}

function registerStorageRoutes(app) {
  const dataDir = process.env.DATA_DIR || path.join(__dirname, "..", "data")
  const root = path.join(dataDir, "..", "avatars")
  fs.mkdirSync(root, { recursive: true })

  // Upload (owner-scoped: :uid precisa ser o do token)
  app.post("/storage/v1/object/avatars/:uid/:file", (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

    const chunks = []
    req.on("data", (c) => chunks.push(c))
    req.on("end", () => {
      const buf = Buffer.concat(chunks)
      if (!buf.length) return res.status(400).json({ error: "arquivo_vazio" })
      if (buf.length > AVATAR_MAX) return res.status(400).json({ error: "avatar_grande" })

      const magic = magicDeImagem(buf)
      if (!magic) return res.status(400).json({ error: "avatar_nao_imagem" })

      if (req.params.uid !== uid) {
        return res.status(403).json({ error: "permissao_negada" })
      }

      const file = req.params.file
      if (!/^[0-9]+\.(png|jpe?g|webp|gif)$/i.test(file)) {
        return res.status(400).json({ error: "nome_invalido" })
      }

      const dir = path.join(root, uid)
      fs.mkdirSync(dir, { recursive: true })
      fs.writeFileSync(path.join(dir, file), buf)
      res.status(200).json({ Key: `${uid}/${file}` })
    })
  })

  // Serve (publico)
  app.get("/storage/v1/object/public/avatars/:uid/:file", (req, res) => {
    const dest = path.join(root, req.params.uid, req.params.file)
    if (!fs.existsSync(dest)) return res.status(404).json({ error: "nao_encontrado" })
    res.set("content-type", "image/*")
    res.sendFile(dest)
  })

  // Remove (owner-scoped). O shim manda DELETE /storage/v1/object/avatars
  // com { paths: ["uid/ts.ext"] }, entao parseia o caminho da lista.
  app.delete(["/storage/v1/object/avatars", "/storage/v1/object/avatars/:uid/:file"], (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })

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
