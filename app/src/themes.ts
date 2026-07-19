// Temas completos do Arcadia (estilo Heroic): cada tema define fundo, sidebar,
// superfície de cards, cor de destaque e cores de texto. Aplicado via CSS vars
// em :root + uma folha de estilo dinâmica (ver applyTheme em AccessibilityView).

export interface Theme {
  id: string
  nome: string
  /** Fundo principal do app */
  bg: string
  /** Fundo da sidebar */
  sidebar: string
  /** Superfície de cards/inputs */
  card: string
  /** Cor de destaque (bordas ativas, toggles, botões) */
  accent: string
  /** Texto principal */
  text: string
  /** Texto secundário/muted */
  muted: string
}

export const TEMAS: Theme[] = [
  {
    id: "midnight",
    nome: "Midnight Mirage",
    bg: "#000000",
    sidebar: "#0d0d0f",
    card: "#141419",
    accent: "#22d3ee",
    text: "#ffffff",
    muted: "#8a93a6",
  },
  {
    id: "cyber",
    nome: "Cyberspace Oasis",
    bg: "#0a0e1a",
    sidebar: "#0d1220",
    card: "#131a2e",
    accent: "#00e5ff",
    text: "#e6f1ff",
    muted: "#7d8cae",
  },
  {
    id: "dracula",
    nome: "Dracula",
    bg: "#282a36",
    sidebar: "#21222c",
    card: "#44475a",
    accent: "#bd93f9",
    text: "#f8f8f2",
    muted: "#a2a5bb",
  },
  {
    id: "nord",
    nome: "Nord Dark",
    bg: "#2e3440",
    sidebar: "#242933",
    card: "#3b4252",
    accent: "#88c0d0",
    text: "#eceff4",
    muted: "#aab2c5",
  },
  {
    id: "nord-light",
    nome: "Nord Light",
    bg: "#eceff4",
    sidebar: "#e5e9f0",
    card: "#ffffff",
    accent: "#5e81ac",
    text: "#2e3440",
    muted: "#7b88a1",
  },
  {
    id: "gruvbox",
    nome: "Gruvbox Dark",
    bg: "#282828",
    sidebar: "#1d2021",
    card: "#3c3836",
    accent: "#fabd2f",
    text: "#ebdbb2",
    muted: "#bdae93",
  },
  {
    id: "marine",
    nome: "Marine",
    bg: "#0b1c2c",
    sidebar: "#0e2438",
    card: "#123152",
    accent: "#4fc3f7",
    text: "#e3f2fd",
    muted: "#90a4ae",
  },
  {
    id: "zombie",
    nome: "Zombie",
    bg: "#0f1a0f",
    sidebar: "#0c150c",
    card: "#1a2e1a",
    accent: "#76ff03",
    text: "#e8f5e9",
    muted: "#a5c9a5",
  },
  {
    id: "sakura",
    nome: "Sakura",
    bg: "#1a1016",
    sidebar: "#150c12",
    card: "#2a1a26",
    accent: "#f472b6",
    text: "#fce7f3",
    muted: "#c9a4bb",
  },
  {
    id: "sunset",
    nome: "Sunset",
    bg: "#1a120b",
    sidebar: "#150e08",
    card: "#2e2018",
    accent: "#ff7a45",
    text: "#fff4e6",
    muted: "#c9ae94",
  },
  {
    id: "crimson",
    nome: "Crimson",
    bg: "#160b0b",
    sidebar: "#120808",
    card: "#2a1414",
    accent: "#ef4444",
    text: "#fee2e2",
    muted: "#caa0a0",
  },
  {
    id: "oled",
    nome: "High Contrast (OLED)",
    bg: "#000000",
    sidebar: "#000000",
    card: "#0a0a0a",
    accent: "#ffffff",
    text: "#ffffff",
    muted: "#9e9e9e",
  },
  {
    id: "sweet",
    nome: "Sweet",
    bg: "#1b1e2e",
    sidebar: "#161929",
    card: "#252a41",
    accent: "#c792ea",
    text: "#e1e3f5",
    muted: "#9aa0c3",
  },
  {
    id: "old-school",
    nome: "Old School Heroic",
    bg: "#101014",
    sidebar: "#0c0c10",
    card: "#1a1a22",
    accent: "#f5c518",
    text: "#f0ead6",
    muted: "#a39f8d",
  },
]

export const TEMA_PADRAO = "midnight"

export function temaPorId(id?: string): Theme {
  return TEMAS.find((t) => t.id === id) ?? TEMAS.find((t) => t.id === TEMA_PADRAO)!
}
