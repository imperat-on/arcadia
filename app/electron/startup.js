"use strict"

function resolveLauncherMode(env = process.env, config = {}) {
  if (env.ARCADIA_MODE === "console" || env.ARCADIA_MODE === "desktop") return env.ARCADIA_MODE
  if (env.ARCADIA_FORCE_DESKTOP === "1") return "desktop"
  if (typeof config.start_in_console_mode === "boolean") {
    return config.start_in_console_mode ? "console" : "desktop"
  }
  return env.PS5_FULLSCREEN === "1" ? "console" : "desktop"
}

function ignoreBrokenPipe(stream) {
  stream?.on?.("error", (error) => {
    if (error?.code !== "EPIPE") throw error
  })
}

module.exports = { resolveLauncherMode, ignoreBrokenPipe }
