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
  /** Cadastro instantâneo (sem verificação de email). */
  signUp: (email: string, username: string) => Promise<{ ok: boolean; error?: string }>
  /** Envia o código OTP por email (entrar em conta existente). */
  requestCode: (email: string) => Promise<{ ok: boolean; error?: string }>
  /** Valida o código de 6 dígitos e completa o login. */
  verifyCode: (email: string, token: string) => Promise<{ ok: boolean; error?: string }>
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

  const signUp = useCallback(async (email: string, username: string) => {
    return (
      (await window.launcherAPI?.accountSignUp({ email, username })) || {
        ok: false,
        error: "API indisponível",
      }
    )
  }, [])

  const requestCode = useCallback(async (email: string) => {
    return (
      (await window.launcherAPI?.accountRequestCode({ email })) || {
        ok: false,
        error: "API indisponível",
      }
    )
  }, [])

  const verifyCode = useCallback(async (email: string, token: string) => {
    return (
      (await window.launcherAPI?.accountVerifyCode({ email, token })) || {
        ok: false,
        error: "API indisponível",
      }
    )
  }, [])

  const signOut = useCallback(async () => {
    await window.launcherAPI?.accountSignOut()
  }, [])

  return (
    <Ctx.Provider value={{ status, session, signUp, requestCode, verifyCode, signOut }}>
      {children}
    </Ctx.Provider>
  )
}

export function useAccount() {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useAccount precisa estar dentro de <AccountProvider>")
  return ctx
}
