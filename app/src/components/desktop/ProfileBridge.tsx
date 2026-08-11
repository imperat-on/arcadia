"use client"

import { useEffect, useRef } from "react"
import type { Profile } from "../../global"
import { useAccount } from "../account/AccountContext"

export function ProfileBridge({
  perfilLocal,
  setPerfilLocal,
}: {
  perfilLocal: Profile
  setPerfilLocal: React.Dispatch<React.SetStateAction<Profile>>
}) {
  const { perfil } = useAccount()
  const original = useRef<Profile | null>(null)
  const jaInicializou = useRef(false)

  useEffect(() => {
    if (!perfil) {
      if (original.current) {
        setPerfilLocal(original.current)
        original.current = null
        jaInicializou.current = false
      }
      return
    }
    // Salva o original ANTES do primeiro merge (perfil local PURO,
    // nunca mergeado de sessão anterior — jaInicializou garante isso)
    if (!jaInicializou.current) {
      original.current = perfilLocal
      jaInicializou.current = true
    }
    setPerfilLocal((p) => ({
      ...p,
      // Logado: o servidor é a fonte da verdade — SEM fallback pro local,
      // senão a conta anterior vaza campos (summary/país/avatar...).
      name: perfil.display_name || perfil.username || "",
      avatar: perfil.avatar_url || "",
      summary: perfil.summary ?? "",
      country: perfil.country ?? "",
      city: perfil.city ?? "",
      background: perfil.background_url ?? "",
      // NOTE: showcase NÃO vem do online aqui — o perfil online é buscado uma
      // vez no login e fica STALE; mergeá-lo sobrescrevia a vitrine editada
      // com a lista antiga do servidor (bug da contagem/0 de seleção). A
      // vitrine é local (config.json) e sobe via updatePerfil ao editar.
    }))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [perfil])

  return null
}
