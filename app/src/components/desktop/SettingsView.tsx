"use client"

import { useEffect, useState } from "react"
import type { AppConfig, IntegrationsStatus } from "../../global"
import {
  IntegrationsSection,
  MetadataSection,
} from "../ps5-launcher/SettingsPanel"
import { GeneralSection } from "./GeneralSection"
import { StoreSetup } from "./StoreSetup"

type Sub = "gerais" | "integracoes" | "metadados"

// Configurações do modo desktop: o conteúdo das seções reais; a sub-navegação
// (Integrações/Temas/Metadados) fica expandida na sidebar principal.
export function SettingsView({ sub, onSaved }: { sub: Sub; onSaved: () => void }) {
  const [cfg, setCfg] = useState<AppConfig>({})
  const [status, setStatus] = useState<IntegrationsStatus | null>(null)

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setCfg(c || {}))
    window.launcherAPI?.integrationsStatus().then(setStatus)
  }, [sub]) // recarrega ao trocar de seção (status sempre fresco)

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      {sub === "gerais" && <GeneralSection onSaved={onSaved} />}
      {sub === "integracoes" && (
        <>
          <StoreSetup />
          <IntegrationsSection
            cfg={cfg}
            status={status}
            onSaveKey={async (steam_api_key, steam_id64) => {
              await window.launcherAPI?.setConfig({ steam_api_key, steam_id64 })
              setCfg((c) => ({ ...c, steam_api_key, steam_id64 }))
              onSaved()
            }}
            onToggle={async (name, val) => {
              setCfg((c) => ({ ...c, sources: { ...(c.sources || {}), [name]: val } }))
              await window.launcherAPI?.setConfig({ sources: { [name]: val } })
              onSaved()
            }}
            onSlsPath={async (path) => {
              setCfg((c) => ({ ...c, slssteam_path: path }))
              await window.launcherAPI?.setConfig({ slssteam_path: path })
              onSaved()
            }}
          />
        </>
      )}
      {sub === "metadados" && <MetadataSection onSaved={onSaved} />}
    </div>
  )
}
