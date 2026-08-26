"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const {
  MANIFEST_VERSION,
  THEME_API_VERSION,
  validateManifest,
  parseManifest,
  readManifest,
  apiCompat,
} = require("../electron/themes/manifest")

const FIXTURES = path.join(__dirname, "fixtures", "themes")

function validOverrides(overrides) {
  return {
    manifestVersion: MANIFEST_VERSION,
    themeApiVersion: THEME_API_VERSION,
    id: "com.arcadia.demo",
    name: "Demo",
    version: "1.2.3",
    entry: "theme.css",
    mode: "fullscreen",
    ...overrides,
  }
}

test("manifest v1 canoniza campos e devolve descritor público sem paths", () => {
  const result = validateManifest(validOverrides())
  assert.equal(result.ok, true)
  assert.equal(result.manifest.id, "com.arcadia.demo")
  assert.equal(result.manifest.mode, "fullscreen")
  assert.equal(result.manifest.entry, "theme.css")
  assert.equal(result.manifest.themeApiVersion, THEME_API_VERSION)
  assert.deepEqual(result.manifest.previews, [])
  assert.deepEqual(result.manifest.layouts, {})
  const pub = apiCompat(result.manifest.themeApiVersion)
  assert.equal(pub, "ok")
})

test("manifest aceita aliases snake_case e canonicaliza", () => {
  const result = validateManifest({
    manifest_version: MANIFEST_VERSION,
    theme_api_version: "1",
    id: "alias.demo",
    name: "Alias",
    version: "1.0.0",
    entrypoint: "theme.css",
    target: "fullscreen",
  })
  assert.equal(result.ok, true)
  assert.equal(result.manifest.mode, "fullscreen")
  assert.equal(result.manifest.entry, "theme.css")
  assert.equal(result.manifest.themeApiVersion, 1)
})

test("manifest normaliza layouts/previews/features/options declarativos", () => {
  const result = validateManifest(
    validOverrides({
      id: "opts.demo",
      layouts: { home: "layouts/home.json", overview: "layouts/overview.json" },
      previews: ["previews/home.webp"],
      features: ["tokens", "assets", "layout:home"],
      options: {
        rail: { type: "enum", values: ["top", "bottom"], default: "top" },
        blur: { type: "intensity", min: 0, max: 1, default: 0.5 },
        showClock: { type: "boolean", default: true },
        glow: { type: "color", default: "#72ddff" },
        padding: { type: "number", min: 0, max: 64, default: 16 },
      },
    }),
  )
  assert.equal(result.ok, true)
  assert.deepEqual(result.manifest.layouts, { home: "layouts/home.json", overview: "layouts/overview.json" })
  assert.deepEqual(result.manifest.previews, ["previews/home.webp"])
  assert.deepEqual(result.manifest.options.rail.default, "top")
  assert.equal(result.manifest.options.blur.default, 0.5)
  assert.equal(result.manifest.options.showClock.default, true)
  assert.equal(result.manifest.options.glow.default, "#72ddff")
  assert.equal(result.manifest.options.padding.default, 16)
})

test("manifest rejeita id, versão, mode, entry e API inválidos", () => {
  for (const id of ["../theme", "Aurora", "", ".", "a".repeat(65)]) {
    assert.equal(validateManifest(validOverrides({ id })).ok, false, id)
  }
  for (const version of ["1.0", "v1.0.0", "1.0.0.1"]) {
    assert.equal(validateManifest(validOverrides({ version })).ok, false, version)
  }
  assert.equal(validateManifest(validOverrides({ mode: "desktop" })).ok, false)
  assert.equal(validateManifest(validOverrides({ entry: "../theme.css" })).ok, false)
  assert.equal(validateManifest(validOverrides({ entry: "theme.css.map" })).ok, false)
  assert.equal(validateManifest(validOverrides({ manifestVersion: 2 })).ok, false)
  // API maior é estruturalmente válida, porém incompatível (vai ser marcada e não ativada).
  const higher = validateManifest(validOverrides({ themeApiVersion: 2 }))
  assert.equal(higher.ok, true)
  assert.equal(higher.manifest.themeApiVersion, 2)
  assert.equal(apiCompat(higher.manifest.themeApiVersion), "higher")
})

test("manifest aceita campos desconhecidos apenas como diagnóstico", () => {
  const result = validateManifest(validOverrides({ customField: { anything: 1 } }))
  assert.equal(result.ok, true)
  assert.deepEqual(result.manifest.unknownFields, ["customField"])
})

test("manifest preserva layout com path de traversal como inválido", () => {
  const result = validateManifest(
    validOverrides({ id: "trav.demo", layouts: { home: "../../secret.json" } }),
  )
  assert.equal(result.ok, false)
})

test("apiCompat diferencia igual, menor e maior", () => {
  assert.equal(apiCompat(1), "ok")
  // API menor não é um número válido de manifesto → tratado como incompatível.
  assert.equal(apiCompat(2), "higher")
  assert.equal(apiCompat(0), "higher")
})

test("parseManifest rejeita JSON inválido", () => {
  assert.equal(parseManifest("not-json").ok, false)
  assert.deepEqual(parseManifest("not-json").errors, ["json_invalido"])
})

test("readManifest lê fixtures válidas e rejeita inválidas", () => {
  const valid = readManifest(path.join(FIXTURES, "valid-fullscreen", "theme.json"))
  assert.equal(valid.ok, true)
  assert.equal(valid.manifest.id, "fixture.valid")

  const invalid = readManifest(path.join(FIXTURES, "invalid-fullscreen", "theme.json"))
  assert.equal(invalid.ok, false)
  assert.ok(invalid.errors.includes("entry_invalido"))

  const incompatible = readManifest(path.join(FIXTURES, "incompatible-fullscreen", "theme.json"))
  assert.equal(incompatible.ok, true)
  assert.equal(incompatible.manifest.themeApiVersion, 2)
  assert.equal(apiCompat(incompatible.manifest.themeApiVersion), "higher")

  const missing = readManifest(path.join(FIXTURES, "valid-fullscreen", "missing.json"))
  assert.equal(missing.ok, false)
  assert.equal(missing.errors[0], "manifest_nao_encontrado")
})
