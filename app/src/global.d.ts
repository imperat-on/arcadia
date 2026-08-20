import type { Game } from "./components/ps5-launcher/types"

export interface Profile {
  name?: string
  realName?: string
  country?: string
  city?: string
  summary?: string
  avatar?: string // caminho file:// da foto/gif
  background?: string // fundo do projeto (atmosfera de tela inteira)
  background_blur?: boolean // se o fundo fica borrado (padrão true)
  banner?: string // faixa de capa no topo do perfil (estilo Steam)
  showcase?: string[] // ids dos jogos em destaque na vitrine
  owner?: boolean
}

/** Permissões declarativas reconhecidas pelo SDK de plugins locais. */
export type PluginPermission =
  | "library:read"
  | "library:write"
  | "games:read"
  | "games:launch"
  | "events:subscribe"
  | "commands:register"
  | "network"
  | "filesystem:read"
  | "filesystem:write"
  | "process:spawn"
  | "notifications"
  | "settings:read"
  | "settings:write"

export interface PluginManifest {
  manifestVersion: 1
  apiVersion: 1
  id: string
  name: string
  version: string
  description?: string
  entry: string
  entrySha256?: string
  permissions: PluginPermission[]
}

/** Resultado de verificação; nunca contém paths locais. */
export interface PluginVerification {
  id: string
  ok: boolean
  valid: boolean
  verified: boolean
  declared: boolean
  algorithm: "sha256"
  expectedDigest: string
  actualDigest: string
  digest: string
  source: "registry" | "manifest" | "none" | "builtin"
  error: string
}

/** Metadados de plugin devolvidos pelo main (não inclui caminho privado). */
export interface PluginInfo {
  id: string
  name: string
  descKey: string
  installed: boolean
  enabled: boolean
  manifest?: PluginManifest | null
  valid?: boolean
  error?: string
  source?: "builtin" | "local"
}

/** Review publica da comunidade (dados sem credenciais ou paths locais). */
export interface CommunityReview {
  id: number
  appid: string
  title?: string | null
  text: string
  rating: number
  positive: boolean
  hours: number
  created_at: string
  updated_at: string
  user_id: string
  username: string
  display_name?: string | null
  avatar_url?: string | null
  author?: {
    id: string
    username: string
    display_name?: string | null
    avatar_url?: string | null
  }
}

export interface CommunityCollectionItem {
  appid: string
  position: number
  title?: string | null
  note?: string | null
  created_at?: string
}

export interface CommunityCollection {
  id: string
  title: string
  description?: string | null
  visibility: "public" | "unlisted" | "private"
  created_at: string
  updated_at: string
  user_id: string
  username: string
  display_name?: string | null
  avatar_url?: string | null
  owner?: {
    id: string
    username: string
    display_name?: string | null
    avatar_url?: string | null
  }
  item_count: number
  items?: CommunityCollectionItem[]
}

export interface CommunityPagination {
  limit: number
  offset: number
  has_more: boolean
}

export interface CommunityReviewsResult {
  ok: boolean
  reviews: CommunityReview[]
  items: CommunityReview[]
  pagination?: CommunityPagination
  offline?: boolean
  error?: string
  code?: string
}

export interface CommunityCollectionsResult {
  ok: boolean
  collections: CommunityCollection[]
  lists: CommunityCollection[]
  items: CommunityCollection[]
  pagination?: CommunityPagination
  offline?: boolean
  error?: string
  code?: string
}

export interface Sources {
  steam?: boolean
  heroic?: boolean
  lutris?: boolean
  slssteam?: boolean
  psn?: boolean
}

/** Uma arte candidata devolvida pela busca online. */
export interface ArtCandidate {
  fonte: string
  url: string
  thumb: string
  largura: number
  altura: number
  animado: boolean
  autor: string
}

/** Uma descrição candidata devolvida pela busca online. */
export interface TextCandidate {
  fonte: string
  texto: string
}

/** Por que este clone não pode se atualizar sozinho. */
export type UpdateMotivo = "sem-git" | "branch" | "sujo" | "nao-enviado"

export interface UpdateState {
  podeAtualizar: boolean
  motivo?: UpdateMotivo
  /** Contexto do motivo: nome da branch, ou quantos arquivos/commits. */
  detalhe?: string
}

export interface UpdateInfo {
  ok: boolean
  error?: string
  /** Commit local, curto. */
  local?: string
  /** Quantos commits o GitHub tem a mais que nós. */
  atrasado?: number
  commits?: { sha: string; titulo: string }[]
  /** O package-lock mudou — vai precisar de `npm install`. */
  depsMudaram?: boolean
}

export type UpdateEtapa = "pull" | "deps" | "build" | "pronto"

