"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  normalizeThemeCss,
  validateCssContent,
  namespaceKeyframes,
  rewriteThemeUrls,
  scopeSelectors,
  validateSelectorScoping,
} = require("../electron/themes/css")

// --- Validação ---

test("css rejeita @import", () => {
  const result = validateCssContent('@import url("other.css");\n:theme { color: red; }')
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("padrao_proibido")))
})

test("css rejeita URL externa", () => {
  const result = validateCssContent(':theme { background: url("https://evil.com/bg.png"); }')
  assert.equal(result.ok, false)
})

test("css rejeita URL file:", () => {
  const result = validateCssContent(':theme { background: url("file:///etc/passwd"); }')
  assert.equal(result.ok, false)
})

test("css rejeita URL data:", () => {
  const result = validateCssContent(':theme { background: url("data:text/html,<script>alert(1)</script>"); }')
  assert.equal(result.ok, false)
})

test("css rejeita URL blob:", () => {
  const result = validateCssContent(':theme { background: url("blob:..."); }')
  assert.equal(result.ok, false)
})

test("css rejeita propriedades Electron", () => {
  const result = validateCssContent(':theme { -webkit-app-region: drag; }')
  assert.equal(result.ok, false)
})

test("css aceita conteúdo válido com :theme", () => {
  const result = validateCssContent(`
    :theme {
      --fs-color-bg: #000;
      --fs-color-accent: #72ddff;
    }
    :theme [data-theme-slot="home.hero"] {
      backdrop-filter: blur(18px);
    }
  `)
  assert.equal(result.ok, true)
  assert.deepEqual(result.errors, [])
})

test("css rejeita seletor não escopado", () => {
  const result = validateCssContent("body { color: red; }")
  assert.equal(result.ok, false)
  assert.ok(result.errors.some((e) => e.includes("seletor_nao_escopado")))
})

test("css aceita @keyframes e @media dentro de :theme", () => {
  const result = validateCssContent(`
    :theme { color: red; }
    @keyframes fade { from { opacity: 0; } to { opacity: 1; } }
    @media (max-width: 720px) { :theme { font-size: 12px; } }
  `)
  assert.equal(result.ok, true)
})

// --- Namespace de keyframes ---

test("css namespaceKeyframes prefixa nomes de keyframes", () => {
  const input = `
    @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
    .box { animation: fadeIn 1s; }
  `
  const result = namespaceKeyframes(input, "arcadia.aurora")
  assert.ok(result.includes("@keyframes t-arcadia.aurora-fadeIn"))
  assert.ok(result.includes("animation: t-arcadia.aurora-fadeIn 1s"))
  assert.ok(!result.match(/@keyframes\s+fadeIn\b/))
})

test("css namespaceKeyframes não afeta keyframes já prefixados", () => {
  const input = "@keyframes t-old-fade { from { opacity: 0; } }"
  const result = namespaceKeyframes(input, "test")
  assert.ok(result.includes("@keyframes t-test-t-old-fade"))
})

// --- Reescrita de URLs ---

test("css rewriteThemeUrls converte theme:// para arcadia-theme://", () => {
  const input = 'background: url("theme://assets/bg.webp");'
  const result = rewriteThemeUrls(input, "arcadia.aurora")
  assert.ok(result.includes("arcadia-theme://arcadia.aurora/assets/bg.webp"))
  // O theme:// original deve ter sido substituído
  assert.ok(!result.match(/[^-]theme:\/\//))
})

test("css rewriteThemeUrls rejeita traversal", () => {
  const input = 'background: url("theme://../../etc/passwd");'
  const result = rewriteThemeUrls(input, "test")
  assert.ok(result.includes("about:invalid"))
})

// --- Escopo de seletores ---

test("css scopeSelectors substitui :theme por data-attribute", () => {
  const input = ":theme { color: red; } :theme .child { color: blue; }"
  const result = scopeSelectors(input, "arcadia.aurora")
  assert.ok(result.includes('[data-fullscreen-theme="arcadia.aurora"]'))
  assert.ok(!result.includes(":theme"))
})

// --- Normalização completa ---

test("css normalizeThemeCss aplica todas as transformações", () => {
  const input = `
    :theme {
      --fs-color-accent: #72ddff;
      background: url("theme://assets/bg.webp");
    }
    :theme [data-theme-slot="home.hero"] {
      animation: slideIn 0.3s;
    }
    @keyframes slideIn { from { transform: translateY(20px); } to { transform: translateY(0); } }
  `
  const result = normalizeThemeCss(input, "arcadia.aurora")
  assert.equal(result.ok, true)
  assert.ok(result.css.includes('[data-fullscreen-theme="arcadia.aurora"]'))
  assert.ok(result.css.includes("arcadia-theme://arcadia.aurora/assets/bg.webp"))
  assert.ok(result.css.includes("t-arcadia.aurora-slideIn"))
})

test("css normalizeThemeCss rejeita CSS inválido", () => {
  const result = normalizeThemeCss('body { color: red; }', "test")
  assert.equal(result.ok, false)
  assert.ok(result.errors.length > 0)
})
