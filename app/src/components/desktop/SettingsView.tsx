"use client"

import { useEffect, useState } from "react"
import type { AppConfig } from "../../global"
import { IntegrationsSection, MetadataSection } from "../ps5-launcher/SettingsPanel"
import { GeneralSection } from "./GeneralSection"
import { StoreSetup } from "./StoreSetup"
import { AccessibilityView } from "./AccessibilityView"
import { EmulationSection } from "./EmulationSection"

type Sub = "gerais" | "integracoes" | "metadados" | "acessibilidade" | "emulacao"

// Configurações do modo desktop: o conteúdo das seções reais; a sub-navegação
// (Integrações/Metadados/Emulação) fica expandida na sidebar principal.
export function SettingsView({ sub, onSaved }: { sub: Sub; onSaved: () => void }) {
  const [cfg, setCfg] = useState<AppConfig>({})

  useEffect(() => {
    window.launcherAPI?.getConfig().then((c) => setCfg(c || {}))
  }, [sub])

  return (
    <div className="h-full overflow-y-auto px-8 py-6">
      {sub === "gerais" && <GeneralSection onSaved={onSaved} />}
      {sub === "integracoes" && (
        <>
          <StoreSetup />
          <IntegrationsSection cfg={cfg} />
        </>
      )}
      {sub === "metadados" && <MetadataSection onSaved={onSaved} />}
      {sub === "acessibilidade" && <AccessibilityView />}
      {sub === "emulacao" && <EmulationSection />}
    </div>
  )
}
