import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

export default defineConfig({
  // Caminhos relativos para o build carregar via file:// no Electron.
  base: "./",
  plugins: [react(), tailwindcss()],
})
