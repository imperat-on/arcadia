"use client"

import { useCallback, useEffect, useState } from "react"
import { setControllerConfig } from "./controllerConfig"
import type { Profile } from "../global"
import type { Game } from "./ps5-launcher/types"

export function useLibraryState(initialGames: Game[] = []) {
  const [games, setGames] = useState<Game[]>(initialGames)
  const [profile, setProfile] = useState<Profile>({})
  const [config, setConfig] = useState<Record<string, any>>({})
  const [libraryLoaded, setLibraryLoaded] = useState(false)
  const [configLoaded, setConfigLoaded] = useState(false)

  const reloadLibrary = useCallback(async () => {
    // Biblioteca e config são independentes: pedir em paralelo tira um
    // round-trip IPC do caminho crítico do primeiro paint.
    const [libRes, cfgRes] = await Promise.allSettled([
      window.launcherAPI?.getLibrary(),
      window.launcherAPI?.getConfig(),
    ])
    try {
      const next = libRes.status === "fulfilled" ? libRes.value : undefined
      if (Array.isArray(next)) setGames(next)
    } catch {
      // A biblioteca anterior continua visível enquanto o backend se recupera.
    }
    try {
      const cfg = cfgRes.status === "fulfilled" ? cfgRes.value : undefined
      if (cfg) {
        setConfig(cfg)
        setProfile(cfg.profile || {})
        // Semeia o cache dos laços de gamepad: eles leem zona morta e
        // liga/desliga a 60fps, fora do React, e um config salvo precisa valer
        // já no primeiro quadro.
        setControllerConfig({
          enable_controller_navigation: cfg.enable_controller_navigation,
          controller_deadzone: cfg.controller_deadzone,
        })
      }
    } catch {
      // Configuração indisponível não deve impedir o boot da interface.
    } finally {
      setConfigLoaded(true)
    }
    // Os launchers usam este sinal para aplicar defaults. Só o levante depois
    // de biblioteca E configuração terem tido a primeira oportunidade de
    // chegar; caso contrário, um config salvo pode ser sobrescrito por {}.
    setLibraryLoaded(true)
  }, [])

  useEffect(() => {
    void reloadLibrary()
    return window.launcherAPI?.onLibraryChanged(() => void reloadLibrary())
  }, [reloadLibrary])

  return { games, setGames, profile, setProfile, config, libraryLoaded, configLoaded, reloadLibrary }
}
