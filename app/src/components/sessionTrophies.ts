// TypeScript · sessionTrophies.ts · renderer · session ownership
import type { TrophyUnlock } from "../global"

export function addSessionTrophy(items: Map<string, TrophyUnlock>, gameId: string, start: number, end: number, trophy: TrophyUnlock) {
  const unlock = trophy.unlock ?? 0
  if (!trophy.notice || `steam:${trophy.appid}` !== gameId || !Number.isFinite(unlock) || unlock <= 0 || unlock < start || unlock > end) return false
  const key = (trophy.apiname || trophy.key).toLowerCase()
  if (!key || items.has(key)) return false
  items.set(key, trophy)
  return true
}
