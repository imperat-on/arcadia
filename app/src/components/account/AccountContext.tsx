"use client"

// Estado global da conta online (Supabase) no renderer.
// Consome a accountAPI exposta pelo preload; escuta onAuthChanged.
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react"

export type AccountStatus = "carregando" | "deslogado" | "logado"

interface AccountCtx {
  status: AccountStatus
  session: AccountSession | null
  /** Cadastro: email + username + senha (sem verificação). */
  signUp: (
    email: string,
    username: string,
    password: string,
  ) => Promise<{ ok: boolean; error?: string }>
  /** Login com username + senha. */
  signIn: (username: string, password: string) => Promise<{ ok: boolean; error?: string }>
  signOut: () => Promise<void>
}

const Ctx = createContext<AccountCtx | null>(null)

export function AccountProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AccountStatus>("carregando")
  const [session, setSession] = useState<AccountSession | null>(null)

  useEffect(() => {
    let vivo = true
    window.launcherAPI?.accountStatus().then((r) => {
      if (!vivo) return
      setSession(r?.session ?? null)
      setStatus(r?.session ? "logado" : "deslogado")
    })
    const off = window.launcherAPI?.onAuthChanged((data) => {
      if (!vivo) return
      setSession(data.session)
      setStatus(data.session ? "logado" : "deslogado")
    })
    return () => {
      vivo = false
      off?.()
    }
  }, [])

  const signUp = useCallback(async (email: string, username: string, password: string) => {
    return (
      (await window.launcherAPI?.accountSignUp({ email, username, password })) || {
        ok: false,
        error: "API indisponível",
      }
    )
  }, [])

  const signIn = useCallback(async (username: string, password: string) => {
    return (
      (await window.launcherAPI?.accountSignIn({ username, password })) || {
        ok: false,
        error: "API indisponível",
      }
    )
  }, [])

  const signOut = useCallback(async () => {
    await window.launcherAPI?.accountSignOut()
  }, [])

  return (
    <Ctx.Provider value={{ status, session, signUp, signIn, signOut }}>{children}</Ctx.Provider>
  )
}

export function useAccount() {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useAccount precisa estar dentro de <AccountProvider>")
  return ctx
}
