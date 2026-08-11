"use client"

// Estado global da conta online (backend proprio) no renderer.
// Consome a accountAPI exposta pelo preload; escuta onAuthChanged; mantém o
// perfil online (username + avatar do servidor) atualizado após o login.
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react"

export type AccountStatus = "carregando" | "deslogado" | "logado"

/** Única conta considerada "dona/criadora" do Arcadia — badge de dono. */
export const OWNER_USERNAME = "zesmehentperu"

export interface PerfilOnline {
  username: string | null
  avatar_url: string | null
  display_name?: string | null
  summary?: string | null
  country?: string | null
  city?: string | null
  showcase?: string[]
  background_url?: string | null
}

/** Campos que o perfil local espelha do online (whitelist). */
export const CAMPOS_ESPELHO: Array<keyof PerfilOnline> = [
  "display_name",
  "summary",
  "country",
  "city",
  "showcase",
  "background_url",
]

interface AccountCtx {
  status: AccountStatus
  session: AccountSession | null
  /** Perfil online (username/avatar/campos do servidor) — após login. */
  perfil: PerfilOnline | null
  /** Cadastro: email + username + senha (sem verificação). */
  signUp: (
    email: string,
    username: string,
    password: string,
  ) => Promise<{ ok: boolean; error?: string; usernameReal?: string }>
  /** Login com username + senha. */
  signIn: (username: string, password: string) => Promise<{ ok: boolean; error?: string; usernameReal?: string }>
  signOut: () => Promise<void>
  /** Sobe o avatar pro servidor e devolve a URL pública. */
  setAvatar: (filePath: string) => Promise<{ ok: boolean; avatar_url?: string; error?: string }>
  /** Sobe o avatar recortado (bytes prontos, ex: PNG 256² do recorte). */
  setAvatarBytes: (
    bytes: Uint8Array,
    mime: string,
    ext: string,
  ) => Promise<{ ok: boolean; avatar_url?: string; error?: string }>
  /** Sobe o background (imagem/video) pro servidor e devolve a URL publica. */
  setBackground: (filePath: string) => Promise<{ ok: boolean; background_url?: string; error?: string }>
  /** Grava campos do perfil online (display_name, summary, country, city, showcase). */
  updatePerfil: (campos: Partial<PerfilOnline>) => Promise<{ ok: boolean; error?: string }>
}

const Ctx = createContext<AccountCtx | null>(null)

export function AccountProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AccountStatus>("carregando")
  const [session, setSession] = useState<AccountSession | null>(null)
  const [perfil, setPerfil] = useState<PerfilOnline | null>(null)

  const carregarPerfil = useCallback(async () => {
    try {
      const r = await window.launcherAPI?.accountProfile()
      if (r?.ok && r.profile) {
        setPerfil(r.profile)
        return
      }
    } catch {
      /* rede falhou — fallback mínimo abaixo */
    }
    // Fallback com o username da sessão (evita splash eterno se a rede falhar)
    setPerfil({ username: session?.user?.username ?? null, avatar_url: null })
  }, [session?.user?.username])

  useEffect(() => {
    let vivo = true
    window.launcherAPI?.accountStatus().then(async (r) => {
      if (!vivo) return
      setSession(r?.session ?? null)
      setStatus(r?.session ? "logado" : "deslogado")
      if (r?.session) await carregarPerfil()
    })
    const off = window.launcherAPI?.onAuthChanged((data) => {
      if (!vivo) return
      setSession(data.session)
      setStatus(data.session ? "logado" : "deslogado")
      if (data.session) carregarPerfil()
      else setPerfil(null)
    })
    return () => {
      vivo = false
      off?.()
    }
  }, [carregarPerfil])

  const signUp = useCallback(async (email: string, username: string, password: string) => {
    try {
      return (
        (await window.launcherAPI?.accountSignUp({ email, username, password })) || {
          ok: false,
          error: "API indisponível",
        }
      )
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  const signIn = useCallback(async (username: string, password: string) => {
    try {
      return (
        (await window.launcherAPI?.accountSignIn({ username, password })) || {
          ok: false,
          error: "API indisponível",
        }
      )
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  const signOut = useCallback(async () => {
    await window.launcherAPI?.accountSignOut()
    setPerfil(null)
  }, [])

  const setAvatar = useCallback(async (filePath: string) => {
    try {
      const r = await window.launcherAPI?.accountSetAvatar(filePath)
      if (r?.ok && r.avatar_url) {
        // ATENÇÃO: o spread tem que vir ANTES — se vier depois, o avatar_url
        // ANTIGO do ...p sobrescreve o novo (bug: só atualizava no restart).
        setPerfil((p) => ({ ...(p ?? { username: null, avatar_url: null }), avatar_url: r.avatar_url ?? null }))
      }
      return r || { ok: false, error: "API indisponível" }
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  const setAvatarBytes = useCallback(async (bytes: Uint8Array, mime: string, ext: string) => {
    try {
      const r = await window.launcherAPI?.accountSetAvatarBytes(bytes, mime, ext)
      if (r?.ok && r.avatar_url) {
        setPerfil((p) => ({ ...(p ?? { username: null, avatar_url: null }), avatar_url: r.avatar_url ?? null }))
      }
      return r || { ok: false, error: "API indisponível" }
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  const setBackground = useCallback(async (filePath: string) => {
    try {
      const r = await window.launcherAPI?.accountSetBackground(filePath)
      if (r?.ok && r.background_url) {
        setPerfil((p) => ({ ...(p ?? { username: null, avatar_url: null }), background_url: r.background_url ?? null }))
      }
      return r || { ok: false, error: "API indisponível" }
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  const updatePerfil = useCallback(async (campos: Partial<PerfilOnline>) => {
    try {
      const alvo = { display_name: campos.display_name, summary: campos.summary, country: campos.country, city: campos.city, showcase: campos.showcase, background_url: campos.background_url }
      const r = await window.launcherAPI?.accountUpdateProfile(alvo)
      if (r?.ok) {
        setPerfil((p) => ({ ...(p ?? { username: null, avatar_url: null }), ...campos }))
      }
      return r || { ok: false, error: "API indisponível" }
    } catch (e) {
      return { ok: false, error: e?.message || "exceção" }
    }
  }, [])

  return (
    <Ctx.Provider value={{ status, session, perfil, signUp, signIn, signOut, setAvatar, setAvatarBytes, setBackground, updatePerfil }}>
      {children}
    </Ctx.Provider>
  )
}

export function useAccount() {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useAccount precisa estar dentro de <AccountProvider>")
  return ctx
}

/** Igual ao useAccount, mas devolve null fora do provider (para componentes
 *  compartilhados com o modo console, que não tem o provider). */
export function useAccountOptional() {
  return useContext(Ctx)
}