export interface AppConfig {
  steam_api_key?: string
  steamgriddb_api_key?: string
  steam_id64?: string
  ui_scale?: number
  /** API key do Hubcap (catálogo de manifestos Steam, aba Lojas). */
  hubcap_api_key?: string
  /** Tokens dos serviços debrid (aba Integrações). Ter QUALQUER um já basta —
   * a ordem de tentativa é RD → TorBox → AllDebrid → Premiumize. */
  realdebrid_token?: string
  torbox_token?: string
  alldebrid_token?: string
  premiumize_token?: string
  /** Zoom do modo console (separado do desktop para não conflitarem). */
  console_ui_scale?: number
  card_scale?: number
  accent?: string
  sources?: Sources
  slssteam_path?: string
  psn_npsso?: string
  trailer_auto?: boolean
  youtube_cookies?: string
  profile?: Profile
  // Acessibilidade (modo desktop)
  theme_name?: string // id do tema em src/themes.ts (midnight, dracula, nord, gruvbox…)
  content_font?: string
  actions_font?: string
  custom_css_path?: string // pasta com .css injetados ao abrir
  tiles_color?: boolean // capas sempre coloridas
  always_titles?: boolean // títulos sempre visíveis (padrão true)
  no_click_outside?: boolean // não fechar diálogos clicando fora
  no_smooth_scroll?: boolean
  no_anim?: boolean
  // Configurações Globais → Config. Gerais
  language?: string // ex.: "en-US" (padrão), "pt-BR", "es-ES"
  default_install_path?: string // pasta padrão de instalação de jogos
  default_wine_prefix_path?: string // pasta p/ novos prefixos Wine
  steam_path?: string // instalação local da Steam
  epic_egs_prefix?: string // prefixo onde o EGS está instalado
  check_updates_on_start?: boolean
  auto_update_games?: boolean
  hide_changelog_on_start?: boolean
  start_in_console_mode?: boolean
  hide_tray_icon?: boolean
  close_to_tray?: boolean
  start_minimized?: boolean
  minimize_on_game_launch?: boolean
  dark_tray_icon?: boolean
  frameless_window?: boolean
  auto_desktop_shortcuts?: boolean
  auto_start_menu_shortcuts?: boolean
  auto_add_to_steam?: boolean
  disable_playtime_tracking?: boolean
  discord_rich_presence?: boolean
  discord_client_id?: string
  library_featured_column?: "disabled" | "recent" | "favorites" | "most-played"
  recent_games_max?: number
  download_cpu_cores?: number // 0 = máximo
  library_sidebar?: boolean // mostra a lista de jogos na sidebar (estilo Hydra)
}

export interface NewsItem {
  id: string
  title: string
  summary: string
  source: string
  url: string
  image: string
  date: string // ISO 8601
}

export interface YoutubeResult {
  id: string
  url: string
  title: string
  duration: number
  channel: string
  thumbnail: string
}

export interface IntegrationsStatus {
  steam: boolean
  slssteam: number
  heroic: boolean
}

/** Estatísticas agregadas do perfil (jogos/horas jogadas). */
export interface ProfileStats {
  jogos: number
  playtime_hours: number
}

/** Item da fila de downloads (Epic via Legendary). */
/** Fonte de download (JSON estilo Hydra). */
export interface SourceInfo {
  id: string
  url: string
  name: string
  count?: number
  addedAt?: number
}

/** Jogo do índice leve das fontes (sem uris — essas vêm do sourcesGame). */
export interface SourceGame {
  ref: string
  title: string
  fileSize: string
  uploadDate: string
  src: string
}

/** Jogo completo de uma fonte (lido do disco sob demanda). */
export interface SourceGameFull {
  title: string
  uris?: string[]
  uri?: string
  fileSize?: string
  uploadDate?: string
  [k: string]: unknown
}

/** Arquivo dentro de um torrent (para download seletivo). */
export interface TorrentFileInfo {
  index: number
  path: string
  length: number
}

/** Download torrent ativo/concluído (torrent_state.json + status vivo). */
export interface TorrentItem {
  gameId: string
  url: string
  savePath: string
  title: string
  cover?: string
  engine?: "http" | "debrid" // ausente = torrent (libtorrent)
  cacheando?: boolean // debrid baixando o torrent pro servidor dele
  erro?: string
  pausado?: boolean
  completo?: boolean
  progress?: number
  downloadSpeed?: number
  uploadSpeed?: number
  numPeers?: number
  numSeeds?: number
  bytesDownloaded?: number
  fileSize?: number
  folderName?: string
}

export interface DiagnosticsReport {
  version: 1
  generated_at: string
  app: { version: string }
  runtime: { platform: string; release: string; arch: string; node: string; electron: string }
  storage: {
    writable: boolean
    data_dir_configured: boolean
    library: { present: boolean; bytes: number }
    downloads: { present: boolean; bytes: number }
    snapshots: number
  }
  library: { total: number; by_launcher: Record<string, number> }
  downloads: { total: number; by_status: Record<string, number> }
}

export interface SaveSnapshot {
  version: 1
  id: string
  gameId: string
  created_at: string
  label: string
  source_name: string
}

export interface DmItem {
  appid: string // epic:<app_name>
  appName: string
  title: string
  cover: string
  status: "queued" | "downloading" | "paused" | "done" | "error" | "canceled"
  percent: number
  done: number // arquivos baixados (Legendary reporta % por arquivo)
  total: number // total de arquivos
  doneMiB?: number // MiB baixados (linha "Downloaded" do Legendary)
  eta: string
  speed: number // MiB/s
  error: string
  priority?: number // prioridade [-10,10] da fila persistida
  installPath?: string // pasta escolhida no diálogo de instalação
}

