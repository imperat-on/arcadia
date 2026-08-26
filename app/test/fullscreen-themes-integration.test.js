"use strict"

// Testes de integração do sistema de temas Fullscreen.
// Cobrem os fluxos end-to-end: ativação, payload, CSS, recuperação.

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const { createThemeService } = require("../electron/themes/service")
const { createRegistry } = require("../electron/themes/registry")
const { createPackageInstaller } = require("../electron/themes/package")
const { normalizeThemeCss } = require("../electron/themes/css")
const { validateLayout } = require("../electron/themes/layout")
const {
  BUILTIN_DEFAULT_ID,
  BUILTIN_AURORA_ID,
  MANIFEST_VERSION,
  THEME_API_VERSION,
} = require("../electron/themes/constants")

// --- Fake filesystem ---

function createFakeFs(initial = {}) {
  const files = new Map()
  const dirs = new Set()
  for (const [key, value] of Object.entries(initial)) {
    if (value === null) dirs.add(key)
    else files.set(key, value)
  }
  return {
    readFileSync(p) { const k = String(p); if (!files.has(k)) throw new Error(`ENOENT: ${k}`); return files.get(k) },
    writeFileSync(p, data) { files.set(String(p), typeof data === "string" ? data : String(data)) },
    copyFileSync(src, dest) { files.set(String(dest), files.get(String(src))) },
    renameSync(oldP, newP) {
      const oldKey = String(oldP), newKey = String(newP)
      if (files.has(oldKey)) { files.set(newKey, files.get(oldKey)); files.delete(oldKey); return }
      if (dirs.has(oldKey)) {
        dirs.delete(oldKey); dirs.add(newKey)
        const prefix = oldKey + "/", newPrefix = newKey + "/"
        for (const [k, v] of [...files.entries()]) { if (k.startsWith(prefix)) { files.delete(k); files.set(newPrefix + k.slice(prefix.length), v) } }
        for (const d of [...dirs]) { if (d.startsWith(prefix)) { dirs.delete(d); dirs.add(newPrefix + d.slice(prefix.length)) } }
      }
    },
    mkdirSync(p) { dirs.add(String(p)) },
    lstatSync(p) { const k = String(p); if (files.has(k)) return { isSymbolicLink: () => false, isFile: () => true, isDirectory: () => false }; if (dirs.has(k)) return { isSymbolicLink: () => false, isFile: () => false, isDirectory: () => true }; throw new Error(`ENOENT: ${k}`) },
    statSync(p) { const k = String(p); if (!files.has(k)) throw new Error(`ENOENT: ${k}`); return { size: Buffer.byteLength(files.get(k)) } },
    readdirSync(p) {
      const k = String(p); if (!dirs.has(k)) throw new Error(`ENOTDIR: ${k}`)
      const prefix = k.endsWith("/") ? k : k + "/", entries = [], seen = new Set()
      for (const f of files.keys()) { if (f.startsWith(prefix)) { const rest = f.slice(prefix.length); const name = rest.split("/")[0]; if (!seen.has(name)) { seen.add(name); entries.push(name) } } }
      for (const d of dirs) { if (d.startsWith(prefix)) { const rest = d.slice(prefix.length); const name = rest.split("/")[0]; if (!seen.has(name)) { seen.add(name); entries.push(name) } } }
      return entries.map(name => ({ name, isSymbolicLink: () => false, isFile: () => files.has(prefix + name), isDirectory: () => dirs.has(prefix + name) || dirs.has(prefix + name + "/") }))
    },
    existsSync(p) { return files.has(String(p)) || dirs.has(String(p)) },
    rmSync() {},
    _files: files, _dirs: dirs,
  }
}

// --- Testes de integração ---

