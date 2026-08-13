"use strict"

if (process.env.ALLOW_PRODUCTION_SMOKE !== "1") {
  throw new Error("Defina ALLOW_PRODUCTION_SMOKE=1 para executar")
}

const fs = require("node:fs")
const path = require("node:path")
const WebSocket = require("ws")
const { db } = require("../src/db")
const { issueTokens } = require("../src/jwt")

const BASE_URL = process.env.SMOKE_BASE_URL || "http://127.0.0.1:3000"

async function request(route, options = {}) {
  const response = await fetch(BASE_URL + route, options)
  const text = await response.text()
  if (!response.ok) throw new Error(`${route}: HTTP ${response.status} ${text}`)
  return text ? JSON.parse(text) : null
}

async function realtime(token) {
  const url = BASE_URL.replace(/^http/, "ws") + "/realtime/v1/websocket"
  await new Promise((resolve, reject) => {
    const ws = new WebSocket(url)
    const timeout = setTimeout(() => {
      ws.terminate()
      reject(new Error("realtime timeout"))
    }, 5_000)
    ws.on("open", () => ws.send(JSON.stringify({
      topic: "phoenix",
      event: "phx_join",
      payload: { access_token: token },
    })))
    ws.on("message", (raw) => {
      const message = JSON.parse(raw)
      if (message.event === "phx_reply" && message.payload?.status === "ok") {
        clearTimeout(timeout)
        ws.close()
        resolve()
      }
    })
    ws.on("error", reject)
  })
}

async function main() {
  const profile = (await db.query(
    `SELECT p.* FROM profiles p
     ORDER BY
       (SELECT count(*) FROM user_achievements a WHERE a.user_id = p.id) +
       (SELECT count(*) FROM user_library l WHERE l.user_id = p.id) DESC
     LIMIT 1`,
  )).rows[0]
  if (!profile) throw new Error("Nenhum perfil migrado")
  const token = issueTokens(profile).access_token
  const auth = { authorization: `Bearer ${token}` }
  const filename = `${Date.now()}.png`
  const relative = `${profile.id}/${filename}`
  const storedFile = path.join(__dirname, "..", "avatars", relative)

  try {
    const user = await request("/auth/v1/user", { headers: auth })
    if (user.user.id !== profile.id) throw new Error("auth user divergente")

    const library = await request("/rest/v1/rpc/pull_library", {
      method: "POST",
      headers: { ...auth, "content-type": "application/json" },
      body: "{}",
    })
    const achievements = await request("/rest/v1/rpc/pull_achievements", {
      method: "POST",
      headers: { ...auth, "content-type": "application/json" },
      body: "{}",
    })
    const catalog = await request("/catalog/v1/popular?limite=1", { headers: auth })
    if (!catalog.ok || !Array.isArray(catalog.itens)) throw new Error("catalogo invalido")

    const png = Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
      "base64",
    )
    await request(`/storage/v1/object/avatars/${relative}`, {
      method: "POST",
      headers: auth,
      body: png,
    })
    const served = await fetch(`${BASE_URL}/storage/v1/object/public/avatars/${relative}`)
    if (!served.ok || !(await served.arrayBuffer()).byteLength) throw new Error("storage GET falhou")
    await request("/storage/v1/object/avatars", {
      method: "DELETE",
      headers: { ...auth, "content-type": "application/json" },
      body: JSON.stringify({ paths: [relative] }),
    })
    if (fs.existsSync(storedFile)) throw new Error("storage DELETE falhou")

    await realtime(token)
    const profileCount = Number((await db.query("SELECT count(*) AS count FROM profiles")).rows[0].count)
    console.log(
      `production_smoke=OK profiles=${profileCount} library_rows=${library.length} ` +
      `achievement_rows=${achievements.length} catalog=OK storage=OK realtime=OK`,
    )
  } finally {
    fs.rmSync(storedFile, { force: true })
    await db.end()
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
