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

/**
 * Serialises one launch from its first asynchronous step through confirmed
 * process termination.  A token is returned for each launch; stale callbacks
 * from Steam or a pre-launch script can therefore never mutate a later launch.
 *
 * This is intentionally a small, Electron-free state machine.  `starting`
 * remains busy until the caller either confirms that no process was created or
 * observes the launched process group exit.  Stop changes the state to
 * `stopping`; it does not release the token, so a late spawn callback cannot
 * start a game after the user pressed Stop.
 */
function createLaunchSession({ onState } = {}) {
  let current = null
  let nextId = 0

  function notify(state, token) {
    try {
      onState?.(state, token, current?.meta)
    } catch {
      // State notifications are diagnostics only and must not break cleanup.
    }
  }

  function begin(meta = {}) {
    if (current) return null
    const token = { id: ++nextId }
    current = { token, state: "starting", meta }
    notify("starting", token)
    return token
  }

  function owns(token) {
    return Boolean(current && current.token === token)
  }

  function markRunning(token) {
    if (!owns(token) || current.state !== "starting") return false
    current.state = "running"
    notify("running", token)
    return true
  }

  function requestStop(token) {
    if (!current || (token && current.token !== token)) {
      return { ok: false, state: "idle" }
    }
    if (current.state === "stopping") {
      return { ok: true, state: "stopping", already: true, token: current.token }
    }
    current.state = "stopping"
    current.stopRequested = true
    notify("stopping", current.token)
    return { ok: true, state: "stopping", already: false, token: current.token }
  }

  function finish(token) {
    if (!owns(token)) return false
    current = null
    notify("idle", token)
    return true
  }

  function getState(token) {
    if (token && !owns(token)) return "idle"
    return current?.state || "idle"
  }

  // A replay needs the generation as well as the state.  Without it, an idle
  // event from an older launch could clear a newer pending launch of the same
  // game after a renderer reload.
  function getSnapshot() {
    if (!current) return { state: "idle", token: null, meta: {} }
    return { state: current.state, token: current.token, meta: current.meta }
  }

  return {
    begin,
    markRunning,
    requestStop,
    finish,
    owns,
    getState,
    getSnapshot,
    isBusy: () => current !== null,
    isStarting: (token) => getState(token) === "starting",
    isRunning: (token) => getState(token) === "running",
    isStopping: (token) => getState(token) === "stopping",
  }
}

module.exports = { createFocusSession, createLaunchSession }
