"use strict"

/**
 * Retro Title Parser
 *
 * Extracts and normalizes retro game titles from ROM filenames, detecting
 * platform serials (PS1/PS2/PS3/PSP), release metadata, and classification.
 */

const MAX_TITLE = 300
const MAX_RAW_TITLE = 2048

// Format tags: file extensions and media types
const RETRO_FORMAT_TAG_RE =
  /^(?:7z|archive|bin(?:\s*\/\s*cue)?|cd|cso|cue|chd|disc|disk|dvd|e?boot|gdi|iso|nsp|pkg|rom|rvz|tap|torrent|wad|wbfs|xci)$/i

// Region identifiers
const RETRO_REGION_TAGS = new Set([
  "australia",
  "asia",
  "brazil",
  "canada",
  "europe",
  "eu",
  "eur",
  "france",
  "germany",
  "italy",
  "japan",
  "jp",
  "korea",
  "pal",
  "russia",
  "rus",
  "spain",
  "usa",
  "us",
  "u",
  "world",
  "ww",
])

// Language identifiers
const RETRO_LANGUAGE_TAGS = new Set([
  "ar",
  "br",
  "de",
  "en",
  "es",
  "fr",
  "it",
  "ja",
  "j",
  "ko",
  "pt",
  "ru",
  "sp",
  "zh",
  "eng",
  "jpn",
  "jap",
  "rus",
  "spa",
  "fra",
  "ger",
  "ita",
  "kor",
  "chn",
  "ntsc",
  "english",
  "portuguese",
  "dutch",
  "french",
  "german",
  "italian",
  "japanese",
  "russian",
  "spanish",
])

// Platform identifiers
const RETRO_PLATFORM_TAG_RE =
  /^(?:3do|amiga|arcade|atari(?:\s*2600|\s*5200|\s*7800)?|colecovision|dreamcast|game(?:boy|gear)|gba|gbc|genesis|megadrive|n(?:es|64|ds)|pc(?:\s+engine)?|pce(?:\s+cd)?(?:\s*[-/]\s*ps[1-5])?|psx?(?:\s*[-/]\s*(?:ps[1-5]|dvr))?|ps[1-5](?:\s*[-/]\s*ps[1-5])?|psp(?:\s*[-/]\s*ps[1-5])?|saturn|sega(?:\s*cd|\s*32x)?|snes|switch|wiiu?|xbox)$/i

// Serial/version identifiers
const RETRO_SERIAL_TAG_RE =
  /^(?:[a-z]{2,8}[-_ ]?\d{2,}[a-z0-9\-/ %]*|\d{2,}[a-z][a-z0-9-]*|(?:rev|revision|ver|version|v|disc|disk|cd)\s*[a-z0-9._/-]+)$/i

// Flag/status tags
const RETRO_FLAG_TAG_RE =
  /^(?:[!abfhp]|aftermarket|all|beta|bios?|convert|demo|digital|dendy|download|dlc|folder|full|hack|homebrew|mod|multi\d*|move|ntsc(?:-[a-z0-9]+)?|ode|pal|patch|pirate|proper|prototype|proto|region|redump|repack|soft|translation|trainer|unl|unlicensed|virtual console)$/i

