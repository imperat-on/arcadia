import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

export default defineConfig({
  // Caminhos relativos para o build embarcar no Tauri (mesma origem do app).
  base: "./",
  plugins: [react(), tailwindcss()],
  // Tauri dev carrega esta porta; keep in sync com tauri.conf.json.
  server: { port: 1420, strictPort: true },
})
