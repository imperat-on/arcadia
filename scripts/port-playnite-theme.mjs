#!/usr/bin/env node

/**
 * Conversor assistido de temas Playnite → Arcadia.
 *
 * Esta ferramenta analisa um tema Playnite Fullscreen extraído e gera:
 * - Scaffold theme.json para Arcadia
 * - Assets copiados quando permitidos
 * - Cores XAML comuns convertidas em tokens CSS
 * - Fontes/assets referenciados listados
 * - Views encontradas
 * - Bindings e PART_* listados como pendências
 * - Layouts sugeridos
 * - PORTING_REPORT.md
 *
 * NÃO executa XAML, scripts, assemblies. É uma análise estática.
 *
 * Uso:
 *   node scripts/port-playnite-theme.mjs <pasta-tema-playnite> [pasta-saida]
 */

import { readFileSync, writeFileSync, mkdirSync, copyFileSync, readdirSync, statSync, existsSync } from "node:fs"
import { join, extname, basename, relative, dirname } from "node:path"
import { createHash } from "node:crypto"

// --- Constantes ---

const ALLOWED_ASSET_EXTENSIONS = new Set([
  ".png", ".jpg", ".jpeg", ".webp", ".gif",
  ".woff2", ".mp4", ".webm",
])

const XAML_COLOR_MAP = {
  "#FFFFFFFF": "#ffffff",
  "#FF000000": "#000000",
  "#FFE0E0E0": "#e0e0e0",
  "#FF808080": "#808080",
  "#FF404040": "#404040",
  "#FF202020": "#202020",
  "#FF101010": "#101010",
  "#FF1A1A2E": "#1a1a2e",
  "#FF16213E": "#16213e",
  "#FF0F3460": "#0f3460",
  "#FF533483": "#533483",
  "#FFE94560": "#e94560",
}

const TOKEN_MAPPING = {
  "Background": "--fs-color-bg",
  "CardBackground": "--fs-color-surface",
  "TextBrush": "--fs-color-text",
  "TextBrushDim": "--fs-color-muted",
  "AccentBrush": "--fs-color-accent",
  "AccentBrush2": "--fs-color-accent-2",
  "GlyphBrush": "--fs-color-text",
  "HighlightBrush": "--fs-color-accent",
}

// --- Helpers ---

function readJsonSafe(filePath) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"))
  } catch {
    return null
  }
}

// Parser YAML simples para manifests Playnite (não suporta YAML completo).
// Lida com o formato comum: chave: valor, sem arrays multiline complexas.
function readYamlSafe(filePath) {
  try {
    const content = readFileSync(filePath, "utf8")
    return parseSimpleYaml(content)
  } catch {
    return null
  }
}

