// Temas embutidos do Arcadia. Estes temas estão sempre disponíveis e não
// podem ser removidos. O Default é fallback permanente; Aurora é o tema
// demonstrativo oficial.

import type { FullscreenThemeInfo, FullscreenThemeManifest } from "./types"

const DEFAULT_MANIFEST: FullscreenThemeManifest = {
  manifestVersion: 1,
  themeApiVersion: 1,
  id: "arcadia.default",
  name: "Arcadia Default",
  author: "Arcadia",
  version: "1.0.0",
  description: "Aparência Retro/CRT original do Big Picture.",
  mode: "fullscreen",
  entry: "builtin-default.css",
  layouts: {},
  previews: [],
  features: ["tokens"],
  options: {},
  supports: {},
  homepage: "",
  license: "",
  compat: "ok",
}

const AURORA_MANIFEST: FullscreenThemeManifest = {
  manifestVersion: 1,
  themeApiVersion: 1,
  id: "arcadia.aurora",
  name: "Arcadia Aurora",
  author: "Arcadia",
  version: "1.0.0",
  description: "Tema cinematográfico azul e violeta.",
  mode: "fullscreen",
  entry: "builtin-aurora.css",
  layouts: {},
  previews: [],
  features: ["tokens", "assets"],
  options: {},
  supports: {
    minWidth: 1280,
    minHeight: 720,
    aspectRatios: ["16:9", "16:10", "21:9"],
  },
  homepage: "",
  license: "",
  compat: "ok",
}

export const BUILTIN_THEMES: FullscreenThemeInfo[] = [
  {
    id: "arcadia.default",
    manifest: DEFAULT_MANIFEST,
    source: "builtin",
    installed: true,
    valid: true,
    error: "",
    state: "valid",
    options: {},
    active: false,
  },
  {
    id: "arcadia.aurora",
    manifest: AURORA_MANIFEST,
    source: "builtin",
    installed: true,
    valid: true,
    error: "",
    state: "valid",
    options: {},
    active: false,
  },
]

export function getBuiltinTheme(id: string): FullscreenThemeInfo | undefined {
  return BUILTIN_THEMES.find((t) => t.id === id)
}

export function isBuiltin(id: string): boolean {
  return id === "arcadia.default" || id === "arcadia.aurora"
}
