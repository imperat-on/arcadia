"use strict"

// Importador e instalador de pacotes .arcadiatheme.
// Valida ZIP, extrai com segurança, verifica manifesto/CSS/layout/assets,
// calcula digest e instala por rename atômico com rollback.

const fsDefault = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")
const {
  MANIFEST_FILE,
  PACKAGE_EXTENSION,
  ALLOWED_EXTENSIONS,
  FORBIDDEN_EXTENSIONS,
  LIMITS,
} = require("./constants")
const { parseManifest, validateManifest } = require("./manifest")
const { validateCssContent } = require("./css")

function createPackageInstaller({ themesDir, registry, fsImpl = fsDefault } = {}) {
  const fullscreenDir = themesDir ? path.join(themesDir, "fullscreen") : null
  const stagingDir = themesDir ? path.join(themesDir, "staging") : null

  function ensureDirs() {
    if (fullscreenDir) fsImpl.mkdirSync(fullscreenDir, { recursive: true, mode: 0o700 })
    if (stagingDir) fsImpl.mkdirSync(stagingDir, { recursive: true, mode: 0o700 })
  }

  function isSymlink(file) {
    try {
      return fsImpl.lstatSync(file).isSymbolicLink()
    } catch {
      return false
    }
  }

  // Valida um pacote ZIP já extraído em stagingDir.
  function validateExtractedPackage(extractDir) {
    const errors = []

    const manifestPath = path.join(extractDir, MANIFEST_FILE)
    let manifestText
    try {
      if (isSymlink(manifestPath)) return { ok: false, errors: ["manifest_symlink"] }
      manifestText = fsImpl.readFileSync(manifestPath, "utf8")
    } catch {
      return { ok: false, errors: ["manifest_nao_encontrado"] }
    }

    if (Buffer.byteLength(manifestText, "utf8") > LIMITS.manifestBytes) {
      return { ok: false, errors: ["manifest_muito_grande"] }
    }

    const manifestResult = parseManifest(manifestText)
    if (!manifestResult.ok) {
      return { ok: false, errors: manifestResult.errors }
    }

    const manifest = manifestResult.manifest

    const entryPath = path.join(extractDir, manifest.entry)
    try {
      const stat = fsImpl.lstatSync(entryPath)
      if (!stat.isFile() || stat.isSymbolicLink()) {
        return { ok: false, errors: ["entry_nao_e_arquivo"] }
      }
    } catch {
      return { ok: false, errors: ["entry_nao_encontrado"] }
    }

    try {
      const css = fsImpl.readFileSync(entryPath, "utf8")
      const cssResult = validateCssContent(css)
      if (!cssResult.ok) {
        return { ok: false, errors: cssResult.errors }
      }
    } catch {
      return { ok: false, errors: ["entry_leitura_falhou"] }
    }

    for (const [surface, layoutPath] of Object.entries(manifest.layouts)) {
      const fullLayoutPath = path.join(extractDir, layoutPath)
      try {
        const stat = fsImpl.lstatSync(fullLayoutPath)
        if (!stat.isFile() || stat.isSymbolicLink()) {
          errors.push(`layout_${surface}_invalido`)
        }
      } catch {
        errors.push(`layout_${surface}_nao_encontrado`)
      }
    }

    for (const previewPath of manifest.previews) {
      const fullPreviewPath = path.join(extractDir, previewPath)
      try {
        const stat = fsImpl.lstatSync(fullPreviewPath)
        if (!stat.isFile() || stat.isSymbolicLink()) {
          errors.push("preview_invalido")
        }
      } catch {
        errors.push("preview_nao_encontrado")
      }
    }

    if (errors.length) return { ok: false, errors }

    return { ok: true, manifest, errors: [] }
  }

  // Valida arquivos extraídos contra limites e extensões permitidas.
  function validateExtractedFiles(extractDir) {
    const errors = []
    let fileCount = 0
    let totalBytes = 0

    function walk(dir, depth) {
      if (depth > LIMITS.maxDepth) {
        errors.push("profundidade_excedida")
        return
      }

      let entries
      try {
        entries = fsImpl.readdirSync(dir, { withFileTypes: true })
      } catch {
        errors.push("leitura_diretorio_falhou")
        return
      }

      for (const entry of entries) {
        const fullPath = path.join(dir, entry.name)

        if (entry.isSymbolicLink()) {
          errors.push("symlink_rejeitado")
          continue
        }

        if (entry.isDirectory()) {
          walk(fullPath, depth + 1)
          continue
        }

        if (!entry.isFile()) continue

        fileCount++
        if (fileCount > LIMITS.maxFiles) {
          errors.push("arquivos_excedidos")
          return
        }

        const ext = path.extname(entry.name).toLowerCase()
        if (FORBIDDEN_EXTENSIONS.has(ext)) {
          errors.push(`extensao_proibida:${ext}`)
          continue
        }
        if (!ALLOWED_EXTENSIONS.has(ext)) {
          errors.push(`extensao_desconhecida:${ext}`)
          continue
        }

        try {
          const stat = fsImpl.statSync(fullPath)
          totalBytes += stat.size

          if (totalBytes > LIMITS.packageExtractedBytes) {
            errors.push("tamanho_extraido_excedido")
            return
          }

          if ([".png", ".jpg", ".jpeg", ".webp", ".gif"].includes(ext) && stat.size > LIMITS.imageBytes) {
            errors.push(`imagem_muito_grande:${entry.name}`)
          }
          if ([".mp4", ".webm"].includes(ext) && stat.size > LIMITS.videoBytes) {
            errors.push(`video_muito_grande:${entry.name}`)
          }
          if (ext === ".woff2" && stat.size > LIMITS.fontBytes) {
            errors.push(`fonte_muito_grande:${entry.name}`)
          }
          if (ext === ".css" && stat.size > LIMITS.cssTotalBytes) {
            errors.push(`css_muito_grande:${entry.name}`)
          }
          if (ext === ".json" && entry.name !== MANIFEST_FILE && stat.size > LIMITS.layoutBytes) {
            errors.push(`json_muito_grande:${entry.name}`)
          }
        } catch {
          errors.push(`stat_falhou:${entry.name}`)
        }
      }
    }

    walk(extractDir, 0)
    return { errors, fileCount, totalBytes }
  }

  // Extrai um arquivo ZIP para um diretório temporário com validações de segurança.
  function extractZip(zipPath, extractDir) {
    const AdmZip = require("adm-zip")
    const zip = new AdmZip(zipPath)

    const entries = zip.getEntries()

    // Validações de segurança antes de extrair
    const seenPaths = new Set()
    let compressedSize = 0

    for (const entry of entries) {
      const entryName = entry.entryName

      // Rejeita paths absolutos
      if (path.isAbsolute(entryName)) {
        return { ok: false, errors: ["path_absoluto"] }
      }

      // Rejeita traversal
      const normalized = path.posix.normalize(entryName)
      if (normalized.startsWith("../") || normalized.includes("/../")) {
        return { ok: false, errors: ["traversal_detectado"] }
      }

      // Rejeita backslash ambíguo
      if (entryName.includes("\\")) {
        return { ok: false, errors: ["backslash_ambiguo"] }
      }

      // Rejeita NUL
      if (entryName.includes("\0")) {
        return { ok: false, errors: ["nul_detectado"] }
      }

      // Rejeita entradas duplicadas (case-insensitive)
      const lowerPath = normalized.toLowerCase()
      if (seenPaths.has(lowerPath)) {
        return { ok: false, errors: ["entrada_duplicada"] }
      }
      seenPaths.add(lowerPath)

      // Rejeita extensões proibidas
      const ext = path.extname(entryName).toLowerCase()
      if (ext && FORBIDDEN_EXTENSIONS.has(ext)) {
        return { ok: false, errors: [`extensao_proibida:${ext}`] }
      }

      // Acumula tamanho comprimido
      compressedSize += entry.header.compressedSize || 0
      if (compressedSize > LIMITS.packageCompressedBytes) {
        return { ok: false, errors: ["tamanho_comprimido_excedido"] }
      }
    }

    // Extrai para o diretório
    try {
      zip.extractAllTo(extractDir, true)
    } catch (err) {
      return { ok: false, errors: [`extracao_falhou:${err.message}`] }
    }

    // Valida que não há symlinks no resultado
    try {
      const walkResult = checkForSymlinks(extractDir)
      if (!walkResult.ok) return walkResult
    } catch {}

    return { ok: true, errors: [] }
  }

  function checkForSymlinks(dir) {
    const entries = fsImpl.readdirSync(dir, { withFileTypes: true })
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name)
      if (entry.isSymbolicLink()) {
        return { ok: false, errors: ["symlink_apos_extracao"] }
      }
      if (entry.isDirectory()) {
        const result = checkForSymlinks(fullPath)
        if (!result.ok) return result
      }
    }
    return { ok: true, errors: [] }
  }

  // Instala um tema a partir de um diretório já extraído e validado.
  function installFromExtracted(extractDir, manifest) {
    ensureDirs()

    const themeId = manifest.id
    const targetDir = path.join(fullscreenDir, themeId)

    const stagingThemeDir = path.join(stagingDir, `${themeId}.${Date.now()}`)

    try {
      copyDir(extractDir, stagingThemeDir)

      if (fsImpl.existsSync(targetDir)) {
        const backupDir = `${targetDir}.bak.${Date.now()}`
        fsImpl.renameSync(targetDir, backupDir)
        try {
          fsImpl.renameSync(stagingThemeDir, targetDir)
          fsImpl.rmSync(backupDir, { recursive: true, force: true })
        } catch (err) {
          try { fsImpl.renameSync(backupDir, targetDir) } catch {}
          throw err
        }
      } else {
        fsImpl.renameSync(stagingThemeDir, targetDir)
      }

      const manifestPath = path.join(targetDir, MANIFEST_FILE)
      const digest = crypto.createHash("sha256")
        .update(fsImpl.readFileSync(manifestPath))
        .digest("hex")

      registry.register(themeId, manifest.version, digest)

      return { ok: true, id: themeId, version: manifest.version }
    } catch (err) {
      try { fsImpl.rmSync(stagingThemeDir, { recursive: true, force: true }) } catch {}
      return { ok: false, error: String(err.message || err) }
    }
  }

  function copyDir(src, dest) {
    fsImpl.mkdirSync(dest, { recursive: true })
    const entries = fsImpl.readdirSync(src, { withFileTypes: true })
    for (const entry of entries) {
      const srcPath = path.join(src, entry.name)
      const destPath = path.join(dest, entry.name)
      if (entry.isSymbolicLink()) continue
      if (entry.isDirectory()) {
        copyDir(srcPath, destPath)
      } else if (entry.isFile()) {
        fsImpl.copyFileSync(srcPath, destPath)
      }
    }
  }

  // Instala a partir de um arquivo .arcadiatheme (ZIP real).
  function installFromZip(zipPath) {
    ensureDirs()

    // Verifica tamanho do arquivo comprimido
    try {
      const stat = fsImpl.statSync(zipPath)
      if (stat.size > LIMITS.packageCompressedBytes) {
        return { ok: false, errors: ["tamanho_comprimido_excedido"] }
      }
    } catch {
      return { ok: false, errors: ["arquivo_nao_encontrado"] }
    }

    // Extrai para diretório temporário
    const tempDir = path.join(stagingDir, `_extract_${Date.now()}`)
    fsImpl.mkdirSync(tempDir, { recursive: true })

    try {
      const extractResult = extractZip(zipPath, tempDir)
      if (!extractResult.ok) {
        fsImpl.rmSync(tempDir, { recursive: true, force: true })
        return { ok: false, errors: extractResult.errors }
      }

      // Valida arquivos extraídos
      const fileValidation = validateExtractedFiles(tempDir)
      if (fileValidation.errors.length) {
        fsImpl.rmSync(tempDir, { recursive: true, force: true })
        return { ok: false, errors: fileValidation.errors }
      }

      // Valida manifesto e conteúdo
      const packageValidation = validateExtractedPackage(tempDir)
      if (!packageValidation.ok) {
        fsImpl.rmSync(tempDir, { recursive: true, force: true })
        return { ok: false, errors: packageValidation.errors }
      }

      // Instala
      const result = installFromExtracted(tempDir, packageValidation.manifest)

      // Limpa diretório temporário
      try { fsImpl.rmSync(tempDir, { recursive: true, force: true }) } catch {}

      return result
    } catch (err) {
      try { fsImpl.rmSync(tempDir, { recursive: true, force: true }) } catch {}
      return { ok: false, errors: [`extracao_erro:${err.message}`] }
    }
  }

  // Instala a partir de um diretório já extraído (para testes).
  function installFromDirectory(dirPath) {
    const fileValidation = validateExtractedFiles(dirPath)
    if (fileValidation.errors.length) {
      return { ok: false, errors: fileValidation.errors }
    }

    const packageValidation = validateExtractedPackage(dirPath)
    if (!packageValidation.ok) {
      return { ok: false, errors: packageValidation.errors }
    }

    return installFromExtracted(dirPath, packageValidation.manifest)
  }

  return {
    validateExtractedPackage,
    validateExtractedFiles,
    extractZip,
    installFromExtracted,
    installFromZip,
    installFromDirectory,
    copyDir,
  }
}

module.exports = { createPackageInstaller }