test("integração: fluxo completo de ativação de tema", () => {
  const fs = createFakeFs()
  const themesDir = "/themes"
  const service = createThemeService({ themesDir, fsImpl: fs })

  // Estado inicial: Default ativo
  assert.equal(service.getActiveId(), BUILTIN_DEFAULT_ID)

  // Ativar Aurora
  const activateResult = service.activate(BUILTIN_AURORA_ID)
  assert.equal(activateResult.ok, true)
  assert.equal(service.getPendingId(), BUILTIN_AURORA_ID)
  assert.equal(service.getActiveId(), BUILTIN_DEFAULT_ID) // ainda não confirmado

  // Confirmar
  const confirmResult = service.confirmActivation(BUILTIN_AURORA_ID)
  assert.equal(confirmResult.ok, true)
  assert.equal(service.getActiveId(), BUILTIN_AURORA_ID)
  assert.equal(service.getLastKnownGoodId(), BUILTIN_AURORA_ID)
  assert.equal(service.getPendingId(), null)
})

test("integração: payload contém CSS normalizado para tema externo", () => {
  const fs = createFakeFs({
    "/themes/fullscreen/test.theme/theme.json": JSON.stringify({
      manifestVersion: MANIFEST_VERSION,
      themeApiVersion: THEME_API_VERSION,
      id: "test.theme",
      name: "Test Theme",
      author: "Test",
      version: "1.0.0",
      mode: "fullscreen",
      entry: "theme.css",
    }),
    "/themes/fullscreen/test.theme/theme.css": ":theme { --fs-color-bg: #000; --fs-color-accent: #72ddff; }",
    "/themes/fullscreen/test.theme": null,
  })
  const themesDir = "/themes"
  const service = createThemeService({ themesDir, fsImpl: fs })

  // Registra o tema
  service.registry.register("test.theme", "1.0.0", "abc")

  // Obtém payload
  const payload = service.getPayload("test.theme")
  assert.ok(payload)
  assert.equal(payload.id, "test.theme")
  assert.ok(payload.css.includes('[data-fullscreen-theme="test.theme"]'))
  assert.ok(payload.css.includes("--fs-color-bg"))
  assert.ok(!payload.css.includes(":theme")) // deve ter sido escopado
})

test("integração: payload de built-in não tem CSS externo", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  const payload = service.getPayload(BUILTIN_DEFAULT_ID)
  assert.ok(payload)
  assert.equal(payload.id, BUILTIN_DEFAULT_ID)
  assert.equal(payload.css, "") // built-in: CSS está no bundle
  assert.equal(payload.source, "builtin")
})

test("integração: confirmação rejeita ID diferente do pending", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.activate(BUILTIN_AURORA_ID)
  assert.equal(service.getPendingId(), BUILTIN_AURORA_ID)

  // Tenta confirmar com ID errado
  const result = service.confirmActivation(BUILTIN_DEFAULT_ID)
  assert.equal(result.ok, false)
  assert.equal(result.error, "pendente_diferente")

  // Pending permanece
  assert.equal(service.getPendingId(), BUILTIN_AURORA_ID)
})

test("integração: rollback desfaz ativação pendente", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.activate(BUILTIN_AURORA_ID)
  assert.equal(service.getPendingId(), BUILTIN_AURORA_ID)

  service.rollbackPending()
  assert.equal(service.getPendingId(), null)
  assert.equal(service.getActiveId(), BUILTIN_DEFAULT_ID)
})

test("integração: tema ausente é detectado como missing", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.registry.register("author.missing", "1.0.0", "abc")

  const list = service.list()
  const missing = list.find((t) => t.id === "author.missing")
  assert.ok(missing)
  assert.equal(missing.state, "missing")
  assert.equal(missing.valid, false)
})

test("integração: CSS normalizado rejeita seletores não escopados", () => {
  const result = normalizeThemeCss("body { color: red; }", "test")
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("seletor_nao_escopado")))
})

test("integração: CSS normalizado aceita :theme e reescreve URLs", () => {
  const css = ':theme { background: url("theme://assets/bg.webp"); }'
  const result = normalizeThemeCss(css, "test.theme")
  assert.equal(result.ok, true)
  assert.ok(result.css.includes('[data-fullscreen-theme="test.theme"]'))
  assert.ok(result.css.includes("arcadia-theme://test.theme/assets/bg.webp"))
})

