import type { Game } from "./components/ps5-launcher/types"

export interface Profile {
  name?: string
  realName?: string
  country?: string
  city?: string
  summary?: string
  avatar?: string // caminho file:// da foto/gif
  background?: string // plano de fundo do perfil (imagem/gif)
  showcase?: string[] // ids dos jogos em destaque na vitrine
  owner?: boolean
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
  library_featured_column?: "disabled" | "recent" | "favorites" | "most-played"
  recent_games_max?: number
  download_cpu_cores?: number // 0 = máximo
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

/** Conquista detalhada (Steam), do achievements.json gerado pelo index.py. */
export interface AchievementItem {
  name: string // id interno
  title: string
  desc: string
  icon: string // colorida
  icongray: string // cinza (bloqueada)
  achieved: boolean
  unlock: number // epoch do desbloqueio (0 = bloqueada)
  percent: number // % global de jogadores (raridade)
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

/** Estatísticas agregadas do perfil (nível/insígnias, estilo Steam). */
export interface ProfileStats {
  jogos: number
  playtime_hours: number
  ach_done: number
  ach_total: number
  ach_raras: number // desbloqueadas com ≤10% global
  jogos_100: number // jogos 100% completos
}

/** Item do feed de atividade (conquista desbloqueada recentemente). */
export interface RecentAchievement {
  appid: string
  game: string // título do jogo
  cover: string
  title: string // título da conquista
  desc: string
  icon: string
  percent: number
  unlock: number // epoch
}

/** Item da fila de downloads (Epic via Legendary). */
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
  interface Window {
    /** Modo da UI: console (PS5) ou desktop (estilo Heroic). */
    launcherMode?: "console" | "desktop"
    /** Caminhos dinâmicos da máquina do usuário. */
    launcherPaths?: { home: string; dataDir: string }
    launcherAPI?: {
      getLibrary: () => Promise<Game[]>
      launch: (cmd: string[], gameId?: string) => Promise<{ ok: boolean; error?: string; warnings?: string[] }>
      /** Fecha o jogo em execução (mata o processo do jogo). */
      closeGame: () => Promise<{ ok: boolean; error?: string }>
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
      customGameAdd: (data: { id: string; title: string; platform: "windows" | "linux"; exe: string }) => Promise<{ ok: boolean; error?: string; games?: Game[] }>
      /** Edita um jogo custom existente (título/executável). */
      customGameUpdate: (data: { id: string; title?: string; exe?: string }) => Promise<{ ok: boolean; error?: string; games?: Game[] }>
      /** Roda um instalador .exe no prefixo (botão "Executar instalador antes"). */
      customGameRunInstaller: (opts: { appid: string; wine?: string; prefix?: string }) => Promise<{ ok: boolean; error?: string }>
      /** Tamanhos reais (Epic) + requisitos (Steam) da página do jogo. */
      gameSysinfo: (game: Game) => Promise<{
        ok: boolean
        error?: string
        info?: { download_size?: number; disk_size?: number; version?: string; req_min?: string; req_rec?: string }
      }>
      /** Loja Steam: status dos pré-requisitos (dotnet, depotdownloader, slssteam, key). */
      storeStatus: () => Promise<{ dotnet?: string; depotdownloader: boolean; hubcapKey: boolean; slssteam: boolean; steamDir: string; adicionados?: string[] }>
      /** Loja Steam: busca no catálogo Hubcap. */
      storeSearch: (query: string) => Promise<{ ok: boolean; error?: string; jogos?: { appid: string; title: string; cover?: string; manifest?: boolean }[] }>
      /** Abre a conexão com a Steam antes da primeira busca (evita ~3s de TLS). */
      storeWarm: () => Promise<{ ok: boolean; error?: string }>
      /** Loja Steam: sugestões rápidas enquanto digita (só títulos). */
      storeSuggest: (query: string) => Promise<{ ok: boolean; error?: string; jogos?: { appid: string; title: string }[] }>
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
      }) => Promise<{ ok: boolean; error?: string }>
      /** Instala o .NET 9 local (necessário ao DepotDownloader). */
      storeEnsureDotnet: () => Promise<{ ok: boolean; error?: string; path?: string }>
      /** Adiciona o jogo à Steam sem baixar (lua no stplug-in + AdditionalApps). */
      storeAddToSteam: (payload: { appid: string; token?: string; dlcs?: string[]; title?: string }) => Promise<{ ok: boolean; error?: string }>
      /** Fixes disponíveis para o jogo (GameBypass/OnlineFix, índice luatools). */
      storeCheckFixes: (appid: string) => Promise<{ ok: boolean; error?: string; generic?: boolean; online?: boolean }>
      /** Baixa e extrai o fix na pasta do jogo. */
      storeApplyFix: (payload: { appid: string; type: "generic" | "online"; installPath: string }) => Promise<{ ok: boolean; error?: string }>
      /** Pasta de instalação do jogo (para aplicar fixes). */
      storeInstallDir: (game: Game) => Promise<{ path: string }>
      /** Bibliotecas Steam detectadas (multi-drive) com espaço livre. */
      storeLibraries: () => Promise<{ path: string; steamDir: string; free: number }[]>
      /** Desfaz o "Add": tira o jogo da SLSsteam (lua + AdditionalApps). */
      storeRemoveFromSteam: (appid: string) => Promise<{ ok: boolean; error?: string }>
      /** Remove jogo baixado/adicionado: pasta + appmanifest marcado + SLSsteam. */
      storeRemoveDownloaded: (appid: string) => Promise<{ ok: boolean; removidos?: number; error?: string }>
      /** Reinicia a Steam com a SLSsteam carregada (jogos aparecem como owned). */
      slssteamLaunch: () => Promise<{ ok: boolean; error?: string }>
      /** Instala a SLSsteam (slsteam-moon) do zero via release do GitHub. */
      slssteamInstall: () => Promise<{ ok: boolean; error?: string }>
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
      setConfig: (
        cfg: Partial<AppConfig>,
      ) => Promise<{ ok: boolean; error?: string; config?: AppConfig }>
      quit: () => Promise<void>
      /** Entra no modo console (PS5, tela cheia) — fecha o desktop. */
      enterConsole: () => Promise<{ ok: boolean; error?: string }>
      toggleFullscreen: () => Promise<void>
      setZoom: (z: number) => Promise<number>
      rebuildMeta: () => Promise<Game[]>
      integrationsStatus: () => Promise<IntegrationsStatus>
      pickImage: (
        kind: "avatar" | "background",
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      /** Caminho local (file://) do trailer já baixado, ou "" se não houver. */
      trailerPath: (id: string) => Promise<{ path: string }>
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
      trailerStreamUrl: (
        url: string,
      ) => Promise<{ ok: boolean; url?: string; error?: string }>
      /** Baixa um vídeo específico do YouTube como trailer do jogo. */
      trailerDownloadUrl: (
        id: string,
        url: string,
      ) => Promise<{ ok: boolean; path?: string; error?: string }>
      /** Baixa todos os trailers que faltam. Devolve quantos baixou. */
      trailerDownloadAll: () => Promise<{ ok: boolean; count?: number; error?: string }>
      /** Escolhe o arquivo cookies.txt do YouTube (restrição de idade). */
      trailerPickCookies: () => Promise<{ ok: boolean; path?: string }>
      /** Conquistas detalhadas do jogo (ícone/descrição/raridade/data). */
      achievementsGet: (appid: string) => Promise<AchievementItem[]>
      /** Foco real da janela vindo do processo principal (cobre gamescope). */
      onAppFocus: (cb: (focused: boolean) => void) => () => void
      /** Transições de jogo rodando (true = abriu, false = fechou). */
      onGameRunning: (cb: (running: boolean) => void) => () => void
      /** Biblioteca mudou no disco (download concluído, desinstalação). */
      onLibraryChanged: (cb: () => void) => () => void
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
      /** Conquista desbloqueada em tempo real (toast estilo PS5). */
      onAchievementUnlocked: (
        cb: (data: { appid: string; title: string; desc: string; icon: string; percent: number; unlock: number }) => void,
      ) => () => void
      /** SLScheevo: binário instalado + nº de schemas gerados. */
      slscheevoStatus: () => Promise<{ installed: boolean; schemas: number }>
      /** Estatísticas do perfil (nível/insígnias), ou null se indisponível. */
      profileStats: () => Promise<ProfileStats | null>
      /** Feed: últimas conquistas desbloqueadas (qualquer jogo), por data desc. */
      achievementsRecent: () => Promise<RecentAchievement[]>
      /** Legendary (Epic): binário instalado + sessão ativa. */
      legendaryStatus: () => Promise<{ installed: boolean; logged: boolean; user?: string }>
      /** Baixa o Legendary e abre o login Epic num terminal. */
      legendarySetup: () => Promise<{ ok: boolean; error?: string }>
      /** Biblioteca Epic normalizada (Game[]). */
      legendaryLibrary: () => Promise<{ ok: boolean; games?: Game[]; error?: string }>
      /** Download manager: fila + controle + evento de progresso. */
      dmQueue: () => Promise<DmItem[]>
      dmInstall: (game: { appid: string; title: string; cover?: string; installPath?: string }) => Promise<{ ok: boolean; error?: string }>
      /** Espaço em disco (GiB) do path informado. */
      diskSpace: (p?: string) => Promise<{ ok: boolean; total?: number; free?: number; error?: string }>
      dmPause: (appid: string) => Promise<void>
      /** Recoloca na fila um download que falhou. */
      dmRetry: (appid: string) => Promise<void>
      /** Tira da lista um item já finalizado (erro/concluído). */
      dmDismiss: (appid: string) => Promise<void>
      dmResume: (appid: string) => Promise<void>
      dmCancel: (appid: string) => Promise<void>
      onDmProgress: (cb: (items: DmItem[]) => void) => () => void
      /** Wine: versões instaladas + disponíveis, instalar/remover. */
      wineList: () => Promise<{ installed: WineVer[]; available: WineVer[]; error?: string }>
      wineInstall: (id: string, kind?: "ge-proton" | "wine-ge") => Promise<{ ok: boolean; error?: string }>
      wineRemove: (id: string) => Promise<{ ok: boolean }>
      /** Progresso de download de uma versão de Wine/Proton. */
      onWineProgress: (cb: (p: { id: string; done: number; total: number }) => void) => () => void
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
      /** Baixa o SLScheevo e abre a sessão interativa de login num terminal. */
      slscheevoSetup: () => Promise<{ ok: boolean; error?: string }>
      /** Assina o progresso do "baixar todos". Retorna a função de cancelar. */
      onTrailerProgress: (
        cb: (data: { done: number; total: number; title: string }) => void,
      ) => () => void
      /** Progresso do download de um trailer específico (escolha manual). */
      onTrailerDlProgress: (
        cb: (data: { id: string; percent: number; stage: string }) => void,
      ) => () => void
    }
  }
}

export {}
