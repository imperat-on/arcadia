"use strict"

// Cliente HTTP para a RetroAchievements. Este módulo não conhece Electron:
// recebe um `fetchImpl` injetável (padrão do resto do projeto) e devolve
// apenas dados normalizados. Nunca loga usuário/senha/token.
//
// Dois protocolos distintos são usados aqui, de propósito:
//  - API pública (retroachievements.org/API/API_*.php): consultas de dados,
//    autenticadas pela Web API Key (obtida em controlpanel.php). Usada para
//    validar a conta e (no futuro) buscar progresso/achievements de um jogo.
//  - Protocolo interno dos clients rcheevos (dorequest.php, r=login2): troca
//    usuário+senha por um token de sessão. É o mesmo endpoint que PCSX2,
//    DuckStation, PPSSPP e RetroArch usam por baixo dos panos — não tem doc
//    pública versionada, mas é estável há anos. O token resultante é o que
//    entra nos arquivos de config desses emuladores (nunca a senha, nunca a
//    Web API Key).

const DEFAULT_HOST = "https://retroachievements.org"
const REQUEST_TIMEOUT_MS = 15000
// O servidor da RetroAchievements devolve 403 (nginx) para requisições sem um
// User-Agent reconhecível — o fetch padrão do Node/Electron não envia nada
// por padrão. Formato recomendado pelo guia de integração do rcheevos:
// "<produto>/<versão> (<info-sistema>) <extensões>".
const USER_AGENT = "Arcadia/1.0 (Electron)"
// Domínio de mídia da RA (badges de achievement, avatares). Confirmado por
// requisição real: /Badge/<badgeName>.png (desbloqueada) e _lock.png (bloqueada).
const MEDIA_HOST = "https://media.retroachievements.org"

function defaultFetch(...args) {
  return fetch(...args)
}

function badgeUrl(badgeName, locked) {
  if (!badgeName) return ""
  return `${MEDIA_HOST}/Badge/${badgeName}${locked ? "_lock" : ""}.png`
}

// A API pública devolve erro em formatos diferentes dependendo do caso:
// {"Error": "..."} (endpoints de dados) ou {"message": "..."} (401 do
// middleware de autenticação do Laravel, ex.: Web API Key inválida).
function extractApiError(data) {
  return data?.Error || data?.message || ""
}

function withTimeout(ms) {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), ms)
  return { controller, cleanup: () => clearTimeout(timer) }
}

/**
 * Troca usuário+senha por um token de sessão via o protocolo interno usado
 * pelos emuladores (dorequest.php, r=login2). A senha nunca é persistida por
 * este módulo; o chamador deve descartá-la da memória assim que possível.
 */
async function loginRequest({ username, password, host = DEFAULT_HOST, fetchImpl = defaultFetch } = {}) {
  const user = String(username || "").trim()
  const pass = String(password || "")
  if (!user || !pass) return { ok: false, error: "credenciais_vazias" }

  const { controller, cleanup } = withTimeout(REQUEST_TIMEOUT_MS)
  try {
    const response = await fetchImpl(`${host}/dorequest.php`, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded", "User-Agent": USER_AGENT },
      body: new URLSearchParams({ r: "login2", u: user, p: pass }).toString(),
      signal: controller.signal,
    })
    const text = await response.text()
    let data
    try {
      data = JSON.parse(text)
    } catch {
      return { ok: false, error: "resposta_invalida" }
    }
    return finishLogin({ data })
  } catch (cause) {
    const aborted = cause?.name === "AbortError"
    return { ok: false, error: aborted ? "tempo_esgotado" : "falha_rede" }
  } finally {
    cleanup()
  }
}

// O servidor devolve um corpo JSON estruturado (Success/Error) mesmo em
// respostas com status HTTP de erro (401 para credencial inválida, por
// exemplo) — por isso não olhamos response.ok aqui, só o campo Success.
function finishLogin({ data }) {
  if (!data?.Success || !data?.Token) {
    return { ok: false, error: data?.Error || "credenciais_invalidas" }
  }
  return {
    ok: true,
    username: String(data.User || ""),
    token: String(data.Token),
    score: Number(data.Score || 0),
  }
}

/**
 * Valida usuário + Web API Key chamando um endpoint leve da API pública.
 * Não retorna nem loga a chave.
 */
