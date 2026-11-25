"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  loginRequest,
  verifyApiKey,
  findGameByTitle,
  getGameProgress,
  DEFAULT_HOST,
} = require("../electron/retroachievements/client")

function jsonResponse(body, { ok = true, status = 200 } = {}) {
  return {
    ok,
    status,
    text: async () => JSON.stringify(body),
  }
}

test("loginRequest troca usuário+senha por token quando o servidor confirma sucesso", async () => {
  let capturedUrl = ""
  let capturedBody = ""
  const fetchImpl = async (url, opts) => {
    capturedUrl = url
    capturedBody = opts?.body || ""
    return jsonResponse({ Success: true, User: "Fulano", Token: "tok-123", Score: 42 })
  }
  const result = await loginRequest({ username: "Fulano", password: "senha", fetchImpl })
  assert.equal(result.ok, true)
  assert.equal(result.username, "Fulano")
  assert.equal(result.token, "tok-123")
  assert.equal(result.score, 42)
  assert.equal(capturedUrl, `${DEFAULT_HOST}/dorequest.php`)
  assert.match(capturedBody, /r=login2/)
  assert.match(capturedBody, /u=Fulano/)
})

test("loginRequest devolve erro quando o servidor rejeita as credenciais", async () => {
  const fetchImpl = async () => jsonResponse({ Success: false, Error: "Invalid User/Password combination." })
  const result = await loginRequest({ username: "Fulano", password: "errada", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "Invalid User/Password combination.")
})

test("loginRequest lê a mensagem de erro real mesmo quando o status HTTP é 401 (comportamento real do servidor)", async () => {
  // O servidor da RetroAchievements devolve um corpo JSON estruturado mesmo
  // com status 401 para credencial inválida; o client não deve descartar
  // essa mensagem só porque response.ok é false.
  const fetchImpl = async () =>
    jsonResponse(
      { Success: false, Status: 401, Code: "invalid_credentials", Error: "Invalid user/password combination. Please try again." },
      { ok: false, status: 401 },
    )
  const result = await loginRequest({ username: "Fulano", password: "errada", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "Invalid user/password combination. Please try again.")
})

test("loginRequest envia um User-Agent reconhecível (o servidor bloqueia requisições sem ele)", async () => {
  let capturedHeaders = {}
  const fetchImpl = async (_url, opts) => {
    capturedHeaders = opts?.headers || {}
    return jsonResponse({ Success: true, User: "Fulano", Token: "tok-123" })
  }
  await loginRequest({ username: "Fulano", password: "senha", fetchImpl })
  assert.ok(capturedHeaders["User-Agent"], "esperava um header User-Agent")
})

test("loginRequest rejeita usuário ou senha vazios sem chamar a rede", async () => {
  let called = false
  const fetchImpl = async () => {
    called = true
    return jsonResponse({})
  }
  const result = await loginRequest({ username: "", password: "x", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(called, false)
})

test("loginRequest trata resposta não-JSON como falha", async () => {
  const fetchImpl = async () => ({ ok: true, status: 200, text: async () => "<html>not json</html>" })
  const result = await loginRequest({ username: "a", password: "b", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "resposta_invalida")
})

test("verifyApiKey confirma usuário válido pela API pública", async () => {
  let capturedUrl = ""
  const fetchImpl = async (url) => {
    capturedUrl = url
    return jsonResponse({ User: "Fulano" })
  }
  const result = await verifyApiKey({ username: "Fulano", apiKey: "chave", fetchImpl })
  assert.equal(result.ok, true)
  assert.equal(result.username, "Fulano")
  assert.match(capturedUrl, /API_GetUserSummary\.php/)
})

test("verifyApiKey propaga erro devolvido pela API (formato Error)", async () => {
  const fetchImpl = async () => jsonResponse({ Error: "Invalid API Key" }, { ok: false, status: 401 })
  const result = await verifyApiKey({ username: "Fulano", apiKey: "invalida", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "Invalid API Key")
})

test("verifyApiKey propaga erro no formato 'message' (401 do middleware de auth)", async () => {
  // Confirmado empiricamente contra o servidor real: chave inválida devolve
  // {"message":"Unauthenticated."}, não {"Error": "..."}.
  const fetchImpl = async () => jsonResponse({ message: "Unauthenticated." }, { ok: false, status: 401 })
  const result = await verifyApiKey({ username: "Fulano", apiKey: "invalida", fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "Unauthenticated.")
})

test("findGameByTitle encontra jogo por título exato dentro do console certo", async () => {
  let capturedUrl = ""
  const fetchImpl = async (url) => {
    capturedUrl = url
    return jsonResponse([
      { ID: 10, Title: "Outro Jogo" },
      { ID: 21, Title: "Grand Theft Auto: San Andreas" },
    ])
  }
  const result = await findGameByTitle({
    username: "Fulano",
    apiKey: "chave",
    title: "Grand Theft Auto: San Andreas",
    consoleId: 21,
    fetchImpl,
  })
  assert.equal(result.ok, true)
  assert.equal(result.game.id, 21)
  assert.match(capturedUrl, /API_GetGameList\.php/)
  assert.match(capturedUrl, /i=21/)
})

test("findGameByTitle devolve game:null quando não encontra nenhuma correspondência", async () => {
  const fetchImpl = async () => jsonResponse([{ ID: 1, Title: "Outro Jogo Qualquer" }])
  const result = await findGameByTitle({ username: "Fulano", apiKey: "chave", title: "Jogo Inexistente", consoleId: 21, fetchImpl })
  assert.equal(result.ok, true)
  assert.equal(result.game, null)
})

test("findGameByTitle propaga erro real do servidor em vez de mensagem genérica", async () => {
  const fetchImpl = async () => jsonResponse({ message: "Unauthenticated." }, { ok: false, status: 401 })
  const result = await findGameByTitle({ username: "Fulano", apiKey: "invalida", title: "X", consoleId: 21, fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(result.error, "Unauthenticated.")
})

test("getGameProgress normaliza achievements com URLs de badge desbloqueada/bloqueada", async () => {
  const fetchImpl = async () =>
    jsonResponse({
      ID: 21,
      Title: "GTA: SA",
      ConsoleName: "PlayStation 2",
      NumAchievements: 2,
      NumAwardedToUser: 1,
      NumAwardedToUserHardcore: 0,
      Achievements: {
        "9": {
          ID: 9,
          Title: "Conquista A",
          Description: "Desc A",
          Points: 10,
          BadgeName: "325108",
          DateEarned: "2024-01-01 00:00:00",
        },
        "10": {
          ID: 10,
          Title: "Conquista B",
          Description: "Desc B",
          Points: 5,
          BadgeName: "325109",
        },
      },
    })
  const result = await getGameProgress({ username: "Fulano", apiKey: "chave", gameId: 21, fetchImpl })
  assert.equal(result.ok, true)
  assert.equal(result.game.title, "GTA: SA")
  assert.equal(result.achievements.length, 2)
  const [a, b] = result.achievements
  assert.equal(a.unlocked, true)
  assert.equal(a.badgeUrl, "https://media.retroachievements.org/Badge/325108.png")
  assert.equal(a.badgeLockedUrl, "https://media.retroachievements.org/Badge/325108_lock.png")
  assert.equal(b.unlocked, false)
})

test("getGameProgress rejeita gameId inválido sem chamar a rede", async () => {
  let called = false
  const fetchImpl = async () => {
    called = true
    return jsonResponse({})
  }
  const result = await getGameProgress({ username: "Fulano", apiKey: "chave", gameId: 0, fetchImpl })
  assert.equal(result.ok, false)
  assert.equal(called, false)
})