/** Versão de Wine/Proton (instalada ou disponível p/ baixar). */
export interface WineVer {
  id: string
  name: string
  path?: string
  wine?: string
  url?: string
  size?: number // MiB
  /** Data de lançamento (ISO) — só para disponíveis. */
  releaseDate?: string
  /** Origem da versão: GE-Proton (baixável), Wine-GE (baixável) ou Proton da Steam (detectado). */
  kind?: "ge-proton" | "wine-ge" | "steam"
}

/** Configurações por jogo (diálogo estilo Heroic). Salvas em game_settings.json. */
export interface GameSettings {
  /** Id da versão do Wine (do wine:list) usada nas ferramentas do prefixo. */
  wineVersion?: string
  /** Prefixo customizado (vazio = padrão ~/.local/share/arcadia/prefixes/<id>). */
  prefixPath?: string
  autoDXVK?: boolean
  autoNVAPI?: boolean
  autoVKD3D?: boolean
  esync?: boolean
  fsync?: boolean
  wineWayland?: boolean
  wow64?: boolean
  fsrHack?: boolean
  /** Rodar o jogo dentro do gamescope (não se aplica a jogos Steam). */
  gamescope?: boolean
  gsWidth?: number
  gsHeight?: number
  gsFps?: number
  /** DXVK_HUD ("" desligado; ex.: "fps", "full"). */
  dxvkHud?: string
  mangohud?: boolean
  /** Rodar via gamemoderun (Feral GameMode). Não se aplica a jogos Steam. */
  gamemode?: boolean
  /** Grava stdout/stderr do jogo em logs/<id>.log. */
  verboseLogs?: boolean
  /** Argumentos extras passados após o comando do jogo. */
  gameArgs?: string
  /** Executável customizado (aba Localizações); vazio = launch_cmd padrão. */
  exePath?: string
  /** Script executado antes do jogo iniciar. */
  scriptPre?: string
  /** Script executado quando o jogo fechar. */
  scriptPost?: string
  /** Wrappers customizados: [wrapper, ...args, ...comando]. */
  wrappers?: { cmd: string; args: string }[]
  /** Variáveis de ambiente extras. */
  envVars?: { name: string; value: string }[]
}