async function verifyApiKey({ username, apiKey, host = DEFAULT_HOST, fetchImpl = defaultFetch } = {}) {
  const user = String(username || "").trim()
  const key = String(apiKey || "").trim()
  if (!user || !key) return { ok: false, error: "credenciais_vazias" }

  const params = new URLSearchParams({ u: user, y: key })
  const { controller, cleanup } = withTimeout(REQUEST_TIMEOUT_MS)
  try {
    const response = await fetchImpl(`${host}/API/API_GetUserSummary.php?${params.toString()}`, {
      headers: { "User-Agent": USER_AGENT },
      signal: controller.signal,
    })
    const text = await response.text()
    let data
    try {
      data = JSON.parse(text)
    } catch {
      return { ok: false, error: "resposta_invalida" }
    }
    if (!response.ok) {
      return { ok: false, error: extractApiError(data) || "chave_invalida" }
    }
    return { ok: true, username: String(data?.User || user) }
  } catch (cause) {
    const aborted = cause?.name === "AbortError"
    return { ok: false, error: aborted ? "tempo_esgotado" : "falha_rede" }
  } finally {
    cleanup()
  }
}

/**
 * Busca um jogo pelo título dentro de um console RA específico (fallback sem
 * hash: usado só até a Fase 4 implementar identificação por hash de ROM).
 * Devolve o primeiro resultado plausível ou null.
 */
async function findGameByTitle({ username, apiKey, title, consoleId, host = DEFAULT_HOST, fetchImpl = defaultFetch } = {}) {
  const user = String(username || "").trim()
  const key = String(apiKey || "").trim()
  const query = String(title || "").trim()
  if (!user || !key || !query) return { ok: false, error: "parametros_invalidos" }

  const params = new URLSearchParams({ u: user, y: key, i: String(consoleId || "") })
  const { controller, cleanup } = withTimeout(REQUEST_TIMEOUT_MS)
  try {
    const response = await fetchImpl(`${host}/API/API_GetGameList.php?${params.toString()}`, {
      headers: { "User-Agent": USER_AGENT },
      signal: controller.signal,
    })
    const text = await response.text()
    let data
    try {
      data = JSON.parse(text)
    } catch {
      return { ok: false, error: "resposta_invalida" }
    }
    if (!response.ok) return { ok: false, error: extractApiError(data) || "falha_busca" }
    if (!Array.isArray(data)) return { ok: false, error: "falha_busca" }
    const normalized = query.toLowerCase()
    const match = data.find((game) => String(game?.Title || "").toLowerCase() === normalized)
      || data.find((game) => String(game?.Title || "").toLowerCase().includes(normalized))
    if (!match) return { ok: true, game: null }
    return { ok: true, game: { id: Number(match.ID), title: String(match.Title || "") } }
  } catch (cause) {
    const aborted = cause?.name === "AbortError"
    return { ok: false, error: aborted ? "tempo_esgotado" : "falha_rede" }
  } finally {
    cleanup()
  }
}

/**
 * Progresso do usuário para um jogo RA específico (lista de achievements +
 * quais já foram desbloqueados, hardcore ou não).
 */
async function getGameProgress({ username, apiKey, gameId, host = DEFAULT_HOST, fetchImpl = defaultFetch } = {}) {
  const user = String(username || "").trim()
  const key = String(apiKey || "").trim()
  const id = Number(gameId)
  if (!user || !key || !Number.isInteger(id) || id <= 0) return { ok: false, error: "parametros_invalidos" }

  const params = new URLSearchParams({ u: user, y: key, g: String(id) })
  const { controller, cleanup } = withTimeout(REQUEST_TIMEOUT_MS)
  try {
    const response = await fetchImpl(`${host}/API/API_GetGameInfoAndUserProgress.php?${params.toString()}`, {
      headers: { "User-Agent": USER_AGENT },
      signal: controller.signal,
    })
    const text = await response.text()
    let data
    try {
      data = JSON.parse(text)
    } catch {
      return { ok: false, error: "resposta_invalida" }
    }
    if (!response.ok) return { ok: false, error: extractApiError(data) || "falha_busca" }
    const achievements = Object.values(data?.Achievements || {}).map((item) => ({
      id: Number(item.ID),
      title: String(item.Title || ""),
      description: String(item.Description || ""),
      points: Number(item.Points || 0),
      badgeName: String(item.BadgeName || ""),
      badgeUrl: badgeUrl(item.BadgeName, false),
      badgeLockedUrl: badgeUrl(item.BadgeName, true),
      unlocked: Boolean(item.DateEarned),
      unlockedHardcore: Boolean(item.DateEarnedHardcore),
    }))
    return {
      ok: true,
      game: {
        id: Number(data.ID),
        title: String(data.Title || ""),
        consoleName: String(data.ConsoleName || ""),
        numAchievements: Number(data.NumAchievements || achievements.length),
        numAwardedToUser: Number(data.NumAwardedToUser || 0),
        numAwardedToUserHardcore: Number(data.NumAwardedToUserHardcore || 0),
      },
      achievements,
    }
  } catch (cause) {
    const aborted = cause?.name === "AbortError"
    return { ok: false, error: aborted ? "tempo_esgotado" : "falha_rede" }
  } finally {
    cleanup()
  }
}

module.exports = {
  DEFAULT_HOST,
  loginRequest,
  verifyApiKey,
  findGameByTitle,
  getGameProgress,
}
