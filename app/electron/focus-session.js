"use strict"

/**
 * Gate for the launcher focus state while a child game owns the foreground.
 * It is deliberately independent from Electron so the launch/close ordering
 * can be tested without creating a BrowserWindow.
 */
function createFocusSession({ onFocus } = {}) {
  let active = false
  let restored = false

  function begin() {
    if (active) return false
    active = true
    restored = false
    onFocus?.(false, true)
    return true
  }

  function nativeFocus(focused) {
    const value = Boolean(focused)
    // A compositor/window focus event can arrive while Steam is replacing its
    // URI wrapper with the real game. It must not reopen launcher input.
    if (active && value) return false
    onFocus?.(value)
    return true
  }

  function finish() {
    if (!active || restored) return false
    restored = true
    active = false
    onFocus?.(true, true)
    return true
  }

  return {
    begin,
    nativeFocus,
    finish,
    isActive: () => active,
    isRestored: () => restored,
  }
}

module.exports = { createFocusSession }