declare global {
  /** Sessão da conta online (backend proprio) — só dados seguros, nunca tokens. */
  interface AccountSession {
    user: { id: string; email?: string; username?: string }
  }

  /** Perfil de usuário no contexto de amigos. */
  interface FriendProfile {
    id: string
    username: string
    display_name?: string | null
    avatar_url?: string | null
    status?: "pending" | "accepted" | null
    incoming?: boolean
    /** ISO da data em que a amizade foi aceita (presente na lista). */
    since?: string | null
  }

  /** Conquista pública de um amigo (RPC friend_achievements). */
  interface FriendAchievement {
    appid: string
    apiname: string
    unlocked_at: string
    updated_at: string
    title?: string | null
    icon?: string | null
    percent?: number | null
  }

  /** Lista estruturada de relações do usuário logado. */
  interface FriendsListData {
    friends: FriendProfile[]
    incoming: FriendProfile[]
    outgoing: FriendProfile[]
  }

  /** Estado do sync de conquistas (espelhado do main process). */
  interface SyncState {
    lastPullAt: number | null
    lastSyncAt: number | null
    lastError: string | null
    queueLen: number
  }

  interface Window {
    /** Modo da UI: console (PS5) ou desktop (estilo Heroic). */
    launcherMode?: "console" | "desktop"
    /** Caminhos dinâmicos da máquina do usuário. */
    launcherPaths?: { home: string; dataDir: string }
    launcherAPI?: {
      getLibrary: () => Promise<Game[]>
      /** Conta online (backend proprio) — fluxo por código enviado por email (OTP). */
      accountStatus: () => Promise<{ session: AccountSession | null; error?: string | null }>
      accountProfile: () => Promise<{
        ok: boolean
        profile?: {
          username: string | null
          avatar_url: string | null
          display_name?: string | null
          summary?: string | null
          country?: string | null
          city?: string | null
          showcase?: string[]
        }
        error?: string
      }>
      accountUpdateProfile: (campos: {
        display_name?: string
        summary?: string
        country?: string
        city?: string
        showcase?: string[]
        background_url?: string
      }) => Promise<{ ok: boolean; error?: string }>
      accountSetBackground: (
        filePath: string,
        kind?: "background" | "banner",
      ) => Promise<{ ok: boolean; background_url?: string; error?: string }>
      accountSetAvatar: (
        filePath: string,
      ) => Promise<{ ok: boolean; avatar_url?: string; error?: string }>
      accountSetAvatarBytes: (
        bytes: Uint8Array,
        mime: string,
        ext: string,
      ) => Promise<{ ok: boolean; avatar_url?: string; error?: string }>
      accountSignUp: (payload: {
        email: string
        username: string
        password: string
      }) => Promise<{ ok: boolean; error?: string; usernameReal?: string }>
      accountSignIn: (payload: {
        username: string
        password: string
      }) => Promise<{ ok: boolean; error?: string; usernameReal?: string }>
      accountSignOut: () => Promise<{ ok: boolean; error?: string }>
      onAuthChanged: (
        cb: (data: { event: string; session: AccountSession | null }) => void,
      ) => () => void
      /** Amigos (backend proprio). */
      friendsSearch: (
        query: string,
      ) => Promise<{ ok: boolean; results?: FriendProfile[]; error?: string }>
      friendsSend: (userId: string) => Promise<{ ok: boolean; error?: string }>
      friendsAccept: (userId: string) => Promise<{ ok: boolean; error?: string }>
      friendsCancel: (userId: string) => Promise<{ ok: boolean; error?: string }>
      friendsList: (opts?: { forcar?: boolean }) => Promise<{ ok: boolean; data?: FriendsListData; deCache?: boolean; error?: string }>
      friendsAchievements: (
        userId: string,
      ) => Promise<{ ok: boolean; achievements?: FriendAchievement[]; error?: string }>
      friendsProfile: (userId: string) => Promise<{
        ok: boolean
        error?: string
        profile?: Profile & { username?: string | null; avatar_url?: string | null; display_name?: string | null; background_url?: string | null; banner_url?: string | null }
        games?: { appid: string; title: string; platform?: string; minutes?: number }[]
        friends?: FriendProfile[]
        stats?: ProfileStats
      }>
      friendsRemove: (userId: string) => Promise<{ ok: boolean; error?: string }>
      onFriendRequest: (cb: (data: { from?: string }) => void) => () => void
      onFriendsChanged: (cb: (data: FriendsListData) => void) => () => void
      /** Sync de conquistas (backend proprio). */
      syncNow: () => Promise<{ ok: boolean; pushed?: number; pulled?: number; error?: string }>
      syncState: () => Promise<SyncState>
      onSyncState: (cb: (st: SyncState) => void) => () => void
      /** Reviews/listas da comunidade; GET usa cache local quando offline. */
      communityReviews: (appid: string, options?: { limit?: number; offset?: number }) => Promise<CommunityReviewsResult>
      communityReviewCreate: (payload: { appid: string; title?: string; text: string; rating: number; positive?: boolean; hours?: number }) => Promise<{ ok: boolean; review?: CommunityReview; error?: string; code?: string }>
      communityReviewUpdate: (id: string | number, payload: Partial<CommunityReview>) => Promise<{ ok: boolean; review?: CommunityReview; error?: string; code?: string }>
      communityReviewRemove: (id: string | number) => Promise<{ ok: boolean; error?: string; code?: string }>
      communityReviewReport: (id: string | number, payload: { reason: string; details?: string }) => Promise<{ ok: boolean; report?: { id: number; status: string }; error?: string; code?: string }>
      communityCollections: (options?: { limit?: number; offset?: number; mine?: boolean; owner?: string; visibility?: string }) => Promise<CommunityCollectionsResult>
      communityCollectionGet: (id: string) => Promise<{ ok: boolean; collection?: CommunityCollection; data?: CommunityCollection; offline?: boolean; error?: string; code?: string }>
      communityCollectionCreate: (payload: { title: string; description?: string; visibility?: string; items?: CommunityCollectionItem[] }) => Promise<{ ok: boolean; collection?: CommunityCollection; error?: string; code?: string }>
      communityCollectionUpdate: (id: string, payload: Partial<CommunityCollection> & { items?: CommunityCollectionItem[] }) => Promise<{ ok: boolean; collection?: CommunityCollection; error?: string; code?: string }>
      communityCollectionRemove: (id: string) => Promise<{ ok: boolean; error?: string; code?: string }>
      communityCollectionAddItem: (id: string, appid: string) => Promise<{ ok: boolean; items?: CommunityCollectionItem[]; error?: string; code?: string }>
      communityCollectionReplaceItems: (id: string, items: CommunityCollectionItem[]) => Promise<{ ok: boolean; items?: CommunityCollectionItem[]; error?: string; code?: string }>
      communityCollectionRemoveItem: (id: string, appid: string) => Promise<{ ok: boolean; error?: string; code?: string }>
      communityCollectionReport: (id: string, payload: { reason: string; details?: string }) => Promise<{ ok: boolean; report?: { id: number; status: string }; error?: string; code?: string }>
      launch: (
        cmd: string[],
        gameId?: string,
        mode?: "steam" | "exe",
      ) => Promise<{ ok: boolean; error?: string; warnings?: string[] }>
      /** Fecha o jogo em execução (mata o processo do jogo). */
      closeGame: () => Promise<{ ok: boolean; error?: string }>
      fixesCheck: (appid: string) => Promise<{
        ok: boolean
        appid?: string
        generic?: { available: boolean; status: number; url?: string }
        online?: { available: boolean; status: number; url?: string }
        crack?: {
          available: boolean
          status: number
          url?: string
          badge?: string
          gameName?: string
          requiresAuth?: boolean
        }
        authConfigured?: boolean
      }>
      fixesApply: (opts: {
        appid: string
        url: string
        type: "generic" | "online" | "crack"
        installPath: string
      }) => Promise<{ ok: boolean; error?: string; errorCode?: string }>
      fixesStatus: (appid: string) => Promise<{
        status: string
        bytesRead?: number
        totalBytes?: number
        error?: string
        errorCode?: string
      }>
      fixesCancel: (appid: string) => Promise<{ ok: boolean }>
      fixesInstalled: (opts: {
        appid: string
        installPath: string
      }) => Promise<{ ok: boolean; installed: boolean }>
      fixesUnfix: (opts: {
        appid: string
        installPath: string
      }) => Promise<{ ok: boolean; error?: string }>
      fixesLauncherRedirect: (opts: {
        installPath: string
      }) => Promise<{ ok: boolean; redirect: string | null }>
      fixesSetRyuuAuth: (key: string) => Promise<{ ok: boolean }>
      fixesRyuuAuthStatus: () => Promise<{ configured: boolean }>
      fixesClearRyuuAuth: () => Promise<{ ok: boolean }>
      winMinimize: () => Promise<void>
      /** Alterna maximizar/restaurar; resolve com o novo estado maximizado. */
      winMaximize: () => Promise<boolean>
      winClose: () => Promise<void>
      /** Abre o log de lançamento do jogo (logs/<id>.log). */
      gamelogOpen: (id: string) => Promise<{ ok: boolean; error?: string }>
      /** Desinstala o jogo (Steam via URI dela; Epic via legendary). */
      gameUninstall: (
        game: Game,
        opts?: { removePrefix?: boolean; removeSettings?: boolean },
      ) => Promise<{ ok: boolean; error?: string }>
      /** Importa uma instalação existente do jogo (legendary import). */
      gameImport: (game: Game) => Promise<{ ok: boolean; error?: string }>
      /** Adiciona um jogo manualmente à biblioteca. */
      customGameAdd: (data: {
        id: string
        title: string
        platform: "windows" | "linux"
        exe: string
      }) => Promise<{ ok: boolean; error?: string; games?: Game[] }>
      /** Edita um jogo custom existente (título/executável). */
      customGameUpdate: (data: {
        id: string
        title?: string
        exe?: string
      }) => Promise<{ ok: boolean; error?: string; games?: Game[] }>
      /** Roda um instalador .exe no prefixo (botão "Executar instalador antes"). */
      customGameRunInstaller: (opts: {
        appid: string
        wine?: string
        prefix?: string
      }) => Promise<{ ok: boolean; error?: string }>
      /** Tamanhos reais (Epic) + requisitos (Steam) da página do jogo. */
      gameSysinfo: (game: Game) => Promise<{
        ok: boolean
        error?: string
        info?: {
          download_size?: number
          disk_size?: number
          version?: string
          req_min?: string
          req_rec?: string
          appid?: string
          short_description?: string
          about?: string
          publishers?: string[]
          developers?: string[]
          release_date?: string
          controller_support?: string
          languages?: string
          header?: string
          background?: string
          screenshots?: { thumb: string; full: string }[]
          movies?: { id: number; name: string; thumb: string; mp4: string; webm: string }[]
        }
      }>
      /** ProtonDB: tier, Steam Deck e score da compatibilidade (Linux). */
      gameProtonDb: (appid: string | number) => Promise<{
        ok: boolean
        error?: string
        info?: {
          tier?: string
          score?: number | null
          deckCompatibility?: string
          total?: number
          url?: string
        } | null
      }>
      /** Estatísticas (SteamSpy) + resumo de reviews (Steam), APIs públicas. */
      gameStats: (appid: string | number) => Promise<{
        ok: boolean
        error?: string
        info?: {
          owners?: string
          ccu?: number
          reviewDesc?: string
          reviewPositivePct?: number | null
          totalReviews?: number
          comments?: {
            author: string
            text: string
            positive: boolean
            hours: number
            helpful: number
          }[]
        } | null
      }>
      /** Snapshots locais de saves; o caminho nunca é devolvido como token/rede. */
      savesList: (gameId: string) => Promise<SaveSnapshot[]>
      savesCreate: (payload: { gameId: string; sourceDir: string; label?: string }) => Promise<{
        ok: boolean
        error?: string
        snapshot?: SaveSnapshot
      }>
      savesRestore: (payload: { gameId: string; snapshotId: string; targetDir: string; backup?: boolean }) => Promise<{
        ok: boolean
        error?: string
        backupPath?: string
        snapshot?: SaveSnapshot
      }>
      savesDelete: (payload: { gameId: string; snapshotId: string }) => Promise<{ ok: boolean; error?: string }>
      /** Loja Steam: status dos pré-requisitos (dotnet, depotdownloader, slssteam, key). */
      storeStatus: () => Promise<{
        dotnet?: string
        depotdownloader: boolean
        hubcapKey: boolean
        slssteam: boolean
        luatools?: boolean
        steamDir: string
        adicionados?: string[]
      }>
      /** Loja Steam: busca no catálogo Hubcap. */
      storeSearch: (query: string) => Promise<{
        ok: boolean
        error?: string
        jogos?: { appid: string; title: string; cover?: string; manifest?: boolean }[]
      }>
      /** Abre a conexão com a Steam antes da primeira busca (evita ~3s de TLS). */
      storeWarm: () => Promise<{ ok: boolean; error?: string }>
      /** Loja Steam: sugestões rápidas enquanto digita (só títulos). */
      storeSuggest: (
        query: string,
      ) => Promise<{
        ok: boolean
        error?: string
        jogos?: { appid: string; title: string; cover?: string }[]
      }>
      /** Loja Steam: mais jogados. Sem argumento, os da quinzena. Paginado. */
      storeRecent: (
        lista?: string,
        limite?: number,
        offset?: number,
      ) => Promise<{
        ok: boolean
        error?: string
        jogos?: { appid: string; title: string; cover?: string; manifest?: boolean }[]
        offset?: number
        total?: number
      }>
      /** Loja Steam: manifesto/depots/token de um appid. */
      storeInstallInfo: (appid: string) => Promise<{
        ok: boolean
        error?: string
        depots?: { depotId: string; manifestId: string; key: string }[]
        token?: string
        dlcs?: string[]
        fonte?: string
      }>
      /** Loja Steam: enfileira o download via DepotDownloader. */
      storeInstall: (payload: {
        appid: string
        title: string
        cover?: string
        installdir: string
        depots: { depotId: string; manifestId: string; key: string }[]
        token?: string
        dlcs?: string[]
        steamDir?: string
      }) => Promise<{ ok: boolean; error?: string; plugin?: string }>
      /** Instala o .NET 9 local (necessário ao DepotDownloader). */
      storeEnsureDotnet: () => Promise<{ ok: boolean; error?: string; path?: string }>
      /** Adiciona o jogo só à biblioteca do Arcadia. Não toca na Steam. */
      storeAddToLibrary: (payload: {
        appid: string
        title?: string
        cover?: string
        capa?: string
        hero?: string
        heroi?: string
      }) => Promise<{ ok: boolean; error?: string }>
      /** Adiciona o jogo à Steam sem baixar (lua no stplug-in + AdditionalApps). */
      storeAddToSteam: (payload: {
        appid: string
        token?: string
        dlcs?: string[]
        title?: string
      }) => Promise<{ ok: boolean; error?: string; plugin?: string }>
      /** Pasta de instalação do jogo. */
      storeInstallDir: (game: Game) => Promise<{ path: string }>
      /** Bibliotecas Steam detectadas (multi-drive) com espaço livre. */
      storeLibraries: () => Promise<{ path: string; steamDir: string; free: number }[]>
      /** Desfaz o "Add": tira o jogo da SLSsteam (lua + AdditionalApps). */
      storeRemoveFromSteam: (appid: string) => Promise<{ ok: boolean; error?: string }>
      /** Remove jogo só da biblioteca do Arcadia. */
      storeRemoveFromLibrary: (appid: string) => Promise<{ ok: boolean; error?: string }>
      /** Remove jogo baixado/adicionado: pasta + appmanifest marcado + SLSsteam. */
      storeRemoveDownloaded: (
        appid: string,
      ) => Promise<{ ok: boolean; removidos?: number; error?: string }>
      /** Reinicia a Steam com a SLSsteam carregada (jogos aparecem como owned). */
      slssteamLaunch: () => Promise<{ ok: boolean; error?: string }>
      refresh: () => Promise<Game[]>
      /** Notícias de jogos (RSS PT-BR), já normalizadas e cacheadas. */
      getNews: () => Promise<NewsItem[]>
      /** Abre um link no navegador padrão do sistema. */
      openExternal: (url: string) => Promise<void>
      /** Salva edições do usuário (null num campo desfaz a edição). */
      setOverride: (
        id: string,
        patch: Partial<Game> | Record<string, unknown> | null,
      ) => Promise<Game[]>
      /** Escolhe uma arte para um jogo e copia para art/. Devolve o caminho. */
      pickArt: (
        id: string,
        kind: "cover" | "hero" | "logo",
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      /** Procura arte online nas fontes ligadas. `dimensions` filtra resolução. */
      searchArt: (
        gameId: string,
        titulo: string,
        kind: "cover" | "hero" | "logo",
        dimensions?: string[],
        sgdbId?: number,
      ) => Promise<{
        ok: boolean
        candidatos?: ArtCandidate[]
        jogos?: { id: number; titulo: string; ano?: number }[]
        erros?: string[]
        error?: string
      }>
      /** Procura descrições nas fontes ligadas. */
      searchText: (
        gameId: string,
        titulo: string,
      ) => Promise<{ ok: boolean; textos?: TextCandidate[]; erros?: string[] }>
      /** Baixa a arte escolhida para art/. Devolve o caminho salvo. */
      downloadArt: (
        id: string,
        kind: "cover" | "hero" | "logo",
        url: string,
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      getConfig: () => Promise<AppConfig>
      diagnostics: () => Promise<DiagnosticsReport>
      diagnosticsExport: () => Promise<{ ok: boolean; canceled?: boolean; path?: string; files?: string[]; error?: string }>
      setConfig: (
        cfg: Partial<AppConfig>,
      ) => Promise<{ ok: boolean; error?: string; config?: AppConfig }>
      quit: () => Promise<void>
      /** Entra no modo console (PS5, tela cheia) — fecha o desktop. */
      enterConsole: () => Promise<{ ok: boolean; error?: string }>
      toggleFullscreen: () => Promise<void>
      setFullscreen: (on: boolean) => Promise<void>
      setZoom: (z: number, modo?: "console" | "desktop") => Promise<number>
      rebuildMeta: () => Promise<Game[]>
      integrationsStatus: () => Promise<IntegrationsStatus>
      pickImage: (
        kind: "avatar" | "banner" | "background",
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      avatarLoad: (url: string) => Promise<{ ok: boolean; src?: string; error?: string }>
      /** Caminho local (file://) do trailer já baixado, ou "" se não houver. */
      trailerPath: (id: string) => Promise<{ path: string }>
      /** HowLongToBeat: tempos de jogo em horas (min), ou null se falhar. */
      hltbGet: (
        titulo: string,
      ) => Promise<{ main: number; mainExtra: number; completionist: number; ts: number } | null>
      /** Baixa o trailer do YouTube via yt-dlp. Devolve o caminho local. */
      trailerDownload: (
        id: string,
        title: string,
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      /** Lista vídeos do YouTube (sem baixar) para escolha manual. */
      trailerSearch: (
        query: string,
      ) => Promise<{ ok: boolean; results?: YoutubeResult[]; error?: string }>
      /** URL direta (mp4) para pré-visualizar o vídeo num <video>. */
      trailerStreamUrl: (url: string) => Promise<{ ok: boolean; url?: string; error?: string }>
      /** Baixa um vídeo específico do YouTube como trailer do jogo. */
      trailerDownloadUrl: (
        id: string,
        url: string,
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      /** Baixa todos os trailers que faltam. Devolve quantos baixou. */
      trailerDownloadAll: () => Promise<{ ok: boolean; count?: number; error?: string }>
      /** Escolhe o arquivo cookies.txt do YouTube (restrição de idade). */
      trailerPickCookies: () => Promise<{ ok: boolean; path?: string }>
      /** Foco real da janela vindo do processo principal (cobre gamescope). */
      onAppFocus: (cb: (focused: boolean) => void) => () => void
      /** Transições de jogo rodando (true = abriu, false = fechou). */
      onGameRunning: (cb: (running: boolean) => void) => () => void
      onGameActive: (cb: (info: { rodando: boolean; gameId: string }) => void) => () => void
      /** Biblioteca mudou no disco (download concluído, desinstalação). */
      onLibraryChanged: (cb: () => void) => () => void
      /** Falha no lançamento do jogo (main process → renderer). */
      onLaunchError: (cb: (payload: { gameId?: string; error: string }) => void) => () => void
      /** Avisos do lançamento (ex.: wrapper não instalado). */
      onLaunchWarning: (
        cb: (payload: { gameId?: string; warnings: string[] }) => void,
      ) => () => void
      /** Este clone pode receber atualização automática? */
      updateState: () => Promise<UpdateState>
      /** Compara o commit local com o do GitHub. */
      updateCheck: () => Promise<UpdateInfo>
      /** git pull + npm install (se preciso) + build + reinício. */
      updateApply: (data?: { depsMudaram?: boolean }) => Promise<{
        ok: boolean
        error?: string
        sha?: string
        reiniciou?: boolean
      }>
      /** Há commits novos no GitHub (verificado na abertura). */
      onUpdateAvailable: (cb: (info: UpdateInfo) => void) => () => void
      /** Etapa atual da atualização em andamento. */
      onUpdateProgress: (cb: (p: { etapa: UpdateEtapa }) => void) => () => void
      /** Download da loja concluído — oferecer restart da Steam. */
      onStoreDownloaded: (cb: (data: { appid: string; title: string }) => void) => () => void
      /** Estatísticas do perfil (jogos/horas jogadas), ou null se indisponível. */
      profileStats: () => Promise<ProfileStats | null>
      /** Legendary (Epic): binário instalado + sessão ativa. */
      legendaryStatus: () => Promise<{ installed: boolean; logged: boolean; user?: string }>
      /** Baixa o Legendary e abre o login Epic num terminal. */
      legendarySetup: () => Promise<{ ok: boolean; error?: string }>
      /** Biblioteca Epic normalizada (Game[]). */
      legendaryLibrary: () => Promise<{ ok: boolean; games?: Game[]; error?: string }>
      /** Download manager: fila + controle + evento de progresso. */
      dmQueue: () => Promise<DmItem[]>
      dmInstall: (game: {
        appid: string
        title: string
        cover?: string
        installPath?: string
        priority?: number
      }) => Promise<{ ok: boolean; error?: string }>
      /** Espaço em disco (GiB) do path informado. */
      diskSpace: (
        p?: string,
      ) => Promise<{ ok: boolean; total?: number; free?: number; error?: string }>
      dmPause: (appid: string) => Promise<void>
      /** Recoloca na fila um download que falhou. */
      dmRetry: (appid: string) => Promise<void>
      /** Tira da lista um item já finalizado (erro/concluído). */
      dmDismiss: (appid: string) => Promise<void>
      dmResume: (appid: string) => Promise<void>
      dmSetPriority: (appid: string, priority: number) => Promise<boolean>
      dmCancel: (appid: string) => Promise<void>
      onDmProgress: (cb: (items: DmItem[]) => void) => () => void
      /** Wine: versões instaladas + disponíveis, instalar/remover. */
      wineList: () => Promise<{ installed: WineVer[]; available: WineVer[]; error?: string }>
      /** Ferramentas do prefixo do jogo (winecfg/regedit/explorer/winetricks/wineboot). */
      prefixTool: (
        appid: string,
        tool: "winecfg" | "regedit" | "explorer" | "winetricks" | "wineboot",
        opts?: { wine?: string; prefix?: string },
      ) => Promise<{ ok: boolean; error?: string }>
      /** Executa um .exe/.msi dentro do prefixo do jogo (abre seletor de arquivo). */
      wineRunExe: (
        appid: string,
        opts?: { wine?: string; prefix?: string },
      ) => Promise<{ ok: boolean; error?: string }>
      /** Configurações por jogo (diálogo estilo Heroic), salvas automaticamente. */
      gameSettingsGet: (id: string) => Promise<{ settings: GameSettings; defaultPrefix: string }>
      gameSettingsSet: (id: string, patch: Partial<GameSettings>) => Promise<GameSettings>
      /** Escolhe uma pasta no sistema (temas customizados). */
      pickFolder: () => Promise<{ ok: boolean; path?: string }>
      /** Escolhe um arquivo qualquer (scripts pré/pós-jogo). */
      pickFile: () => Promise<{ ok: boolean; path?: string }>
      /** Plugins opcionais (SLSsteam, LuaTools) — shape histórico. */
      pluginsList: () => Promise<{ ok: boolean; plugins: PluginInfo[] }>
      /** Registro/manifest v1; respostas nunca incluem paths locais. */
      pluginsDetails: () => Promise<{ ok: boolean; plugins: PluginInfo[] }>
      pluginsGet: (id: string) => Promise<{
        ok: boolean
        plugin?: PluginInfo
        error?: string
      }>
      pluginsRegister: (pluginPath: string) => Promise<{
        ok: boolean
        plugin?: PluginInfo
        error?: string
        errors?: string[]
      }>
      pluginsUnregister: (id: string) => Promise<{ ok: boolean; error?: string }>
      pluginsEnable: (id: string) => Promise<{ ok: boolean; error?: string }>
      pluginsDisable: (id: string) => Promise<{ ok: boolean; error?: string }>
      pluginsVerify: (id: string) => Promise<PluginVerification>
      /** APIs históricas preservadas para built-ins. */
      pluginsInstall: (id: string) => Promise<{ ok: boolean; error?: string }>
      pluginsRemove: (id: string) => Promise<{ ok: boolean; error?: string }>
      /** Plugin ativado/desativado — telas de loja atualizam ações em tempo real. */
      onPluginsChanged: (cb: () => void) => () => void
      /** Assina o progresso do "baixar todos". Retorna a função de cancelar. */
      onTrailerProgress: (
        cb: (data: { done: number; total: number; title: string }) => void,
      ) => () => void
      /** Progresso do download de um trailer específico (escolha manual). */
      onTrailerDlProgress: (
        cb: (data: { id: string; percent: number; stage: string }) => void,
      ) => () => void
      /** Fontes de download (JSONs estilo Hydra, locais). */
      sourcesList: () => Promise<{ ok: boolean; sources: SourceInfo[] }>
      sourcesAdd: (url: string) => Promise<{ ok: boolean; source?: SourceInfo; error?: string }>
      sourcesRemove: (id: string) => Promise<{ ok: boolean; error?: string }>
      sourcesSync: () => Promise<{
        ok: boolean
        results: { id: string; ok: boolean; error?: string }[]
      }>
      sourcesSearch: (
        query: string,
        limit?: number,
      ) => Promise<{ ok: boolean; results: SourceGame[] }>
      sourcesGame: (
        ref: string,
      ) => Promise<{ ok: boolean; game?: SourceGameFull; source?: string; error?: string }>
      /** Torrent (worker Python + libtorrent). Ids sempre "tor:...". */
      torrentStart: (payload: {
        gameId: string
        url: string
        savePath?: string
        fileIndices?: number[]
        title?: string
        cover?: string
      }) => Promise<{ ok: boolean; error?: string }>
      torrentPause: (gameId: string) => Promise<{ ok: boolean; error?: string }>
      torrentResume: (gameId: string) => Promise<{ ok: boolean; error?: string }>
      torrentCancel: (gameId: string) => Promise<{ ok: boolean; error?: string }>
      torrentFiles: (
        magnet: string,
        timeoutMs?: number,
      ) => Promise<{
        ok: boolean
        name?: string
        totalSize?: number
        files?: TorrentFileInfo[]
        error?: string
      }>
      torrentSetLimit: (bytes: number) => Promise<{ ok: boolean; error?: string }>
      torrentList: () => Promise<{ ok: boolean; downloads: TorrentItem[] }>
      onTorrentProgress: (cb: (items: TorrentItem[]) => void) => () => void
      /** Conquistas do jogo (Steam): achievements.json local ou scrape da loja. */
      achievementsGet: (appid: string) => Promise<
        Array<{
          title: string
          desc?: string
          icon?: string
          icongray?: string
          apiname?: string
          block?: number | null
          bit?: number | null
          achieved?: boolean
          unlock?: number
          percent?: number
        }>
      >
      /** Conquista desbloqueada em tempo real (watcher do processo principal). */
      onAchievementUnlocked: (
        cb: (payload: {
          appid: string
          key: string
          title: string
          desc?: string
          icon?: string
          percent?: number
          unlock?: number
        }) => void,
      ) => () => void
      /** Força desbloqueio de uma conquista escrevendo no .bin do Steam (sem cliente Steam). */
      achievementsForceUnlock: (
        appid: string,
        apiname: string,
      ) => Promise<{ ok: boolean; error?: string; epoch?: number }>
      /** Recarrega apiname/título/desc/ícones dos itens a partir dos UserGameStatsSchema_*.bin da Steam. */
      achievementsSchemasLoad: () => Promise<{
        ok: boolean
        error?: string
        updated?: number
        iconsCopied?: number
        total?: number
      }>
    }
  }
}

export {}
