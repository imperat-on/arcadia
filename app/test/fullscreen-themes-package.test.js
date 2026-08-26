"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const { createPackageInstaller } = require("../electron/themes/package")
const { createRegistry } = require("../electron/themes/registry")
const { MANIFEST_VERSION, THEME_API_VERSION } = require("../electron/themes/constants")

// --- Fake filesystem ---

function createFakeFs(initial = {}) {
  const files = new Map()
  const dirs = new Set()

  for (const [key, value] of Object.entries(initial)) {
    if (value === null) {
      dirs.add(key)
    } else {
      files.set(key, value)
    }
  }

  return {
    readFileSync(p) {
      const key = String(p)
      if (!files.has(key)) throw new Error(`ENOENT: ${key}`)
      return files.get(key)
    },
    writeFileSync(p, data) {
      files.set(String(p), data)
    },
    copyFileSync(src, dest) {
      files.set(String(dest), files.get(String(src)))
    },
    renameSync(oldP, newP) {
      const oldKey = String(oldP)
      const newKey = String(newP)
      // Se é arquivo, move direto
      if (files.has(oldKey)) {
        files.set(newKey, files.get(oldKey))
        files.delete(oldKey)
        return
      }
      // Se é diretório, move todos os filhos
      if (dirs.has(oldKey)) {
        dirs.delete(oldKey)
        dirs.add(newKey)
        const prefix = oldKey + "/"
        const newPrefix = newKey + "/"
        // Move arquivos filhos
        for (const [k, v] of [...files.entries()]) {
          if (k.startsWith(prefix)) {
            files.delete(k)
            files.set(newPrefix + k.slice(prefix.length), v)
          }
        }
        // Move subdiretórios
        for (const d of [...dirs]) {
          if (d.startsWith(prefix)) {
            dirs.delete(d)
            dirs.add(newPrefix + d.slice(prefix.length))
          }
        }
      }
    },
    mkdirSync(p) {
      dirs.add(String(p))
    },
    lstatSync(p) {
      const key = String(p)
      if (files.has(key)) {
        return { isSymbolicLink: () => false, isFile: () => true, isDirectory: () => false }
      }
      if (dirs.has(key)) {
        return { isSymbolicLink: () => false, isFile: () => false, isDirectory: () => true }
      }
      throw new Error(`ENOENT: ${key}`)
    },
    statSync(p) {
      const key = String(p)
      if (!files.has(key)) throw new Error(`ENOENT: ${key}`)
      const data = files.get(key)
      return { size: typeof data === "string" ? Buffer.byteLength(data) : data.length || 0 }
    },
    readdirSync(p) {
      const key = String(p)
      if (!dirs.has(key)) throw new Error(`ENOTDIR: ${key}`)
      const prefix = key.endsWith("/") ? key : key + "/"
      const entries = []
      const seen = new Set()
      for (const f of files.keys()) {
        if (f.startsWith(prefix)) {
          const rest = f.slice(prefix.length)
          const name = rest.split("/")[0]
          if (!seen.has(name)) {
            seen.add(name)
            entries.push(name)
          }
        }
      }
      for (const d of dirs) {
        if (d.startsWith(prefix)) {
          const rest = d.slice(prefix.length)
          const name = rest.split("/")[0]
          if (!seen.has(name)) {
            seen.add(name)
            entries.push(name)
          }
        }
      }
      return entries.map((name) => ({
        name,
        isSymbolicLink: () => false,
        isFile: () => files.has(prefix + name),
        isDirectory: () => dirs.has(prefix + name) || dirs.has(prefix + name + "/"),
      }))
    },
    existsSync(p) {
      return files.has(String(p)) || dirs.has(String(p))
    },
    rmSync() {},
    _files: files,
    _dirs: dirs,
  }
}

// --- Fixtures ---

function validManifest() {
  return JSON.stringify({
    manifestVersion: MANIFEST_VERSION,
    themeApiVersion: THEME_API_VERSION,
    id: "test.import",
    name: "Test Import",
    author: "Test",
    version: "1.0.0",
    mode: "fullscreen",
    entry: "theme.css",
  })
}

function validCss() {
  return ":theme { --fs-color-bg: #000; }"
}

function createValidPackageFs() {
  const extractDir = "/tmp/extract"
  return {
    [path.join(extractDir, "theme.json")]: validManifest(),
    [path.join(extractDir, "theme.css")]: validCss(),
    [extractDir]: null,
  }
}

// --- Testes ---

test("package validateExtractedPackage aceita pacote válido", () => {
  const fs = createFakeFs(createValidPackageFs())
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.validateExtractedPackage("/tmp/extract")
  assert.equal(result.ok, true)
  assert.equal(result.manifest.id, "test.import")
})

test("package validateExtractedPackage rejeita manifesto ausente", () => {
  const fs = createFakeFs({ "/tmp/extract": null })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.validateExtractedPackage("/tmp/extract")
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("manifest_nao_encontrado"))
})

test("package validateExtractedPackage rejeita CSS inválido", () => {
  const fs = createFakeFs({
    "/tmp/extract/theme.json": validManifest(),
    "/tmp/extract/theme.css": "body { color: red; }", // seletor não escopado
    "/tmp/extract": null,
  })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.validateExtractedPackage("/tmp/extract")
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("seletor_nao_escopado")))
})

test("package validateExtractedFiles rejeita extensão proibida", () => {
  const fs = createFakeFs({
    "/tmp/extract/theme.json": validManifest(),
    "/tmp/extract/theme.css": validCss(),
    "/tmp/extract/script.js": "alert(1)",
    "/tmp/extract": null,
  })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.validateExtractedFiles("/tmp/extract")
  assert.ok(result.errors.some((e) => e.includes("extensao_proibida")))
})

test("package validateExtractedFiles aceita extensões permitidas", () => {
  const fs = createFakeFs({
    "/tmp/extract/theme.json": validManifest(),
    "/tmp/extract/theme.css": validCss(),
    "/tmp/extract/assets/bg.webp": Buffer.from("fake"),
    "/tmp/extract/assets/font.woff2": Buffer.from("fake"),
    "/tmp/extract": null,
  })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.validateExtractedFiles("/tmp/extract")
  assert.deepEqual(result.errors, [])
})

test("package installFromDirectory instala tema válido", () => {
  const fs = createFakeFs(createValidPackageFs())
  const themesDir = "/themes"
  const registry = createRegistry({ themesDir, fsImpl: fs })
  const installer = createPackageInstaller({ themesDir, registry, fsImpl: fs })

  const result = installer.installFromDirectory("/tmp/extract")
  assert.equal(result.ok, true)
  assert.equal(result.id, "test.import")
  assert.equal(result.version, "1.0.0")

  // Verifica que foi registrado
  const entry = registry.getEntry("test.import")
  assert.ok(entry)
  assert.equal(entry.version, "1.0.0")
  assert.ok(entry.digest)
})

test("package installFromDirectory rejeita pacote com erro", () => {
  const fs = createFakeFs({ "/tmp/extract": null })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.installFromDirectory("/tmp/extract")
  assert.equal(result.ok, false)
  assert.ok(result.errors.length > 0)
})
