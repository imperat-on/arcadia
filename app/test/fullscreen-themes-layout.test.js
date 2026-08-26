"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { validateLayout, layoutToGridCss, KNOWN_SLOTS, getRequiredSlots } = require("../electron/themes/layout")
const { LAYOUT_SCHEMA_VERSION } = require("../electron/themes/constants")

// --- Helpers ---

function validLayout(overrides = {}) {
  return {
    schemaVersion: LAYOUT_SCHEMA_VERSION,
    surface: "home",
    grid: {
      columns: ["minmax(0, 1fr)", "360px"],
      rows: ["auto", "minmax(0, 1fr)", "auto"],
      areas: [
        ["topbar", "topbar"],
        ["library", "hero"],
        ["footer", "footer"],
      ],
    },
    slots: {
      "home.topbar": { area: "topbar", required: true },
      "home.library": { area: "library", required: true },
      "home.hero": { area: "hero", required: true },
      "home.footer": { area: "footer", required: false },
    },
    ...overrides,
  }
}

// --- Testes ---

test("layout aceita layout válido", () => {
  const result = validateLayout(validLayout())
  assert.equal(result.ok, true)
  assert.equal(result.layout.surface, "home")
  assert.equal(result.layout.grid.columns.length, 2)
  assert.equal(result.layout.grid.rows.length, 3)
})

test("layout rejeita schema version inválida", () => {
  const result = validateLayout(validLayout({ schemaVersion: 99 }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("schema_version_invalida"))
})

test("layout rejeita surface inválida", () => {
  const result = validateLayout(validLayout({ surface: "unknown" }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("surface_invalida"))
})

test("layout rejeita grid ausente", () => {
  const result = validateLayout(validLayout({ grid: null }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("grid_invalido"))
})

test("layout rejeita columns inválidas", () => {
  const result = validateLayout(validLayout({
    grid: { columns: "invalid", rows: ["auto"], areas: [["x"]] },
  }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("columns_invalido"))
})

test("layout rejeita track com URL", () => {
  const result = validateLayout(validLayout({
    grid: {
      columns: ["url(evil)", "1fr"],
      rows: ["auto"],
      areas: [["a", "b"]],
    },
  }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("track_proibido")))
})

test("layout rejeita areas com linhas inconsistentes", () => {
  const result = validateLayout(validLayout({
    grid: {
      columns: ["1fr", "1fr"],
      rows: ["auto", "auto"],
      areas: [["a", "b"]], // 1 linha, mas 2 rows
    },
  }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.includes("areas_linhas_inconsistentes"))
})

test("layout rejeita slot com area inexistente", () => {
  const result = validateLayout(validLayout({
    slots: {
      "home.topbar": { area: "nonexistent", required: true },
    },
  }))
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("slot_area_invalida")))
})

test("layout ignora slots desconhecidos", () => {
  const result = validateLayout(validLayout({
    slots: {
      "home.topbar": { area: "topbar", required: true },
      "unknown.slot": { area: "topbar", required: false },
    },
  }))
  assert.equal(result.ok, true)
  assert.ok(!("unknown.slot" in result.layout.slots))
})

test("layoutToGridCss gera CSS correto", () => {
  const layout = validLayout()
  const result = validateLayout(layout)
  const css = layoutToGridCss(result.layout)
  assert.ok(css.includes("grid-template-columns: minmax(0, 1fr) 360px"))
  assert.ok(css.includes("grid-template-rows: auto minmax(0, 1fr) auto"))
  assert.ok(css.includes('"topbar topbar"'))
  assert.ok(css.includes('"library hero"'))
})

test("KNOWN_SLOTS contém slots obrigatórios", () => {
  assert.ok(KNOWN_SLOTS.has("home.topbar"))
  assert.ok(KNOWN_SLOTS.has("home.library"))
  assert.ok(KNOWN_SLOTS.has("home.hero"))
  assert.ok(KNOWN_SLOTS.has("overview.root"))
  assert.ok(KNOWN_SLOTS.has("overview.actions"))
})

test("getRequiredSlots retorna slots corretos por surface", () => {
  const homeRequired = getRequiredSlots("home")
  assert.ok(homeRequired.includes("home.topbar"))
  assert.ok(homeRequired.includes("home.library"))
  assert.ok(homeRequired.includes("home.hero"))

  const overviewRequired = getRequiredSlots("overview")
  assert.ok(overviewRequired.includes("overview.root"))
  assert.ok(overviewRequired.includes("overview.actions"))
})
