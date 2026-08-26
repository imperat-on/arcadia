"use strict"

// IPC handlers para o sistema de temas Fullscreen.
// Registra os handlers no ipcMain e conecta ao themeService.

const path = require("node:path")
const { THEME_PROTOCOL } = require("./constants")
const { createPackageInstaller } = require("./package")

function registerThemeIpc({ ipcMain, themeService, protocolHandler, dialog, browserWindow }) {
  if (!ipcMain || !themeService) return

  // Instalador de pacotes
  const installer = createPackageInstaller({
    themesDir: path.join(themeService.registry._themesDir || "", ".."),
    registry: themeService.registry,
  })

  // Emite evento para todas as janelas
  function emitChanged(payload) {
    if (!browserWindow) return
    const wins = browserWindow.getAllWindows ? browserWindow.getAllWindows() : []
    for (const w of wins) {
      if (!w.isDestroyed()) {
        w.webContents.send("fullscreenThemes:changed", payload)
      }
    }
  }

  ipcMain.handle("fullscreenThemes:list", async () => {
    return themeService.list()
  })

  ipcMain.handle("fullscreenThemes:get", async (_e, id) => {
    if (typeof id !== "string") return null
    return themeService.get(id)
  })

  // Payload seguro com CSS normalizado, layouts e opções.
  ipcMain.handle("fullscreenThemes:getPayload", async (_e, id) => {
    if (typeof id !== "string") return null
    return themeService.getPayload(id)
  })

  ipcMain.handle("fullscreenThemes:activate", async (_e, id) => {
    if (typeof id !== "string") return { ok: false, error: "id_invalido" }
    const result = themeService.activate(id)
    if (result.ok) {
      emitChanged({ reason: "activate", activeId: themeService.getActiveId(), pendingId: id })
    }
    return result
  })

  ipcMain.handle("fullscreenThemes:confirmReady", async (_e, id) => {
    if (typeof id !== "string") return { ok: false, error: "id_invalido" }
    const pendingId = themeService.getPendingId()
    if (pendingId !== id) return { ok: false, error: "pendente_diferente" }
    const result = themeService.confirmActivation(id)
    if (result.ok) {
      emitChanged({ reason: "confirmed", activeId: id, pendingId: null })
    }
    return result
  })

  ipcMain.handle("fullscreenThemes:rollbackPending", async () => {
    themeService.rollbackPending()
    emitChanged({ reason: "rollback", activeId: themeService.getActiveId(), pendingId: null })
    return { ok: true }
  })

  ipcMain.handle("fullscreenThemes:remove", async (_e, id) => {
    if (typeof id !== "string") return { ok: false, error: "id_invalido" }
    const result = themeService.remove(id)
    if (result.ok) {
      if (protocolHandler) protocolHandler.unregisterTheme(id)
      emitChanged({ reason: "remove", changedId: id, activeId: themeService.getActiveId() })
    }
    return result
  })

  // Importação: path permanece apenas no main. Retorna resultado público.
  ipcMain.handle("fullscreenThemes:import", async () => {
    if (!dialog || !browserWindow) return { ok: false, error: "dialog_indisponivel" }
    const win = browserWindow.getFocusedWindow() || browserWindow.getAllWindows()[0]
    if (!win) return { ok: false, error: "janela_nao_encontrada" }

    const result = await dialog.showOpenDialog(win, {
      title: "Importar tema Arcadia",
      filters: [{ name: "Tema Arcadia", extensions: ["arcadiatheme"] }],
      properties: ["openFile"],
    })

    if (result.canceled || !result.filePaths.length) {
      return { ok: false, error: "cancelado" }
    }

    const filePath = result.filePaths[0]

    // Instala o pacote ZIP (valida, extrai, registra)
    try {
      const installResult = installer.installFromZip(filePath)
      if (installResult.ok && protocolHandler) {
        const themeDir = path.join(
          themeService.registry._themesDir || "",
          "fullscreen",
          installResult.id,
        )
        protocolHandler.registerTheme(installResult.id, themeDir)
      }
      if (installResult.ok) {
        emitChanged({ reason: "install", changedId: installResult.id, activeId: themeService.getActiveId() })
      }
      // Nunca devolve filePath ao renderer
      return { ok: installResult.ok, id: installResult.id, version: installResult.version, error: installResult.error, errors: installResult.errors }
    } catch (err) {
      return { ok: false, error: String(err.message || err) }
    }
  })

  ipcMain.handle("fullscreenThemes:recover", async () => {
    const recovered = themeService.recoverToLastKnownGood()
    emitChanged({ reason: "recover", activeId: recovered, pendingId: null })
    return { ok: true, id: recovered }
  })

  ipcMain.handle("fullscreenThemes:getActiveId", async () => {
    return themeService.getActiveId()
  })
}

module.exports = { registerThemeIpc }