// Release group patterns
const RETRO_GROUP_TAG_RE = /^[\p{L}\p{N}][\p{L}\p{N}._'-]{1,31}$/u

// PlayStation serial patterns
const PS1_SERIAL_RE = /\b([SB][CL][UE][SD])[-_ ]?(\d{5})\b/i
const PS2_SERIAL_RE = /\b([SB][CL][UE][SD])[-_ ]?(\d{5})\b/i
const PS3_SERIAL_RE = /\b([BN][CL][UE][SD])[-_ ]?(\d{5})\b/i
const PSP_SERIAL_RE = /\b([UN][CL][UJE][SD])[-_ ]?(\d{5})\b/i

// Combined PlayStation serial pattern
const PLAYSTATION_SERIAL_RE =
  /\b([SBUN][CL][UUJE][SD])[-_ ]?(\d{5})\b/i

/**
 * Extract PlayStation serial/Title ID from text.
 * Supports PS1, PS2, PS3, and PSP formats.
 *
 * @param {string} text - Text to search for serials
 * @returns {Object|null} - { serial, platform } or null
 */
function extractPlayStationSerial(text) {
  if (!text || typeof text !== "string") return null

  const match = text.match(PLAYSTATION_SERIAL_RE)
  if (!match) return null

  const prefix = match[1].toUpperCase()
  const number = match[2]
  const serial = `${prefix}-${number}`

  // Determine platform based on prefix
  let platform = null

  // PS3 prefixes (B/N prefix)
  if (/^[BN][CL][UE][SD]$/.test(prefix)) {
    platform = "PS3"
  }
  // PSP prefixes (U prefix with specific second characters)
  else if (/^[UN][CL][UJ][SD]$/.test(prefix)) {
    platform = "PSP"
  }
  // PS1/PS2 prefixes (S prefix)
  // Distinguish by number range: PS2 typically starts at 20000+
  else if (/^[SB][CL][UE][SD]$/.test(prefix)) {
    const num = parseInt(number, 10)
    if (num >= 20000) {
      platform = "PS2"
    } else {
      platform = "PS1"
    }
  }

  return platform ? { serial, platform } : null
}

/**
 * Classify release kind based on title and metadata.
 *
 * @param {string} title - Original title text
 * @returns {string|null} - Release kind or null
 */
function classifyReleaseKind(title) {
  if (!title || typeof title !== "string") return null

  const lower = title.toLowerCase()

  // BIOS (highest priority - most specific)
  if (/\b(?:bios|firmware|bootrom|system\s*files?)\b/i.test(lower)) {
    return "bios"
  }

  // DLC
  if (/\b(?:dlc|downloadable\s*content|expansion\s*pack)\b/i.test(lower)) {
    return "dlc"
  }

  // Update/Patch
  if (/\b(?:update|patch(?:es)?|hotfix|bugfix|ver(?:sion)?\s*\d+\.\d+)\b/i.test(lower)) {
    return "update"
  }

  // Homebrew
  if (/\b(?:homebrew|home\s*brew|unofficial|fan\s*made)\b/i.test(lower)) {
    return "homebrew"
  }

  // Hack/Mod
  if (/\b(?:hack|rom\s*hack|romhack|mod(?:ified)?|hack\s*by)\b/i.test(lower)) {
    return "hack"
  }

  // Translation
  if (
    /\b(?:translation|translated|patch[-_\s]+(?:eng|rus|jap|jpn|eur|usa|spa|fra|ger|ita))\b/i.test(
      lower,
    )
  ) {
    return "translation"
  }

  // Collection (check for indicators)
  if (
    /\b(?:collection|anthology|compilation|complete\s*(?:edition|set)|greatest\s*hits|all\s*in\s*one)\b/i.test(
      lower,
    ) ||
    /\b\d+\s*in\s*\d+\b/i.test(lower)
  ) {
    return "collection"
  }

  return null
}

/**
 * Parse title text into normalized words for comparison.
 *
 * @param {string} value - Text to parse
 * @returns {string[]} - Array of normalized words
 */
function retroTagWords(value) {
  return String(value || "")
    .normalize("NFKC")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .split(/[\s,;|/+&().\-]+/)
    .map((word) => word.trim())
    .filter(Boolean)
}

/**
 * Check if a token is metadata rather than part of the game title.
 *
 * @param {string} value - Token to check
 * @returns {boolean} - True if metadata token
 */
function isRetroMetadataToken(value) {
  const token = String(value || "")
    .trim()
    .toLowerCase()
  if (!token) return true
  if (RETRO_REGION_TAGS.has(token) || RETRO_LANGUAGE_TAGS.has(token)) return true
  if (RETRO_FORMAT_TAG_RE.test(token) || RETRO_PLATFORM_TAG_RE.test(token)) return true
  if (RETRO_FLAG_TAG_RE.test(token)) return true

  // Combined language/region qualifiers
  if (/^(?:full|multi)?(?:rus|eng|eur|jap|jpn|spa|fra|ger|ita|kor|chn)(?:sound)?$/i.test(token))
    return true
  if (/^(?:ntsc|pal)(?:[-_][a-z0-9]+)?$/i.test(token)) return true
  if (/^\d{1,3}%$/.test(token) || /^ird(?:\d+%?)?$/i.test(token)) return true
  if (/^(?:dvd|cd|disc)\d+$/i.test(token)) return true
  if (/^\d+$/.test(token) || /^~?\d+(?:x\d+)?$/i.test(token)) return true
  if (/^\d+:\d+$/.test(token) || /^(?:true|false)$/i.test(token)) return true
  if (/^(?:eng|rus)(?:sound|sounds|soundtracks|audio|bonus)(?:s)?$/i.test(token)) return true

  return RETRO_SERIAL_TAG_RE.test(token)
}

/**
 * Check if a bracketed/parenthetical tag is metadata.
 *
 * @param {string} value - Tag content (without brackets)
 * @returns {boolean} - True if metadata tag
 */
function isRetroMetadataTag(value) {
  const tag = String(value || "")
    .replace(/^[-_.]+|[-_.]+$/g, "")
    .trim()
  if (!tag) return true
  if (RETRO_FORMAT_TAG_RE.test(tag) || RETRO_PLATFORM_TAG_RE.test(tag)) return true
  if (RETRO_SERIAL_TAG_RE.test(tag) || RETRO_FLAG_TAG_RE.test(tag)) return true

  // Date patterns
  if (/^(?:19|20)\d{2}(?:[-/.](?:0?\d|1[0-2]))?(?:[-/.](?:0?[1-9]|[12]\d|3[01]))?$/.test(tag))
    return true
  if (/^\d{1,2}[./-]\d{1,2}[./-](?:19|20)\d{2}$/.test(tag)) return true
  if (/^~?\d[\d\s.,~]*(?:шт|items?|games?)\.?$/iu.test(tag)) return true
  if (/^(?:all\s+)?region$|^non[-\s]?redump$/i.test(tag)) return true
  if (/\bdlc\b/i.test(tag)) return true

  const words = retroTagWords(tag)
  if (!words.length) return true
  if (words.every(isRetroMetadataToken)) return true

  // Mixed metadata with separators
  if (/[|/+,]/.test(tag) && words.filter((word) => isRetroMetadataToken(word)).length >= 2)
    return true
  if (words.some((word) => /^(?:rev|revision|ver|version|v)\d*$/i.test(word))) return true

  return false
}

/**
 * Check if a tag is a release group identifier.
 *
 * @param {string} value - Tag content
 * @returns {boolean} - True if release group tag
 */
function isRetroReleaseGroupTag(value) {
  const tag = String(value || "").trim()
  if (!tag || isRetroMetadataTag(tag)) return false
  if (/^\d+\s+in\s+\d+$/i.test(tag)) return false

  // Preserve edition names
  if (
    /\b(?:anniversary|collection|complete|cut|deluxe|director|edition|greatest|limited|remaster(?:ed)?|special|ultimate)\b/i.test(
      tag,
    )
  )
    return false

  // Release group indicators
  if (
    /[\/|+,]/.test(tag) ||
    /[-–—]/.test(tag) ||
    /\b(?:company|convert|fan(?:s)?|mvo|retrogaming|rgr|studio|team|text|translation|transgen|version|voice|vhs)\b/i.test(
      tag,
    ) ||
    /(?:текст|озвуч|фанат|перевод|версии)/i.test(tag)
  )
    return true

  const words = retroTagWords(tag)
  return (
    words.length > 0 &&
    words.length <= 4 &&
    /^[\p{L}\p{N}][\p{L}\p{N}._'']*(?:\s+[\p{L}\p{N}][\p{L}\p{N}._'']*)*$/u.test(tag)
  )
}

/**
 * Remove metadata groups (brackets/parentheses) from title.
 *
 * @param {string} title - Title with metadata groups
 * @returns {string} - Title with metadata removed
 */
function stripRetroGroups(title) {
  let cleaned = title
  const squareGroups = [...cleaned.matchAll(/\[([^\]\n]*)\]/g)]

  const explicitRelease = (value) =>
    /(?:team|studio|company|translation|translated|voice|sound|patch|озвуч|перевод|текст|фанат|версии)/iu.test(
      value,
    )

  if (
    squareGroups.some(
      (match) =>
        isRetroMetadataTag(match[1]) ||
        (isRetroReleaseGroupTag(match[1]) && explicitRelease(match[1])),
    )
  ) {
    cleaned = cleaned.replace(/\s*\[([^\]\n]*)\]/g, (raw, value) => {
      if (isRetroMetadataTag(value) || isRetroReleaseGroupTag(value)) return ""
      return raw
    })
  }

  // Handle trailing parenthetical groups
  let rest = cleaned
  const groups = []
  let match
  while ((match = /(?:\s*)(\[[^\]\n]*\]|\([^()\n]*\))\s*$/.exec(rest))) {
    const raw = match[1]
    groups.unshift({ raw, value: raw.slice(1, -1).trim() })
    rest = rest.slice(0, match.index).trim()
  }
  if (!groups.some((group) => isRetroMetadataTag(group.value))) return cleaned
  const kept = groups.filter(
    (group) =>
      !isRetroMetadataTag(group.value) &&
      !(group.raw.startsWith("[") && isRetroReleaseGroupTag(group.value)),
  )
  return `${rest}${kept.map((group) => ` ${group.raw}`).join("")}`.trim()
}

