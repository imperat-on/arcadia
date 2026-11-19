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
const retroCache = new Map<string, Promise<string | null>>()

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

/**
 * Jogo retrô da biblioteca só tem o id local; a capa real vem do catálogo do
 * servidor (`retro:game`), que resolve Launchbox/libretro por `providerId`.
 */
function resolveRetroCover(id: string): Promise<string | null> {
  const cached = retroCache.get(id)
  if (cached) return cached
  const promise = (async () => {
    try {
      const response = await window.launcherAPI?.retroGame?.(id)
      const game = response?.game
      const cover = game?.cover || game?.capa || game?.fallbackCover
      return typeof cover === "string" && cover ? cover : null
    } catch {
      return null
    }
  })()
  retroCache.set(id, promise)
  return promise
}

function resolveExtraCover(game: CoverGame): Promise<string | null> {
  const id = String(game.id ?? "")
  if (id.startsWith("retro:")) return resolveRetroCover(id)
  return resolveHeader(game)
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

  // Sem nenhuma fonte inicial (retrô adicionado pela biblioteca), busca a arte
  // canônica uma vez em vez de renderizar o placeholder para sempre.
  useEffect(() => {
    if (deduped.length > 0) return
    let alive = true
    void resolveExtraCover(game).then((cover) => {
      if (!alive) return
      if (cover) setSources([cover])
      else setFailed(true)
    })
    return () => {
      alive = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id, game.cover, game.icon, preferIcon])

  const onError = useCallback(() => {
    setIndex((current) => {
      if (current + 1 < sources.length) return current + 1
      if (!asked.current) {
        asked.current = true
        void resolveExtraCover(game).then((cover) => {
          if (cover && !sources.includes(cover)) {
            setSources((previous) => [...previous, cover])
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
