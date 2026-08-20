"use strict"

/**
 * Resolve somente a intenção de lançamento. Spawn, Wine, SLSsteam e Electron
 * ficam fora deste módulo; callbacks permitem testar a política sem executar
 * processos ou ler a máquina.
 */
function resolveLaunchRequest(
  payload,
  {
    findGame = () => null,
    customLaunchCmd = () => null,
    getGameSettings = () => ({}),
    exeLaunchCmd = () => null,
    emulatorLaunch = () => null,
  } = {},
) {
  let rawCmd = Array.isArray(payload) ? payload : payload?.cmd
  const gameId = Array.isArray(payload) ? undefined : payload?.gameId
  const mode = Array.isArray(payload) ? undefined : payload?.mode
  let envExtra = {}

  if (typeof gameId === "string" && gameId) {
    if (gameId.startsWith("custom:")) {
      // Jogos custom de emulador não possuem exe/launch_cmd: o perfil local
      // ainda passa pelo mesmo callback seguro usado pelos jogos da biblioteca.
      const settings = getGameSettings(gameId) || {}
      const emulated = emulatorLaunch(gameId, findGame(gameId), settings)
      if (emulated) {
        if (!emulated.ok || !Array.isArray(emulated.cmd) || !emulated.cmd.length) return emulated
        rawCmd = emulated.cmd
        envExtra = emulated.env || {}
      } else {
        const built = customLaunchCmd(gameId)
        if (!built?.cmd?.length) {
          return { ok: false, error: `Jogo custom não encontrado em custom_games.json (id: ${gameId}).` }
        }
        rawCmd = built.cmd
        envExtra = built.env || {}
      }
    } else {
      const game = findGame(gameId)
      if (!game) return { ok: false, error: "Jogo não encontrado na biblioteca." }
      if (!Array.isArray(game.launch_cmd) || !game.launch_cmd.length) {
        return { ok: false, error: "Sem comando de lançamento (cmd vazio). Verifique o executável do jogo em Configurações." }
      }
      rawCmd = game.launch_cmd

      if (mode !== "steam") {
        const settings = getGameSettings(gameId) || {}
        const emulated = emulatorLaunch(gameId, game, settings)
        if (emulated) {
          if (!emulated.ok || !Array.isArray(emulated.cmd) || !emulated.cmd.length) return emulated
          rawCmd = emulated.cmd
          envExtra = emulated.env || {}
        } else {
          const exe = settings.exePath
          if (exe) {
            const built = exeLaunchCmd(gameId, exe)
            if (!built?.cmd?.length) {
              return { ok: false, error: "Executável configurado não encontrado (exePath vazio)." }
            }
            rawCmd = built.cmd
            envExtra = built.env || {}
          }
        }
      }
    }
  } else if (Array.isArray(rawCmd)) {
    const legado = rawCmd.map((command) => String(command))
    if (!(legado.length === 2 && legado[0] === "steam" && /^steam:\/\/(install|run)\/[0-9]+$/.test(legado[1]))) {
      return { ok: false, error: "Comando de lançamento rejeitado (padrão não permitido)." }
    }
    rawCmd = legado
  }

  if (!Array.isArray(rawCmd) || rawCmd.length === 0) {
    return { ok: false, error: "Sem comando de lançamento (cmd vazio). Verifique o executável do jogo em Configurações." }
  }
  return { ok: true, gameId, mode, rawCmd: rawCmd.map((part) => String(part)), envExtra }
}

module.exports = { resolveLaunchRequest }
