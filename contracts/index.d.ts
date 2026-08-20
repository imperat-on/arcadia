export const CONTRACT_VERSION: 1
export const MAX_ID_LENGTH: 512
export const MAX_TITLE_LENGTH: 1024
export const MAX_COMMAND_ARGS: 64
export const MAX_SYNC_ITEMS: 1000

export interface ArcadiaGame {
  id: string
  title: string
  launcher: string
  launch_cmd: string[]
  cover?: string
  hero?: string
  logo?: string
  icon?: string
  installed?: boolean
  description?: string
  genre?: string
  year?: string | number
  rating?: number
  hidden?: boolean
  favorite?: boolean
  categories?: string[]
  last_played?: number
  exe?: string
  temExe?: boolean
  platform?: string
  developer?: string
  publisher?: string
  metacritic?: number
  playtime_minutes?: number
  players?: string
  size?: number
  [key: string]: unknown
}

export type ArcadiaLibrary = ArcadiaGame[]

export interface ArcadiaLibrarySyncItem {
  appid: string
  title?: string
  platform?: "linux" | "windows"
  removed?: boolean
}

export interface ArcadiaPlaytimeItem {
  appid: string
  minutes: number
}

export function normalizeGame(value: unknown): ArcadiaGame | null
export function normalizeLibrary(value: unknown): ArcadiaLibrary
export function normalizeLibrarySyncItem(value: unknown): ArcadiaLibrarySyncItem | null
export function normalizeLibrarySyncItems(value: unknown): ArcadiaLibrarySyncItem[]
export function normalizePlaytimeItem(value: unknown): ArcadiaPlaytimeItem | null
export function normalizePlaytimeItems(value: unknown): ArcadiaPlaytimeItem[]
