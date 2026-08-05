// Registro dos IPC de conta + amigos (Supabase) + espelhamento de eventos.
// main.js chama registerAccountIpc(broadcast) uma vez no whenReady.
"use strict"

const { ipcMain } = require("electron")
const auth = require("./auth")
const friends = require("./friends")
const sync = require("./sync")
const { getClient, attachAuthPersistence, restoreSession } = require("./client")

// Sessão restaurada UMA vez por boot (memoizado). Todo handler que depende de
// sessão existente aguarda essa promise antes de responder — elimina o race
// entre o boot e a primeira consulta do renderer (bug: pedia login toda hora).
let restorePromise = null
function garantirSessao() {
  if (!restorePromise) {
    restorePromise = restoreSession().catch(() => null)
  }
  return restorePromise
}

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

  // Inicia a restauração da sessão salva já no registro (paralelo ao boot).
  garantirSessao()

  ipcMain.handle("account:status", async () => {
    await garantirSessao()
    return auth.status()
  })
  ipcMain.handle("account:signUp", async (_e, payload) => auth.signUp(payload || {}))
  ipcMain.handle("account:signIn", async (_e, payload) => auth.signIn(payload || {}))
  ipcMain.handle("account:signOut", async () => auth.signOut())

  ipcMain.handle("friends:search", async (_e, query) => {
    await garantirSessao()
    return friends.search(query)
  })
  ipcMain.handle("friends:send", async (_e, userId) => {
    await garantirSessao()
    return friends.send(userId)
  })
  ipcMain.handle("friends:accept", async (_e, userId) => {
    await garantirSessao()
    return friends.accept(userId)
  })
  ipcMain.handle("friends:cancel", async (_e, userId) => {
    await garantirSessao()
    return friends.cancel(userId)
  })
  ipcMain.handle("friends:list", async () => {
    await garantirSessao()
    return friends.list()
  })
  ipcMain.handle("friends:achievements", async (_e, userId) => {
    await garantirSessao()
    return friends.friendAchievements(userId)
  })
  ipcMain.handle("friends:remove", async (_e, userId) => {
    await garantirSessao()
    return friends.removeFriend(userId)
  })

  ipcMain.handle("sync:now", async () => {
    await garantirSessao()
    return sync.syncNow()
  })
  ipcMain.handle("sync:state", async () => {
    await garantirSessao()
    return sync.getState()
  })

  return () => realtime.stop()
}

module.exports = { registerAccountIpc }