function parseSimpleYaml(text) {
  const result = {}
  const lines = text.split("\n")
  let currentKey = null
  let currentArray = null

  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith("#")) continue

    // Array item
    if (trimmed.startsWith("- ")) {
      if (currentKey && currentArray) {
        currentArray.push(trimmed.slice(2).trim().replace(/^["']|["']$/g, ""))
      }
      continue
    }

    // Key: value
    const match = trimmed.match(/^([a-zA-Z_][\w]*):\s*(.*)$/)
    if (match) {
      const [, key, value] = match
      currentKey = key
      currentArray = null

      if (value === "" || value === undefined) {
        // Could be start of array or nested object
        result[key] = []
        currentArray = result[key]
      } else {
        // Scalar value
        const cleanValue = value.replace(/^["']|["']$/g, "").trim()
        if (cleanValue === "true") result[key] = true
        else if (cleanValue === "false") result[key] = false
        else if (/^\d+$/.test(cleanValue)) result[key] = parseInt(cleanValue, 10)
        else if (/^\d+\.\d+$/.test(cleanValue)) result[key] = parseFloat(cleanValue)
        else result[key] = cleanValue
        currentArray = null
      }
    }
  }

  return Object.keys(result).length > 0 ? result : null
}

function walkDir(dir, callback) {
  if (!existsSync(dir)) return
  const entries = readdirSync(dir, { withFileTypes: true })
  for (const entry of entries) {
    const fullPath = join(dir, entry.name)
    if (entry.isDirectory()) {
      walkDir(fullPath, callback)
    } else if (entry.isFile()) {
      callback(fullPath)
    }
  }
}

function extractXamlColors(content) {
  const colors = {}
  // Match StaticResource and DynamicResource color references
  const colorRe = /(?:StaticResource|DynamicResource)\s+(\w+Brush)\b/g
  let m
  while ((m = colorRe.exec(content)) !== null) {
    const name = m[1]
    if (TOKEN_MAPPING[name]) {
      colors[name] = TOKEN_MAPPING[name]
    }
  }
  // Match hex colors
  const hexRe = /#[0-9A-Fa-f]{6,8}\b/g
  while ((m = hexRe.exec(content)) !== null) {
    const hex = m[0]
    const normalized = XAML_COLOR_MAP[hex.toUpperCase()] || hex.toLowerCase()
    if (normalized.startsWith("#")) {
      colors[hex] = normalized
    }
  }
  return colors
}

function extractBindings(content) {
  const bindings = []
  const bindingRe = /\{Binding\s+(\w+(?:\.\w+)*)\}/g
  let m
  while ((m = bindingRe.exec(content)) !== null) {
    bindings.push(m[1])
  }
  return [...new Set(bindings)]
}

function extractParts(content) {
  const parts = []
  const partRe = /PART_(\w+)/g
  let m
  while ((m = partRe.exec(content)) !== null) {
    parts.push(`PART_${m[1]}`)
  }
  return [...new Set(parts)]
}

function extractFonts(content) {
  const fonts = []
  const fontRe = /FontFamily\s*=\s*"([^"]+)"/g
  let m
  while ((m = fontRe.exec(content)) !== null) {
    fonts.push(m[1])
  }
  return [...new Set(fonts)]
}

function extractMediaReferences(content) {
  const media = []
  const mediaRe = /(?:Source|Background)\s*=\s*"([^"]+\.(png|jpg|jpeg|webp|gif|mp4|webm))"/gi
  let m
  while ((m = mediaRe.exec(content)) !== null) {
    media.push(m[1])
  }
  return [...new Set(media)]
}

// --- Conversão principal ---

