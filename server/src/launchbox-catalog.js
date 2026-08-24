"use strict"

const fs = require("node:fs")
const path = require("node:path")
const { Readable } = require("node:stream")
const { pipeline } = require("node:stream/promises")
const sax = require("sax")
const unzipper = require("unzipper")

const LAUNCHBOX_METADATA_URL = "https://gamesdb.launchbox-app.com/Metadata.zip"
const IMAGE_BASE_URL = "https://images.launchbox-app.com/"
const INDEX_VERSION = 1
const REFRESH_MS = 24 * 60 * 60 * 1000
const MAX_DOWNLOAD_BYTES = 256 * 1024 * 1024
const MAX_METADATA_XML_BYTES = 768 * 1024 * 1024

const PLATFORM_TO_SYSTEM = Object.freeze({
  "Sony Playstation": "sony-playstation",
  "Sony Playstation 2": "sony-playstation-2",
  "Sony Playstation 3": "sony-playstation-3",
  "Sony PSP": "sony-psp",
  "Nintendo GameCube": "nintendo-gamecube",
  "Nintendo Wii": "nintendo-wii",
  "Nintendo DS": "nintendo-ds",
  "Nintendo DSi": "nintendo-dsi",
  "Nintendo Entertainment System": "nintendo-nes",
  "Super Nintendo Entertainment System": "nintendo-snes",
  "Nintendo Game Boy": "nintendo-game-boy",
  "Nintendo Game Boy Color": "nintendo-game-boy-color",
  "Nintendo Game Boy Advance": "nintendo-game-boy-advance",
  "Nintendo 64": "nintendo-64",
})

