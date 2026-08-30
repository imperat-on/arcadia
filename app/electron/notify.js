// Toast nativo de conquista desbloqueada (estilo Steam): janela borderless/
// transparente no canto inferior direito, click-through e sempre no topo —
// aparece por cima do jogo sem roubar foco nem cliques.
// Fila FIFO: um toast por vez (~6s). O notify.html se fecha sozinho e o
// evento 'closed' dispara o próximo da fila.
const { BrowserWindow, screen } = require("electron")
const path = require("path")
const fs = require("fs")
const { dataPath } = require("./runtime-paths")

const CONFIG = dataPath("config.json")

const HEADING_MAP = {
  "pt-BR": "Conquista desbloqueada!",
  "en-US": "Achievement unlocked!",
  "es-ES": "¡Logro desbloqueado!",
}

// Idioma vem do config.json (campo `language`); sem ele, default pt-BR.
function headingTraduzido() {
  try {
    const cfg = JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
    return HEADING_MAP[cfg.language] || HEADING_MAP["pt-BR"]
  } catch {
    return HEADING_MAP["pt-BR"]
  }
}

// Mesmo padrão do main.js para a janela principal: o app sempre roda a
// partir do dist/ (`npm start` = `vite build && electron .`, e o Vite copia
// public/ para dist/), então não há bifurcação dev/produção de caminho.
const NOTIFY_HTML = path.join(__dirname, "..", "dist", "notify.html")

const TOAST_W = 360
const TOAST_H = 96
const MARGIN = 16

let toastWin = null
const fila = []

function showAchievementToast(payload, { platinum = false } = {}) {
  // Limite de segurança: rajada de conquistas com o toast travado (loadFile
  // falhou, 'closed' nunca dispara) faria a fila crescer sem limite.
  if (fila.length >= 30) fila.shift()
  fila.push(payload || {})
  processarFila()
}

function processarFila() {
  // Um toast por vez: se há janela viva, o 'closed' dela chama de volta.
  if (toastWin && !toastWin.isDestroyed()) return
  const payload = fila.shift()
  if (!payload) return

  // Canto inferior direito da área de trabalho do monitor primário.
  const { workArea } = screen.getPrimaryDisplay()
  toastWin = new BrowserWindow({
    width: TOAST_W,
    height: TOAST_H,
    x: workArea.x + workArea.width - TOAST_W - MARGIN,
    y: workArea.y + workArea.height - TOAST_H - MARGIN,
    frame: false,
    transparent: true,
    resizable: false,
    skipTaskbar: true,
    focusable: false,
    alwaysOnTop: true,
    show: false,
    webPreferences: {
      // O som de unlock toca sem gesto do usuário.
      autoplayPolicy: "no-user-gesture-required",
      contextIsolation: true,
      nodeIntegration: false,
    },
  })
  // Por cima de tudo, inclusive jogo em tela cheia.
  toastWin.setAlwaysOnTop(true, "screen-saver")
  toastWin.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
  // Click-through: o toast nunca atrapalha o jogo.
  toastWin.setIgnoreMouseEvents(true)
  toastWin.once("ready-to-show", () => toastWin?.show())
  // Timeout de segurança: se o loadFile falhar, o 'closed' nunca dispara e a
  // fila ficaria presa — destrói e segue para o próximo da fila.
  const seguro = setTimeout(() => {
    if (toastWin && !toastWin.isDestroyed()) toastWin.destroy()
  }, 8000)
  toastWin.on("closed", () => {
    clearTimeout(seguro)
    toastWin = null
    processarFila()
  })
  // Calculate done/total from payload if available
  const done = payload.done || 0
  const total = payload.total || 0

  toastWin.loadFile(NOTIFY_HTML, {
    query: {
      title: String(payload.title || ""),
      desc: String(payload.desc || ""),
      icon: String(payload.icon || ""),
      heading: headingTraduzido(),
      sound: platinum ? "platinum" : "unlock",
      done: String(done),
      total: String(total),
    },
  })
}

// Chamado no before-quit do main.js (mesmo padrão do killActive do
// downloadmanager): sem isso sobraria uma janela always-on-top órfã.
function closeAchievementToast() {
  try {
    if (toastWin && !toastWin.isDestroyed()) toastWin.destroy()
  } catch {}
  toastWin = null
  fila.length = 0
}

module.exports = { showAchievementToast, closeAchievementToast }
