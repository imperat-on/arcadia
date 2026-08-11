"use strict"

// Realtime WS (Phoenix-lite) para o badge de amigos. Espelha o Supabase
// Realtime o suficiente para o canal `friends-<me>`:
//   - socket em /realtime/v1/websocket
//   - handshake phx_join com config.postgres_changes[{event,schema,table,filter}]
//   - evento `postgres_changes` quando um INSERT em friendships tiver
//     user_b = <me> (payload.new.requester_id, como o app consome)
//
// Autenticacao: o realtime-js manda CHANNEL_EVENTS.access_token no join.
// Validamos o JWT e rejeitamos (phx_reply error) se invalido.

const { WebSocketServer } = require("ws")
const { verifyToken } = require("./jwt")

// Map<userId, Set<ws>> para os canais friends-<me>
const listeners = new Map()

function notifyFriendshipInsert(userId, requesterId) {
  const sockets = listeners.get(userId)
  if (!sockets) return
  const msg = JSON.stringify({
    event: "postgres_changes",
    payload: {
      schema: "public",
      table: "friendships",
      new: { requester_id: requesterId },
    },
  })
  for (const ws of sockets) {
    if (ws.readyState === 1) ws.send(msg)
  }
}

function registerRealtime(server) {
  const wss = new WebSocketServer({ server, path: "/realtime/v1/websocket" })

  wss.on("connection", (ws) => {
    ws.on("message", (raw) => {
      let msg
      try {
        msg = JSON.parse(raw.toString())
      } catch {
        return
      }

      // handshake de autenticacao (channel + access_token)
      if (msg.topic === "phoenix" && msg.event === "phx_join") {
        const token = msg.payload?.access_token || ""
        const v = verifyToken(token)
        if (!v.ok) {
          ws.send(
            JSON.stringify({
              event: "phx_reply",
              payload: { status: "error", response: { reason: "unauthorized" } },
            })
          )
          return
        }
        ws.send(
          JSON.stringify({
            event: "phx_reply",
            payload: { status: "ok", response: {} },
          })
        )
        return
      }

      // join no canal friends-<me>
      if (msg.event === "phx_join" && msg.topic && msg.topic.startsWith("friends-")) {
        const me = msg.topic.slice("friends-".length)
        if (!listeners.has(me)) listeners.set(me, new Set())
        listeners.get(me).add(ws)
        ws.send(
          JSON.stringify({
            event: "phx_reply",
            payload: { status: "ok", response: {} },
          })
        )
        return
      }

      if (msg.event === "phx_leave") {
        ws.close()
      }
    })

    ws.on("close", () => {
      for (const [userId, set] of listeners) {
        if (set.delete(ws) && set.size === 0) listeners.delete(userId)
      }
    })
  })
}

module.exports = { registerRealtime, notifyFriendshipInsert }
