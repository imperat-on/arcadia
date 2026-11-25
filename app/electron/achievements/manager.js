// Orquestra o vigia por appid: bin da Steam (se o jogo tem índice de
// block/bit). startWatchersForAppid devolve a função que o derruba.

const { loadItemIndex } = require("./schema")
const { startSteamBinWatcher } = require("./steam_bin")

// True quando achievements.json tem entrada com block/bit para este appid —
// ou seja, quando a detecção por bin da Steam se aplica a ele.
function hasSteamBinFor(appid) {
  const map = loadItemIndex()[String(appid)]
  return Boolean(map && Object.keys(map).length)
}

// Sobe o vigia do bin da Steam do appid e devolve a função que o derruba.
function startWatchersForAppid(_appid, onUnlock) {
  return startSteamBinWatcher(onUnlock)
}

module.exports = { startWatchersForAppid, hasSteamBinFor }
