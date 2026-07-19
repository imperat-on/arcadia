const { contextBridge, ipcRenderer } = require("electron")

// Modo da UI: "console" (PS5, tela cheia) ou "desktop" (estilo Heroic, janela).
contextBridge.exposeInMainWorld("launcherMode", process.env.ARCADIA_MODE || "console")

// Caminhos dinâmicos da máquina (NUNCA hardcodar /home/<usuário> no código).
// Sem require("os"/"path"): o preload sandboxed não tem esses módulos.
const HOME = process.env.HOME || ""
contextBridge.exposeInMainWorld("launcherPaths", {
  home: HOME,
  dataDir: `${HOME}/.local/share/arcadia`,
})

// Ponte segura: o renderer (React) só enxerga estas funções.
contextBridge.exposeInMainWorld("launcherAPI", {
  getLibrary: () => ipcRenderer.invoke("library:get"),
  launch: (cmd, gameId) => ipcRenderer.invoke("game:launch", { cmd, gameId }),
  closeGame: () => ipcRenderer.invoke("game:close"),
  gamelogOpen: (id) => ipcRenderer.invoke("gamelog:open", id),
  gameUninstall: (game, opts) => ipcRenderer.invoke("game:uninstall", { game, ...(opts || {}) }),
  gameImport: (game) => ipcRenderer.invoke("game:import", game),
  gameSysinfo: (game) => ipcRenderer.invoke("game:sysinfo", game),
  storeStatus: () => ipcRenderer.invoke("store:status"),
  storeSearch: (query) => ipcRenderer.invoke("store:search", query),
  storeRecent: () => ipcRenderer.invoke("store:recent"),
  storeInstallInfo: (appid) => ipcRenderer.invoke("store:installInfo", appid),
  storeInstall: (payload) => ipcRenderer.invoke("store:install", payload),
  storeEnsureDotnet: () => ipcRenderer.invoke("store:ensureDotnet"),
  storeAddToSteam: (payload) => ipcRenderer.invoke("store:addToSteam", payload),
  storeCheckFixes: (appid) => ipcRenderer.invoke("store:checkFixes", appid),
  storeApplyFix: (payload) => ipcRenderer.invoke("store:applyFix", payload),
  storeInstallDir: (game) => ipcRenderer.invoke("store:installDir", game),
  storeLibraries: () => ipcRenderer.invoke("store:libraries"),
  storeRemoveFromSteam: (appid) => ipcRenderer.invoke("store:removeFromSteam", appid),
  storeRemoveDownloaded: (appid) => ipcRenderer.invoke("store:removeDownloaded", appid),
  slssteamLaunch: () => ipcRenderer.invoke("slssteam:launchSteam"),
  slssteamInstall: () => ipcRenderer.invoke("slssteam:install"),
  customGameAdd: (data) => ipcRenderer.invoke("customgame:add", data),
  customGameUpdate: (data) => ipcRenderer.invoke("customgame:update", data),
  customGameRunInstaller: (opts) => ipcRenderer.invoke("customgame:runInstaller", opts),
  refresh: () => ipcRenderer.invoke("library:refresh"),
  setOverride: (id, patch) => ipcRenderer.invoke("overrides:set", { id, patch }),
  pickArt: (id, kind) => ipcRenderer.invoke("art:pick", { id, kind }),
  searchArt: (gameId, titulo, kind, dimensions, sgdbId) =>
    ipcRenderer.invoke("meta:art", { gameId, titulo, kind, dimensions, sgdbId }),
  searchText: (gameId, titulo) => ipcRenderer.invoke("meta:text", { gameId, titulo }),
  downloadArt: (id, kind, url) => ipcRenderer.invoke("art:download", { id, kind, url }),
  getNews: () => ipcRenderer.invoke("news:get"),
  openExternal: (url) => ipcRenderer.invoke("app:openExternal", url),
  getConfig: () => ipcRenderer.invoke("config:get"),
  setConfig: (cfg) => ipcRenderer.invoke("config:set", cfg),
  quit: () => ipcRenderer.invoke("app:quit"),
  enterConsole: () => ipcRenderer.invoke("app:enterConsole"),
  toggleFullscreen: () => ipcRenderer.invoke("app:toggleFullscreen"),
  setZoom: (z) => ipcRenderer.invoke("app:setZoom", z),
  rebuildMeta: () => ipcRenderer.invoke("meta:rebuild"),
  integrationsStatus: () => ipcRenderer.invoke("integrations:status"),
  pickImage: (kind) => ipcRenderer.invoke("profile:pickImage", kind),
  trailerPath: (id) => ipcRenderer.invoke("trailer:path", id),
  trailerDownload: (id, title) => ipcRenderer.invoke("trailer:download", { id, title }),
  trailerSearch: (query) => ipcRenderer.invoke("trailer:search", { query }),
  trailerStreamUrl: (url) => ipcRenderer.invoke("trailer:streamUrl", { url }),
  trailerDownloadUrl: (id, url) => ipcRenderer.invoke("trailer:downloadUrl", { id, url }),
  trailerDownloadAll: () => ipcRenderer.invoke("trailer:downloadAll"),
  trailerPickCookies: () => ipcRenderer.invoke("trailer:pickCookies"),
  achievementsGet: (appid) => ipcRenderer.invoke("achievements:get", appid),
  slscheevoStatus: () => ipcRenderer.invoke("slscheevo:status"),
  slscheevoSetup: () => ipcRenderer.invoke("slscheevo:setup"),
  profileStats: () => ipcRenderer.invoke("profile:stats"),
  achievementsRecent: () => ipcRenderer.invoke("achievements:recent"),
  legendaryStatus: () => ipcRenderer.invoke("runner:legendary:status"),
  legendarySetup: () => ipcRenderer.invoke("runner:legendary:setup"),
  legendaryLibrary: () => ipcRenderer.invoke("runner:legendary:library"),
  dmQueue: () => ipcRenderer.invoke("dm:queue"),
  dmInstall: (game) => ipcRenderer.invoke("dm:install", game),
  dmPause: (appid) => ipcRenderer.invoke("dm:pause", appid),
  dmResume: (appid) => ipcRenderer.invoke("dm:resume", appid),
  dmCancel: (appid) => ipcRenderer.invoke("dm:cancel", appid),
  diskSpace: (p) => ipcRenderer.invoke("app:diskSpace", p),
  onDmProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("dm:progress", h)
    return () => ipcRenderer.removeListener("dm:progress", h)
  },
  wineList: () => ipcRenderer.invoke("wine:list"),
  wineInstall: (id, kind) => ipcRenderer.invoke("wine:install", { id, kind }),
  wineRemove: (id) => ipcRenderer.invoke("wine:remove", id),
  onWineProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("wine:progress", h)
    return () => ipcRenderer.removeListener("wine:progress", h)
  },
  prefixTool: (appid, tool, opts) => ipcRenderer.invoke("wine:prefixTool", { appid, tool, ...(opts || {}) }),
  wineRunExe: (appid, opts) => ipcRenderer.invoke("wine:runExe", { appid, ...(opts || {}) }),
  gameSettingsGet: (id) => ipcRenderer.invoke("gamesettings:get", id),
  gameSettingsSet: (id, patch) => ipcRenderer.invoke("gamesettings:set", { id, patch }),
  pickFolder: () => ipcRenderer.invoke("app:pickFolder"),
  pickFile: () => ipcRenderer.invoke("app:pickFile"),
  onTrailerProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("trailer:progress", h)
    return () => ipcRenderer.removeListener("trailer:progress", h)
  },
  onTrailerDlProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("trailer:dlprogress", h)
    return () => ipcRenderer.removeListener("trailer:dlprogress", h)
  },
  onAchievementUnlocked: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("achievement:unlocked", h)
    return () => ipcRenderer.removeListener("achievement:unlocked", h)
  },
  onAppFocus: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("app:focus", h)
    return () => ipcRenderer.removeListener("app:focus", h)
  },
  onGameRunning: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("game:running", h)
    return () => ipcRenderer.removeListener("game:running", h)
  },
  onLibraryChanged: (cb) => {
    const h = () => cb()
    ipcRenderer.on("library:changed", h)
    return () => ipcRenderer.removeListener("library:changed", h)
  },
  onStoreDownloaded: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("store:downloaded", h)
    return () => ipcRenderer.removeListener("store:downloaded", h)
  },
})