function convertTheme(inputDir, outputDir) {
  const report = {
    inputDir,
    outputDir,
    manifest: null,
    views: [],
    colors: {},
    fonts: [],
    assets: [],
    bindings: [],
    parts: [],
    warnings: [],
    errors: [],
  }

  // 1. Ler manifesto Playnite (YAML ou JSON)
  const themeYaml = readYamlSafe(join(inputDir, "theme.yaml"))
  const themeJson = readJsonSafe(join(inputDir, "theme.json"))

  const playniteManifest = themeYaml || themeJson
  if (!playniteManifest) {
    report.errors.push("Manifesto Playnite não encontrado (theme.yaml ou theme.json)")
    return report
  }

  // 2. Gerar manifesto Arcadia
  const themeId = `ported.${(playniteManifest.name || "unknown").toLowerCase().replace(/[^a-z0-9]/g, ".").slice(0, 50)}`
  const arcadiaManifest = {
    manifestVersion: 1,
    themeApiVersion: 1,
    id: themeId,
    name: playniteManifest.name || "Ported Theme",
    author: playniteManifest.author || "Unknown",
    version: playniteManifest.version || "1.0.0",
    description: playniteManifest.description || `Ported from Playnite theme: ${playniteManifest.name || "unknown"}`,
    mode: "fullscreen",
    entry: "theme.css",
    layouts: {},
    previews: [],
    features: ["tokens"],
    options: {},
    supports: {
      minWidth: 1280,
      minHeight: 720,
      aspectRatios: ["16:9"],
    },
    license: playniteManifest.license || "",
    homepage: playniteManifest.homepage || "",
  }
  report.manifest = arcadiaManifest

  // 3. Analisar views XAML
  const viewsDir = join(inputDir, "Views")
  const xamlFiles = []
  walkDir(inputDir, (filePath) => {
    if (extname(filePath).toLowerCase() === ".xaml") {
      xamlFiles.push(filePath)
    }
  })

  for (const xamlFile of xamlFiles) {
    const content = readFileSync(xamlFile, "utf8")
    const relativePath = relative(inputDir, xamlFile)
    const viewName = basename(xamlFile, ".xaml")

    report.views.push({
      file: relativePath,
      name: viewName,
      size: content.length,
    })

    // Extrair cores
    const colors = extractXamlColors(content)
    Object.assign(report.colors, colors)

    // Extrair bindings
    const bindings = extractBindings(content)
    report.bindings.push(...bindings.map((b) => ({ view: viewName, binding: b })))

    // Extrair PART_*
    const parts = extractParts(content)
    report.parts.push(...parts.map((p) => ({ view: viewName, part: p })))

    // Extrair fontes
    const fonts = extractFonts(content)
    report.fonts.push(...fonts)

    // Extrair referências de mídia
    const media = extractMediaReferences(content)
    report.assets.push(...media)
  }

  // 4. Copiar assets permitidos
  const assetsDir = join(outputDir, "assets")
  mkdirSync(assetsDir, { recursive: true })

  walkDir(inputDir, (filePath) => {
    const ext = extname(filePath).toLowerCase()
    if (ALLOWED_ASSET_EXTENSIONS.has(ext)) {
      const relativePath = relative(inputDir, filePath)
      const destPath = join(assetsDir, relativePath)
      try {
        mkdirSync(dirname(destPath), { recursive: true })
        copyFileSync(filePath, destPath)
        report.assets.push(relativePath)
      } catch (err) {
        report.warnings.push(`Falha ao copiar asset ${relativePath}: ${err.message}`)
      }
    }
  })

  // 5. Gerar CSS com tokens
  let css = `/* Tema portado de Playnite: ${playniteManifest.name || "unknown"} */\n`
  css += `/* Gerado automaticamente por port-playnite-theme.mjs */\n\n`
  css += `:theme {\n`

  // Cores mapeadas
  const uniqueColors = new Set()
  for (const [token, value] of Object.entries(report.colors)) {
    if (!uniqueColors.has(value)) {
      uniqueColors.add(value)
      const cssToken = TOKEN_MAPPING[token] || `--ported-${token.toLowerCase().replace(/[^a-z0-9]/g, "-")}`
      css += `  ${cssToken}: ${value};\n`
    }
  }

  // Fontes
  if (report.fonts.length > 0) {
    css += `\n  /* Fontes encontradas (verificar disponibilidade) */\n`
    for (const font of report.fonts.slice(0, 5)) {
      css += `  /* font: ${font}; */\n`
    }
  }

  css += `}\n`

  // 6. Escrever arquivos de saída
  mkdirSync(outputDir, { recursive: true })
  writeFileSync(join(outputDir, "theme.json"), JSON.stringify(arcadiaManifest, null, 2))
  writeFileSync(join(outputDir, "theme.css"), css)

  // 7. Gerar relatório
  const reportMd = generateReport(report)
  writeFileSync(join(outputDir, "PORTING_REPORT.md"), reportMd)

  return report
}

