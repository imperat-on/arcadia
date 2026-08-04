// Reexporta os módulos de conquistas do Arcadia. O main process importa tudo
// daqui (require("./achievements")) — os nomes públicos de antes são
// preservados por compatibilidade.

const {
  startSteamBinWatcher,
  fetchAchievementsForApp,
  writeAchievementUnlock,
} = require("./steam_bin")
const { loadAchievements, saveAchievements, loadItemIndex } = require("./schema")
const { hasSteamBinFor } = require("./manager")

module.exports = {
  // Nomes públicos históricos (antes em electron/achievements.js).
  startAchievementWatcher: startSteamBinWatcher,
  fetchAchievementsForApp,
  loadAchievements,
  saveAchievements,
  loadItemIndex,
  // Vigia por bin da Steam + desbloqueio forçado no .bin.
  startSteamBinWatcher,
  writeAchievementUnlock,
  hasSteamBinFor,
}
