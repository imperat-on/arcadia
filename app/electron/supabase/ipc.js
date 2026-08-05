// Registro dos IPC de conta (Supabase) + espelhamento de eventos pro renderer.
// main.js chama registerAccountIpc(broadcast) uma vez no whenReady.
"use strict"

const { ipcMain } = require("electron")
const auth = require("./auth")
const { getClient, attachAuthPersistence } = require("./client")

function registerAccountIpc(broadcast) {
  // Persistência da sessão em session.json (SIGNED_IN/TOKEN_REFRESHED → salva;
  // SIGNED_OUT/USER_DELETED → limpa).
  const unsubscribe = attachAuthPersistence()

  // Eventos de auth → renderer (só dados seguros, nunca tokens).
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
  })

  ipcMain.handle("account:status", async () => auth.status())
  ipcMain.handle("account:requestCode", async (_e, payload) => auth.requestCode(payload || {}))
  ipcMain.handle("account:verifyCode", async (_e, payload) => auth.verifyCode(payload || {}))
  ipcMain.handle("account:signOut", async () => auth.signOut())

  return unsubscribe
}

module.exports = { registerAccountIpc }
