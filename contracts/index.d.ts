export const CONTRACT_VERSION: 1
export const LIBRARY_SCHEMA_VERSION: 1
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

export interface ArcadiaLibraryDocument {
  version: 1
  generated_at?: number
  sources?: Record<string, number>
  errors?: string[]
  games: ArcadiaLibrary
}

export interface SafeAccountUser {
  id: string
  email?: string
  username?: string
}

export interface SafeAccountSession {
  user: SafeAccountUser
}

export interface SafeAuthResult {
  ok: boolean
  error?: string
  usernameReal?: string
}

export interface SafeAccountStatus {
  session: SafeAccountSession | null
  error: string | null
}

export interface SafeAccountEvent {
  event: string
  session: SafeAccountSession | null
}

export function safeAccountSession(value: unknown): SafeAccountSession | null
export function safeAuthResult(value: unknown): SafeAuthResult
export function safeAccountStatus(value: unknown): SafeAccountStatus
export function safeAccountEvent(event: unknown, session: unknown): SafeAccountEvent

export interface ArcadiaLibrarySyncItem {
  appid: string
  title?: string
  platform?: "linux" | "windows" | "emulator"
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


export const MAX_SYNC_MINUTES: 999999

export interface SyncAchievementRecord {
  appid: string
  apiname: string
  achieved?: boolean
  unlocked_at?: number | string | null
  unlock?: number | string | null
  [key: string]: unknown
}

export interface SyncLibraryRecord {
  appid: string
  title?: string
  platform?: "linux" | "windows" | "emulator"
  removed?: boolean
  revision?: number
  updated_at?: number | string
  [key: string]: unknown
}

export function normalizeSyncTimestamp(value: unknown): number | null
export function resolveAchievementConflict(
  local: SyncAchievementRecord | null | undefined,
  remote: SyncAchievementRecord | null | undefined,
): SyncAchievementRecord | null
export function resolveLibraryConflict(
  local: SyncLibraryRecord | null | undefined,
  remote: SyncLibraryRecord | null | undefined,
): SyncLibraryRecord | null
export function resolvePlaytimeConflict(local: unknown, remote: unknown): number
export function resolveSyncConflict(
  kind: "achievement" | "library" | "playtime" | string,
  local: unknown,
  remote: unknown,
): SyncAchievementRecord | SyncLibraryRecord | number | null
