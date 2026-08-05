// Registro dos IPC de conta + amigos (Supabase) + espelhamento de eventos.
// main.js chama registerAccountIpc(broadcast) uma vez no whenReady.
"use strict"

const { ipcMain } = require("electron")
const auth = require("./auth")
const friends = require("./friends")
const sync = require("./sync")
const { getClient, attachAuthPersistence } = require("./client")

function registerAccountIpc(broadcast) {
  // Persistência da sessão em session.json (SIGNED_IN/TOKEN_REFRESHED → salva;
  // SIGNED_OUT/USER_DELETED → limpa).
  attachAuthPersistence()

  // Realtime de amigos: liga ao logar, desliga ao sair.
  const realtime = friends.watchRequests((channel, payload) => broadcast(channel, payload))

  // Estado do sync de conquistas → renderer (indicador + botão).
  sync.onSyncState((st) => broadcast("sync:state", st))

  // Eventos de auth → renderer (só dados seguros, nunca tokens) + realtime + sync.
  getClient().auth.onAuthStateChange((event, session) => {
    broadcast("account:changed", {
      event,
      session: session
        ? {
            user: {
              id: session.user?.id,
              email: session.user?.email,
              username: session.user?.user_metadata?.username,
            },
          }
        : null,
    })
    if (event === "SIGNED_IN" || event === "TOKEN_REFRESHED") {
      realtime.start()
      sync.reconcile().catch(() => {}) // sobe a fila + baixa delta
    }
    if (event === "SIGNED_OUT" || event === "USER_DELETED") realtime.stop()
  })

  ipcMain.handle("account:status", async () => auth.status())
  ipcMain.handle("account:requestCode", async (_e, payload) => auth.requestCode(payload || {}))
  ipcMain.handle("account:verifyCode", async (_e, payload) => auth.verifyCode(payload || {}))
  ipcMain.handle("account:signOut", async () => auth.signOut())

  ipcMain.handle("friends:search", async (_e, query) => friends.search(query))
  ipcMain.handle("friends:send", async (_e, userId) => friends.send(userId))
  ipcMain.handle("friends:accept", async (_e, userId) => friends.accept(userId))
  ipcMain.handle("friends:cancel", async (_e, userId) => friends.cancel(userId))
  ipcMain.handle("friends:list", async () => friends.list())

  ipcMain.handle("sync:now", async () => sync.syncNow())
  ipcMain.handle("sync:state", async () => sync.getState())

  return () => realtime.stop()
}

module.exports = { registerAccountIpc }
