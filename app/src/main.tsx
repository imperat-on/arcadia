import { createRoot } from "react-dom/client"
import "@fontsource/inter/400.css"
import "@fontsource/inter/500.css"
import "@fontsource/inter/600.css"
import "@fontsource/inter/700.css"
import "./index.css"
import { PS5Launcher } from "./components/ps5-launcher/PS5Launcher"
import { DesktopLauncher } from "./components/desktop/DesktopLauncher"

// Modo console (PS5, padrão) x modo desktop (estilo Heroic) — o backend é o
// mesmo; muda só a UI montada na raiz.
const root = window.launcherMode === "desktop" ? <DesktopLauncher /> : <PS5Launcher />

createRoot(document.getElementById("root")!).render(root)
