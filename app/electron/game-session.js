"use strict"

function isSteamInstallUri(value) {
  return /^steam:\/\/install\//i.test(String(value || ""))
}

function shouldTrackGameSession(command) {
  return !Array.from(command || []).some(isSteamInstallUri)
}

module.exports = { isSteamInstallUri, shouldTrackGameSession }
