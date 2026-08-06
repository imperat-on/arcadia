"use client"

// Estado global de amigos (Supabase) no renderer.
// Carrega a lista quando logado; escuta onFriendRequest (realtime) e
// onAuthChanged (login/logout) para atualizar sozinho.
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react"
import { useAccount } from "./AccountContext"

interface FriendsCtx {
  data: FriendsListData | null
  pedidos: number
  refresh: () => Promise<void>
}

const Ctx = createContext<FriendsCtx | null>(null)

export function FriendsProvider({ children }: { children: ReactNode }) {
  const { status } = useAccount()
  const [data, setData] = useState<FriendsListData | null>(null)

  const refresh = useCallback(async (forcar?: boolean) => {
    const r = await window.launcherAPI?.friendsList(forcar ? { forcar: true } : undefined)
    if (r?.ok && r.data) setData(r.data)
  }, [])

  useEffect(() => {
    if (status === "logado") refresh()
    else setData(null)
  }, [status, refresh])

  // Realtime: pedido novo → lista fresca na hora (sem cache — o badge não atrasa).
  useEffect(() => {
    if (status !== "logado") return
    const off = window.launcherAPI?.onFriendRequest(() => refresh(true))
    return () => off?.()
  }, [status, refresh])

  // Background do cache (friends.js): pinta o fresco quando a busca termina.
  useEffect(() => {
    if (status !== "logado") return
    const off = window.launcherAPI?.onFriendsChanged((dados) => {
      if (dados) setData(dados)
    })
    return () => off?.()
  }, [status])

  const pedidos = data?.incoming?.length ?? 0

  return <Ctx.Provider value={{ data, pedidos, refresh }}>{children}</Ctx.Provider>
}

export function useFriends() {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error("useFriends precisa estar dentro de <FriendsProvider>")
  return ctx
}
