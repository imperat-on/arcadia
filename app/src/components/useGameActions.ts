"use client"

import { useCallback, useRef, type Dispatch, type SetStateAction } from "react"
import type { Game } from "./ps5-launcher/types"

export type GameLaunchMode = "steam" | "exe"

export type GameLaunchResult = {
  ok: boolean
  error?: string
  warnings?: string[]
  /** Steam has an alternate native/executable launch path to choose. */
  needsMode?: boolean
}

export interface GameActionsOptions {
  setGames: Dispatch<SetStateAction<Game[]>>
  onChooseLaunch?: (game: Game) => void
  onLaunchWarning?: (game: Game | null, warnings: string[]) => void
  onLaunchError?: (game: Game | null, error: string) => void
  /** Optional host guard used to reject input while another game owns focus. */
  canLaunch?: () => boolean
}

export interface GameActions {
  /** Launches a library game and records the last session when enabled. */
  launch: (game: Game, mode?: GameLaunchMode) => Promise<GameLaunchResult>
  /** Low-level command launch for mode-specific installation fallbacks. */
  launchCommand: (
    command: string[],
    gameId?: string,
    mode?: GameLaunchMode,
  ) => Promise<GameLaunchResult>
  saveOverride: (
    gameId: string,
    patch: Record<string, unknown> | null,
  ) => Promise<Game[] | undefined>
  saveMetadata: (game: Game, patch: Record<string, unknown>) => Promise<Game[] | undefined>
  toggleFavorite: (game: Game) => Promise<Game[] | undefined>
  toggleHidden: (game: Game) => Promise<Game[] | undefined>
  refresh: () => Promise<Game[]>
}

/**
 * Actions that must behave identically in the desktop and console UIs.
 *
 * The hook deliberately receives the library setter instead of owning a
 * second library state. This keeps the state hook responsible for loading and
 * subscriptions while every mutation goes through one implementation.
 */
export function useGameActions({
  setGames,
  onChooseLaunch,
  onLaunchWarning,
  onLaunchError,
  canLaunch,
}: GameActionsOptions): GameActions {
  const callbacks = useRef({ onChooseLaunch, onLaunchWarning, onLaunchError })
  callbacks.current = { onChooseLaunch, onLaunchWarning, onLaunchError }
  const canLaunchRef = useRef(canLaunch)
  canLaunchRef.current = canLaunch
  const launchBlocked = () =>
    canLaunchRef.current ? !canLaunchRef.current() : false

  const applyLibrary = useCallback(
    (value: unknown) => {
      if (!Array.isArray(value)) return undefined
      const library = value as Game[]
      setGames(library)
      return library
    },
    [setGames],
  )

  const saveOverride = useCallback(
    async (gameId: string, patch: Record<string, unknown> | null) => {
      if (patch && Object.keys(patch).length === 0) return undefined
      const api = window.launcherAPI
      if (!api?.setOverride) {
        // Browser/dev fallback: keep the mock library usable without Electron.
        if (patch) {
          setGames((current) =>
            current.map((game) => {
              if (game.id !== gameId) return game
              const next = { ...game } as Record<string, unknown>
              for (const [key, value] of Object.entries(patch)) {
                if (value === null) delete next[key]
                else next[key] = value
              }
              return next as Game
            }),
          )
        }
        return undefined
      }
      try {
        const library = await api.setOverride(gameId, patch)
        return applyLibrary(library)
      } catch {
        // A failed metadata write must not create an unhandled rejection in
        // fire-and-forget UI actions. The durable library remains untouched.
        return undefined
      }
    },
    [applyLibrary, setGames],
  )

  const recordPlaytime = useCallback(
    async (game: Game) => {
      try {
        const config = await window.launcherAPI?.getConfig()
        if (config?.disable_playtime_tracking === true) return
        await saveOverride(game.id, { last_played: Date.now() })
      } catch {
        // A launch that succeeded must not be reported as failed because a
        // local preference or metadata write was unavailable.
      }
    },
    [saveOverride],
  )

  const executeLaunch = useCallback(
    async (
      command: string[],
      gameId?: string,
      mode?: GameLaunchMode,
      game: Game | null = null,
    ): Promise<GameLaunchResult> => {
      let result: GameLaunchResult
      try {
        result =
          (await window.launcherAPI?.launch(command, gameId, mode)) || {
            ok: false,
            error: "A ponte do launcher não está disponível.",
          }
      } catch (error) {
        result = { ok: false, error: String(error instanceof Error ? error.message : error) }
      }

      const warnings = Array.isArray(result.warnings) ? result.warnings.filter(Boolean) : []
      if (warnings.length) callbacks.current.onLaunchWarning?.(game, warnings)
      if (result.ok === false && result.error) callbacks.current.onLaunchError?.(game, result.error)
      return { ...result, warnings }
    },
    [],
  )

  const launchCommand = useCallback(
    (command: string[], gameId?: string, mode?: GameLaunchMode) => {
      if (launchBlocked())
        return Promise.resolve({ ok: false, error: "O launcher está ocupado com outro jogo." })
      return executeLaunch(command, gameId, mode)
    },
    [executeLaunch],
  )

  const launch = useCallback(
    async (game: Game, mode?: GameLaunchMode) => {
      if (launchBlocked()) return { ok: false, error: "O launcher está ocupado com outro jogo." }
      if (mode === undefined && game.launcher === "steam" && game.temExe) {
        callbacks.current.onChooseLaunch?.(game)
        return { ok: false, needsMode: true, warnings: [] }
      }
      const result = await executeLaunch(game.launch_cmd, game.id, mode, game)
      if (result.ok) await recordPlaytime(game)
      return result
    },
    [executeLaunch, recordPlaytime],
  )

  const saveMetadata = useCallback(
    (game: Game, patch: Record<string, unknown>) => saveOverride(game.id, patch),
    [saveOverride],
  )

  const toggleFavorite = useCallback(
    (game: Game) => saveOverride(game.id, { favorite: game.favorite ? null : true }),
    [saveOverride],
  )

  const toggleHidden = useCallback(
    (game: Game) => saveOverride(game.id, { hidden: game.hidden ? null : true }),
    [saveOverride],
  )

  const refresh = useCallback(async () => {
    const api = window.launcherAPI
    try {
      const library = await api?.refresh()
      if (Array.isArray(library)) return applyLibrary(library) || []
    } catch {
      // Fall through to the last durable library snapshot.
    }
    try {
      const library = await api?.getLibrary()
      return applyLibrary(library) || []
    } catch {
      return []
    }
  }, [applyLibrary])

  return {
    launch,
    launchCommand,
    saveOverride,
    saveMetadata,
    toggleFavorite,
    toggleHidden,
    refresh,
  }
}
