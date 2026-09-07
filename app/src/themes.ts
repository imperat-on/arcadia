// Tema padrão do Arcadia. A seleção de temas foi reduzida a um único tema
// (Midnight Mirage) a pedido do usuário. O sistema de variáveis e o fallback
// continuam: configs antigas com theme_name de um tema removido caem no
// tema padrão via temaPorId.

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
]

export const TEMA_PADRAO = "midnight"

export function temaPorId(id?: string): Theme {
  return TEMAS.find((t) => t.id === id) ?? TEMAS.find((t) => t.id === TEMA_PADRAO)!
}