function englishTitle(value) {
  const title = String(value || "").trim()
  return Boolean(title && /[A-Za-z]/.test(title) && !/[\p{Script=Cyrillic}\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(title))
}

function imagePriority(type, region) {
  if (type !== "Box - Front") return -1
  const normalized = String(region || "").toLowerCase()
  if (normalized === "world") return 60
  if (normalized === "north america") return 55
  if (normalized === "united states") return 54
  if (normalized === "europe") return 50
  if (!normalized) return 45
  if (["united kingdom", "australia", "canada"].includes(normalized)) return 40
  return 20
}

function imageKind(type) {
  const value = String(type || "").toLowerCase()
  if (value.includes("screenshot") || value.includes("gameplay")) return "screenshots"
  if (value.includes("title screen") || value.includes("title-screen")) return "titleScreens"
  if (value.includes("fanart") || value.includes("background") || value.includes("banner")) return "backgrounds"
  if (value.includes("logo")) return "logos"
  if (value.includes("box - back")) return "backCover"
  if (value === "box - front") return "cover"
  return "other"
}

function imageUrl(fileName) {
  const value = String(fileName || "").trim()
  if (!/^[a-zA-Z0-9_-]+\.(?:avif|gif|jpe?g|png|webp)$/i.test(value)) return ""
  return `${IMAGE_BASE_URL}${encodeURIComponent(value)}`
}

function acceptedReleaseType(value) {
  const type = String(value || "").trim().toLowerCase()
  return !type || ["released", "homebrew", "unreleased"].includes(type)
}

function parseLaunchboxMetadata(input, { matchKey }) {
  if (typeof matchKey !== "function") throw new TypeError("matchKey is required")

  return new Promise((resolve, reject) => {
    const games = new Map()
    const aliases = new Map()
    const artwork = new Map()
    let entity = null
    let current = null
    let field = ""
    let text = ""
    let settled = false

    const parser = sax.createStream(true, { trim: false, normalize: false })
    const finishError = (error) => {
      if (settled) return
      settled = true
      reject(error)
    }

    parser.on("opentag", ({ name }) => {
      if (name === "Game" || name === "GameAlternateName" || name === "GameImage") {
        entity = name
        current = {}
      }
      field = name
      text = ""
    })
    parser.on("text", (value) => {
      if (current && ["Name", "Platform", "DatabaseID", "ReleaseType", "ReleaseYear", "AlternateName", "Region", "FileName", "Type", "Overview", "Description", "Developer", "Publisher", "Genre", "Genres", "Series", "PlayMode", "MaxPlayers", "Rating", "ESRB"].includes(field)) text += value
    })
    parser.on("cdata", (value) => {
      if (current) text += value
    })
    parser.on("closetag", (name) => {
      if (current && field === name && text) {
        const value = text.trim()
        if (["Genre", "Genres", "Series", "Developer", "Publisher"].includes(name)) {
          const values = Array.isArray(current[name]) ? current[name] : current[name] ? [current[name]] : []
          values.push(value)
          current[name] = values
        } else current[name] = value
      }

      if (name === entity && current) {
        if (entity === "Game") {
          const systemId = PLATFORM_TO_SYSTEM[current.Platform]
          if (systemId && current.DatabaseID && englishTitle(current.Name) && acceptedReleaseType(current.ReleaseType)) {
            const list = (value) => [...new Set((Array.isArray(value) ? value : [value]).flatMap((item) => String(item || "").split(/[,;|]/).map((part) => part.trim()).filter(Boolean)))].slice(0, 20)
            games.set(String(current.DatabaseID), {
              id: String(current.DatabaseID),
              systemId,
              title: current.Name.trim(),
              releaseYear: Number(current.ReleaseYear) || null,
              description: String(current.Overview || current.Description || "").trim().slice(0, 4000),
              developer: list(current.Developer),
              publisher: list(current.Publisher),
              genres: list(current.Genre || current.Genres),
              series: list(current.Series),
              playMode: list(current.PlayMode),
              maxPlayers: String(current.MaxPlayers || "").trim().slice(0, 40),
            })
          }
        } else if (entity === "GameAlternateName" && current.DatabaseID && englishTitle(current.AlternateName)) {
          const list = aliases.get(String(current.DatabaseID)) || []
          list.push(current.AlternateName.trim())
          aliases.set(String(current.DatabaseID), list)
        } else if (entity === "GameImage" && current.DatabaseID) {
          const url = imageUrl(current.FileName)
          const kind = imageKind(current.Type)
          const priority = imagePriority(current.Type, current.Region)
          const id = String(current.DatabaseID)
          const previous = artwork.get(id) || { images: {} }
          if (url && (kind === "cover" ? priority >= 0 && (!previous.cover || priority > previous.cover.priority) : true)) {
            if (kind === "cover") previous.cover = { url, priority }
            else {
              const list = previous.images[kind] || []
              if (!list.some((item) => item.url === url)) list.push({ url, priority })
              list.sort((a, b) => b.priority - a.priority)
              previous.images[kind] = list.slice(0, kind === "screenshots" || kind === "backgrounds" ? 8 : 2)
            }
            artwork.set(id, previous)
          }
        }
        entity = null
        current = null
      }
      field = ""
      text = ""
    })
    parser.on("error", finishError)
    parser.on("end", () => {
      if (settled) return
      const gamesBySystem = new Map()
      let aliasCount = 0
      let artworkCount = 0
      for (const game of games.values()) {
        const names = [game.title, ...(aliases.get(game.id) || [])]
        const record = artwork.get(game.id) || {}
        const cover = record.cover?.url || ""
        if (cover) artworkCount++
        const imageGroups = record.images || {}
        const candidate = {
          provider: "launchbox", providerId: game.id, title: game.title, cover, releaseYear: game.releaseYear,
          artwork: {
            provider: "launchbox", providerId: game.id, cover,
            screenshots: (imageGroups.screenshots || []).map((item) => item.url),
            backgrounds: (imageGroups.backgrounds || []).map((item) => item.url),
            titleScreens: (imageGroups.titleScreens || []).map((item) => item.url),
            logos: (imageGroups.logos || []).map((item) => item.url),
            backCover: imageGroups.backCover?.[0]?.url || "",
            description: game.description,
            releaseYear: game.releaseYear,
            developer: game.developer,
            publisher: game.publisher,
            genres: game.genres,
            series: game.series,
            playMode: game.playMode,
            maxPlayers: game.maxPlayers,
          },
        }
        if (!gamesBySystem.has(game.systemId)) gamesBySystem.set(game.systemId, new Map())
        const index = gamesBySystem.get(game.systemId)
        for (const name of names) {
          const key = matchKey(name)
          if (!key) continue
          const previous = index.get(key)
          if (!previous) index.set(key, candidate)
          else if (previous.providerId !== candidate.providerId) index.set(key, null)
          aliasCount++
        }
      }
      for (const index of gamesBySystem.values()) {
        for (const [key, candidate] of index) {
          if (!candidate) index.delete(key)
        }
      }
      settled = true
      resolve({
        gamesBySystem,
        stats: { games: games.size, aliases: aliasCount, artwork: artworkCount },
      })
    })

    input.on("error", finishError)
    input.pipe(parser)
  })
}

function serializeIndex(result) {
  const systems = {}
  for (const [systemId, lookup] of result.gamesBySystem) {
    const entries = []
    const gamePositions = new Map()
    const games = []
    for (const [key, candidate] of lookup) {
      const candidateKey = candidate.providerId
      let position = gamePositions.get(candidateKey)
      if (position === undefined) {
        position = games.length
        games.push(candidate)
        gamePositions.set(candidateKey, position)
      }
      entries.push([key, position])
    }
    systems[systemId] = { games, entries }
  }
  return { version: INDEX_VERSION, generatedAt: new Date().toISOString(), stats: result.stats, systems }
}

function deserializeIndex(payload) {
  if (!payload || payload.version !== INDEX_VERSION || !payload.systems) throw new Error("launchbox_index_invalid")
  const gamesBySystem = new Map()
  for (const [systemId, data] of Object.entries(payload.systems)) {
    const games = Array.isArray(data.games) ? data.games : []
    gamesBySystem.set(systemId, new Map((data.entries || []).map(([key, position]) => [key, games[position]]).filter(([, game]) => game)))
  }
  return { gamesBySystem, stats: payload.stats || {}, generatedAt: payload.generatedAt }
}

async function writeJsonAtomic(filePath, value) {
  const temporary = `${filePath}.${process.pid}.${Date.now()}.tmp`
  await fs.promises.writeFile(temporary, JSON.stringify(value), { mode: 0o600 })
  await fs.promises.rename(temporary, filePath)
}

async function downloadMetadata(zipPath, state, fetchImpl) {
  const headers = { accept: "application/zip", "user-agent": "Arcadia-Retro/3" }
  if (state.etag) headers["if-none-match"] = state.etag
  if (state.lastModified) headers["if-modified-since"] = state.lastModified
  const response = await fetchImpl(LAUNCHBOX_METADATA_URL, { headers, redirect: "error", signal: AbortSignal.timeout(180_000) })
  if (response.status === 304 && fs.existsSync(zipPath)) return { changed: false, state: { ...state, checkedAt: Date.now() } }
  if (!response.ok || !response.body) throw new Error(`launchbox_metadata_http_${response.status}`)
  const length = Number(response.headers.get("content-length") || 0)
  if (length && length > MAX_DOWNLOAD_BYTES) throw new Error("launchbox_metadata_too_large")

  const temporary = `${zipPath}.${process.pid}.${Date.now()}.part`
  let received = 0
  const source = Readable.fromWeb(response.body)
  source.on("data", (chunk) => {
    received += chunk.length
    if (received > MAX_DOWNLOAD_BYTES) source.destroy(new Error("launchbox_metadata_too_large"))
  })
  try {
    await pipeline(source, fs.createWriteStream(temporary, { mode: 0o600 }))
    const archive = await unzipper.Open.file(temporary)
    const metadata = archive.files.find((entry) => entry.path === "Metadata.xml" && entry.type === "File")
    if (!metadata) throw new Error("launchbox_metadata_xml_missing")
    if (Number(metadata.uncompressedSize || 0) > MAX_METADATA_XML_BYTES) throw new Error("launchbox_metadata_xml_too_large")
    await fs.promises.rename(temporary, zipPath)
  } catch (error) {
    await fs.promises.unlink(temporary).catch(() => {})
    throw error
  }
  return {
    changed: true,
    state: {
      etag: response.headers.get("etag") || "",
      lastModified: response.headers.get("last-modified") || "",
      checkedAt: Date.now(),
      bytes: received,
    },
  }
}

async function parseZip(zipPath, matchKey) {
  const archive = await unzipper.Open.file(zipPath)
  const metadata = archive.files.find((entry) => entry.path === "Metadata.xml" && entry.type === "File")
  if (!metadata) throw new Error("launchbox_metadata_xml_missing")
  if (Number(metadata.uncompressedSize || 0) > MAX_METADATA_XML_BYTES) throw new Error("launchbox_metadata_xml_too_large")
  return parseLaunchboxMetadata(metadata.stream(), { matchKey })
}

async function loadLaunchboxIndex({ cacheDir, matchKey, fetchImpl = fetch, force = false }) {
  await fs.promises.mkdir(cacheDir, { recursive: true, mode: 0o700 })
  const zipPath = path.join(cacheDir, "Metadata.zip")
  const indexPath = path.join(cacheDir, "index-v1.json")
  const statePath = path.join(cacheDir, "state-v1.json")
  let state = {}
  try { state = JSON.parse(await fs.promises.readFile(statePath, "utf8")) } catch {}

  const fresh = !force && fs.existsSync(indexPath) && Date.now() - Number(state.checkedAt || 0) < REFRESH_MS
  if (fresh) return deserializeIndex(JSON.parse(await fs.promises.readFile(indexPath, "utf8")))

  let download
  try {
    download = await downloadMetadata(zipPath, state, fetchImpl)
    state = download.state
    await writeJsonAtomic(statePath, state)
  } catch (error) {
    if (fs.existsSync(indexPath)) return { ...deserializeIndex(JSON.parse(await fs.promises.readFile(indexPath, "utf8"))), stale: true, error: error.message }
    throw error
  }

  if (!download.changed && fs.existsSync(indexPath)) return deserializeIndex(JSON.parse(await fs.promises.readFile(indexPath, "utf8")))
  const parsed = await parseZip(zipPath, matchKey)
  const serialized = serializeIndex(parsed)
  await writeJsonAtomic(indexPath, serialized)
  return deserializeIndex(serialized)
}

module.exports = {
  LAUNCHBOX_METADATA_URL,
  IMAGE_BASE_URL,
  PLATFORM_TO_SYSTEM,
  englishTitle,
  imagePriority,
  imageUrl,
  parseLaunchboxMetadata,
  serializeIndex,
  deserializeIndex,
  loadLaunchboxIndex,
}