/**
 * Check if parenthetical content is an alias/alternate name.
 *
 * @param {string} group - Content inside parentheses
 * @param {string} before - Text before the parentheses
 * @returns {boolean} - True if alias group
 */
function isRetroAliasGroup(group, before) {
  const text = String(group || "").trim()
  if (/^(?:dot\s+hack|biohazard|resident\s+evil)\b/i.test(text)) return true
  if (/^(?:password|senha)\s*:/i.test(text)) return true
  if (
    /(?:team|studio|company)\s*$/iu.test(text) ||
    /(?:translation|translated|voice|sound|patch|озвуч|перевод|текст|фанат|версии)/iu.test(text)
  )
    return true

  const beforeWords = String(before || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
  const words = text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .split(/\s+/)
    .filter((word) => word.length > 2)
  return words.filter((word) => beforeWords.includes(word)).length >= 2
}

/**
 * Remove alias parentheticals from title.
 *
 * @param {string} title - Title with potential aliases
 * @returns {string} - Title with aliases removed
 */
function stripRetroAliasParentheticals(title) {
  let output = ""
  for (let index = 0; index < title.length;) {
    if (title[index] !== "(") {
      output += title[index++]
      continue
    }
    let depth = 1
    let end = index + 1
    while (end < title.length && depth > 0) {
      if (title[end] === "(") depth++
      else if (title[end] === ")") depth--
      end++
    }
    if (depth !== 0) {
      output += title[index++]
      continue
    }
    const group = title.slice(index + 1, end - 1)
    if (isRetroAliasGroup(group, output)) {
      output = output.replace(/\s+$/g, "")
    } else {
      output += title.slice(index, end)
    }
    index = end
  }
  return output
}

/**
 * Normalize retro game title for display and search.
 *
 * @param {string} value - Raw title text
 * @param {number} max - Maximum length
 * @returns {string} - Normalized title
 */
function normalizeRetroTitle(value, max = MAX_TITLE) {
  const original = String(value || "")
    .replace(/<br\s*\/?>(?=\s*)/gi, "\n")
    .replace(/<[^>]*>/g, " ")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&#(x[0-9a-f]+|\d+);/gi, (_match, code) => {
      const parsed = /^x/i.test(code) ? parseInt(code.slice(1), 16) : parseInt(code, 10)
      return Number.isFinite(parsed) && parsed >= 0 && parsed <= 0x10ffff
        ? String.fromCodePoint(parsed)
        : " "
    })
    .replace(/[ -]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, MAX_RAW_TITLE)

  if (!original) return ""

  let title = original
    .normalize("NFKC")
    .replace(/[‐-―]/g, "-")
    .replace(/[_]+/g, " ")
    .replace(/\s+/g, " ")
    .replace(/\.(?:7z|7zip|bin|chd|cue|iso|nsp|pkg|rar|rom|rvz|wad|wbfs|xci)$/i, "")
    .trim()

  // Remove malformed groups
  title = title
    .replace(/\s+\[[^\]\n]*$/u, "")
    .replace(/\s+\([^()\n]*$/u, "")
    .trim()

  // Remove leading platform/metadata tags
  while (true) {
    const leading = /^\s*(\[[^\]\n]*\]|\([^()\n]*\))\s*/.exec(title)
    if (!leading || !isRetroMetadataTag(leading[1].slice(1, -1))) break
    title = title.slice(leading[0].length).trim()
  }

  title = stripRetroGroups(title)
  title = stripRetroAliasParentheticals(title)

  // Normalize .hack// series
  title = title
    .replace(/^\.hack\/(?!\/)/i, ".hack//")
    .replace(/^\.hack\/\/frägment\b/i, ".hack//Fragment")

  // Remove translation patch suffixes
  let previousPatchTitle
  do {
    previousPatchTitle = title
    title = title.replace(
      /\s*(?:[+]\s*)?(?:patch|translation)[-_\s]+(?:eng|rus|jap|jpn|eur|usa)(?:[+|/]?(?:eng|rus|jap|jpn|eur|usa))*[+]?\s*$/i,
      "",
    )
  } while (title !== previousPatchTitle)

  // Remove format suffixes
  title = title
    .replace(
      /(?:^|[\s–—|,:-])(?:7z|bin(?:\s*\/\s*cue)?|chd|cso|cue|iso|nsp|pkg|rom|rvz|wad|wbfs|xci)\s*$/iu,
      "",
    )
    .replace(/\s+\[[^\]\n]*$/u, "")
    .replace(/\s+\([^()\n]*$/u, "")
    .replace(/\s+/g, " ")
    .replace(/^[\s_-]+|[\s_-]+$/g, "")
    .trim()

  return title.slice(0, max)
}

