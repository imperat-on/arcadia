// Tipos públicos da API visual de temas Fullscreen do Arcadia.
//
// Estes tipos espelham o contrato do processo principal (app/electron/themes/)
// e são a superfície estável que autores de tema usam. Nenhum campo aqui contém
// caminhos locais, tokens de acesso ou qualquer dado privado da máquina.

/** Compatibilidade da API do tema com a do host. */
export type ThemeApiCompat = "ok" | "lower" | "higher"

/** Tipo de uma opção declarativa de tema. */
export type ThemeOptionType = "boolean" | "enum" | "number" | "color" | "intensity"

export interface ThemeOption {
  type: ThemeOptionType
  default?: boolean | number | string
  min?: number
  max?: number
  values?: string[]
}

/** Manifesto público de um tema, como devolvido pelo main. */
export interface FullscreenThemeManifest {
  manifestVersion: number
  themeApiVersion: number
  id: string
  name: string
  author: string
  version: string
  description: string
  mode: "fullscreen"
  entry: string
  layouts: Record<string, string>
  previews: string[]
  features: string[]
  options: Record<string, ThemeOption>
  supports: {
    minWidth?: number
    minHeight?: number
    aspectRatios?: string[]
    [k: string]: unknown
  }
  homepage: string
  license: string
  compat: ThemeApiCompat
}

/** Estado de ativação/compatibilidade de um tema na listagem. */
export type FullscreenThemeState = "valid" | "invalid" | "incompatible" | "active" | "missing"

/** Descritor de um tema exibido na galeria/configurações. */
export interface FullscreenThemeInfo {
  id: string
  manifest: FullscreenThemeManifest | null
  source: "builtin" | "local"
  installed: boolean
  valid: boolean
  error: string
  state: FullscreenThemeState
  options: Record<string, boolean | number | string>
  active: boolean
}

/** Conteúdo normalizado de um tema entregue ao renderer para aplicação. */
export interface FullscreenThemePayload {
  id: string
  name: string
  themeApiVersion: number
  /** CSS já escopado/normalizado, com URLs reescritas para arcadia-theme://. */
  css: string
  /** Erros de normalização do CSS (não fatais). */
  cssErrors: string[]
  /** Layout declarativo por superfície (vazio = layout padrão do host). */
  layouts: Record<string, FullscreenLayout>
  /** Opções resolvidas (defaults do tema mesclados com preferências do user). */
  options: Record<string, boolean | number | string>
  /** URLs de preview normalizadas (arcadia-theme://). */
  previews: string[]
  compat: ThemeApiCompat
  source: "builtin" | "local"
}

export interface FullscreenGridArea {
  row: number
  col: number
  rowSpan: number
  colSpan: number
  slot: string | null
}

export interface FullscreenGridSpec {
  columns: string[]
  rows: string[]
  areas: string[][]
}

export interface FullscreenLayoutSlot {
  area: string
  required: boolean
}

/** Layout declarativo validado (nunca contém HTML/JSX/scripts). */
export interface FullscreenLayout {
  schemaVersion: number
  surface: "home" | "overview"
  grid: FullscreenGridSpec
  slots: Record<string, FullscreenLayoutSlot>
}

/** Slot angular (componente do host) passado ao renderer de layout. */
export type FullscreenSlotMap = Record<string, React.ReactNode>

/** Estados que o host anexa aos itens de biblioteca (data-theme-state). */
export type FullscreenGameState =
  | "selected"
  | "running"
  | "opening"
  | "installed"
  | "missing"
  | "favorite"
  | "active"
  | "disabled"

/** Ações que o host implementa e o tema apenas estiliza (data-theme-action). */
export type FullscreenThemeAction =
  | "launch"
  | "stop"
  | "install"
  | "details"
  | "back"
  | "favorite"
  | "settings"
  | "downloads"