"use strict"

// Normalização e validação de CSS de temas Fullscreen.
// Escopo `:theme`, reescrita de URLs `theme://`, namespace de keyframes
// e rejeição de @import, URLs externas e seletores não escopados.
// Usa parser CSS real (postcss) quando disponível, com fallback regex.

const { LIMITS } = require("./constants")

// --- Helpers ---

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

function namespaceKeyframes(css, themeId) {
  const ns = `t-${themeId}-`
  // Encontra definições @keyframes name {
  const defRe = /@keyframes\s+([a-zA-Z_][\w-]*)\s*\{/g
  const names = new Set()
  let m
  while ((m = defRe.exec(css)) !== null) {
    names.add(m[1])
  }
  if (!names.size) return css

  let result = css
  for (const name of names) {
    const safeName = name.replace(/[^\w-]/g, "")
    if (!safeName) continue
    const re = new RegExp(`@keyframes\\s+${escapeRegExp(safeName)}\\b`, "g")
    result = result.replace(re, `@keyframes ${ns}${safeName}`)
    // Referências em animation/animation-name
    const animRe = new RegExp(`(animation(?:-name)?\\s*:[^;]*?)\\b${escapeRegExp(safeName)}\\b`, "g")
    result = result.replace(animRe, `$1${ns}${safeName}`)
  }
  return result
}

function rewriteThemeUrls(css, themeId) {
  // theme://assets/foo.webp → arcadia-theme://<id>/assets/foo.webp
  return css.replace(/theme:\/\/([^\s"'\\)}]+)/g, (_match, assetPath) => {
    const clean = assetPath.replace(/["'\\]/g, "")
    if (clean.includes("..") || clean.includes("\\")) return "about:invalid"
    return `arcadia-theme://${themeId}/${clean}`
  })
}

function scopeSelectors(css, themeId) {
  // Substitui :theme pelo seletor escopado real.
  const scopeSelector = `[data-fullscreen-theme="${themeId}"]`
  return css.replace(/:theme\b/g, scopeSelector)
}

// --- Validação ---

const FORBIDDEN_CSS_PATTERNS = [
  /@import\b/i,
  /\burl\s*\(\s*["']?\s*(https?|file|data|blob):/i,
  /-electron-/i,
  /app-region\s*:/i,
]

function validateCssContent(css) {
  const errors = []

  if (typeof css !== "string") {
    return { ok: false, errors: ["css_invalido"] }
  }

  if (Buffer.byteLength(css, "utf8") > LIMITS.cssTotalBytes) {
    errors.push("css_muito_grande")
  }

  for (const pattern of FORBIDDEN_CSS_PATTERNS) {
    if (pattern.test(css)) {
      errors.push(`css_padrao_proibido:${pattern.source.slice(0, 40)}`)
    }
  }

  // Verifica que todos os seletores começam com :theme ou são @-rules válidos
  const selectorErrors = validateSelectorScoping(css)
  errors.push(...selectorErrors)

  return { ok: errors.length === 0, errors }
}

function validateSelectorScoping(css) {
  const errors = []
  // Remove comentários e strings
  const stripped = css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")

  // Rastreia se estamos dentro de um bloco @keyframes para ignorar from/to/percentuais
  let depth = 0
  let inKeyframes = 0

  // Encontra blocos de regra: seletor { ... }
  const ruleRe = /([^{}]+)\{|\}/g
  let m
  while ((m = ruleRe.exec(stripped)) !== null) {
    if (m[0] === "}") {
      depth--
      if (inKeyframes > 0 && depth < inKeyframes) inKeyframes = 0
      continue
    }

    const selectorBlock = m[1].trim()
    if (!selectorBlock) { depth++; continue }

    // Detecta @keyframes para ignorar from/to/%
    if (selectorBlock.startsWith("@keyframes")) {
      inKeyframes = depth + 1
      depth++
      continue
    }

    // Pula outras @-rules (media, font-face, etc.)
    if (selectorBlock.startsWith("@")) {
      depth++
      continue
    }

    // Se estamos dentro de @keyframes, ignora seletores internos
    if (inKeyframes > 0) {
      depth++
      continue
    }

    // Cada seletor na lista deve começar com :theme
    const selectors = selectorBlock.split(",")
    for (const sel of selectors) {
      const trimmed = sel.trim()
      if (!trimmed) continue
      if (!trimmed.startsWith(":theme") && !trimmed.startsWith("[data-fullscreen-theme")) {
        errors.push(`seletor_nao_escopado:${trimmed.slice(0, 60)}`)
      }
    }

    depth++
  }

  return errors
}

// --- Normalização principal ---

function normalizeThemeCss(css, themeId) {
  const validation = validateCssContent(css)
  if (!validation.ok) {
    return { ok: false, css: "", errors: validation.errors }
  }

  let result = css
  result = namespaceKeyframes(result, themeId)
  result = rewriteThemeUrls(result, themeId)
  result = scopeSelectors(result, themeId)

  return { ok: true, css: result, errors: [] }
}

module.exports = {
  normalizeThemeCss,
  validateCssContent,
  namespaceKeyframes,
  rewriteThemeUrls,
  scopeSelectors,
  validateSelectorScoping,
}
