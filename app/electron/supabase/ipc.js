// Registro dos IPC de conta + amigos (backend proprio) + espelhamento de eventos.
// main.js chama registerAccountIpc(broadcast) uma vez no whenReady.
"use strict"

const { ipcMain } = require("electron")
const auth = require("./auth")
const friends = require("./friends")
const sync = require("./sync")
const biblioteca = require("./biblioteca")
const sourcesSync = require("./sources")
const community = require("./community")
const { getClient, attachAuthPersistence, restoreSession, warmBackend } = require("./client")
const {
  safeAuthResult,
  safeAccountStatus,
  safeAccountEvent,
} = require("../../../contracts")

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
  // Pré-aquece DNS/TLS em paralelo ao primeiro paint. Não bloqueia o registro
  // dos IPCs nem transforma backend fora do ar em erro de boot.
  warmBackend().catch(() => {})

  // Persistência da sessão em session.json (SIGNED_IN/TOKEN_REFRESHED → salva;
  // SIGNED_OUT/USER_DELETED → limpa).
  attachAuthPersistence()

  // Realtime de amigos: liga ao logar, desliga ao sair.
  const realtime = friends.watchRequests((channel, payload) => broadcast(channel, payload))

  // Realtime de biblioteca: outra maquina adicionou/removeu jogo → puxa na hora
  // em vez de esperar o proximo boot/login.
  const bibliotecaRealtime = biblioteca.watchChanges()

  // Realtime de conquistas: outra maquina desbloqueou → puxa na hora.
  const syncRealtime = sync.watchChanges()

  // Atualização em background do cache de amigos → renderer pinta o fresco.
  friends.onAtualizado((data) => broadcast("friends:changed", data))

  // Estado do sync de conquistas → renderer (indicador + botão).
  sync.onSyncState((st) => broadcast("sync:state", st))

  // Biblioteca/horas sincronizadas → renderer recarrega a biblioteca.
  biblioteca.onChanged((channel) => broadcast(channel, { source: "biblioteca" }))

  // Eventos de auth → renderer (só dados seguros, nunca tokens) + realtime + sync.
  // A geração cancela trabalho agendado se o usuário sair antes do próximo
  // tick (evita reabrir canais da conta anterior após um login muito rápido).
  let authWorkGeneration = 0
  getClient().auth.onAuthStateChange((event, session) => {
    const username = session?.user?.user_metadata?.username || null
    broadcast("account:changed", safeAccountEvent(event, session))
    if (event === "SIGNED_IN" || event === "TOKEN_REFRESHED") {
      // O escopo precisa mudar ANTES de qualquer reconcile/realtime: esses
      // módulos leem arquivos por conta e não podem iniciar como guest.
      onConta?.(username)
      const generation = ++authWorkGeneration
      // Não faça reconciliações no mesmo tick do login: elas podem emitir
      // eventos IPC e competir com a primeira pintura da conta.
      setImmediate(() => {
        if (generation !== authWorkGeneration) return
        realtime.start()
        bibliotecaRealtime.start()
        syncRealtime.start()
        sync.reconcile().then((r) => {
          console.log("[sync] reconcile result:", JSON.stringify(r))
        }).catch((e) => {
          console.error("[sync] reconcile failed:", e?.message || e)
        })
        biblioteca.reconcile().catch(() => {}) // sobe jogos/horas + baixa coleção
        sourcesSync.reconcile().catch(() => {})
      })
    }
    if (event === "SIGNED_OUT" || event === "USER_DELETED") {
      ++authWorkGeneration
      realtime.stop()
      bibliotecaRealtime.stop()
      syncRealtime.stop()
      // Volta pro escopo guest (raiz) — conta nova não vê dados da anterior
      onConta?.(null)
    }
  })

  // Inicia a restauração da sessão salva já no registro (paralelo ao boot).
  garantirSessao()

  ipcMain.handle("account:status", async () => {
    await garantirSessao()
    return safeAccountStatus(await auth.status())
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
  ipcMain.handle("account:setBackground", async (_e, filePath, kind = "background") => {
    try {
      const r = await auth.setBackground(filePath, kind)
      return r
    } catch (e) {
      return { ok: false, error: String(e?.message || e) }
    }
  })
  ipcMain.handle("account:setAvatarBytes", async (_e, buf, mime, ext) => {
    await garantirSessao()
    return auth.setAvatarBytes(buf, mime, ext)
  })
  ipcMain.handle("account:signUp", async (_e, payload) =>
    safeAuthResult(await auth.signUp(payload || {})),
  )
  ipcMain.handle("account:signIn", async (_e, payload) =>
    safeAuthResult(await auth.signIn(payload || {})),
  )
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
    return friends.friendAchievementsCached(userId)
  })
  ipcMain.handle("friends:profile", async (_e, userId) => {
    await garantirSessao()
    return friends.friendProfile(userId)
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

  ipcMain.handle("sync:full", async () => {
    await garantirSessao()
    return sync.fullSync()
  })

  ipcMain.handle("sync:all", async () => {
    await garantirSessao()
    return sync.syncAllLocal()
  })

  ipcMain.handle("sync:deleteAchievement", async (_e, { appid, apiname }) => {
    await garantirSessao()
    return sync.deleteAchievement(appid, apiname)
  })

  // Comunidade (reviews/listas): requests e cache ficam no main, nunca no renderer.
  ipcMain.handle("community:reviews", async (_e, appid, options) => {
    await garantirSessao()
    return community.listReviews(typeof appid === "string" ? appid : "", options || {})
  })
  ipcMain.handle("community:review:create", async (_e, payload) => {
    await garantirSessao()
    return community.createReview(payload && typeof payload === "object" ? payload : {})
  })
  ipcMain.handle("community:review:update", async (_e, id, payload) => {
    await garantirSessao()
    return community.updateReview(String(id || ""), payload && typeof payload === "object" ? payload : {})
  })
  ipcMain.handle("community:review:remove", async (_e, id) => {
    await garantirSessao()
    return community.removeReview(String(id || ""))
  })
  ipcMain.handle("community:review:report", async (_e, id, payload) => {
    await garantirSessao()
    return community.reportReview(String(id || ""), payload && typeof payload === "object" ? payload : {})
  })
  ipcMain.handle("community:collections", async (_e, options) => {
    await garantirSessao()
    return community.listCollections(options || {})
  })
  ipcMain.handle("community:collection:get", async (_e, id) => {
    await garantirSessao()
    return community.getCollection(String(id || ""))
  })
  ipcMain.handle("community:collection:create", async (_e, payload) => {
    await garantirSessao()
    return community.createCollection(payload && typeof payload === "object" ? payload : {})
  })
  ipcMain.handle("community:collection:update", async (_e, id, payload) => {
    await garantirSessao()
    return community.updateCollection(String(id || ""), payload && typeof payload === "object" ? payload : {})
  })
  ipcMain.handle("community:collection:remove", async (_e, id) => {
    await garantirSessao()
    return community.removeCollection(String(id || ""))
  })
  ipcMain.handle("community:collection:item:add", async (_e, id, appid) => {
    await garantirSessao()
    return community.addCollectionItem(String(id || ""), String(appid || ""))
  })
  ipcMain.handle("community:collection:item:replace", async (_e, id, items) => {
    await garantirSessao()
    return community.replaceCollectionItems(String(id || ""), Array.isArray(items) ? items : [])
  })
  ipcMain.handle("community:collection:item:remove", async (_e, id, appid) => {
    await garantirSessao()
    return community.removeCollectionItem(String(id || ""), String(appid || ""))
  })
  ipcMain.handle("community:collection:report", async (_e, id, payload) => {
    await garantirSessao()
    return community.reportCollection(String(id || ""), payload && typeof payload === "object" ? payload : {})
  })

  return () => {
    realtime.stop()
    bibliotecaRealtime.stop()
    syncRealtime.stop()
  }
}

module.exports = { registerAccountIpc, garantirSessao }
