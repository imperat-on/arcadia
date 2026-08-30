// Reexporta os módulos de leitura e monitoramento de conquistas do Arcadia.
// O main process importa tudo daqui (require("./achievements")).

const { startSteamBinWatcher, fetchAchievementsForApp } = require("./steam_bin")
const { loadAchievements, saveAchievements, loadItemIndex } = require("./schema")
const { hasSteamBinFor } = require("./manager")
const uplay = require("./uplay")

module.exports = {
  // Nomes públicos históricos (antes em electron/achievements.js).
  startAchievementWatcher: startSteamBinWatcher,
  fetchAchievementsForApp,
  loadAchievements,
  saveAchievements,
  loadItemIndex,
  // Vigia somente o progresso produzido pelo jogo/Steam.
  startSteamBinWatcher,
  hasSteamBinFor,
  // Provider UPC/voices38 (runtime JSON + preparação segura do schema).
  ...uplay,
}
