"use strict"

/**
 * Restores keyboard/mouse input to an Electron window after a child game has
 * finished.  Keeping this tiny adapter free of Electron makes the focus
 * lifecycle testable without booting the main process.
 *
 * The game can leave the launcher minimized, hidden, or merely behind another
 * fullscreen surface.  Restore/show first, then focus both the native window
 * and its renderer.  `onFocused` lets the caller update its own focus state
 * (and is intentionally called even when a compositor declines the focus
 * request; the next native focus event will correct it).
 */
function restoreWindowFocus(window, { onFocused } = {}) {
  if (!window) return false
  try {
    if (typeof window.isDestroyed === "function" && window.isDestroyed()) return false
  } catch {
    return false
  }

  try {
    if (typeof window.isMinimized === "function" && window.isMinimized()) {
      window.restore?.()
    }
  } catch {}

  try {
    if (typeof window.isVisible === "function" && !window.isVisible()) {
      // show() activates the native window, unlike showInactive(), which would
      // leave keyboard/mouse events with the game or the previously active app.
      window.show?.()
    }
  } catch {}

  try {
    // `steal` is meaningful on macOS and harmless elsewhere.  It is useful
    // when the game closed while another window still owns the active slot.
    window.focus?.({ steal: true })
  } catch {
    try {
      window.focus?.()
    } catch {}
  }

  try {
    window.webContents?.focus?.()
  } catch {}

  try {
    onFocused?.()
  } catch {}
  return true
}

module.exports = { restoreWindowFocus }
