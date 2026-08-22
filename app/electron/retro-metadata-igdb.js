"use strict"

/**
 * IGDB Metadata Provider
 *
 * Busca metadados ricos de jogos PS2/PS3 usando a API IGDB (Twitch).
 * Requer Client ID e Client Secret (gratuitos).
 *
 * Rate limit: 4 requests/segundo
 * Docs: https://api-docs.igdb.com/
 */

const https = require("https")
const crypto = require("crypto")

// Mapeamento de systemId para platform ID do IGDB
const PLATFORM_MAP = {
  "sony-playstation": 7,      // PlayStation 1
  "sony-playstation-2": 8,    // PlayStation 2
  "sony-playstation-3": 9,    // PlayStation 3
  "sony-psp": 38,             // PSP
}

// Cache de token OAuth2
let cachedToken = null
let tokenExpiry = 0

/**
 * Obtém token de acesso OAuth2 do IGDB.
 * @param {string} clientId - Client ID do Twitch
 * @param {string} clientSecret - Client Secret do Twitch
 * @returns {Promise<string>} - Access token
 */
async function getAccessToken(clientId, clientSecret) {
  const now = Date.now()

  // Reusar token cacheado se ainda válido
  if (cachedToken && tokenExpiry > now + 60000) {
    return cachedToken
  }

  return new Promise((resolve, reject) => {
    const postData = new URLSearchParams({
      client_id: clientId,
      client_secret: clientSecret,
      grant_type: "client_credentials",
    }).toString()

    const options = {
      hostname: "id.twitch.tv",
      path: "/oauth2/token",
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        "Content-Length": Buffer.byteLength(postData),
      },
    }

    const req = https.request(options, (res) => {
      let data = ""
      res.on("data", chunk => data += chunk)
      res.on("end", () => {
        try {
          const result = JSON.parse(data)
          if (result.access_token) {
            cachedToken = result.access_token
            tokenExpiry = now + (result.expires_in * 1000)
            resolve(cachedToken)
          } else {
            reject(new Error("No access_token in response"))
          }
        } catch (err) {
          reject(err)
        }
      })
    })

    req.on("error", reject)
    req.write(postData)
    req.end()
  })
}

/**
 * Faz request para a API IGDB.
 * @param {string} endpoint - Endpoint (ex: "games")
 * @param {string} body - Query Apicalypse
 * @param {string} clientId - Client ID
 * @param {string} accessToken - Access token
 * @returns {Promise<object[]>} - Resultado da query
 */
async function igdbRequest(endpoint, body, clientId, accessToken) {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: "api.igdb.com",
      path: `/v4/${endpoint}`,
      method: "POST",
      headers: {
        "Client-ID": clientId,
        "Authorization": `Bearer ${accessToken}`,
        "Content-Type": "text/plain",
        "Content-Length": Buffer.byteLength(body),
      },
    }

    const req = https.request(options, (res) => {
      let data = ""
      res.on("data", chunk => data += chunk)
      res.on("end", () => {
        try {
          const result = JSON.parse(data)
          resolve(result)
        } catch (err) {
          reject(new Error(`Failed to parse IGDB response: ${err.message}`))
        }
      })
    })

    req.on("error", reject)
    req.write(body)
    req.end()
  })
}

/**
 * Busca jogo por serial PlayStation.
 * @param {string} serial - Serial do jogo (ex: "SLUS-20312")
 * @param {string} systemId - ID do sistema
 * @param {object} credentials - { clientId, clientSecret }
 * @returns {Promise<object|null>} - Metadados do jogo
 */
async function searchBySerial(serial, systemId, credentials) {
  try {
    const token = await getAccessToken(credentials.clientId, credentials.clientSecret)
    const platformId = PLATFORM_MAP[systemId]

    if (!platformId) {
      return null
    }

    // Normalizar serial (remover traços)
    const normalizedSerial = serial.replace(/[-_]/g, "")

    // Query Apicalypse
    const query = `
      fields name, summary, genres.name, involved_companies.company.name,
             involved_companies.developer, involved_companies.publisher,
             first_release_date, cover.url, cover.image_id,
             artworks.url, artworks.image_id, screenshots.url, screenshots.image_id,
             aggregated_rating, aggregated_rating_count, rating, rating_count,
             storyline, game_modes.name, player_perspectives.name, themes.name;
      where platforms = ${platformId} & (version_title ~ "${normalizedSerial}"*
            | alternative_names.name ~ "*${normalizedSerial}*");
      limit 5;
    `

    const results = await igdbRequest("games", query.trim(), credentials.clientId, token)

    if (!results || results.length === 0) {
      return null
    }

    return normalizeIgdbGame(results[0])
  } catch (error) {
    console.error(`[igdb] Error searching by serial ${serial}:`, error)
    return null
  }
}

