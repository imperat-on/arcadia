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
  const gameId = Array.isArray(payload) ? undefined : payload?.gameId
  let rawCmd = gameId ? null : Array.isArray(payload) ? payload : payload?.cmd
  const mode = Array.isArray(payload) ? undefined : payload?.mode
  let envExtra = {}
  let cwd = ""

  if (typeof gameId === "string" && gameId) {
    if (gameId.startsWith("custom:")) {
      // Jogos custom de emulador não possuem exe/launch_cmd: o perfil local
      // ainda passa pelo mesmo callback seguro usado pelos jogos da biblioteca.
      const settings = getGameSettings(gameId) || {}
      const customGame = findGame(gameId)
      const emulated = customGame ? emulatorLaunch(gameId, customGame, settings) : null
      if (emulated) {
        if (!emulated.ok || !Array.isArray(emulated.cmd) || !emulated.cmd.length) return emulated
        rawCmd = emulated.cmd
        envExtra = emulated.env || {}
        cwd = emulated.cwd || ""
      } else {
        const built = customLaunchCmd(gameId)
        if (!built?.cmd?.length) {
          return { ok: false, error: `Jogo custom não encontrado em custom_games.json (id: ${gameId}).` }
        }
        rawCmd = built.cmd
        envExtra = built.env || {}
        cwd = built.cwd || ""
      }
    } else {
      const game = findGame(gameId)
      if (!game) return { ok: false, error: "Jogo não encontrado na biblioteca." }

      const settings = getGameSettings(gameId) || {}
      // Jogos retro não têm launch_cmd, mas só um perfil persistido autoriza o
      // registry a montar argv. Um payload do renderer nunca escolhe o comando.
      if (mode !== "steam" && settings.emulatorId) {
        const emulated = emulatorLaunch(gameId, game, settings)
        if (emulated) {
          if (!emulated.ok || !Array.isArray(emulated.cmd) || !emulated.cmd.length) return emulated
          rawCmd = emulated.cmd
          envExtra = emulated.env || {}
          cwd = emulated.cwd || ""
        }
      }
      if (!rawCmd && mode === "exe" && settings.exePath) {
        const built = exeLaunchCmd(gameId, settings.exePath)
        if (!built?.cmd?.length) {
          return { ok: false, error: "Executável configurado não encontrado (exePath vazio)." }
        }
        rawCmd = built.cmd
        envExtra = built.env || {}
        cwd = built.cwd || ""
      }

      // Se não conseguiu comando via emulador/exe, usa launch_cmd da biblioteca
      if (!rawCmd) {
        if (!Array.isArray(game.launch_cmd) || !game.launch_cmd.length) {
          return { ok: false, error: "Sem comando de lançamento (cmd vazio). Verifique o executável do jogo em Configurações." }
        }
        rawCmd = game.launch_cmd
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
  return { ok: true, gameId, mode, rawCmd: rawCmd.map((part) => String(part)), envExtra, cwd }
}

module.exports = { resolveLaunchRequest }