/**
 * Parse retro title with full metadata extraction.
 *
 * @param {string} value - Raw title text
 * @returns {Object} - Parsed title information
 */
function parseRetroTitle(value) {
  const originalTitle = String(value || "").trim()
  const normalizedTitle = normalizeRetroTitle(originalTitle)
  const serial = extractPlayStationSerial(originalTitle)
  const releaseKind = classifyReleaseKind(originalTitle)

  return {
    title: normalizedTitle,
    originalTitle,
    serial: serial?.serial || null,
    platform: serial?.platform || null,
    releaseKind,
  }
}

module.exports = {
  parseRetroTitle,
  normalizeRetroTitle,
  extractPlayStationSerial,
  classifyReleaseKind,
  retroTagWords,
  isRetroMetadataToken,
  isRetroMetadataTag,
  isRetroReleaseGroupTag,
  stripRetroGroups,
  isRetroAliasGroup,
  stripRetroAliasParentheticals,

  // Constants for external use
  RETRO_FORMAT_TAG_RE,
  RETRO_REGION_TAGS,
  RETRO_LANGUAGE_TAGS,
  RETRO_PLATFORM_TAG_RE,
  RETRO_SERIAL_TAG_RE,
  RETRO_FLAG_TAG_RE,
  RETRO_GROUP_TAG_RE,
  PS1_SERIAL_RE,
  PS2_SERIAL_RE,
  PS3_SERIAL_RE,
  PSP_SERIAL_RE,
  PLAYSTATION_SERIAL_RE,
}
