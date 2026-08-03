import { useState, useEffect } from "react"
import { createRoot } from "react-dom/client"
import "@fontsource/inter/400.css"
import "@fontsource/inter/500.css"
import "@fontsource/inter/600.css"
import "@fontsource/inter/700.css"
import "./index.css"
import { I18nProvider } from "./i18n/I18nContext"
import { PS5Launcher } from "./components/ps5-launcher/PS5Launcher"
import { DesktopLauncher } from "./components/desktop/DesktopLauncher"

// Modo console (PS5, padrão) x modo desktop (estilo Heroic) — o backend é o
// mesmo; muda só a UI montada na raiz. F11 alterna em runtime.
function Root() {
  const [modo, setModo] = useState<"console" | "desktop">(
    (window.launcherMode as any) || "console"
  )
  const [erro, setErro] = useState<string | null>(null)
  useEffect(() => {
    window.launcherAPI?.setFullscreen(modo === "console")
    const offErr = window.launcherAPI?.onLaunchError?.((p) => setErro(p.error))
    const offWarn = window.launcherAPI?.onLaunchWarning?.((p) => setErro(p.warnings.join("\n")))
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F11") {
        e.preventDefault()
        setModo(m => {
          const novo = m === "console" ? "desktop" : "console"
          window.launcherAPI?.setFullscreen(novo === "console")
          return novo
        })
      }
    }
    window.addEventListener("keydown", onKey)
    return () => { offErr?.(); offWarn?.(); window.removeEventListener("keydown", onKey) }
  }, [])
  return (
    <>
      {modo === "console" ? <PS5Launcher /> : <DesktopLauncher />}
      {erro && (
        <div className="fixed bottom-6 right-6 z-[9999] max-w-md rounded-lg bg-red-900/95 text-white px-5 py-4 shadow-2xl border border-red-500/50 backdrop-blur">
          <div className="font-bold mb-1">Falha ao lançar jogo</div>
          <div className="text-sm whitespace-pre-line opacity-90">{erro}</div>
          <button onClick={() => setErro(null)} className="mt-3 text-xs underline">Fechar</button>
        </div>
      )}
    </>
  )
}

createRoot(document.getElementById("root")!).render(
  <I18nProvider>
    <Root />
  </I18nProvider>
)
