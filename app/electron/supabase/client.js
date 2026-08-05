// Cliente Supabase (singleton) — roda NO MAIN PROCESS (nunca no renderer).
// - createClient com a config (URL + publishable key)
// - persistSession: false → a persistência é nossa (session.js, criptografada)
// - restoreSession(): recupera a sessão salva no boot
// - attachAuthPersistence(): espelha onAuthStateChange → session.json
"use strict"

const { createClient } = require("@supabase/supabase-js")
// Electron 33 roda Node 20, que NÃO tem WebSocket nativo (só Node 22+). Sem
// isso o supabase-js estoura ao inicializar o cliente Realtime. O pacote `ws`
// fornece a implementação; a opção vai em `realtime.transport`.
const WebSocket = require("ws")
const config = require("./config")
const sessionStore = require("./session")

let client = null

function getClient() {
  if (!client) {
    client = createClient(config.url, config.anonKey, {
      auth: {
        persistSession: false,
        autoRefreshToken: true,
        detectSessionInUrl: false,
      },
      realtime: { transport: WebSocket },
    })
  }
  return client
}

/** Boot: restaura a sessão salva (se houver) e devolve o estado atual. */
async function restoreSession() {
  const saved = sessionStore.loadSession()
  const auth = getClient().auth
  if (saved) {
    try {
      const { error } = await auth.setSession(saved)
      if (error) sessionStore.clearSession()
    } catch {
      sessionStore.clearSession()
    }
  }
  const { data, error } = await auth.getSession()
  return error ? { session: null, error } : { session: data.session, error: null }
}

/**
 * Espelha mudanças de auth no session.json:
 *  - SIGNED_IN / TOKEN_REFRESHED → salva
 *  - SIGNED_OUT / USER_DELETED   → limpa
 * Devolve a função de unsubscribe.
 */
function attachAuthPersistence() {
  const { data } = getClient().auth.onAuthStateChange((event, session) => {
    if (event === "SIGNED_IN" || event === "TOKEN_REFRESHED") {
      if (session) sessionStore.saveSession(session)
    } else if (event === "SIGNED_OUT" || event === "USER_DELETED") {
      sessionStore.clearSession()
    }
  })
  return () => data?.subscription?.unsubscribe()
}

module.exports = { getClient, restoreSession, attachAuthPersistence }
