import { lazy, Suspense, useEffect, useRef, useState } from "react"
import { ModeProvider, useMode } from "./components/ModeContext"
import { createRoot } from "react-dom/client"
import "@fontsource/inter/400.css"
import "@fontsource/inter/500.css"
import "@fontsource/inter/600.css"
import "@fontsource/inter/700.css"
import "./index.css"
import "./themes/fullscreen/base.css"
import "./themes/fullscreen/accessibility.css"
import "./themes/fullscreen/builtin-default.css"
import "./themes/fullscreen/builtin-aurora.css"
import { I18nProvider } from "./i18n/I18nContext"
import { AccountProvider } from "./components/account/AccountContext"
import { FriendsProvider } from "./components/account/FriendsContext"
import { FullscreenThemeProvider } from "./components/themes/FullscreenThemeContext"
import { FullscreenThemeStyle } from "./components/themes/FullscreenThemeStyle"

// O modo inicial é conhecido pelo processo principal. Carregar somente a UI
// escolhida evita baixar o desktop inteiro durante o boot do Big Picture (e
// vice-versa), reduzindo o custo do primeiro render.
const PS5Launcher = lazy(() =>
  import("./components/ps5-launcher/PS5Launcher").then(({ PS5Launcher }) => ({ default: PS5Launcher })),
)
const DesktopLauncher = lazy(() =>
  import("./components/desktop/DesktopLauncher").then(({ DesktopLauncher }) => ({ default: DesktopLauncher })),
)

function LauncherLoading() {
  return (
    <div className="min-h-screen bg-[#05070d] text-white/70 grid place-items-center">
      <div className="text-xs tracking-[0.3em] uppercase animate-pulse">Arcadia</div>
    </div>
  )
}

// Modo console (PS5, padrão) x modo desktop (estilo Heroic) — o backend é o
// mesmo; muda só a UI montada na raiz. F11 alterna em runtime.
function Root() {
  const { mode, setMode } = useMode()
  const modeRef = useRef(mode)
  const [erro, setErro] = useState<string | null>(null)

  useEffect(() => {
    modeRef.current = mode
  }, [mode])

  useEffect(() => {
    const offErr = window.launcherAPI?.onLaunchError?.((p) => setErro(p.error))
    const offWarn = window.launcherAPI?.onLaunchWarning?.((p) => setErro(p.warnings.join("\n")))
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F11") {
        e.preventDefault()
        const next = modeRef.current === "console" ? "desktop" : "console"
        void setMode(next).catch((error) => setErro(String(error.message || error)))
      }
    }
    window.addEventListener("keydown", onKey)
    return () => {
      offErr?.()
      offWarn?.()
      window.removeEventListener("keydown", onKey)
    }
  }, [setMode])
  return (
    <AccountProvider>
      <FriendsProvider>
        <Suspense fallback={<LauncherLoading />}>
          {mode === "console" ? (
            <FullscreenThemeProvider>
              <FullscreenThemeStyle />
              <PS5Launcher />
            </FullscreenThemeProvider>
          ) : (
            <DesktopLauncher />
          )}
        </Suspense>
        {erro && (
        <div className="fixed bottom-6 right-6 z-[9999] max-w-md rounded-lg bg-red-900/95 text-white px-5 py-4 shadow-2xl border border-red-500/50 backdrop-blur">
          <div className="font-bold mb-1">Falha ao lançar jogo</div>
          <div className="text-sm whitespace-pre-line opacity-90">{erro}</div>
          <button onClick={() => setErro(null)} className="mt-3 text-xs underline">
            Fechar
          </button>
        </div>
      )}
      </FriendsProvider>
    </AccountProvider>
  )
}

createRoot(document.getElementById("root")!).render(
  <I18nProvider>
    <ModeProvider>
      <Root />
    </ModeProvider>
  </I18nProvider>,
)
