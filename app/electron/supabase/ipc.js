// Registro dos IPC de conta + amigos (backend proprio) + espelhamento de eventos.
// main.js chama registerAccountIpc(broadcast) uma vez no whenReady.
"use strict"

const { ipcMain } = require("electron")
const auth = require("./auth")
const friends = require("./friends")
const sync = require("./sync")
const biblioteca = require("./biblioteca")
const sourcesSync = require("./sources")
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

function registerAccountIpc(broadcast, onConta) {
  // Persistência da sessão em session.json (SIGNED_IN/TOKEN_REFRESHED → salva;
  // SIGNED_OUT/USER_DELETED → limpa).
  attachAuthPersistence()

  // Realtime de amigos: liga ao logar, desliga ao sair.
  const realtime = friends.watchRequests((channel, payload) => broadcast(channel, payload))

  // Atualização em background do cache de amigos → renderer pinta o fresco.
  friends.onAtualizado((data) => broadcast("friends:changed", data))

  // Estado do sync de conquistas → renderer (indicador + botão).
  sync.onSyncState((st) => broadcast("sync:state", st))

  // Biblioteca/horas sincronizadas → renderer recarrega a biblioteca.
  biblioteca.onChanged((channel) => broadcast(channel, { source: "biblioteca" }))

  // Eventos de auth → renderer (só dados seguros, nunca tokens) + realtime + sync.
  getClient().auth.onAuthStateChange((event, session) => {
    const username = session?.user?.user_metadata?.username || null
    broadcast("account:changed", {
      event,
      session: session
        ? {
            user: {
              id: session.user?.id,
              email: session.user?.email,
              username,
            },
          }
        : null,
    })
    if (event === "SIGNED_IN" || event === "TOKEN_REFRESHED") {
      realtime.start()
      sync.reconcile().catch(() => {}) // sobe a fila + baixa delta (conquistas)
      biblioteca.reconcile().catch(() => {}) // sobe jogos/horas + baixa coleção
      sourcesSync.reconcile().catch(() => {})
      // Troca o escopo dos arquivos locais pra conta logada
      onConta?.(username)
    }
    if (event === "SIGNED_OUT" || event === "USER_DELETED") {
      realtime.stop()
      // Volta pro escopo guest (raiz) — conta nova não vê dados da anterior
      onConta?.(null)
    }
  })

  // Inicia a restauração da sessão salva já no registro (paralelo ao boot).
  garantirSessao()

  ipcMain.handle("account:status", async () => {
    await garantirSessao()
    return auth.status()
  })
  ipcMain.handle("account:profile", async () => {
    await garantirSessao()
    return auth.myProfile()
  })
  ipcMain.handle("account:updateProfile", async (_e, campos) => {
    await garantirSessao()
    return auth.updateProfile(campos || {})
  })
  ipcMain.handle("account:setAvatar", async (_e, filePath) => {
    await garantirSessao()
    return auth.setAvatar(filePath)
  })
  ipcMain.handle("account:setBackground", async (_e, filePath) => {
    try {
      const r = await auth.setBackground(filePath)
      return r
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })
  ipcMain.handle("account:setAvatarBytes", async (_e, buf, mime, ext) => {
    await garantirSessao()
    return auth.setAvatarBytes(buf, mime, ext)
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
  ipcMain.handle("friends:list", async (_e, opts) => {
    await garantirSessao()
    return friends.list({ forcar: !!opts?.forcar })
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
