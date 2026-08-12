"use strict"

// Realtime WS (Phoenix-lite) para o badge de amigos e o sync de biblioteca.
// Espelha o Supabase Realtime o suficiente para os canais `friends-<me>` e
// `library-<me>`:
//   - socket em /realtime/v1/websocket
//   - handshake phx_join com config.postgres_changes[{event,schema,table,filter}]
//   - evento `postgres_changes` quando algo relevante mudar pro dono do canal
//
// Autenticacao: o realtime-js manda CHANNEL_EVENTS.access_token no join.
// Validamos o JWT e rejeitamos (phx_reply error) se invalido.

const { WebSocketServer } = require("ws")
const { verifyToken } = require("./jwt")

// Map<topic, Set<ws>> — topic e o nome completo do canal (ex: "friends-<id>",
// "library-<id>"), nao so o userId: usar so o userId colidiria os dois canais
// no mesmo Set e um evento de amigos vazaria pros listeners de biblioteca.
const listeners = new Map()

function notify(topic, msg) {
  const sockets = listeners.get(topic)
  if (!sockets) return
  const payload = JSON.stringify(msg)
  for (const ws of sockets) {
    if (ws.readyState === 1) ws.send(payload)
  }
}

function notifyFriendshipInsert(userId, requesterId) {
  notify(`friends-${userId}`, {
    event: "postgres_changes",
    payload: {
      schema: "public",
      table: "friendships",
      new: { requester_id: requesterId },
    },
  })
}

function notifyLibraryChange(userId) {
  notify(`library-${userId}`, {
    event: "postgres_changes",
    payload: {
      schema: "public",
      table: "user_library",
      new: {},
    },
  })
}

function notifyAchievementsChange(userId) {
  notify(`achievements-${userId}`, {
    event: "postgres_changes",
    payload: {
      schema: "public",
      table: "user_achievements",
      new: {},
    },
  })
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

      // join nos canais friends-<me> / library-<me>
      if (
        msg.event === "phx_join" &&
        msg.topic &&
        (msg.topic.startsWith("friends-") || msg.topic.startsWith("library-") || msg.topic.startsWith("achievements-"))
      ) {
        if (!listeners.has(msg.topic)) listeners.set(msg.topic, new Set())
        listeners.get(msg.topic).add(ws)
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
      for (const [topic, set] of listeners) {
        if (set.delete(ws) && set.size === 0) listeners.delete(topic)
      }
    })
  })
}

module.exports = { registerRealtime, notifyFriendshipInsert, notifyLibraryChange, notifyAchievementsChange }