/**
 * Busca jogo por título.
 * @param {string} title - Título do jogo
 * @param {string} systemId - ID do sistema
 * @param {object} credentials - { clientId, clientSecret }
 * @returns {Promise<object|null>} - Metadados do jogo
 */
async function searchByTitle(title, systemId, credentials) {
  try {
    const token = await getAccessToken(credentials.clientId, credentials.clientSecret)
    const platformId = PLATFORM_MAP[systemId]

    if (!platformId) {
      return null
    }

    // Escapar aspas no título
    const safeTitle = title.replace(/"/g, '\\"')

    const query = `
      fields name, summary, genres.name, involved_companies.company.name,
             involved_companies.developer, involved_companies.publisher,
             first_release_date, cover.url, cover.image_id,
             artworks.url, artworks.image_id, screenshots.url, screenshots.image_id,
             aggregated_rating, aggregated_rating_count, rating, rating_count,
             storyline, game_modes.name, player_perspectives.name, themes.name;
      search "${safeTitle}";
      where platforms = ${platformId};
      limit 3;
    `

    const results = await igdbRequest("games", query.trim(), credentials.clientId, token)

    if (!results || results.length === 0) {
      return null
    }

    // Retornar o melhor match (primeiro resultado)
    return normalizeIgdbGame(results[0])
  } catch (error) {
    console.error(`[igdb] Error searching by title "${title}":`, error)
    return null
  }
}

/**
 * Normaliza resposta da IGDB para o formato do Arcadia.
 * @param {object} igdbGame - Jogo da IGDB
 * @returns {object} - Metadados normalizados
 */
function normalizeIgdbGame(igdbGame) {
  const metadata = {
    title: igdbGame.name,
    summary: igdbGame.summary || igdbGame.storyline || null,
    releaseDate: igdbGame.first_release_date
      ? new Date(igdbGame.first_release_date * 1000).toISOString().split('T')[0]
      : null,
    genres: igdbGame.genres?.map(g => g.name) || [],
    themes: igdbGame.themes?.map(t => t.name) || [],
    gameModes: igdbGame.game_modes?.map(m => m.name) || [],
    perspectives: igdbGame.player_perspectives?.map(p => p.name) || [],
    rating: igdbGame.aggregated_rating || igdbGame.rating || null,
    ratingCount: igdbGame.aggregated_rating_count || igdbGame.rating_count || 0,
    developer: null,
    publisher: null,
    artwork: {},
  }

  // Extrair desenvolvedora e publisher
  if (igdbGame.involved_companies) {
    for (const ic of igdbGame.involved_companies) {
      if (ic.developer && ic.company?.name) {
        metadata.developer = ic.company.name
      }
      if (ic.publisher && ic.company?.name) {
        metadata.publisher = ic.company.name
      }
    }
  }

  // Artwork URLs (IGDB usa image_id, precisa construir URL)
  if (igdbGame.cover?.image_id) {
    metadata.artwork.cover = `https://images.igdb.com/igdb/image/upload/t_cover_big/${igdbGame.cover.image_id}.jpg`
    metadata.artwork.coverHD = `https://images.igdb.com/igdb/image/upload/t_1080p/${igdbGame.cover.image_id}.jpg`
  }

  if (igdbGame.artworks?.length > 0) {
    metadata.artwork.artworks = igdbGame.artworks.map(a =>
      `https://images.igdb.com/igdb/image/upload/t_1080p/${a.image_id}.jpg`
    )
  }

  if (igdbGame.screenshots?.length > 0) {
    metadata.artwork.screenshots = igdbGame.screenshots.map(s =>
      `https://images.igdb.com/igdb/image/upload/t_screenshot_big/${s.image_id}.jpg`
    )
  }

  return metadata
}

/**
 * Gera hash de cache para credenciais.
 * @param {object} credentials - { clientId, clientSecret }
 * @returns {string} - Hash SHA-256
 */
function getCredentialsHash(credentials) {
  const data = `${credentials.clientId}:${credentials.clientSecret}`
  return crypto.createHash("sha256").update(data).digest("hex").substring(0, 16)
}

module.exports = {
  searchBySerial,
  searchByTitle,
  getCredentialsHash,
  PLATFORM_MAP,
}
