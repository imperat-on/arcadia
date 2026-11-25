// Toast nativo de conquista: janela borderless no canto superior direito,
// click-through e sempre no topo —
// aparece por cima do jogo sem roubar foco nem cliques.
// Fila FIFO: um toast por vez (~6s). O notify.html se fecha sozinho e o
// evento 'closed' dispara o próximo da fila.
const { BrowserWindow, screen } = require("electron")
const path = require("path")
const fs = require("fs")
const { dataPath } = require("./runtime-paths")

const CONFIG = dataPath("config.json")

const HEADING_MAP = {
  "pt-BR": ["Conquista desbloqueada!", "Conquista rara!", "Platina Arcadia"],
  "en-US": ["Achievement unlocked!", "Rare achievement!", "Arcadia Platinum"],
  "es-ES": ["¡Logro desbloqueado!", "¡Logro raro!", "Platino Arcadia"],
}

// Idioma vem do config.json (campo `language`); sem ele, default pt-BR.
function headingTraduzido() {
  try {
    const cfg = JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
    return Object.hasOwn(HEADING_MAP, cfg.language) ? HEADING_MAP[cfg.language] : HEADING_MAP["pt-BR"]
  } catch {
    return HEADING_MAP["pt-BR"]
  }
}

// Mesmo padrão do main.js para a janela principal: o app sempre roda a
// partir do dist/ (`npm start` = `vite build && electron .`, e o Vite copia
// public/ para dist/), então não há bifurcação dev/produção de caminho.
const NOTIFY_HTML = path.join(__dirname, "..", "dist", "notify.html")

// Default toast size (will be scaled based on screen resolution)
const DEFAULT_TOAST_W = 360
const DEFAULT_TOAST_H = 96
const DEFAULT_MARGIN = 16

let toastWin = null
const fila = []

function showAchievementToast(payload, { platinum = false } = {}) {
  // Limite de segurança: rajada de conquistas com o toast travado (loadFile
  // falhou, 'closed' nunca dispara) faria a fila crescer sem limite.
  if (fila.length >= 30) fila.shift()
  fila.push({ ...(payload || {}), _platinum: platinum })
  processarFila()
}

function processarFila() {
  // Um toast por vez: se há janela viva, o 'closed' dela chama de volta.
  if (toastWin && !toastWin.isDestroyed()) return
  const payload = fila.shift()
  if (!payload) return

  // Fixed size toast - no scaling with system zoom
  const TOAST_W = DEFAULT_TOAST_W
  const TOAST_H = DEFAULT_TOAST_H
  const MARGIN = DEFAULT_MARGIN

  // Top-right corner with margin - use bounds (not workArea) to avoid scaling issues
  const { bounds } = screen.getPrimaryDisplay()
  toastWin = new BrowserWindow({
    width: TOAST_W,
    height: TOAST_H,
    x: bounds.x + bounds.width - TOAST_W - MARGIN,
    y: bounds.y + MARGIN,
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
  toastWin.once("ready-to-show", () => toastWin?.showInactive())
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
  const isPlatinum = payload._platinum === true
  const current = toastWin
  current.loadFile(NOTIFY_HTML, {
    query: {
      title: String(payload.title || ""),
      desc: String(payload.gameTitle || payload.desc || ""),
      icon: String(payload.icon || ""),
      heading: headingTraduzido()[isPlatinum ? 2 : payload.rare ? 1 : 0],
      variant: isPlatinum ? "platinum" : payload.rare ? "rare" : "unlock",
      sound: payload.sound === false ? "off" : isPlatinum ? "platinum" : "unlock",
      reducedMotion: payload.reducedMotion ? "1" : "0",
    },
  }).catch(() => { if (!current.isDestroyed()) current.destroy() })
}

// Chamado no before-quit do main.js (mesmo padrão do killActive do
// downloadmanager): sem isso sobraria uma janela always-on-top órfã.
function closeAchievementToast() {
  fila.length = 0
  try {
    if (toastWin && !toastWin.isDestroyed()) toastWin.destroy()
  } catch {}
  toastWin = null
}

module.exports = { showAchievementToast, closeAchievementToast }