function generateReport(report) {
  let md = `# Relatório de Portabilidade Playnite → Arcadia\n\n`
  md += `**Data:** ${new Date().toISOString().split("T")[0]}\n`
  md += `**Tema:** ${report.manifest?.name || "Unknown"}\n`
  md += `**ID Arcadia:** ${report.manifest?.id || "unknown"}\n\n`

  md += `## Resumo\n\n`
  md += `- **Views encontradas:** ${report.views.length}\n`
  md += `- **Cores mapeadas:** ${Object.keys(report.colors).length}\n`
  md += `- **Fontes:** ${report.fonts.length}\n`
  md += `- **Assets copiados:** ${report.assets.length}\n`
  md += `- **Bindings Playnite:** ${report.bindings.length}\n`
  md += `- **PART_*:** ${report.parts.length}\n`
  md += `- **Avisos:** ${report.warnings.length}\n`
  md += `- **Erros:** ${report.errors.length}\n\n`

  if (report.errors.length > 0) {
    md += `## Erros\n\n`
    for (const err of report.errors) {
      md += `- ❌ ${err}\n`
    }
    md += `\n`
  }

  if (report.warnings.length > 0) {
    md += `## Avisos\n\n`
    for (const warn of report.warnings) {
      md += `- ⚠️ ${warn}\n`
    }
    md += `\n`
  }

  md += `## Views\n\n`
  for (const view of report.views) {
    md += `- \`${view.file}\` (${view.name}, ${view.size} bytes)\n`
  }
  md += `\n`

  md += `## Cores Mapeadas\n\n`
  md += `| XAML | Token Arcadia |\n|---|---|\n`
  for (const [xaml, token] of Object.entries(report.colors).slice(0, 20)) {
    md += `| \`${xaml}\` | \`${token}\` |\n`
  }
  if (Object.keys(report.colors).length > 20) {
    md += `| ... | ... |\n`
  }
  md += `\n`

  if (report.bindings.length > 0) {
    md += `## Bindings Playnite (pendências)\n\n`
    md += `Estes bindings precisam ser mapeados para componentes do Arcadia:\n\n`
    const uniqueBindings = [...new Set(report.bindings.map((b) => b.binding))]
    for (const binding of uniqueBindings.slice(0, 30)) {
      md += `- \`${binding}\`\n`
    }
    if (uniqueBindings.length > 30) {
      md += `- ... (${uniqueBindings.length - 30} mais)\n`
    }
    md += `\n`
  }

  if (report.parts.length > 0) {
    md += `## PART_* (controles Playnite)\n\n`
    md += `Estes controles precisam de equivalente no Arcadia:\n\n`
    const uniqueParts = [...new Set(report.parts.map((p) => p.part))]
    for (const part of uniqueParts) {
      md += `- \`${part}\`\n`
    }
    md += `\n`
  }

  md += `## Próximos Passos\n\n`
  md += `1. Revisar o \`theme.css\` gerado e ajustar tokens\n`
  md += `2. Verificar se as fontes estão disponíveis ou incluí-las no pacote\n`
  md += `3. Mapear bindings para componentes do Arcadia\n`
  md += `4. Criar layouts home/overview se necessário\n`
  md += `5. Testar com gamepad e em diferentes resoluções\n`
  md += `6. Verificar licença antes de distribuir\n`

  return md
}

// --- CLI ---

const args = process.argv.slice(2)
if (args.length < 1) {
  console.error("Uso: node scripts/port-playnite-theme.mjs <pasta-tema-playnite> [pasta-saida]")
  process.exit(1)
}

const inputDir = args[0]
const outputDir = args[1] || join(process.cwd(), "ported-theme")

if (!existsSync(inputDir)) {
  console.error(`Erro: pasta não encontrada: ${inputDir}`)
  process.exit(1)
}

console.log(`Convertendo tema Playnite: ${inputDir}`)
console.log(`Saída: ${outputDir}`)

const report = convertTheme(inputDir, outputDir)

if (report.errors.length > 0) {
  console.error(`\nErros:`)
  for (const err of report.errors) {
    console.error(`  ❌ ${err}`)
  }
  process.exit(1)
}

console.log(`\n✅ Conversão concluída!`)
console.log(`   Views: ${report.views.length}`)
console.log(`   Cores: ${Object.keys(report.colors).length}`)
console.log(`   Assets: ${report.assets.length}`)
console.log(`   Avisos: ${report.warnings.length}`)
console.log(`\nArquivos gerados:`)
console.log(`   ${join(outputDir, "theme.json")}`)
console.log(`   ${join(outputDir, "theme.css")}`)
console.log(`   ${join(outputDir, "PORTING_REPORT.md")}`)