test("integração: layout válido é aceito", () => {
  const layout = {
    schemaVersion: 1,
    surface: "home",
    grid: {
      columns: ["1fr", "300px"],
      rows: ["auto", "1fr"],
      areas: [["topbar", "topbar"], ["library", "hero"]],
    },
    slots: {
      "home.topbar": { area: "topbar", required: true },
      "home.library": { area: "library", required: true },
      "home.hero": { area: "hero", required: true },
    },
  }
  const result = validateLayout(layout)
  assert.equal(result.ok, true)
  assert.equal(result.layout.surface, "home")
})

test("integração: layout inválido cai no padrão", () => {
  const result = validateLayout({ schemaVersion: 99, surface: "home", grid: null })
  assert.equal(result.ok, false)
})

test("integração: safe mode força Default", () => {
  const fs = createFakeFs()
  const registry = createRegistry({ themesDir: "/themes", fsImpl: fs })

  // Simula safe mode: reset + confirm Default
  registry.reset()
  registry.confirmActivation(BUILTIN_DEFAULT_ID)

  assert.equal(registry.getActiveId(), BUILTIN_DEFAULT_ID)
  assert.equal(registry.getPendingId(), null)
})

test("integração: built-in nunca pode ser removido", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  assert.equal(service.remove(BUILTIN_DEFAULT_ID).ok, false)
  assert.equal(service.remove(BUILTIN_DEFAULT_ID).error, "built_in_nao_removivel")
  assert.equal(service.remove(BUILTIN_AURORA_ID).ok, false)
  assert.equal(service.remove(BUILTIN_AURORA_ID).error, "built_in_nao_removivel")
})

test("integração: tema ativo não pode ser removido", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.registry.register("author.active", "1.0.0", "")
  service.activate("author.active")
  service.confirmActivation("author.active")

  assert.equal(service.remove("author.active").ok, false)
  assert.equal(service.remove("author.active").error, "tema_ativo")
})

test("integração: instalação de tema externo via diretório", () => {
  const fs = createFakeFs({
    "/tmp/extract/theme.json": JSON.stringify({
      manifestVersion: MANIFEST_VERSION,
      themeApiVersion: THEME_API_VERSION,
      id: "test.install",
      name: "Test Install",
      author: "Test",
      version: "1.0.0",
      mode: "fullscreen",
      entry: "theme.css",
    }),
    "/tmp/extract/theme.css": ":theme { --fs-color-bg: #000; }",
    "/tmp/extract": null,
  })
  const themesDir = "/themes"
  const registry = createRegistry({ themesDir, fsImpl: fs })
  const installer = createPackageInstaller({ themesDir, registry, fsImpl: fs })

  const result = installer.installFromDirectory("/tmp/extract")
  assert.equal(result.ok, true)
  assert.equal(result.id, "test.install")
  assert.equal(result.version, "1.0.0")

  // Verifica registro
  const entry = registry.getEntry("test.install")
  assert.ok(entry)
  assert.equal(entry.version, "1.0.0")
  assert.ok(entry.digest)
})

test("integração: pacote com extensão proibida é rejeitado", () => {
  const fs = createFakeFs({
    "/tmp/extract/theme.json": JSON.stringify({
      manifestVersion: MANIFEST_VERSION,
      themeApiVersion: THEME_API_VERSION,
      id: "test.bad",
      name: "Test Bad",
      author: "Test",
      version: "1.0.0",
      mode: "fullscreen",
      entry: "theme.css",
    }),
    "/tmp/extract/theme.css": ":theme { color: red; }",
    "/tmp/extract/script.js": "alert(1)",
    "/tmp/extract": null,
  })
  const registry = createRegistry({ fsImpl: fs })
  const installer = createPackageInstaller({ themesDir: "/themes", registry, fsImpl: fs })

  const result = installer.installFromDirectory("/tmp/extract")
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("extensao_proibida")))
})

test("integração: lista mostra built-ins e temas externos", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.registry.register("author.test", "1.0.0", "abc")

  const list = service.list()
  assert.ok(list.length >= 3) // Default + Aurora + author.test
  assert.ok(list.some((t) => t.id === BUILTIN_DEFAULT_ID))
  assert.ok(list.some((t) => t.id === BUILTIN_AURORA_ID))
  assert.ok(list.some((t) => t.id === "author.test"))
})
