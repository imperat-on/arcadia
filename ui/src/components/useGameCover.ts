import { useCallback, useEffect, useRef, useState } from "react"

/**
 * Capa da biblioteca com fallback real.
 *
 * A URL `library_600x900.jpg` do CDN da Steam dá 404 em jogos novos (a Steam
 * passou a publicar `store_item_assets` com hash). Nesses casos o appdetails
 * ainda devolve `header_image`; esta camada busca uma vez por appid e reaproveita
 * entre sidebar, perfil e biblioteca.
 */
type CoverGame = {
  id?: string
  appid?: string | number
  title?: string
  cover?: string
  icon?: string
}

const headerCache = new Map<string, Promise<string | null>>()

function appidOf(game: CoverGame): string {
  const raw = String(game.appid ?? game.id ?? "")
  if (raw.startsWith("steam:")) return raw.slice(6)
  return /^\d+$/.test(raw) ? raw : ""
}

function resolveHeader(game: CoverGame): Promise<string | null> {
  const appid = appidOf(game)
  if (!appid) return Promise.resolve(null)
  const cached = headerCache.get(appid)
  if (cached) return cached
  const promise = (async () => {
    try {
      const response = await window.launcherAPI?.gameSysinfo?.(game as never)
      const header = response?.info?.header
      return typeof header === "string" && header ? header : null
    } catch {
      return null
    }
  })()
  headerCache.set(appid, promise)
  return promise
}

export function useGameCover(game: CoverGame, preferIcon = false) {
  const appid = appidOf(game)
  const steamPortrait = appid
    ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/library_600x900.jpg`
    : ""
  const steamHeader = appid
    ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`
    : ""
  const deduped = [
    ...new Set(
      [
        preferIcon ? game.icon : game.cover,
        preferIcon ? game.cover : game.icon,
        steamPortrait,
        steamHeader,
      ].filter((value): value is string => Boolean(value)),
    ),
  ]
  const [sources, setSources] = useState<string[]>(deduped)
  const [index, setIndex] = useState(0)
  const [failed, setFailed] = useState(false)
  const asked = useRef(false)

  useEffect(() => {
    setSources(deduped)
    setIndex(0)
    setFailed(false)
    asked.current = false
    // `deduped` é derivado de id/cover/icon; preferIcon não muda em runtime.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id, game.cover, game.icon, preferIcon])

  const src = failed ? undefined : sources[index]

  const onError = useCallback(() => {
    setIndex((current) => {
      if (current + 1 < sources.length) return current + 1
      if (!asked.current) {
        asked.current = true
        void resolveHeader(game).then((header) => {
          if (header && !sources.includes(header)) {
            setSources((previous) => [...previous, header])
            setIndex(sources.length)
          } else {
            setFailed(true)
          }
        })
      } else {
        setFailed(true)
      }
      return current
    })
  }, [game, sources])

  return { src, onError, failed }
}
