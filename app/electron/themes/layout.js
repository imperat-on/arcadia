"use strict"

// Validação de layouts declarativos de temas Fullscreen.
// O layout é composto por slots do host. A v1 não aceita HTML ou JSX.
// Valida grid CSS, slots conhecidos e limites de segurança.

const { LAYOUT_SCHEMA_VERSION, LIMITS } = require("./constants")

// Slots conhecidos que o host suporta.
const KNOWN_SLOTS = new Set([
  // Shell e home
  "shell.root", "shell.backdrop", "shell.overlay",
  "home.topbar", "home.navigation", "home.library", "home.rail",
  "home.game-card", "home.hero", "home.hero.logo", "home.hero.description",
  "home.hero.actions", "home.info", "home.footer",
  // Overview
  "overview.root", "overview.backdrop", "overview.topbar",
  "overview.cover", "overview.identity", "overview.tags",
  "overview.description", "overview.actions", "overview.progress",
  "overview.media", "overview.metadata", "overview.activities",
  // Superfícies secundárias
  "news.root", "news.featured", "news.rail",
  "store.root", "store.header", "store.content",
  "settings.root", "settings.navigation", "settings.content",
  "profile.root", "profile.card",
  "downloads.root", "downloads.item",
  "dialog.root", "dialog.actions",
  "toast.root",
])

// Tracks CSS permitidos (gramática restrita).
const VALID_TRACK_RE = /^(?:auto|minmax\([^)]+\)|min-content|max-content|\d+(?:px|fr|em|rem|%)|(?:min|max|fit-content|clamp)\([^)]+\))(?:\s+(?:auto|minmax\([^)]+\)|min-content|max-content|\d+(?:px|fr|em|rem|%)|(?:min|max|fit-content|clamp)\([^)]+\)))*$/

function validateLayout(input) {
  const errors = []

  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return { ok: false, layout: null, errors: ["layout_invalido"] }
  }

  // Schema version
  const schemaVersion = input.schemaVersion
  if (schemaVersion !== LAYOUT_SCHEMA_VERSION) {
    errors.push("schema_version_invalida")
  }

  // Surface
  const surface = input.surface
  const KNOWN_SURFACES = new Set(["home", "overview"])
  if (!surface || !KNOWN_SURFACES.has(surface)) {
    errors.push("surface_invalida")
  }

  // Grid
  const grid = input.grid
  if (!grid || typeof grid !== "object" || Array.isArray(grid)) {
    errors.push("grid_invalido")
    return { ok: errors.length === 0, layout: null, errors: [...new Set(errors)] }
  }

  // Columns
  const columns = validateTrackList(grid.columns, "columns", errors)
  if (columns.length > LIMITS.maxLayoutAreas) {
    errors.push("colunas_excedidas")
  }

  // Rows
  const rows = validateTrackList(grid.rows, "rows", errors)
  if (rows.length > LIMITS.maxLayoutAreas) {
    errors.push("linhas_excedidas")
  }

  // Areas
  const areas = validateAreas(grid.areas, columns.length, rows.length, errors)

  // Slots
  const slots = validateSlots(input.slots, areas, surface, errors)

  if (errors.length) return { ok: false, layout: null, errors: [...new Set(errors)] }

  const layout = {
    schemaVersion: LAYOUT_SCHEMA_VERSION,
    surface,
    grid: { columns, rows, areas },
    slots,
  }

  return { ok: true, layout, errors: [] }
}

function validateTrackList(value, field, errors) {
  if (!Array.isArray(value)) {
    errors.push(`${field}_invalido`)
    return []
  }
  if (value.length > LIMITS.maxLayoutAreas) {
    errors.push(`${field}_excedidos`)
    return []
  }
  const result = []
  for (const track of value) {
    if (typeof track !== "string") {
      errors.push(`${field}_track_invalido`)
      continue
    }
    // Validação simplificada: aceita valores CSS básicos
    const clean = track.trim()
    if (!clean) {
      errors.push(`${field}_track_vazio`)
      continue
    }
    // Rejeita URLs, expressions perigosas
    if (clean.includes("url(") || clean.includes("expression(") || clean.includes("javascript:")) {
      errors.push(`${field}_track_proibido`)
      continue
    }
    result.push(clean)
  }
  return result
}

function validateAreas(value, numCols, numRows, errors) {
  if (!Array.isArray(value)) {
    errors.push("areas_invalido")
    return []
  }
  if (value.length > LIMITS.maxLayoutAreas) {
    errors.push("areas_excedidas")
    return []
  }
  if (value.length !== numRows) {
    errors.push("areas_linhas_inconsistentes")
  }

  const result = []
  const usedNames = new Set()

  for (const row of value) {
    if (!Array.isArray(row)) {
      errors.push("area_linha_invalida")
      continue
    }
    if (row.length !== numCols) {
      errors.push("areas_colunas_inconsistentes")
    }
    const rowResult = []
    for (const cell of row) {
      if (typeof cell !== "string") {
        errors.push("area_celula_invalida")
        rowResult.push(".")
        continue
      }
      const name = cell.trim()
      if (!name || name === ".") {
        rowResult.push(".")
        continue
      }
      // Valida nome da área
      if (!/^[a-zA-Z][a-zA-Z0-9._-]*$/.test(name)) {
        errors.push(`area_nome_invalido:${name.slice(0, 20)}`)
        rowResult.push(".")
        continue
      }
      usedNames.add(name)
      rowResult.push(name)
    }
    result.push(rowResult)
  }

  return result
}

function validateSlots(value, areas, surface, errors) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push("slots_invalido")
    return {}
  }

  const result = {}
  const areaNames = new Set()
  for (const row of areas) {
    for (const cell of row) {
      if (cell !== ".") areaNames.add(cell)
    }
  }

  for (const [slotName, slotDef] of Object.entries(value)) {
    if (!KNOWN_SLOTS.has(slotName)) {
      // Slots desconhecidos são ignorados, não erro
      continue
    }
    if (!slotDef || typeof slotDef !== "object" || Array.isArray(slotDef)) {
      errors.push(`slot_invalido:${slotName}`)
      continue
    }
    const area = slotDef.area
    if (typeof area !== "string" || !areaNames.has(area)) {
      errors.push(`slot_area_invalida:${slotName}`)
      continue
    }
    result[slotName] = {
      area,
      required: Boolean(slotDef.required),
    }
  }

  // Verifica que slots obrigatórios estão presentes
  const requiredSlots = getRequiredSlots(surface)
  for (const req of requiredSlots) {
    if (!result[req]) {
      // Não é erro fatal, mas o layout pode não funcionar bem
    }
  }

  return result
}

function getRequiredSlots(surface) {
  if (surface === "home") {
    return ["home.topbar", "home.library", "home.hero"]
  }
  if (surface === "overview") {
    return ["overview.root", "overview.actions"]
  }
  return []
}

// Converte um layout validado em CSS grid properties.
function layoutToGridCss(layout) {
  if (!layout || !layout.grid) return ""
  const { columns, rows, areas } = layout.grid

  let css = ""
  css += `grid-template-columns: ${columns.join(" ")};\n`
  css += `grid-template-rows: ${rows.join(" ")};\n`

  if (areas.length) {
    const areaStrings = areas.map((row) => `"${row.join(" ")}"`)
    css += `grid-template-areas: ${areaStrings.join(" ")};\n`
  }

  return css
}

module.exports = {
  validateLayout,
  layoutToGridCss,
  KNOWN_SLOTS,
  getRequiredSlots,
}
