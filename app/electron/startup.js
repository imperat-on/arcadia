"use strict"

function resolveLauncherMode(env = process.env) {
  if (env.ARCADIA_MODE === "console" || env.ARCADIA_MODE === "desktop") return env.ARCADIA_MODE
  return env.PS5_FULLSCREEN === "1" ? "console" : "desktop"
}

function ignoreBrokenPipe(stream) {
  stream?.on?.("error", (error) => {
    if (error?.code !== "EPIPE") throw error
  })
}

module.exports = { resolveLauncherMode, ignoreBrokenPipe }
