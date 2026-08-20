const { contextBridge, ipcRenderer } = require("electron")

// Modo da UI: "console" (PS5, tela cheia) ou "desktop" (estilo Heroic, janela).
contextBridge.exposeInMainWorld("launcherMode", process.env.ARCADIA_MODE || "console")

// Caminhos dinâmicos da máquina (NUNCA hardcodar /home/<usuário> no código).
// Sem require("os"/"path"): o preload sandboxed não tem esses módulos.
const HOME = process.env.HOME || ""
const DATA_DIR = process.env.ARCADIA_DATA_DIR || `${HOME}/.local/share/arcadia`
contextBridge.exposeInMainWorld("launcherPaths", {
  home: HOME,
  dataDir: DATA_DIR,
})

// Ponte segura: o renderer (React) só enxerga estas funções.
contextBridge.exposeInMainWorld("launcherAPI", {
  getLibrary: () => ipcRenderer.invoke("library:get"),
  launch: (cmd, gameId, mode) => ipcRenderer.invoke("game:launch", { cmd, gameId, mode }),
  closeGame: () => ipcRenderer.invoke("game:close"),
  winMinimize: () => ipcRenderer.invoke("win:minimize"),
  winMaximize: () => ipcRenderer.invoke("win:maximize"),
  winClose: () => ipcRenderer.invoke("win:close"),
  fixesCheck: (appid) => ipcRenderer.invoke("fixes:check", appid),
  fixesApply: (opts) => ipcRenderer.invoke("fixes:apply", opts),
  fixesStatus: (appid) => ipcRenderer.invoke("fixes:status", appid),
  fixesCancel: (appid) => ipcRenderer.invoke("fixes:cancel", appid),
  fixesInstalled: (opts) => ipcRenderer.invoke("fixes:installed", opts),
  fixesUnfix: (opts) => ipcRenderer.invoke("fixes:unfix", opts),
  fixesLauncherRedirect: (opts) => ipcRenderer.invoke("fixes:launcherRedirect", opts),
  fixesSetRyuuAuth: (key) => ipcRenderer.invoke("fixes:setRyuuAuth", key),
  fixesRyuuAuthStatus: () => ipcRenderer.invoke("fixes:ryuuAuthStatus"),
  fixesClearRyuuAuth: () => ipcRenderer.invoke("fixes:clearRyuuAuth"),
  gamelogOpen: (id) => ipcRenderer.invoke("gamelog:open", id),
  gameUninstall: (game, opts) => ipcRenderer.invoke("game:uninstall", { game, ...(opts || {}) }),
  gameImport: (game) => ipcRenderer.invoke("game:import", game),
  gameSysinfo: (game) => ipcRenderer.invoke("game:sysinfo", game),
  gameProtonDb: (appid) => ipcRenderer.invoke("game:protondb", appid),
  gameStats: (appid) => ipcRenderer.invoke("game:stats", appid),
  storeStatus: () => ipcRenderer.invoke("store:status"),
  storeSearch: (query) => ipcRenderer.invoke("store:search", query),
  storeSuggest: (query) => ipcRenderer.invoke("store:suggest", query),
  storeWarm: () => ipcRenderer.invoke("store:warm"),
  storeRecent: (lista, limite, offset) =>
    ipcRenderer.invoke("store:recent", { lista, limite, offset }),
  /** Capa retrato alternativa (SteamGridDB), só para quem a Steam não publica. */
  storeInstallInfo: (appid) => ipcRenderer.invoke("store:installInfo", appid),
  storeInstall: (payload) => ipcRenderer.invoke("store:install", payload),
  storeEnsureDotnet: () => ipcRenderer.invoke("store:ensureDotnet"),
  storeAddToLibrary: (payload) => ipcRenderer.invoke("store:addToLibrary", payload),
  storeAddToSteam: (payload) => ipcRenderer.invoke("store:addToSteam", payload),
  storeInstallDir: (game) => ipcRenderer.invoke("store:installDir", game),
  storeLibraries: () => ipcRenderer.invoke("store:libraries"),
  storeRemoveFromSteam: (appid) => ipcRenderer.invoke("store:removeFromSteam", appid),
  storeRemoveFromLibrary: (appid) => ipcRenderer.invoke("store:removeFromLibrary", appid),
  storeRemoveDownloaded: (appid) => ipcRenderer.invoke("store:removeDownloaded", appid),
  slssteamLaunch: () => ipcRenderer.invoke("slssteam:launchSteam"),
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
  setFullscreen: (on) => ipcRenderer.invoke("app:setFullscreen", on),
  setZoom: (z, modo) => ipcRenderer.invoke("app:setZoom", z, modo),
  rebuildMeta: () => ipcRenderer.invoke("meta:rebuild"),
  integrationsStatus: () => ipcRenderer.invoke("integrations:status"),
  pickImage: (kind) => ipcRenderer.invoke("profile:pickImage", kind),
  avatarLoad: (url) => ipcRenderer.invoke("avatar:load", url),
  hltbGet: (titulo) => ipcRenderer.invoke("hltb:get", titulo),
  trailerPath: (id) => ipcRenderer.invoke("trailer:path", id),
  trailerDownload: (id, title) => ipcRenderer.invoke("trailer:download", { id, title }),
  trailerSearch: (query) => ipcRenderer.invoke("trailer:search", { query }),
  trailerStreamUrl: (url) => ipcRenderer.invoke("trailer:streamUrl", { url }),
  trailerDownloadUrl: (id, url) => ipcRenderer.invoke("trailer:downloadUrl", { id, url }),
  trailerDownloadAll: () => ipcRenderer.invoke("trailer:downloadAll"),
  trailerPickCookies: () => ipcRenderer.invoke("trailer:pickCookies"),
  sourcesList: () => ipcRenderer.invoke("sources:list"),
  sourcesAdd: (url) => ipcRenderer.invoke("sources:add", url),
  sourcesRemove: (id) => ipcRenderer.invoke("sources:remove", id),
  sourcesSync: () => ipcRenderer.invoke("sources:sync"),
  sourcesSearch: (query, limit) => ipcRenderer.invoke("sources:search", { query, limit }),
  sourcesGame: (ref) => ipcRenderer.invoke("sources:game", ref),
  torrentStart: (payload) => ipcRenderer.invoke("torrent:start", payload),
  torrentPause: (gameId) => ipcRenderer.invoke("torrent:pause", gameId),
  torrentResume: (gameId) => ipcRenderer.invoke("torrent:resume", gameId),
  torrentCancel: (gameId) => ipcRenderer.invoke("torrent:cancel", gameId),
  torrentFiles: (magnet, timeoutMs) => ipcRenderer.invoke("torrent:files", { magnet, timeoutMs }),
  torrentSetLimit: (bytes) => ipcRenderer.invoke("torrent:setLimit", bytes),
  torrentList: () => ipcRenderer.invoke("torrent:list"),
  onTorrentProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("torrent:progress", h)
    return () => ipcRenderer.removeListener("torrent:progress", h)
  },
  pluginsList: () => ipcRenderer.invoke("plugins:list"),
  pluginsInstall: (id) => ipcRenderer.invoke("plugins:install", id),
  pluginsRemove: (id) => ipcRenderer.invoke("plugins:remove", id),
  profileStats: () => ipcRenderer.invoke("profile:stats"),
  achievementsGet: (appid) => ipcRenderer.invoke("achievements:get", appid),
  achievementsForceUnlock: (appid, apiname) =>
    ipcRenderer.invoke("achievements:force:unlock", { appid, apiname }),
  achievementsSchemasLoad: () => ipcRenderer.invoke("achievements:schemas:load"),
  appDiagnostico: () => ipcRenderer.invoke("app:diagnostico"),
  legendaryStatus: () => ipcRenderer.invoke("runner:legendary:status"),
  legendarySetup: () => ipcRenderer.invoke("runner:legendary:setup"),
  legendaryLibrary: () => ipcRenderer.invoke("runner:legendary:library"),
  dmQueue: () => ipcRenderer.invoke("dm:queue"),
  dmInstall: (game) => ipcRenderer.invoke("dm:install", game),
  dmPause: (appid) => ipcRenderer.invoke("dm:pause", appid),
  dmRetry: (appid) => ipcRenderer.invoke("dm:retry", appid),
  dmDismiss: (appid) => ipcRenderer.invoke("dm:dismiss", appid),
  dmResume: (appid) => ipcRenderer.invoke("dm:resume", appid),
  dmSetPriority: (appid, priority) => ipcRenderer.invoke("dm:setPriority", appid, priority),
  dmCancel: (appid) => ipcRenderer.invoke("dm:cancel", appid),
  diskSpace: (p) => ipcRenderer.invoke("app:diskSpace", p),
  onDmProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("dm:progress", h)
    return () => ipcRenderer.removeListener("dm:progress", h)
  },
  wineList: () => ipcRenderer.invoke("wine:list"),
  prefixTool: (appid, tool, opts) =>
    ipcRenderer.invoke("wine:prefixTool", { appid, tool, ...(opts || {}) }),
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
  onGameActive: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("game:active", h)
    return () => ipcRenderer.removeListener("game:active", h)
  },
  onAchievementUnlocked: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("achievement:unlocked", h)
    return () => ipcRenderer.removeListener("achievement:unlocked", h)
  },
  onLibraryChanged: (cb) => {
    const h = () => cb()
    ipcRenderer.on("library:changed", h)
    return () => ipcRenderer.removeListener("library:changed", h)
  },
  onPluginsChanged: (cb) => {
    const h = () => cb()
    ipcRenderer.on("plugins:changed", h)
    return () => ipcRenderer.removeListener("plugins:changed", h)
  },
  // Atualização do próprio Arcadia (git pull + rebuild + reinício).
  updateState: () => ipcRenderer.invoke("update:state"),
  updateCheck: () => ipcRenderer.invoke("update:check"),
  updateApply: (data) => ipcRenderer.invoke("update:apply", data),
  onUpdateAvailable: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("update:available", h)
    return () => ipcRenderer.removeListener("update:available", h)
  },
  onUpdateProgress: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("update:progress", h)
    return () => ipcRenderer.removeListener("update:progress", h)
  },
  onStoreDownloaded: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("store:downloaded", h)
    return () => ipcRenderer.removeListener("store:downloaded", h)
  },
  onLaunchError: (cb) => {
    const h = (_e, payload) => cb(payload)
    ipcRenderer.on("game:launchError", h)
    return () => ipcRenderer.removeListener("game:launchError", h)
  },
  onLaunchWarning: (cb) => {
    const h = (_e, payload) => cb(payload)
    ipcRenderer.on("game:launchWarning", h)
    return () => ipcRenderer.removeListener("game:launchWarning", h)
  },
  // Conta online (backend proprio) — cadastro email+username+senha; login username+senha.
  accountStatus: () => ipcRenderer.invoke("account:status"),
  accountProfile: () => ipcRenderer.invoke("account:profile"),
  accountUpdateProfile: (campos) => ipcRenderer.invoke("account:updateProfile", campos),
  accountSetAvatar: (filePath) => ipcRenderer.invoke("account:setAvatar", filePath),
  accountSetAvatarBytes: (bytes, mime, ext) => ipcRenderer.invoke("account:setAvatarBytes", bytes, mime, ext),
  accountSetBackground: (filePath, kind) => ipcRenderer.invoke("account:setBackground", filePath, kind),
  accountSignUp: (payload) => ipcRenderer.invoke("account:signUp", payload),
  accountSignIn: (payload) => ipcRenderer.invoke("account:signIn", payload),
  accountSignOut: () => ipcRenderer.invoke("account:signOut"),
  onAuthChanged: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("account:changed", h)
    return () => ipcRenderer.removeListener("account:changed", h)
  },
  // Amigos (backend proprio)
  friendsSearch: (query) => ipcRenderer.invoke("friends:search", query),
  friendsSend: (userId) => ipcRenderer.invoke("friends:send", userId),
  friendsAccept: (userId) => ipcRenderer.invoke("friends:accept", userId),
  friendsCancel: (userId) => ipcRenderer.invoke("friends:cancel", userId),
  friendsList: (opts) => ipcRenderer.invoke("friends:list", opts),
  friendsAchievements: (userId) => ipcRenderer.invoke("friends:achievements", userId),
  friendsProfile: (userId) => ipcRenderer.invoke("friends:profile", userId),
  friendsRemove: (userId) => ipcRenderer.invoke("friends:remove", userId),
  onFriendRequest: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("friends:request", h)
    return () => ipcRenderer.removeListener("friends:request", h)
  },
  onFriendsChanged: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("friends:changed", h)
    return () => ipcRenderer.removeListener("friends:changed", h)
  },
  // Sync de conquistas (backend proprio)
  syncNow: () => ipcRenderer.invoke("sync:now"),
  syncState: () => ipcRenderer.invoke("sync:state"),
  onSyncState: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("sync:state", h)
    return () => ipcRenderer.removeListener("sync:state", h)
  },
})
